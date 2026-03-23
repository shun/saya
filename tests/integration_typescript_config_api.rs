use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use saya::bootstrap::prepare_launch;
use saya::callback_registry_seed::CallbackRegistrySeed;
use saya::cli::{LaunchRequest, ConfigSource};
use saya::screen_model::{ProjectionInput, project};
use saya::saya_live_runtime::{
    BoxFuture, BufferEventPayload, CallbackRegistryBuilder, HostCapabilityBridge,
    ReadonlyBufferSnapshot, ReadonlyEditorSnapshot, ReadonlyWindowSnapshot,
    RuntimeCommandError, RuntimeEventPayload, RuntimeMode, SayaLiveRuntime,
};
use tokio::sync::Mutex;

fn unique_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-ts-config-{name}-{nanos}"))
}

#[test]
fn startup_typescript_config_reflects_options_registry_and_headless_projection() {
    let _lock = saya::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("target.txt");
    let config_path = unique_path("init.ts");
    std::fs::write(&target_path, "alpha\nbeta\n").expect("target file");
    std::fs::write(
        &config_path,
        r#"
            saya.options.tabSize = 4;
            saya.options.lineNumbers = true;
            saya.keymap.set("normal", "<leader>w", saya.commands.execute("writeCurrent"));
            saya.commands.register("writeCurrent", () => {
                saya.commands.execute("write");
            });
            saya.events.on("bufferOpen", (payload) => {
                console.log(payload.buffer.id);
            });
        "#,
    )
    .expect("config file");

    let outcome = prepare_launch(LaunchRequest {
        target_path: Some(target_path.clone()),
        config_source: ConfigSource::File(config_path.clone()),
    })
    .expect("startup with typescript config");

    assert_eq!(outcome.initial_tab_size, 4);
    assert!(outcome.initial_line_numbers);
    assert_eq!(outcome.startup_registry.keymaps.len(), 1);
    assert_eq!(outcome.callback_registry.commands().len(), 1);
    assert_eq!(outcome.callback_registry.events().len(), 1);

    let session_state = outcome.editor_session_state();
    let model = project(&ProjectionInput {
        snapshot: &outcome.initial_snapshot,
        session_state: &session_state,
        transient_message: None,
    });

    assert_eq!(model.lines[0], "1 alpha");
    assert_eq!(model.lines[1], "2 beta");

    std::fs::remove_file(&target_path).expect("remove target");
    std::fs::remove_file(&config_path).expect("remove config");
}

#[test]
fn startup_config_failure_keeps_default_session_and_presentation_state() {
    let _lock = saya::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("target-fallback.txt");
    let config_path = unique_path("missing-init.ts");
    std::fs::write(&target_path, "alpha\nbeta\n").expect("target file");

    let outcome = prepare_launch(LaunchRequest {
        target_path: Some(target_path.clone()),
        config_source: ConfigSource::File(config_path.clone()),
    })
    .expect("startup should continue with default fallback");

    assert_eq!(outcome.initial_tab_size, 8);
    assert!(!outcome.initial_line_numbers);
    assert!(outcome.warnings.iter().any(|warning| matches!(
        warning,
        saya::bootstrap::BootstrapWarning::ConfigLoadFailed { path, .. } if path == &config_path
    )));

    let session_state = outcome.editor_session_state();
    let model = project(&ProjectionInput {
        snapshot: &outcome.initial_snapshot,
        session_state: &session_state,
        transient_message: None,
    });

    assert_eq!(session_state.tab_size(), 8);
    assert!(!session_state.line_numbers());
    assert_eq!(model.lines[0], "alpha");
    assert_eq!(model.lines[1], "beta");

    std::fs::remove_file(&target_path).expect("remove target");
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
                id: 99,
                path: Some(PathBuf::from("runtime.md")),
                line_count: 2,
            }
        })
    }

    fn current_window(&self) -> BoxFuture<ReadonlyWindowSnapshot> {
        Box::pin(async move { ReadonlyWindowSnapshot { id: 5 } })
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
async fn runtime_event_dispatch_executes_registered_command_headlessly() {
    let host_bridge = Arc::new(RecordingHostBridge::new());
    let observed = Arc::new(Mutex::new(Vec::new()));
    let observed_in_command = observed.clone();
    let observed_in_event = observed.clone();

    let registry = CallbackRegistryBuilder::default()
        .register_command("writeCurrent", move |ctx| {
            let observed_in_command = observed_in_command.clone();
            Box::pin(async move {
                let buffer = ctx.buffer().current().await;
                observed_in_command
                    .lock()
                    .await
                    .push(format!("command:{:?}", buffer.path));
                ctx.commands().execute("write").await
            })
        })
        .on_buffer_open(move |ctx, payload| {
            let observed_in_event = observed_in_event.clone();
            Box::pin(async move {
                observed_in_event
                    .lock()
                    .await
                    .push(format!("event:{}", payload.buffer.id));
                ctx.commands().execute("writeCurrent").await?;
                Ok(())
            })
        })
        .build();

    let runtime = SayaLiveRuntime::spawn(host_bridge.clone(), registry);
    let report = runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: ReadonlyBufferSnapshot {
                id: 7,
                path: Some(PathBuf::from("runtime.md")),
                line_count: 2,
            },
        }))
        .expect("dispatch queued")
        .await_result()
        .await
        .expect("dispatch result");

    assert_eq!(report.handler_count, 1);
    assert_eq!(
        observed.lock().await.clone(),
        vec![
            "event:7".to_string(),
            "command:Some(\"runtime.md\")".to_string(),
        ]
    );
    assert_eq!(
        host_bridge.executed_commands.lock().await.clone(),
        vec!["write".to_string()]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_surface_is_frozen_and_does_not_expose_registration_apis() {
    let host_bridge = Arc::new(RecordingHostBridge::new());
    let seed = CallbackRegistrySeed::from_startup_entries(vec![saya::config_runtime::StartupRegistryEntry::Event {
        name: "bufferOpen".to_string(),
        callback_source: r#"
            async (payload) => {
                if (!Object.isFrozen(saya)) {
                    throw new Error("runtime saya surface should be frozen");
                }
                if (!Object.isFrozen(saya.commands)) {
                    throw new Error("runtime command surface should be frozen");
                }
                if (!Object.isFrozen(saya.buffer)) {
                    throw new Error("runtime buffer surface should be frozen");
                }
                if (!Object.isFrozen(saya.window)) {
                    throw new Error("runtime window surface should be frozen");
                }
                if (!Object.isFrozen(saya.editor)) {
                    throw new Error("runtime editor surface should be frozen");
                }
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
            }
        "#
        .to_string(),
    }]);

    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge, seed)
        .expect("runtime should initialize with frozen public surface");

    let report = runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: ReadonlyBufferSnapshot {
                id: 101,
                path: Some(PathBuf::from("surface.md")),
                line_count: 1,
            },
        }))
        .expect("dispatch queued")
        .await_result()
        .await
        .expect("dispatch result");

    assert_eq!(report.handler_count, 1);
}
