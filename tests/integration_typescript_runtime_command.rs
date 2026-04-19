//! 統合テスト: TypeScript runtime command integration の検証
//!
//! このファイルは `saya` の TypeScript runtime integration suite です。
//!
//! 責務は startup-registered commands の実行、runtime callback dispatch、
//! application boot 後の host/application integration に限定する。詳細な
//! editing semantics は ADR 0001 に従って `vim-core-rs` に委ねる。
//!
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use saya::app_startup::prepare_launch_and_start_terminal;
use saya::cli::{ConfigSource, InputSource, LaunchRequest};
use saya::runtime_integration::{
    RuntimeCommandEffect, RuntimeDispatchOutcome, RuntimeEventMapper, RuntimeHostSession,
    RuntimeOutcomeProjector, RuntimeSessionOwner, RuntimeShutdownIntent,
};
use saya::saya_live_runtime::{
    BoxFuture, BufferEventPayload, HostCapabilityBridge, ReadonlyBufferSnapshot,
    ReadonlyEditorSnapshot, ReadonlyWindowSnapshot, RuntimeCommandError, RuntimeEventPayload,
    RuntimeMode, SayaLiveRuntime,
};
use saya::terminal_lifecycle::TerminalBackend;
use tokio::sync::Mutex as TokioMutex;

fn unique_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-typescript-runtime-command-{name}-{nanos}"))
}

fn typescript_runtime_suite_scope_statement() -> &'static str {
    "TypeScript runtime integration suite for startup-registered commands, runtime callback dispatch, and host/application integration after application boot"
}

#[test]
fn typescript_runtime_suite_scope_statement_stays_pinned_to_host_layer_integration() {
    let statement = typescript_runtime_suite_scope_statement();

    assert!(
        statement.contains("TypeScript runtime integration suite"),
        "suite ownership statement should stay explicit"
    );
    assert!(
        statement.contains("startup-registered commands"),
        "suite ownership statement should keep startup registration visible"
    );
    assert!(
        statement.contains("runtime callback dispatch"),
        "suite ownership statement should keep runtime integration visible"
    );
    assert!(
        statement.contains("host/application"),
        "suite ownership statement should stay anchored to the host layer"
    );
    assert!(
        !statement.contains("editing semantics"),
        "suite ownership statement must not drift into core-editing ownership"
    );
}

struct RecordingHostBridge {
    executed_commands: Arc<TokioMutex<Vec<String>>>,
}

impl RecordingHostBridge {
    fn new() -> Self {
        Self {
            executed_commands: Arc::new(TokioMutex::new(Vec::new())),
        }
    }
}

#[derive(Default)]
struct DummyTerminalBackend {
    calls: Vec<&'static str>,
}

impl TerminalBackend for DummyTerminalBackend {
    fn enable_raw_mode(&mut self) -> std::io::Result<()> {
        self.calls.push("enable_raw_mode");
        Ok(())
    }

    fn enter_alternate_screen(&mut self) -> std::io::Result<()> {
        self.calls.push("enter_alternate_screen");
        Ok(())
    }

    fn leave_alternate_screen(&mut self) -> std::io::Result<()> {
        self.calls.push("leave_alternate_screen");
        Ok(())
    }

    fn disable_raw_mode(&mut self) -> std::io::Result<()> {
        self.calls.push("disable_raw_mode");
        Ok(())
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
async fn startup_registered_command_executes_from_runtime_event_after_application_boot() {
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

    let mut terminal_backend = DummyTerminalBackend::default();
    let (outcome, terminal_broker) = prepare_launch_and_start_terminal(
        LaunchRequest {
            input_source: InputSource::Empty,
            config_source: ConfigSource::File(config_path.clone()),
            ..LaunchRequest::default()
        },
        &mut terminal_backend,
    )
    .expect("startup config should prepare callback seed");
    assert!(terminal_broker.is_raw_mode_enabled());
    assert!(terminal_broker.is_alternate_screen_enabled());
    drop(terminal_broker);
    assert_eq!(
        terminal_backend.calls,
        vec![
            "enable_raw_mode",
            "enter_alternate_screen",
            "leave_alternate_screen",
            "disable_raw_mode",
        ]
    );

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

#[tokio::test(flavor = "current_thread")]
async fn startup_and_runtime_capability_boundaries_survive_application_boot() {
    let _lock = saya::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let config_path = unique_path("boundary-init.ts");
    std::fs::write(
        &config_path,
        r#"
            saya.options.tabSize = 4;
            saya.keymap.set("normal", "<leader>w", saya.commands.execute("writeCurrent"));
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
            });
        "#,
    )
    .expect("config file");

    let mut terminal_backend = DummyTerminalBackend::default();
    let (outcome, terminal_broker) = prepare_launch_and_start_terminal(
        LaunchRequest {
            input_source: InputSource::Empty,
            config_source: ConfigSource::File(config_path.clone()),
            ..LaunchRequest::default()
        },
        &mut terminal_backend,
    )
    .expect("startup config should prepare callback seed");

    assert!(terminal_broker.is_raw_mode_enabled());
    assert!(terminal_broker.is_alternate_screen_enabled());
    drop(terminal_broker);
    assert_eq!(
        terminal_backend.calls,
        vec![
            "enable_raw_mode",
            "enter_alternate_screen",
            "leave_alternate_screen",
            "disable_raw_mode",
        ]
    );

    assert_eq!(outcome.startup_registry.options.tab_size, 4);
    assert_eq!(outcome.startup_registry.keymaps.len(), 1);
    assert_eq!(outcome.callback_registry.commands().len(), 1);
    assert_eq!(outcome.callback_registry.events().len(), 1);

    let host_bridge = Arc::new(RecordingHostBridge::new());
    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), outcome.callback_registry)
        .expect("seed runtime should initialize from startup registry");

    let report = runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: ReadonlyBufferSnapshot {
                id: 22,
                path: Some(PathBuf::from("boundary.md")),
                line_count: 3,
            },
        }))
        .expect("dispatch queued")
        .await_result()
        .await
        .expect("dispatch result");

    assert_eq!(report.handler_count, 1);
    assert_eq!(
        host_bridge.executed_commands.lock().await.clone(),
        vec!["write".to_string()]
    );

    std::fs::remove_file(&config_path).expect("remove config");
}

struct RecordingRuntimeHostSession {
    executed_commands: Vec<String>,
    transient_messages: Vec<String>,
    dispatched_follow_up_events: Vec<RuntimeEventPayload>,
    dispatched_shutdown_intents: Vec<RuntimeShutdownIntent>,
    buffer: ReadonlyBufferSnapshot,
    window: ReadonlyWindowSnapshot,
    editor: ReadonlyEditorSnapshot,
}

impl Default for RecordingRuntimeHostSession {
    fn default() -> Self {
        Self {
            executed_commands: Vec::new(),
            transient_messages: Vec::new(),
            dispatched_follow_up_events: Vec::new(),
            dispatched_shutdown_intents: Vec::new(),
            buffer: ReadonlyBufferSnapshot {
                id: 1,
                path: None,
                line_count: 1,
            },
            window: ReadonlyWindowSnapshot { id: 1 },
            editor: ReadonlyEditorSnapshot {
                mode: RuntimeMode::Normal,
            },
        }
    }
}

impl RecordingRuntimeHostSession {
    fn with_buffer(path: &str, line_count: usize) -> Self {
        Self {
            buffer: ReadonlyBufferSnapshot {
                id: 55,
                path: Some(PathBuf::from(path)),
                line_count,
            },
            window: ReadonlyWindowSnapshot { id: 1 },
            editor: ReadonlyEditorSnapshot {
                mode: RuntimeMode::Normal,
            },
            ..Self::default()
        }
    }
}

impl RuntimeHostSession for RecordingRuntimeHostSession {
    fn current_buffer_snapshot(&mut self) -> ReadonlyBufferSnapshot {
        self.buffer.clone()
    }

    fn current_window_snapshot(&mut self) -> ReadonlyWindowSnapshot {
        self.window.clone()
    }

    fn current_editor_snapshot(&mut self) -> ReadonlyEditorSnapshot {
        self.editor.clone()
    }

    fn execute_host_command(
        &mut self,
        name: &str,
    ) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
        self.executed_commands.push(name.to_string());
        let (follow_up_events, shutdown_intent) = if name == "write" {
            let event = RuntimeEventMapper::buffer_write_post(self.current_buffer_snapshot());
            self.dispatched_follow_up_events.push(event.clone());
            (vec![event], None)
        } else if name == "writeAndQuit" {
            let event = RuntimeEventMapper::buffer_write_post(self.current_buffer_snapshot());
            self.dispatched_follow_up_events.push(event.clone());
            self.dispatched_shutdown_intents
                .push(RuntimeShutdownIntent::UserQuit);
            (vec![event], Some(RuntimeShutdownIntent::UserQuit))
        } else {
            (Vec::new(), None)
        };
        let transient_message = if name == "write" || name == "writeAndQuit" {
            Some("Saved successfully".to_string())
        } else {
            None
        };
        if let Some(message) = transient_message.as_ref() {
            self.transient_messages.push(message.clone());
        }
        Ok(RuntimeCommandEffect {
            transient_message,
            follow_up_events,
            shutdown_intent,
            presentation_intents: Vec::new(),
        })
    }
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_session_owner_dispatches_buffer_open_and_follow_up_write_post_through_normalized_outcome()
 {
    let _lock = saya::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let config_path = unique_path("live-session-owner-init.ts");
    std::fs::write(
        &config_path,
        r#"
            saya.commands.register("writeCurrent", () => {
                return saya.commands.execute("write");
            });
            saya.events.on("bufferOpen", async (payload) => {
                await saya.commands.execute("writeCurrent");
                await saya.commands.execute(`opened:${payload.buffer.id}:${payload.buffer.lineCount}`);
            });
            saya.events.on("bufferWritePost", async (payload) => {
                await saya.commands.execute(`wrote:${payload.buffer.id}:${payload.buffer.lineCount}`);
            });
        "#,
    )
    .expect("config file");

    let outcome = saya::bootstrap::prepare_launch(LaunchRequest {
        input_source: InputSource::Empty,
        config_source: ConfigSource::File(config_path.clone()),
        ..LaunchRequest::default()
    })
    .expect("startup config should prepare callback seed");

    let mut runtime = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
        .expect("live runtime session owner should initialize");
    let mut host_session = RecordingRuntimeHostSession::with_buffer("live-session.md", 4);

    let dispatch_outcome = runtime
        .dispatch(
            RuntimeEventMapper::buffer_open(host_session.current_buffer_snapshot()),
            &mut host_session,
        )
        .await;

    assert_eq!(
        dispatch_outcome,
        RuntimeDispatchOutcome {
            transient_message: Some("Saved successfully".to_string()),
            requires_redraw: true,
            shutdown_intent: None,
            presentation_intents: Vec::new(),
        }
    );
    assert_eq!(
        host_session.executed_commands,
        vec![
            "write".to_string(),
            "opened:55:4".to_string(),
            "wrote:55:4".to_string(),
        ]
    );
    assert_eq!(
        host_session.dispatched_follow_up_events,
        vec![RuntimeEventMapper::buffer_write_post(
            ReadonlyBufferSnapshot {
                id: 55,
                path: Some(PathBuf::from("live-session.md")),
                line_count: 4,
            }
        )]
    );
    assert!(
        host_session.dispatched_shutdown_intents.is_empty(),
        "write only の host command は shutdown intent を持たないこと"
    );

    std::fs::remove_file(&config_path).expect("remove config");
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_session_owner_retains_shutdown_intent_while_preserving_write_follow_up_events() {
    let _lock = saya::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let config_path = unique_path("live-session-owner-shutdown-init.ts");
    std::fs::write(
        &config_path,
        r#"
            saya.events.on("bufferOpen", async () => {
                await saya.commands.execute("writeAndQuit");
            });
            saya.events.on("bufferWritePost", async (payload) => {
                await saya.commands.execute(`wrote:${payload.buffer.id}:${payload.buffer.lineCount}`);
            });
        "#,
    )
    .expect("config file");

    let outcome = saya::bootstrap::prepare_launch(LaunchRequest {
        input_source: InputSource::Empty,
        config_source: ConfigSource::File(config_path.clone()),
        ..LaunchRequest::default()
    })
    .expect("startup config should prepare callback seed");

    let mut runtime = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
        .expect("live runtime session owner should initialize");
    let mut host_session = RecordingRuntimeHostSession::with_buffer("live-session.md", 4);

    let dispatch_outcome = runtime
        .dispatch(
            RuntimeEventMapper::buffer_open(host_session.current_buffer_snapshot()),
            &mut host_session,
        )
        .await;

    assert_eq!(
        dispatch_outcome,
        RuntimeDispatchOutcome {
            transient_message: Some("Saved successfully".to_string()),
            requires_redraw: true,
            shutdown_intent: Some(RuntimeShutdownIntent::UserQuit),
            presentation_intents: Vec::new(),
        }
    );
    assert_eq!(
        host_session.executed_commands,
        vec!["writeAndQuit".to_string(), "wrote:55:4".to_string()]
    );
    assert_eq!(
        host_session.dispatched_follow_up_events,
        vec![RuntimeEventMapper::buffer_write_post(
            ReadonlyBufferSnapshot {
                id: 55,
                path: Some(PathBuf::from("live-session.md")),
                line_count: 4,
            }
        )]
    );
    assert_eq!(
        host_session.dispatched_shutdown_intents,
        vec![RuntimeShutdownIntent::UserQuit]
    );

    std::fs::remove_file(&config_path).expect("remove config");
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_session_owner_projects_callback_failure_into_transient_message_and_redraw() {
    let _lock = saya::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let config_path = unique_path("live-session-owner-failure-init.ts");
    std::fs::write(
        &config_path,
        r#"
            saya.events.on("bufferOpen", () => {
                throw new Error("owner-boom");
            });
        "#,
    )
    .expect("config file");

    let outcome = saya::bootstrap::prepare_launch(LaunchRequest {
        input_source: InputSource::Empty,
        config_source: ConfigSource::File(config_path.clone()),
        ..LaunchRequest::default()
    })
    .expect("startup config should prepare callback seed");

    let mut runtime = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
        .expect("live runtime session owner should initialize");
    let mut host_session = RecordingRuntimeHostSession::with_buffer("failure.md", 2);

    let dispatch_outcome = runtime
        .dispatch(
            RuntimeEventMapper::buffer_open(host_session.current_buffer_snapshot()),
            &mut host_session,
        )
        .await;

    assert!(
        dispatch_outcome
            .transient_message
            .as_deref()
            .is_some_and(|message| message.contains("owner-boom")),
        "runtime callback failure should be projected into the transient message: {:?}",
        dispatch_outcome
    );
    assert!(
        dispatch_outcome.requires_redraw,
        "runtime callback failure should request redraw so the message becomes visible"
    );

    std::fs::remove_file(&config_path).expect("remove config");
}

#[test]
fn runtime_outcome_projector_requests_redraw_for_callback_failure_messages() {
    let projected = RuntimeOutcomeProjector::project_error(
        &saya::saya_live_runtime::RuntimeDispatchError::CallbackFailed {
            event: saya::saya_live_runtime::RuntimeEventName::BufferOpen,
            handler_index: 0,
            error: saya::saya_live_runtime::RuntimeCallbackError::ScriptFailed {
                message: "boom".to_string(),
            },
        },
    );

    assert_eq!(
        projected,
        RuntimeDispatchOutcome {
            transient_message: Some(
                "Runtime callback failed on bufferOpen handler 0: script error: boom".to_string()
            ),
            requires_redraw: true,
            shutdown_intent: None,
            presentation_intents: Vec::new(),
        }
    );
}
