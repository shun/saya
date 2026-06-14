//! 統合テスト: TypeScript runtime command integration の検証
//!
//! このファイルは `saya` の TypeScript runtime integration suite です。
//!
//! 責務は startup-registered commands の実行、runtime callback dispatch、
//! application boot 後の host/application integration に限定する。詳細な
//! editing semantics は ADR 0001 に従って `vim-core-rs` に委ねる。
//!
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use saya::app::cli::{ConfigSource, InputSource, LaunchRequest};
use saya::app::startup::prepare_launch_and_start_terminal;
use saya::runtime::integration::{
    RuntimeCommandEffect, RuntimeDispatchOutcome, RuntimeEventMapper, RuntimeHostSession,
    RuntimeOutcomeProjector, RuntimeSessionOwner, RuntimeShutdownIntent,
};
use saya::runtime::live::{
    BoxFuture, BufferEventPayload, HostCapabilityBridge, ReadonlyBufferSnapshot,
    ReadonlyEditorSnapshot, ReadonlyWindowSnapshot, RuntimeCommandError, RuntimeEventPayload,
    RuntimeFloatOpenRequest, RuntimeFloatSnapshot, RuntimeMode, RuntimePanelOpenRequest,
    RuntimePanelSnapshot, SayaLiveRuntime,
};
use saya::runtime::plugin::{LazyIndex, LazyTarget, PluginCacheRoot, PluginHost};
use saya::terminal::lifecycle::TerminalBackend;
use tokio::sync::Mutex as TokioMutex;

fn unique_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-typescript-runtime-command-{name}-{nanos}"))
}

fn with_isolated_lazy_plugin_event<T>(name: &str, event_name: &str, f: impl FnOnce() -> T) -> T {
    let cache_root = unique_path(name);
    let host = PluginHost::new(PluginCacheRoot::new(cache_root.clone()));
    let mut events = BTreeMap::new();
    events.insert(
        event_name.to_string(),
        vec![LazyTarget {
            plugin: "__test".to_string(),
            module: "__test.ts".to_string(),
            export_name: "setup".to_string(),
        }],
    );
    host.write_lazy_index(&LazyIndex {
        version: LazyIndex::CURRENT_VERSION,
        commands: BTreeMap::new(),
        events,
    })
    .expect("isolated lazy plugin cache should be writable");

    let previous_cache_dir = std::env::var_os("SAYA_CACHE_DIR");
    unsafe {
        std::env::set_var("SAYA_CACHE_DIR", &cache_root);
    }
    let result = f();
    if let Some(value) = previous_cache_dir {
        unsafe {
            std::env::set_var("SAYA_CACHE_DIR", value);
        }
    } else {
        unsafe {
            std::env::remove_var("SAYA_CACHE_DIR");
        }
    }
    let _ = std::fs::remove_dir_all(cache_root);
    result
}

struct RecordingHostBridge {
    executed_commands: Arc<TokioMutex<Vec<String>>>,
    read_buffers: Arc<TokioMutex<Vec<ReadonlyBufferSnapshot>>>,
    read_windows: Arc<TokioMutex<Vec<ReadonlyWindowSnapshot>>>,
    read_editors: Arc<TokioMutex<Vec<ReadonlyEditorSnapshot>>>,
    opened_floats: Arc<TokioMutex<Vec<RuntimeFloatOpenRequest>>>,
    focused_floats: Arc<TokioMutex<Vec<u64>>>,
    closed_floats: Arc<TokioMutex<Vec<u64>>>,
    opened_panels: Arc<TokioMutex<Vec<RuntimePanelOpenRequest>>>,
    focused_panels: Arc<TokioMutex<Vec<String>>>,
    unfocused_panels: Arc<TokioMutex<usize>>,
    closed_panels: Arc<TokioMutex<Vec<String>>>,
    sent_panel_text: Arc<TokioMutex<Vec<(String, String)>>>,
}

impl RecordingHostBridge {
    fn new() -> Self {
        Self {
            executed_commands: Arc::new(TokioMutex::new(Vec::new())),
            read_buffers: Arc::new(TokioMutex::new(Vec::new())),
            read_windows: Arc::new(TokioMutex::new(Vec::new())),
            read_editors: Arc::new(TokioMutex::new(Vec::new())),
            opened_floats: Arc::new(TokioMutex::new(Vec::new())),
            focused_floats: Arc::new(TokioMutex::new(Vec::new())),
            closed_floats: Arc::new(TokioMutex::new(Vec::new())),
            opened_panels: Arc::new(TokioMutex::new(Vec::new())),
            focused_panels: Arc::new(TokioMutex::new(Vec::new())),
            unfocused_panels: Arc::new(TokioMutex::new(0)),
            closed_panels: Arc::new(TokioMutex::new(Vec::new())),
            sent_panel_text: Arc::new(TokioMutex::new(Vec::new())),
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

    fn enable_mouse_capture(&mut self) -> std::io::Result<()> {
        self.calls.push("enable_mouse_capture");
        Ok(())
    }

    fn enable_bracketed_paste(&mut self) -> std::io::Result<()> {
        self.calls.push("enable_bracketed_paste");
        Ok(())
    }

    fn disable_bracketed_paste(&mut self) -> std::io::Result<()> {
        self.calls.push("disable_bracketed_paste");
        Ok(())
    }

    fn disable_mouse_capture(&mut self) -> std::io::Result<()> {
        self.calls.push("disable_mouse_capture");
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
        let read_buffers = self.read_buffers.clone();
        Box::pin(async move {
            let snapshot = ReadonlyBufferSnapshot {
                id: 404,
                path: Some(PathBuf::from("wave6-runtime.md")),
                line_count: 9,
                cursor_row: 0,
                cursor_col: 0,
                current_line: String::new(),
                text: String::new(),
            };
            read_buffers.lock().await.push(snapshot.clone());
            snapshot
        })
    }

    fn current_window(&self) -> BoxFuture<ReadonlyWindowSnapshot> {
        let read_windows = self.read_windows.clone();
        Box::pin(async move {
            let snapshot = ReadonlyWindowSnapshot { id: 12 };
            read_windows.lock().await.push(snapshot.clone());
            snapshot
        })
    }

    fn current_editor(&self) -> BoxFuture<ReadonlyEditorSnapshot> {
        let read_editors = self.read_editors.clone();
        Box::pin(async move {
            let snapshot = ReadonlyEditorSnapshot {
                mode: RuntimeMode::Normal,
            };
            read_editors.lock().await.push(snapshot.clone());
            snapshot
        })
    }

    fn open_float(
        &self,
        request: RuntimeFloatOpenRequest,
    ) -> BoxFuture<Result<RuntimeFloatSnapshot, RuntimeCommandError>> {
        let opened_floats = self.opened_floats.clone();
        Box::pin(async move {
            opened_floats.lock().await.push(request);
            Ok(RuntimeFloatSnapshot {
                id: 77,
                kind: "lines".to_string(),
                focused: false,
                focusable: true,
                width: 24,
                height: 4,
                row: 1,
                col: 2,
                border: "single".to_string(),
                z_index: 80,
                lifecycle: "manual".to_string(),
                replacement_group: None,
            })
        })
    }

    fn focus_float(&self, id: u64) -> BoxFuture<Result<bool, RuntimeCommandError>> {
        let focused_floats = self.focused_floats.clone();
        Box::pin(async move {
            focused_floats.lock().await.push(id);
            Ok(true)
        })
    }

    fn close_float(&self, id: u64) -> BoxFuture<Result<bool, RuntimeCommandError>> {
        let closed_floats = self.closed_floats.clone();
        Box::pin(async move {
            closed_floats.lock().await.push(id);
            Ok(true)
        })
    }

    fn list_float_snapshots(
        &self,
    ) -> BoxFuture<Result<Vec<RuntimeFloatSnapshot>, RuntimeCommandError>> {
        Box::pin(async move {
            Ok(vec![RuntimeFloatSnapshot {
                id: 77,
                kind: "lines".to_string(),
                focused: true,
                focusable: true,
                width: 24,
                height: 4,
                row: 1,
                col: 2,
                border: "single".to_string(),
                z_index: 80,
                lifecycle: "manual".to_string(),
                replacement_group: Some("phase8".to_string()),
            }])
        })
    }

    fn open_panel(
        &self,
        request: RuntimePanelOpenRequest,
    ) -> BoxFuture<Result<RuntimePanelSnapshot, RuntimeCommandError>> {
        let opened_panels = self.opened_panels.clone();
        Box::pin(async move {
            let snapshot = RuntimePanelSnapshot {
                id: request.id.clone(),
                numeric_id: 90,
                position: request.position.clone(),
                size: request.size.clone(),
                kind: request.content.kind.clone(),
                focused: request.focus,
            };
            opened_panels.lock().await.push(request);
            Ok(RuntimePanelSnapshot {
                id: snapshot.id,
                numeric_id: snapshot.numeric_id,
                position: snapshot.position,
                size: snapshot.size,
                kind: snapshot.kind,
                focused: snapshot.focused,
            })
        })
    }

    fn focus_panel(&self, id: String) -> BoxFuture<Result<bool, RuntimeCommandError>> {
        let focused_panels = self.focused_panels.clone();
        Box::pin(async move {
            focused_panels.lock().await.push(id);
            Ok(true)
        })
    }

    fn unfocus_panel(&self) -> BoxFuture<Result<bool, RuntimeCommandError>> {
        let unfocused_panels = self.unfocused_panels.clone();
        Box::pin(async move {
            *unfocused_panels.lock().await += 1;
            Ok(true)
        })
    }

    fn close_panel(&self, id: String) -> BoxFuture<Result<bool, RuntimeCommandError>> {
        let closed_panels = self.closed_panels.clone();
        Box::pin(async move {
            closed_panels.lock().await.push(id);
            Ok(true)
        })
    }

    fn list_panel_snapshots(
        &self,
    ) -> BoxFuture<Result<Vec<RuntimePanelSnapshot>, RuntimeCommandError>> {
        Box::pin(async move {
            Ok(vec![RuntimePanelSnapshot {
                id: "ai-agent".to_string(),
                numeric_id: 90,
                position: "right".to_string(),
                size: "35%".to_string(),
                kind: "terminal".to_string(),
                focused: true,
            }])
        })
    }

    fn send_panel_text(
        &self,
        id: String,
        text: String,
    ) -> BoxFuture<Result<bool, RuntimeCommandError>> {
        let sent_panel_text = self.sent_panel_text.clone();
        Box::pin(async move {
            sent_panel_text.lock().await.push((id, text));
            Ok(true)
        })
    }
}

#[tokio::test(flavor = "current_thread")]
async fn startup_registered_command_executes_from_runtime_event_after_application_boot() {
    let _lock = saya::app::bootstrap::launch_test_lock()
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
                const currentBuffer = await saya.buffer.current();
                const currentWindow = await saya.window.current();
                const currentEditor = await saya.editor.current();
                if (
                    currentBuffer.id !== 404 ||
                    currentBuffer.path !== "wave6-runtime.md" ||
                    currentBuffer.lineCount !== 9 ||
                    currentWindow.id !== 12 ||
                    currentEditor.mode !== "Normal"
                ) {
                    throw new Error(
                        `unexpected runtime state read-back: ${JSON.stringify({
                            currentBuffer,
                            currentWindow,
                            currentEditor,
                        })}`
                    );
                }
                await saya.commands.execute(`opened:${payload.buffer.id}:${payload.buffer.lineCount}`);
            });
        "#,
    )
    .expect("config file");

    let mut terminal_backend = DummyTerminalBackend::default();
    let (outcome, terminal_broker) =
        with_isolated_lazy_plugin_event("runtime-event-plugin-cache", "bufferOpen", || {
            prepare_launch_and_start_terminal(
                LaunchRequest {
                    input_source: InputSource::Empty,
                    config_source: ConfigSource::File(config_path.clone()),
                    ..LaunchRequest::default()
                },
                &mut terminal_backend,
            )
        })
        .expect("startup config should prepare callback seed");
    assert!(terminal_broker.is_raw_mode_enabled());
    assert!(terminal_broker.is_alternate_screen_enabled());
    drop(terminal_broker);
    assert_eq!(
        terminal_backend.calls,
        vec![
            "enable_raw_mode",
            "enter_alternate_screen",
            "enable_mouse_capture",
            "enable_bracketed_paste",
            "disable_bracketed_paste",
            "disable_mouse_capture",
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

    assert_eq!(report.handler_count, 1);
    assert_eq!(
        host_bridge.executed_commands.lock().await.clone(),
        vec!["write".to_string(), "opened:17:4".to_string()]
    );
    assert_eq!(
        host_bridge.read_buffers.lock().await.clone(),
        vec![ReadonlyBufferSnapshot {
            id: 404,
            path: Some(PathBuf::from("wave6-runtime.md")),
            line_count: 9,
            cursor_row: 0,
            cursor_col: 0,
            current_line: String::new(),
            text: String::new(),
        }]
    );
    assert_eq!(
        host_bridge.read_windows.lock().await.clone(),
        vec![ReadonlyWindowSnapshot { id: 12 }]
    );
    assert_eq!(
        host_bridge.read_editors.lock().await.clone(),
        vec![ReadonlyEditorSnapshot {
            mode: RuntimeMode::Normal,
        }]
    );

    std::fs::remove_file(&config_path).expect("remove config");
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_panel_api_forwards_open_focus_list_send_and_close_to_host() {
    let _lock = saya::app::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let config_path = unique_path("panel-init.ts");
    std::fs::write(
        &config_path,
        r#"
            saya.events.on("bufferOpen", async () => {
                const panel = await saya.panel.open({
                    id: "ai-agent",
                    position: "right",
                    size: "35%",
                    content: {
                        kind: "terminal",
                        command: ["codex"],
                        closeBehavior: "detach",
                    },
                    focus: true,
                });
                const listed = await saya.panel.list();
                await saya.panel.send(panel.id, "Review the current file\n");
                await saya.panel.focus(panel.id);
                await saya.panel.unfocus();
                await saya.panel.close(panel.id);
                await saya.commands.execute(`panel:${panel.id}:${listed.length}:${listed[0].kind}`);
            });
        "#,
    )
    .expect("config file");

    let mut terminal_backend = DummyTerminalBackend::default();
    let (outcome, terminal_broker) =
        with_isolated_lazy_plugin_event("runtime-panel-plugin-cache", "bufferOpen", || {
            prepare_launch_and_start_terminal(
                LaunchRequest {
                    input_source: InputSource::Empty,
                    config_source: ConfigSource::File(config_path.clone()),
                    ..LaunchRequest::default()
                },
                &mut terminal_backend,
            )
        })
        .expect("startup config should prepare callback seed");
    drop(terminal_broker);
    let host_bridge = Arc::new(RecordingHostBridge::new());
    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), outcome.callback_registry)
        .expect("runtime should spawn");

    runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: ReadonlyBufferSnapshot {
                id: 1,
                path: Some(PathBuf::from("panel.md")),
                line_count: 1,
                cursor_row: 0,
                cursor_col: 0,
                current_line: "hello".to_string(),
                text: "hello\n".to_string(),
            },
        }))
        .expect("dispatch should enqueue")
        .await_result()
        .await
        .expect("panel callback should complete");

    let opened = host_bridge.opened_panels.lock().await;
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0].id, "ai-agent");
    assert_eq!(opened[0].position, "right");
    assert_eq!(opened[0].size, "35%");
    assert_eq!(opened[0].content.kind, "terminal");
    assert_eq!(opened[0].content.command, vec!["codex".to_string()]);
    assert_eq!(opened[0].content.close_behavior.as_deref(), Some("detach"));
    drop(opened);

    assert_eq!(
        *host_bridge.sent_panel_text.lock().await,
        vec![(
            "ai-agent".to_string(),
            "Review the current file\n".to_string()
        )]
    );
    assert_eq!(
        *host_bridge.focused_panels.lock().await,
        vec!["ai-agent".to_string()]
    );
    assert_eq!(*host_bridge.unfocused_panels.lock().await, 1);
    assert_eq!(
        *host_bridge.closed_panels.lock().await,
        vec!["ai-agent".to_string()]
    );
    assert_eq!(
        *host_bridge.executed_commands.lock().await,
        vec!["panel:ai-agent:1:terminal".to_string()]
    );

    std::fs::remove_file(&config_path).expect("remove config");
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_panel_api_forwards_structured_view_content_to_host() {
    let _lock = saya::app::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let config_path = unique_path("panel-view-init.ts");
    std::fs::write(
        &config_path,
        r#"
            saya.events.on("bufferOpen", async () => {
                const panel = await saya.panel.open({
                    id: "dashboard",
                    position: "right",
                    size: "35%",
                    content: {
                        kind: "view",
                        nodes: [
                            { type: "heading", text: "Weather" },
                            { type: "text", text: "16C" },
                            { type: "badge", label: "rain" },
                            { type: "progress", label: "build", value: 50 },
                            { type: "divider" },
                            { type: "button", label: "Refresh" },
                            { type: "image", src: "/tmp/moon.png", alt: "Moon phase" },
                        ],
                    },
                    focus: true,
                });
                await saya.panel.focus(panel.id);
                await saya.commands.execute(`panel-view:${panel.id}:${panel.kind}`);
            });
        "#,
    )
    .expect("config file");

    let mut terminal_backend = DummyTerminalBackend::default();
    let (outcome, terminal_broker) =
        with_isolated_lazy_plugin_event("runtime-panel-view-plugin-cache", "bufferOpen", || {
            prepare_launch_and_start_terminal(
                LaunchRequest {
                    input_source: InputSource::Empty,
                    config_source: ConfigSource::File(config_path.clone()),
                    ..LaunchRequest::default()
                },
                &mut terminal_backend,
            )
        })
        .expect("startup config should prepare callback seed");
    drop(terminal_broker);
    let host_bridge = Arc::new(RecordingHostBridge::new());
    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), outcome.callback_registry)
        .expect("runtime should spawn");

    runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: ReadonlyBufferSnapshot {
                id: 1,
                path: Some(PathBuf::from("panel-view.md")),
                line_count: 1,
                cursor_row: 0,
                cursor_col: 0,
                current_line: "hello".to_string(),
                text: "hello\n".to_string(),
            },
        }))
        .expect("dispatch should enqueue")
        .await_result()
        .await
        .expect("panel callback should complete");

    let opened = host_bridge.opened_panels.lock().await;
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0].id, "dashboard");
    assert_eq!(opened[0].content.kind, "view");
    assert_eq!(opened[0].content.nodes.len(), 7);
    assert_eq!(opened[0].content.nodes[0].node_type, "heading");
    assert_eq!(opened[0].content.nodes[0].text.as_deref(), Some("Weather"));
    assert_eq!(opened[0].content.nodes[3].label.as_deref(), Some("build"));
    assert_eq!(opened[0].content.nodes[3].value, Some(50));
    assert_eq!(
        opened[0].content.nodes[6].src.as_deref(),
        Some("/tmp/moon.png")
    );
    drop(opened);

    assert_eq!(
        *host_bridge.focused_panels.lock().await,
        vec!["dashboard".to_string()]
    );
    assert_eq!(
        *host_bridge.executed_commands.lock().await,
        vec!["panel-view:dashboard:view".to_string()]
    );

    std::fs::remove_file(&config_path).expect("remove config");
}

#[tokio::test(flavor = "current_thread")]
async fn startup_and_runtime_capability_boundaries_survive_application_boot() {
    let _lock = saya::app::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let config_path = unique_path("boundary-init.ts");
    std::fs::write(
        &config_path,
        r#"
            saya.options.tabstop = 4;
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
    let (outcome, terminal_broker) =
        with_isolated_lazy_plugin_event("runtime-boundary-plugin-cache", "bufferOpen", || {
            prepare_launch_and_start_terminal(
                LaunchRequest {
                    input_source: InputSource::Empty,
                    config_source: ConfigSource::File(config_path.clone()),
                    ..LaunchRequest::default()
                },
                &mut terminal_backend,
            )
        })
        .expect("startup config should prepare callback seed");

    assert!(terminal_broker.is_raw_mode_enabled());
    assert!(terminal_broker.is_alternate_screen_enabled());
    drop(terminal_broker);
    assert_eq!(
        terminal_backend.calls,
        vec![
            "enable_raw_mode",
            "enter_alternate_screen",
            "enable_mouse_capture",
            "enable_bracketed_paste",
            "disable_bracketed_paste",
            "disable_mouse_capture",
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

    assert_eq!(report.handler_count, 1);
    assert_eq!(
        host_bridge.executed_commands.lock().await.clone(),
        vec!["write".to_string()]
    );

    std::fs::remove_file(&config_path).expect("remove config");
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_window_float_api_routes_typed_requests_through_host_bridge() {
    let _lock = saya::app::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let config_path = unique_path("typed-window-float-api-init.ts");
    std::fs::write(
        &config_path,
        r#"
            saya.events.on("bufferOpen", async () => {
                const float = await saya.window.openFloat({
                    content: { kind: "lines", lines: ["phase8", "typed"] },
                    relativeTo: { kind: "editor" },
                    width: 24,
                    height: 4,
                    row: 1,
                    col: 2,
                    focusable: true,
                    border: "single",
                    zIndex: "user",
                    lifecycle: "manual",
                    group: "phase8",
                });
                if (float.id !== 77 || float.kind !== "lines") {
                    throw new Error(`unexpected float snapshot: ${JSON.stringify(float)}`);
                }
                const focused = await saya.window.focus(float.id);
                const snapshots = await saya.window.floats();
                const closed = await saya.window.close(float.id);
                await saya.commands.execute(
                    `float:${float.id}:${focused}:${closed}:${snapshots[0].focused}:${snapshots[0].replacementGroup}`
                );
            });
        "#,
    )
    .expect("config file");

    let outcome = saya::app::bootstrap::prepare_launch(LaunchRequest {
        input_source: InputSource::Empty,
        config_source: ConfigSource::File(config_path.clone()),
        ..LaunchRequest::default()
    })
    .expect("startup config should prepare callback seed");
    let host_bridge = Arc::new(RecordingHostBridge::new());
    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), outcome.callback_registry)
        .expect("runtime should initialize");

    runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: ReadonlyBufferSnapshot {
                id: 101,
                path: Some(PathBuf::from("phase8.md")),
                line_count: 1,
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

    let opened = host_bridge.opened_floats.lock().await.clone();
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0].width, Some(24));
    assert_eq!(opened[0].height, Some(4));
    assert_eq!(opened[0].group.as_deref(), Some("phase8"));
    assert_eq!(host_bridge.focused_floats.lock().await.clone(), vec![77]);
    assert_eq!(host_bridge.closed_floats.lock().await.clone(), vec![77]);
    assert_eq!(
        host_bridge.executed_commands.lock().await.clone(),
        vec!["float:77:true:true:true:phase8".to_string()]
    );

    std::fs::remove_file(&config_path).expect("remove config");
}

struct RecordingRuntimeHostSession {
    executed_commands: Vec<String>,
    full_buffer_snapshot_reads: usize,
    metadata_buffer_snapshot_reads: usize,
    transient_messages: Vec<String>,
    dispatched_follow_up_events: Vec<RuntimeEventPayload>,
    dispatched_shutdown_intents: Vec<RuntimeShutdownIntent>,
    opened_float_requests: Vec<RuntimeFloatOpenRequest>,
    focused_float_ids: Vec<u64>,
    closed_float_ids: Vec<u64>,
    float_snapshots: Vec<RuntimeFloatSnapshot>,
    buffer: ReadonlyBufferSnapshot,
    window: ReadonlyWindowSnapshot,
    editor: ReadonlyEditorSnapshot,
}

impl Default for RecordingRuntimeHostSession {
    fn default() -> Self {
        Self {
            executed_commands: Vec::new(),
            full_buffer_snapshot_reads: 0,
            metadata_buffer_snapshot_reads: 0,
            transient_messages: Vec::new(),
            dispatched_follow_up_events: Vec::new(),
            dispatched_shutdown_intents: Vec::new(),
            opened_float_requests: Vec::new(),
            focused_float_ids: Vec::new(),
            closed_float_ids: Vec::new(),
            float_snapshots: Vec::new(),
            buffer: ReadonlyBufferSnapshot {
                id: 1,
                path: None,
                line_count: 1,
                cursor_row: 0,
                cursor_col: 0,
                current_line: String::new(),
                text: String::new(),
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
                cursor_row: 0,
                cursor_col: 0,
                current_line: String::new(),
                text: String::new(),
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
        self.full_buffer_snapshot_reads += 1;
        self.buffer.clone()
    }

    fn current_buffer_metadata_snapshot(&mut self) -> ReadonlyBufferSnapshot {
        self.metadata_buffer_snapshot_reads += 1;
        ReadonlyBufferSnapshot {
            text: String::new(),
            ..self.buffer.clone()
        }
    }

    fn current_window_snapshot(&mut self) -> ReadonlyWindowSnapshot {
        self.window.clone()
    }

    fn current_editor_snapshot(&mut self) -> ReadonlyEditorSnapshot {
        self.editor.clone()
    }

    fn open_float(
        &mut self,
        request: RuntimeFloatOpenRequest,
    ) -> Result<RuntimeFloatSnapshot, RuntimeCommandError> {
        self.opened_float_requests.push(request);
        let snapshot = RuntimeFloatSnapshot {
            id: 501,
            kind: "lines".to_string(),
            focused: false,
            focusable: true,
            width: 30,
            height: 5,
            row: 2,
            col: 3,
            border: "single".to_string(),
            z_index: 80,
            lifecycle: "manual".to_string(),
            replacement_group: Some("owner-phase8".to_string()),
        };
        self.float_snapshots = vec![snapshot.clone()];
        Ok(snapshot)
    }

    fn focus_float(&mut self, id: u64) -> Result<bool, RuntimeCommandError> {
        self.focused_float_ids.push(id);
        for snapshot in &mut self.float_snapshots {
            snapshot.focused = snapshot.id == id;
        }
        Ok(true)
    }

    fn close_float(&mut self, id: u64) -> Result<bool, RuntimeCommandError> {
        self.closed_float_ids.push(id);
        let before = self.float_snapshots.len();
        self.float_snapshots.retain(|snapshot| snapshot.id != id);
        Ok(self.float_snapshots.len() != before)
    }

    fn list_float_snapshots(&mut self) -> Result<Vec<RuntimeFloatSnapshot>, RuntimeCommandError> {
        Ok(self.float_snapshots.clone())
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
            vfs_load_failed: false,
        })
    }
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_session_owner_dispatches_buffer_open_and_follow_up_write_post_through_normalized_outcome()
 {
    let _lock = saya::app::bootstrap::launch_test_lock()
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

    let outcome = saya::app::bootstrap::prepare_launch(LaunchRequest {
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
                cursor_row: 0,
                cursor_col: 0,
                current_line: String::new(),
                text: String::new(),
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
async fn runtime_current_buffer_path_uses_cached_metadata_without_fetching_full_text() {
    let _lock = saya::app::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let config_path = unique_path("runtime-current-path-init.ts");
    std::fs::write(
        &config_path,
        r#"
            saya.commands.register("pathOnly", async () => {
                const path = await saya.buffer.currentPath();
                await saya.commands.execute(`path:${path}`);
            });
        "#,
    )
    .expect("config file");

    let outcome = saya::app::bootstrap::prepare_launch(LaunchRequest {
        input_source: InputSource::Empty,
        config_source: ConfigSource::File(config_path.clone()),
        ..LaunchRequest::default()
    })
    .expect("startup config should prepare callback seed");

    let mut runtime = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
        .expect("live runtime session owner should initialize");
    let mut host_session = RecordingRuntimeHostSession::with_buffer("huge.md", 200_000);
    host_session.buffer.text = "x".repeat(2_000_000);

    let dispatch_outcome = runtime.execute_command("pathOnly", &mut host_session).await;

    assert_eq!(dispatch_outcome.transient_message, None);
    assert_eq!(host_session.executed_commands, vec!["path:huge.md"]);
    assert_eq!(
        host_session.full_buffer_snapshot_reads, 0,
        "currentPath should not request the full buffer snapshot"
    );
    assert!(
        host_session.metadata_buffer_snapshot_reads >= 1,
        "runtime command still needs cheap path/cursor metadata"
    );

    std::fs::remove_file(&config_path).expect("remove config");
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_session_owner_routes_window_float_api_through_typed_host_session() {
    let _lock = saya::app::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let config_path = unique_path("live-session-owner-float-api-init.ts");
    std::fs::write(
        &config_path,
        r#"
            saya.events.on("bufferOpen", async () => {
                const float = await saya.window.openFloat({
                    content: { kind: "lines", lines: ["owner", "phase8"] },
                    width: 30,
                    height: 5,
                    focusable: true,
                    group: "owner-phase8",
                });
                await saya.window.focus(float.id);
                const snapshots = await saya.window.floats();
                await saya.commands.execute(`owner-float:${snapshots[0].id}:${snapshots[0].focused}`);
                await saya.window.close(float.id);
            });
        "#,
    )
    .expect("config file");

    let outcome = saya::app::bootstrap::prepare_launch(LaunchRequest {
        input_source: InputSource::Empty,
        config_source: ConfigSource::File(config_path.clone()),
        ..LaunchRequest::default()
    })
    .expect("startup config should prepare callback seed");

    let mut runtime = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
        .expect("live runtime session owner should initialize");
    let mut host_session = RecordingRuntimeHostSession::with_buffer("owner-float.md", 2);

    let dispatch_outcome = runtime
        .dispatch(
            RuntimeEventMapper::buffer_open(host_session.current_buffer_snapshot()),
            &mut host_session,
        )
        .await;

    assert!(dispatch_outcome.requires_redraw);
    assert_eq!(host_session.opened_float_requests.len(), 1);
    assert_eq!(host_session.opened_float_requests[0].width, Some(30));
    assert_eq!(host_session.focused_float_ids, vec![501]);
    assert_eq!(host_session.closed_float_ids, vec![501]);
    assert_eq!(
        host_session.executed_commands,
        vec!["owner-float:501:true".to_string()]
    );

    std::fs::remove_file(&config_path).expect("remove config");
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_session_owner_retains_shutdown_intent_while_preserving_write_follow_up_events() {
    let _lock = saya::app::bootstrap::launch_test_lock()
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

    let outcome = saya::app::bootstrap::prepare_launch(LaunchRequest {
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
                cursor_row: 0,
                cursor_col: 0,
                current_line: String::new(),
                text: String::new(),
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
    let _lock = saya::app::bootstrap::launch_test_lock()
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

    let outcome = saya::app::bootstrap::prepare_launch(LaunchRequest {
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
        &saya::runtime::live::RuntimeDispatchError::CallbackFailed {
            event: saya::runtime::live::RuntimeEventName::BufferOpen,
            handler_index: 0,
            error: saya::runtime::live::RuntimeCallbackError::ScriptFailed {
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
