use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use saya::bootstrap::prepare_launch;
use saya::cli::{ConfigSource, InputSource, LaunchRequest};
use saya::saya_live_runtime::{
    BoxFuture, BufferEventPayload, HostCapabilityBridge, ReadonlyBufferSnapshot,
    ReadonlyEditorSnapshot, ReadonlyWindowSnapshot, RuntimeCommandError, RuntimeEventPayload,
    RuntimeMode, SayaLiveRuntime,
};
use tokio::sync::Mutex;

fn unique_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-wave6-runtime-command-{name}-{nanos}"))
}

struct RecordingHostBridge {
    executed_commands: Arc<Mutex<Vec<String>>>,
}

impl RecordingHostBridge {
    fn new() -> Self {
        Self {
            executed_commands: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl HostCapabilityBridge for RecordingHostBridge {
    fn execute_host_command(&self, name: &str) -> BoxFuture<Result<(), RuntimeCommandError>> {
        let executed_commands = self.executed_commands.clone();
        let name = name.to_string();
        Box::pin(async move {
            executed_commands.lock().await.push(name);
            Ok(())
        })
    }

    fn current_buffer(&self) -> BoxFuture<ReadonlyBufferSnapshot> {
        Box::pin(async move {
            ReadonlyBufferSnapshot {
                id: 404,
                path: Some(PathBuf::from("wave6-runtime.md")),
                line_count: 9,
            }
        })
    }

    fn current_window(&self) -> BoxFuture<ReadonlyWindowSnapshot> {
        Box::pin(async move { ReadonlyWindowSnapshot { id: 12 } })
    }

    fn current_editor(&self) -> BoxFuture<ReadonlyEditorSnapshot> {
        Box::pin(async move {
            ReadonlyEditorSnapshot {
                mode: RuntimeMode::Normal,
            }
        })
    }
}

#[tokio::test(flavor = "current_thread")]
async fn startup_registered_command_executes_from_runtime_event_headlessly() {
    let _lock = saya::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let config_path = unique_path("init.ts");
    std::fs::write(
        &config_path,
        r#"
            saya.commands.register("writeCurrent", () => {
                saya.commands.execute("write");
            });
            saya.events.on("bufferOpen", async (payload) => {
                if (typeof saya.commands.register !== "undefined") {
                    throw new Error("startup register api leaked into runtime namespace");
                }
                if (typeof saya.events !== "undefined") {
                    throw new Error("startup events api leaked into runtime namespace");
                }
                if (typeof saya.options !== "undefined") {
                    throw new Error("startup options api leaked into runtime namespace");
                }
                if (typeof saya.keymap !== "undefined") {
                    throw new Error("startup keymap api leaked into runtime namespace");
                }
                await saya.commands.execute("writeCurrent");
                await saya.commands.execute(`opened:${payload.buffer.id}:${payload.buffer.lineCount}`);
            });
        "#,
    )
    .expect("config file");

    let outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::Empty,
        config_source: ConfigSource::File(config_path.clone()),
        ..LaunchRequest::default()
    })
    .expect("startup config should prepare callback seed");

    assert_eq!(outcome.callback_registry.commands().len(), 1);
    assert_eq!(outcome.callback_registry.events().len(), 1);

    let host_bridge = Arc::new(RecordingHostBridge::new());
    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), outcome.callback_registry)
        .expect("seed runtime should initialize from startup registry");

    let report = runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: ReadonlyBufferSnapshot {
                id: 17,
                path: Some(PathBuf::from("headless.md")),
                line_count: 4,
            },
        }))
        .expect("dispatch queued")
        .await_result()
        .await
        .expect("dispatch result");

    assert_eq!(report.handler_count, 1);
    assert_eq!(
        host_bridge.executed_commands.lock().await.clone(),
        vec!["write".to_string(), "opened:17:4".to_string()]
    );

    std::fs::remove_file(&config_path).expect("remove config");
}
