//! TypeScript runtime typed payload integration.
//!
//! This file proves host/application runtime payload dispatch and typed payload
//! projection for buffer events.

use std::path::PathBuf;
use std::sync::Arc;

use saya::runtime::callback_registry_seed::CallbackRegistrySeed;
use saya::runtime::config::StartupRegistryEntry;
use saya::runtime::live::{
    BoxFuture, BufferEventPayload, HostCapabilityBridge, ReadonlyBufferSnapshot,
    ReadonlyEditorSnapshot, ReadonlyWindowSnapshot, RuntimeCommandError, RuntimeEventName,
    RuntimeEventPayload, RuntimeMode, SayaLiveRuntime,
};
use tokio::sync::Mutex;

#[tokio::test(flavor = "current_thread")]
async fn runtime_callback_consumes_typed_payload_for_buffer_write_post_headlessly() {
    let host_bridge = Arc::new(RecordingHostBridge::new());
    let observed = Arc::new(Mutex::new(Vec::new()));
    let observed_in_event = observed.clone();

    let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
        name: "bufferWritePost".to_string(),
        callback_source: r#"
            (payload) => {
                if (payload.buffer.lineCount !== 12) {
                    throw new Error(`unexpected lineCount: ${payload.buffer.lineCount}`);
                }
                if (payload.buffer.id !== 41) {
                    throw new Error(`unexpected buffer id: ${payload.buffer.id}`);
                }
                if (payload.buffer.path !== "typed-payload.md") {
                    throw new Error(`unexpected buffer path: ${payload.buffer.path}`);
                }
                return saya.commands.execute(
                    `typed:${payload.buffer.id}:${payload.buffer.lineCount}:${payload.buffer.path}`
                );
            }
        "#
        .to_string(),
    }]);

    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), seed)
        .expect("seed runtime should initialize");

    let report = runtime
        .dispatch_event(RuntimeEventPayload::BufferWritePost(BufferEventPayload {
            buffer: ReadonlyBufferSnapshot {
                id: 41,
                path: Some(PathBuf::from("typed-payload.md")),
                line_count: 12,
                cursor_row: 0,
                cursor_col: 0,
                current_line: String::new(),
                text: String::new(),
            },
        }))
        .expect("dispatch queued")
        .await_result()
        .await
        .expect("dispatch result");

    observed_in_event
        .lock()
        .await
        .push(format!("{:?}", report.event));

    assert_eq!(report.event, RuntimeEventName::BufferWritePost);
    assert_eq!(report.handler_count, 1);
    assert_eq!(
        observed.lock().await.clone(),
        vec!["BufferWritePost".to_string()]
    );
    assert_eq!(
        host_bridge.executed_commands.lock().await.clone(),
        vec!["typed:41:12:typed-payload.md".to_string()]
    );
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
                id: 1,
                path: None,
                line_count: 0,
                cursor_row: 0,
                cursor_col: 0,
                current_line: String::new(),
                text: String::new(),
            }
        })
    }

    fn current_window(&self) -> BoxFuture<ReadonlyWindowSnapshot> {
        Box::pin(async move { ReadonlyWindowSnapshot { id: 1 } })
    }

    fn current_editor(&self) -> BoxFuture<ReadonlyEditorSnapshot> {
        Box::pin(async move {
            ReadonlyEditorSnapshot {
                mode: RuntimeMode::Normal,
            }
        })
    }
}
