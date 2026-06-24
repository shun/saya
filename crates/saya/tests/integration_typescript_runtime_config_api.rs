//! 統合テスト: TypeScript runtime integration の検証
//!
//! このファイルは `saya` の TypeScript runtime integration suite です。
//!
//! 責務は startup config の反映、runtime callback dispatch、
//! host/application projection に限定する。詳細な editing semantics は
//! ADR 0001 に従って `vim-core-rs` に委ねる。
//!
mod support;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use saya::app::bootstrap::prepare_launch;
use saya::app::cli::{ConfigSource, InputSource, LaunchRequest};
use saya::app::host_io::{SaveResult, write_to_path};
use saya::app::session::EditorSessionState;
use saya::presentation::markdown::structure::MarkdownDocumentMap;
use saya::presentation::screen_model::{ProjectionInput, project};
use saya::presentation::theme::ResolvedThemeColor;
use saya::runtime::callback_registry_seed::CallbackRegistrySeed;
use saya::runtime::live::{
    BoxFuture, BufferEventPayload, CallbackRegistryBuilder, HostCapabilityBridge,
    ReadonlyBufferSnapshot, ReadonlyEditorSnapshot, ReadonlyWindowSnapshot, RuntimeCommandError,
    RuntimeEventPayload, RuntimeMode, SayaLiveRuntime,
};
use saya::runtime::message::runtime_callback_failure_message;
use saya::runtime::plugin::{LazyIndex, LazyTarget, PluginCacheRoot, PluginHost};
use saya::runtime::refresh::runtime_dispatch_requests_redraw;
use tokio::sync::Mutex;

fn unique_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-ts-config-{name}-{nanos}"))
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

/// 非挙動メタゲート（命名 lint・CI 別ロール想定）。挙動は検証しない。
/// runtime 系統合テストファイルが wave6 系の旧命名ではなく
/// typescript_runtime 系の命名規約に揃っていることだけを確認する命名規約 lint。
#[test]
fn meta_lint_runtime_related_test_files_use_typescript_runtime_prefix_instead_of_wave6_prefix() {
    let tests_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let file_names: Vec<String> = std::fs::read_dir(&tests_dir)
        .expect("tests directory should be readable")
        .map(|entry| {
            entry
                .expect("test directory entry should be readable")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();

    assert!(
        file_names.contains(&"integration_typescript_runtime_config_api.rs".to_string()),
        "runtime config API suite should use the runtime-oriented naming convention"
    );
    assert!(
        file_names.contains(&"integration_typescript_runtime_command.rs".to_string()),
        "runtime command suite should use the runtime-oriented naming convention"
    );
    assert!(
        file_names.contains(&"integration_typescript_runtime_typed_payload.rs".to_string()),
        "typed payload suite should use the runtime-oriented naming convention"
    );
    assert!(
        !file_names.contains(&"integration_typescript_config_api.rs".to_string()),
        "typescript config API naming should be retired from the runtime suite"
    );
    assert!(
        !file_names.contains(&"integration_wave6_runtime_command.rs".to_string()),
        "wave6 runtime command naming should be retired from the runtime suite"
    );
    assert!(
        !file_names.contains(&"integration_wave6_typed_payload.rs".to_string()),
        "wave6 typed payload naming should be retired from the runtime suite"
    );
}

#[test]
fn startup_typescript_config_reflects_options_registry_and_headless_projection() {
    let _lock = support::session::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("target.txt");
    let config_path = unique_path("init.ts");
    std::fs::write(&target_path, "alpha\nbeta\n").expect("target file");
    std::fs::write(
        &config_path,
        r#"
            saya.options.tabstop = 4;
            saya.options.number = true;
            saya.options.numberwidth = 4;
            saya.options.syntax = true;
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

    let outcome =
        with_isolated_lazy_plugin_event("startup-options-plugin-cache", "bufferOpen", || {
            prepare_launch(LaunchRequest {
                input_source: InputSource::File(target_path.clone()),
                config_source: ConfigSource::File(config_path.clone()),
                ..LaunchRequest::default()
            })
        })
        .expect("startup with typescript config");

    assert_eq!(outcome.initial_tab_size, 4);
    assert!(outcome.initial_line_numbers);
    assert_eq!(outcome.initial_number_width, 4);
    assert_eq!(outcome.startup_registry.options.syntax, true);
    assert_eq!(outcome.startup_registry.keymaps.len(), 1);
    assert_eq!(outcome.callback_registry.commands().len(), 1);
    assert_eq!(outcome.callback_registry.events().len(), 1);

    let session_state = outcome.editor_session_state();
    let model = project(&ProjectionInput::new(
        &outcome.initial_snapshot,
        &session_state,
        None,
    ));

    assert_eq!(session_state.number_width(), 4);
    assert_eq!(model.lines[0], "   1 alpha");
    assert_eq!(model.lines[1], "   2 beta");

    std::fs::remove_file(&target_path).expect("remove target");
    std::fs::remove_file(&config_path).expect("remove config");
}

#[test]
fn startup_typescript_config_resolves_markdown_theme_for_headless_projection() {
    let _lock = support::session::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("theme-target.md");
    let config_path = unique_path("theme-init.ts");
    let markdown_source = "## Heading\ninline `code` [link](https://example.com)\n";
    std::fs::write(&target_path, markdown_source).expect("target file");
    std::fs::write(
        &config_path,
        r##"
            saya.theme.palette = {
                accent: "#7aa2f7",
                heading2: "#9ece6a",
                code: "#ff9e64",
                link: "#2ac3de",
            };
            saya.theme.markdown = {
                heading: { fg: "accent", bold: true },
                heading2: { fg: "heading2", underline: true },
                inlineCode: { fg: "code" },
                link: { fg: "link", underline: true },
            };
        "##,
    )
    .expect("config file");

    let outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::File(target_path.clone()),
        config_source: ConfigSource::File(config_path.clone()),
        ..LaunchRequest::default()
    })
    .expect("startup with typescript theme config");

    let markdown_map = MarkdownDocumentMap::parse(markdown_source);
    let session_state = outcome.editor_session_state();
    let mut input = ProjectionInput::new(&outcome.initial_snapshot, &session_state, None)
        .with_markdown_document_map(Some(&markdown_map));
    input.is_active = false;
    let model = project(&input);

    assert_eq!(model.lines[0], "## Heading");
    assert_eq!(model.line_projections[0].display_text, "Heading");
    let heading = model
        .markdown_style_ranges
        .iter()
        .find(|range| range.row == 0)
        .expect("heading2 range should be projected");
    assert_eq!(
        heading.style.fg,
        Some(ResolvedThemeColor("#9ece6a".to_string()))
    );
    assert!(
        heading.style.bold,
        "heading2 should inherit bold from heading"
    );
    assert!(
        heading.style.underline,
        "heading2 should add its level-specific underline"
    );
    assert!(
        model.markdown_style_ranges.iter().any(|range| {
            range.row == 1 && range.style.fg == Some(ResolvedThemeColor("#ff9e64".to_string()))
        }),
        "inlineCode should resolve through the palette into a concrete color"
    );
    assert!(
        model.markdown_style_ranges.iter().any(|range| {
            range.row == 1
                && range.style.fg == Some(ResolvedThemeColor("#2ac3de".to_string()))
                && range.style.underline
        }),
        "link should resolve through the palette and keep underline"
    );

    std::fs::remove_file(&target_path).expect("remove target");
    std::fs::remove_file(&config_path).expect("remove config");
}

#[test]
fn startup_typescript_config_keeps_markdown_projection_with_ui_and_syntax_theme() {
    let _lock = support::session::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("theme-ui-syntax-target.md");
    let config_path = unique_path("theme-ui-syntax-init.ts");
    let markdown_source = "## Heading\n- [x] done\ninline `code`\n";
    std::fs::write(&target_path, markdown_source).expect("target file");
    std::fs::write(
        &config_path,
        r##"
            saya.theme.palette = {
                bg: "#24283b",
                fg: "#c0caf5",
                fgGutter: "#3b4261",
                blue: "#7aa2f7",
                green: "#9ece6a",
                orange: "#ff9e64",
                comment: "#565f89",
            };
            saya.theme.ui = {
                text: { fg: "fg", bg: "bg" },
                gutter: { fg: "fgGutter", bg: "bg" },
            };
            saya.theme.syntax = {
                comment: { fg: "comment", italic: true },
                default: { fg: "fg" },
            };
            saya.theme.markdown = {
                heading: { fg: "blue", bold: true },
                heading2: { fg: "green" },
                inlineCode: { fg: "orange" },
                checkboxChecked: { fg: "green", bold: true },
            };
        "##,
    )
    .expect("config file");

    let outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::File(target_path.clone()),
        config_source: ConfigSource::File(config_path.clone()),
        ..LaunchRequest::default()
    })
    .expect("startup with combined typescript theme config");

    let markdown_map = MarkdownDocumentMap::parse(markdown_source);
    let session_state = outcome.editor_session_state();
    let mut input = ProjectionInput::new(&outcome.initial_snapshot, &session_state, None)
        .with_markdown_document_map(Some(&markdown_map));
    input.is_active = false;
    let model = project(&input);

    assert_eq!(model.line_projections[0].display_text, "Heading");
    assert_eq!(model.line_projections[1].display_text, "• ✅ done");
    assert!(
        model
            .markdown_style_ranges
            .iter()
            .any(|range| range.row == 0
                && range.style.fg == Some(ResolvedThemeColor("#9ece6a".to_string()))
                && range.style.bold),
        "heading should stay semantically rendered when ui and syntax theme are also configured"
    );
    assert!(
        model
            .markdown_style_ranges
            .iter()
            .any(|range| range.row == 2
                && range.style.fg == Some(ResolvedThemeColor("#ff9e64".to_string()))),
        "inline code should keep markdown styling with the combined theme"
    );

    std::fs::remove_file(&target_path).expect("remove target");
    std::fs::remove_file(&config_path).expect("remove config");
}

#[test]
fn startup_markdown_projection_parses_repository_agents_md_headings() {
    let _lock = support::session::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("AGENTS.md");

    let outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::File(target_path),
        config_source: ConfigSource::Default,
        ..LaunchRequest::default()
    })
    .expect("startup with repository AGENTS.md");

    let markdown_map = MarkdownDocumentMap::parse(&outcome.core_bridge.snapshot().text);
    assert!(
        markdown_map.blocks.len() >= 2,
        "repository AGENTS.md should expose Markdown heading blocks, got blocks={:?}",
        markdown_map.blocks
    );
}

#[test]
fn startup_typescript_config_heading_level_can_disable_inherited_bold() {
    let _lock = support::session::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("theme-bold-false-target.md");
    let config_path = unique_path("theme-bold-false-init.ts");
    let markdown_source = "## Heading\n";
    std::fs::write(&target_path, markdown_source).expect("target file");
    std::fs::write(
        &config_path,
        r##"
            saya.theme.palette = {
                accent: "#7aa2f7",
                heading2: "#9ece6a",
            };
            saya.theme.markdown = {
                heading: { fg: "accent", bold: true },
                heading2: { fg: "heading2", bold: false, underline: true },
            };
        "##,
    )
    .expect("config file");

    let outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::File(target_path.clone()),
        config_source: ConfigSource::File(config_path.clone()),
        ..LaunchRequest::default()
    })
    .expect("startup with typescript theme config");

    let markdown_map = MarkdownDocumentMap::parse(markdown_source);
    let session_state = outcome.editor_session_state();
    let mut input = ProjectionInput::new(&outcome.initial_snapshot, &session_state, None)
        .with_markdown_document_map(Some(&markdown_map));
    input.is_active = false;
    let model = project(&input);

    let heading = model
        .markdown_style_ranges
        .iter()
        .find(|range| range.row == 0)
        .expect("heading2 range should be projected");
    assert_eq!(
        heading.style.fg,
        Some(ResolvedThemeColor("#9ece6a".to_string()))
    );
    assert!(
        !heading.style.bold,
        "heading2 bold=false should override inherited heading bold=true"
    );
    assert!(heading.style.underline);

    std::fs::remove_file(&target_path).expect("remove target");
    std::fs::remove_file(&config_path).expect("remove config");
}

#[test]
fn startup_typescript_config_applies_heading_bold_to_heading1() {
    let _lock = support::session::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("theme-heading1-target.md");
    let config_path = unique_path("theme-heading1-init.ts");
    let markdown_source = "# AGENTS.md\n";
    std::fs::write(&target_path, markdown_source).expect("target file");
    std::fs::write(
        &config_path,
        r##"
            saya.theme.palette = {
                accent: "#7aa2f7",
            };
            saya.theme.markdown = {
                heading: { fg: "accent", bold: true },
            };
        "##,
    )
    .expect("config file");

    let outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::File(target_path.clone()),
        config_source: ConfigSource::File(config_path.clone()),
        ..LaunchRequest::default()
    })
    .expect("startup with typescript theme config");

    let markdown_map = MarkdownDocumentMap::parse(markdown_source);
    let session_state = outcome.editor_session_state();
    let mut input = ProjectionInput::new(&outcome.initial_snapshot, &session_state, None)
        .with_markdown_document_map(Some(&markdown_map));
    input.is_active = false;
    let model = project(&input);

    let heading = model
        .markdown_style_ranges
        .iter()
        .find(|range| range.row == 0)
        .expect("heading1 range should be projected");
    assert_eq!(
        heading.style.fg,
        Some(ResolvedThemeColor("#7aa2f7".to_string()))
    );
    assert!(
        heading.style.bold,
        "heading1 should inherit bold from heading"
    );

    std::fs::remove_file(&target_path).expect("remove target");
    std::fs::remove_file(&config_path).expect("remove config");
}

#[test]
fn startup_typescript_config_applies_heading_bold_to_active_raw_heading1() {
    let _lock = support::session::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("theme-active-heading1-target.md");
    let config_path = unique_path("theme-active-heading1-init.ts");
    let markdown_source = "# AGENTS.md\n\nbody\n";
    std::fs::write(&target_path, markdown_source).expect("target file");
    std::fs::write(
        &config_path,
        r##"
            saya.theme.palette = {
                accent: "#7aa2f7",
            };
            saya.theme.markdown = {
                heading: { fg: "accent", bold: true },
            };
        "##,
    )
    .expect("config file");

    let outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::File(target_path.clone()),
        config_source: ConfigSource::File(config_path.clone()),
        ..LaunchRequest::default()
    })
    .expect("startup with typescript theme config");

    let markdown_map = MarkdownDocumentMap::parse(markdown_source);
    let session_state = outcome.editor_session_state();
    let input = ProjectionInput::new(&outcome.initial_snapshot, &session_state, None)
        .with_markdown_document_map(Some(&markdown_map));
    let model = project(&input);

    assert_eq!(model.line_projections[0].display_text, "# AGENTS.md");
    let heading = model
        .markdown_style_ranges
        .iter()
        .find(|range| range.row == 0)
        .expect("active raw heading1 range should be projected");
    assert!(
        heading.start_col == 0 && heading.end_col_exclusive > 0,
        "active raw heading1 should keep a visible style range"
    );
    assert_eq!(
        heading.style.fg,
        Some(ResolvedThemeColor("#7aa2f7".to_string()))
    );
    assert!(
        heading.style.bold,
        "active raw heading1 should inherit bold from heading"
    );

    std::fs::remove_file(&target_path).expect("remove target");
    std::fs::remove_file(&config_path).expect("remove config");
}

#[test]
fn startup_config_failure_keeps_default_session_and_presentation_state() {
    let _lock = support::session::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("target-fallback.txt");
    let config_path = unique_path("missing-init.ts");
    std::fs::write(&target_path, "alpha\nbeta\n").expect("target file");

    let outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::File(target_path.clone()),
        config_source: ConfigSource::File(config_path.clone()),
        ..LaunchRequest::default()
    })
    .expect("startup should continue with default fallback");

    assert_eq!(outcome.initial_tab_size, 8);
    assert!(!outcome.initial_line_numbers);
    assert_eq!(outcome.initial_number_width, 4);
    assert!(outcome.warnings.iter().any(|warning| matches!(
        warning,
        saya::app::bootstrap::BootstrapWarning::ConfigLoadFailed { path, .. } if path == &config_path
    )));

    let session_state = outcome.editor_session_state();
    let model = project(&ProjectionInput::new(
        &outcome.initial_snapshot,
        &session_state,
        None,
    ));

    assert_eq!(session_state.tab_size(), 8);
    assert!(!session_state.line_numbers());
    assert_eq!(session_state.number_width(), 4);
    assert_eq!(model.lines[0], "alpha");
    assert_eq!(model.lines[1], "beta");

    std::fs::remove_file(&target_path).expect("remove target");
}

struct RecordingHostBridge {
    executed_commands: Arc<Mutex<Vec<String>>>,
    read_buffers: Arc<Mutex<Vec<ReadonlyBufferSnapshot>>>,
}

impl RecordingHostBridge {
    fn new() -> Self {
        Self {
            executed_commands: Arc::new(Mutex::new(Vec::new())),
            read_buffers: Arc::new(Mutex::new(Vec::new())),
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
        let read_buffers = self.read_buffers.clone();
        Box::pin(async move {
            let snapshot = ReadonlyBufferSnapshot {
                id: 99,
                path: Some(PathBuf::from("runtime.md")),
                line_count: 2,
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

#[derive(Clone, Debug, PartialEq, Eq)]
enum RuntimeObservation {
    EventBufferOpened { buffer_id: u64 },
    CommandReadBuffer { path: Option<PathBuf> },
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
                    .push(RuntimeObservation::CommandReadBuffer { path: buffer.path });
                ctx.commands().execute("write").await
            })
        })
        .on_buffer_open(move |ctx, payload| {
            let observed_in_event = observed_in_event.clone();
            Box::pin(async move {
                observed_in_event
                    .lock()
                    .await
                    .push(RuntimeObservation::EventBufferOpened {
                        buffer_id: payload.buffer.id,
                    });
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
        observed.lock().await.clone(),
        vec![
            RuntimeObservation::EventBufferOpened { buffer_id: 7 },
            RuntimeObservation::CommandReadBuffer {
                path: Some(PathBuf::from("runtime.md")),
            },
        ]
    );
    assert_eq!(
        host_bridge.read_buffers.lock().await.clone(),
        vec![ReadonlyBufferSnapshot {
            id: 99,
            path: Some(PathBuf::from("runtime.md")),
            line_count: 2,
            cursor_row: 0,
            cursor_col: 0,
            current_line: String::new(),
            text: String::new(),
        }]
    );
    assert_eq!(
        host_bridge.executed_commands.lock().await.clone(),
        vec!["write".to_string()]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_surface_is_frozen_and_does_not_expose_registration_apis() {
    let host_bridge = Arc::new(RecordingHostBridge::new());
    let seed = CallbackRegistrySeed::from_startup_entries(vec![
        saya::runtime::config::StartupRegistryEntry::Event {
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
        },
    ]);

    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge, seed)
        .expect("runtime should initialize with frozen public surface");

    let report = runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: ReadonlyBufferSnapshot {
                id: 101,
                path: Some(PathBuf::from("surface.md")),
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

    assert_eq!(report.handler_count, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_callback_failure_projects_as_message_without_corrupting_session_state() {
    let _lock = support::session::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("runtime-failure.txt");
    let config_path = unique_path("runtime-failure-init.ts");
    std::fs::write(&target_path, "alpha\nbeta\n").expect("target file");
    std::fs::write(
        &config_path,
        r#"
            saya.events.on("bufferOpen", () => {
                throw new Error("boom");
            });
        "#,
    )
    .expect("config file");

    let outcome =
        with_isolated_lazy_plugin_event("runtime-failure-plugin-cache", "bufferOpen", || {
            prepare_launch(LaunchRequest {
                input_source: InputSource::File(target_path.clone()),
                config_source: ConfigSource::File(config_path.clone()),
                ..LaunchRequest::default()
            })
        })
        .expect("startup should register failing runtime callback");

    assert_eq!(outcome.callback_registry.events().len(), 1);
    let session_state = outcome.editor_session_state();
    let before_dirty = session_state.is_dirty();
    let before_save_error = session_state.last_save_error().map(ToString::to_string);

    let host_bridge = Arc::new(RecordingHostBridge::new());
    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge, outcome.callback_registry)
        .expect("seed runtime should initialize");

    let error = runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: ReadonlyBufferSnapshot {
                id: 88,
                path: Some(PathBuf::from("runtime-failure.md")),
                line_count: 2,
                cursor_row: 0,
                cursor_col: 0,
                current_line: String::new(),
                text: String::new(),
            },
        }))
        .expect("dispatch queued")
        .await_result()
        .await
        .expect_err("callback failure should surface as a dispatch error");

    let message = runtime_callback_failure_message(&error)
        .expect("callback failure should translate to an application message");

    let model = project(&ProjectionInput::new(
        &outcome.initial_snapshot,
        &session_state,
        Some(message.as_str()),
    ));

    assert!(
        message.contains("boom"),
        "application message should preserve the runtime failure text: {message}"
    );
    assert_eq!(model.message_line, Some(message));
    assert_eq!(session_state.is_dirty(), before_dirty);
    assert_eq!(
        session_state.last_save_error().map(ToString::to_string),
        before_save_error
    );
    assert_eq!(model.dirty, before_dirty);

    std::fs::remove_file(&target_path).expect("remove target");
    std::fs::remove_file(&config_path).expect("remove config");
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_callback_completion_requests_projection_refresh_after_host_save() {
    let _lock = support::session::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("runtime-refresh-target.txt");
    let config_path = unique_path("runtime-refresh-init.ts");
    std::fs::write(&target_path, "alpha\nbeta\n").expect("target file");
    std::fs::write(
        &config_path,
        r#"
            saya.commands.register("writeCurrent", () => {
                saya.commands.execute("write");
            });
            saya.events.on("bufferOpen", () => {
                return saya.commands.execute("writeCurrent");
            });
        "#,
    )
    .expect("config file");

    let mut outcome =
        with_isolated_lazy_plugin_event("runtime-refresh-plugin-cache", "bufferOpen", || {
            prepare_launch(LaunchRequest {
                input_source: InputSource::File(target_path.clone()),
                config_source: ConfigSource::File(config_path.clone()),
                ..LaunchRequest::default()
            })
        })
        .expect("startup should register runtime callback");

    assert_eq!(outcome.callback_registry.commands().len(), 1);
    assert_eq!(outcome.callback_registry.events().len(), 1);

    outcome.core_bridge.dispatch_key("i").unwrap();
    outcome.core_bridge.dispatch_key("X").unwrap();
    outcome.core_bridge.dispatch_key("\x1b").unwrap();

    let mut session_state = EditorSessionState::new(outcome.target_path.clone());
    session_state.update_dirty(outcome.core_bridge.snapshot().dirty);
    let before_model = saya::presentation::screen_model::project(
        &saya::presentation::screen_model::ProjectionInput::new(
            &outcome.core_bridge.snapshot(),
            &session_state,
            None,
        ),
    );
    assert!(before_model.dirty);

    let host_bridge = Arc::new(RecordingHostBridge::new());
    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), outcome.callback_registry)
        .expect("seed runtime should initialize");
    let report = runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: ReadonlyBufferSnapshot {
                id: 77,
                path: Some(target_path.clone()),
                line_count: 2,
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
    assert!(
        runtime_dispatch_requests_redraw(&report),
        "runtime callback completion should request a host refresh"
    );
    assert_eq!(
        host_bridge.executed_commands.lock().await.clone(),
        vec!["write".to_string()]
    );

    let snapshot = outcome.core_bridge.snapshot();
    let request = session_state
        .build_save_request(&snapshot.text)
        .expect("host should build save request after callback completion");
    assert_eq!(write_to_path(&request), SaveResult::Saved);
    session_state.record_save_success();

    let transient_message = Some("Saved successfully");
    let after_model = saya::presentation::screen_model::project(
        &saya::presentation::screen_model::ProjectionInput::new(
            &snapshot,
            &session_state,
            transient_message,
        ),
    );
    assert_eq!(
        after_model.message_line,
        Some("Saved successfully".to_string())
    );
    assert_ne!(before_model.message_line, after_model.message_line);
    assert!(!session_state.is_dirty());

    std::fs::remove_file(&target_path).expect("remove target");
    std::fs::remove_file(&config_path).expect("remove config");
}
