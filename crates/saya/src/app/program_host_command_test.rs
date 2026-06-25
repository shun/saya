use super::program_test_support::*;
use super::*;

#[test]
fn parse_main_host_command_recognizes_save_and_quit_family_commands() {
    assert_eq!(parse_main_host_command(":w"), Some(MainHostCommand::Save));
    assert_eq!(
        parse_main_host_command("write"),
        Some(MainHostCommand::Save)
    );
    assert_eq!(
        parse_main_host_command(":wq"),
        Some(MainHostCommand::SaveThenQuit)
    );
    assert_eq!(
        parse_main_host_command("wq"),
        Some(MainHostCommand::SaveThenQuit)
    );
    assert_eq!(
        parse_main_host_command("exit"),
        Some(MainHostCommand::SaveThenQuit)
    );
    assert_eq!(
        parse_main_host_command("edit /tmp/project"),
        Some(MainHostCommand::Edit(std::path::PathBuf::from(
            "/tmp/project"
        )))
    );
    assert_eq!(
        parse_main_host_command("markdown.previewMermaid"),
        Some(MainHostCommand::MarkdownPreviewMermaid)
    );
    assert_eq!(
        parse_main_host_command(r#"lsp.floatHover {"result":{"contents":"hover"}}"#),
        Some(MainHostCommand::LspHoverFloat(
            r#"{"result":{"contents":"hover"}}"#.to_string()
        ))
    );
    assert_eq!(
        parse_main_host_command(r#"lsp.floatDiagnostics {"diagnostics":[]}"#),
        Some(MainHostCommand::LspDiagnosticFloat(
            r#"{"diagnostics":[]}"#.to_string()
        ))
    );
    assert_eq!(
        parse_main_host_command(r#"lsp.nextDiagnostic {"ui":{"width":40,"height":8}}"#),
        Some(MainHostCommand::LspNextDiagnostic(Some(
            r#"{"ui":{"width":40,"height":8}}"#.to_string()
        )))
    );
    assert_eq!(
        parse_main_host_command(r#"lsp.previewWorkspaceEdit {"title":"Rename"}"#),
        Some(MainHostCommand::LspWorkspaceEditPreview(
            r#"{"title":"Rename"}"#.to_string()
        ))
    );
    assert_eq!(
        parse_main_host_command(r#"lsp.floatCodeActions {"response":{"result":[]}}"#),
        Some(MainHostCommand::LspCodeActionsFloat(
            r#"{"response":{"result":[]}}"#.to_string()
        ))
    );
    assert_eq!(
        parse_main_host_command(r#"buffer.floatWindow {"width":20}"#),
        Some(MainHostCommand::BufferWindowFloat(
            r#"{"width":20}"#.to_string()
        ))
    );
    assert_eq!(
        parse_main_host_command(r#"terminal.float {"command":"sh"}"#),
        Some(MainHostCommand::TerminalFloat(
            r#"{"command":"sh"}"#.to_string()
        ))
    );
    assert_eq!(
        parse_main_host_command(r#"terminal.closeFloat {"terminalId":1}"#),
        Some(MainHostCommand::TerminalCloseFloat(
            r#"{"terminalId":1}"#.to_string()
        ))
    );
    assert_eq!(
        parse_main_host_command(r#"completion.floatMenu {"candidates":["alpha"]}"#),
        None
    );
    assert_eq!(parse_main_host_command("set number"), None);
}

#[test]
fn markdown_preview_mermaid_host_command_requests_manual_preview() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();

    let effect = execute_runtime_host_command_with_floats(
        "markdown.previewMermaid",
        &mut outcome,
        &mut session_state,
        None,
        None,
        None,
        None,
    )
    .expect("manual Mermaid preview command should be accepted");

    assert_eq!(
        effect.transient_message.as_deref(),
        Some("Mermaid preview requested")
    );
    assert!(session_state.mermaid_preview_manual_active());
}

#[tokio::test(flavor = "current_thread")]
async fn startup_keymap_builtin_mermaid_preview_command_falls_back_to_host_command() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
        .expect("runtime session should initialize");
    let mut transient_msg = None;
    let mut need_redraw = false;
    let mut runtime_presentation_intents = Vec::new();
    let mut floating_window_manager = FloatingWindowManager::default();
    let mut completion_float_manager = CompletionFloatManager::default();
    let mut lsp_diagnostic_store = LspDiagnosticStore::default();
    let mut terminal_float_manager = TerminalFloatManager::default();
    let mut panel_manager = PanelManager::default();

    let shutdown = execute_startup_keymap_registered_command(
        Some(&mut runtime_session),
        "markdown.previewMermaid",
        &mut outcome,
        &mut session_state,
        &mut floating_window_manager,
        &mut completion_float_manager,
        &mut lsp_diagnostic_store,
        &mut terminal_float_manager,
        &mut panel_manager,
        None,
        &mut transient_msg,
        &mut need_redraw,
        &mut runtime_presentation_intents,
        None,
    )
    .await;

    assert_eq!(shutdown, None);
    assert_eq!(transient_msg.as_deref(), Some("Mermaid preview requested"));
    assert!(need_redraw);
    assert!(session_state.mermaid_preview_manual_active());
}

#[test]
fn runtime_lsp_hover_float_host_command_opens_replacing_cursor_relative_float() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    let mut floating_window_manager = FloatingWindowManager::default();

    execute_runtime_host_command_with_floats(
        r#"lsp.floatHover {"result":{"contents":"old hover"}}"#,
        &mut outcome,
        &mut session_state,
        Some(&mut floating_window_manager),
        None,
        None,
        None,
    )
    .expect("first hover should open");
    execute_runtime_host_command_with_floats(
        r#"lsp.floatHover {"result":{"contents":"new hover"}}"#,
        &mut outcome,
        &mut session_state,
        Some(&mut floating_window_manager),
        None,
        None,
        None,
    )
    .expect("second hover should replace first");

    let active_window_id = outcome
        .core_bridge
        .light_snapshot()
        .active_window_id()
        .unwrap_or(1);
    let floats = floating_window_manager.resolve_screen_models_with_cursors(
        80,
        24,
        &[(
            active_window_id,
            crate::presentation::screen_model::PaneRect {
                x: 0,
                y: 0,
                width: 80,
                height: 24,
            },
        )],
        &[(active_window_id, 0, 0)],
        Some(active_window_id),
    );
    assert_eq!(floats.len(), 1);
    assert_eq!(floats[0].lines, vec!["new hover"]);
}

#[test]
fn lsp_popup_size_percent_resolves_against_window_by_default() {
    let context = PopupSizingContext {
        terminal_width: 100,
        terminal_height: 40,
        parent_window_rect: crate::presentation::screen_model::PaneRect {
            x: 0,
            y: 0,
            width: 60,
            height: 20,
        },
    };

    let limit = resolve_lsp_popup_size_limit(
        PopupSizeSpec {
            width: PopupSizeValue::Percent(50),
            height: PopupSizeValue::Percent(50),
            basis: default_lsp_popup_basis(LspPopupKind::Hover),
        },
        &context,
    );

    assert_eq!(
        limit,
        ResolvedPopupSizeLimit {
            max_width: 30,
            max_height: 10,
        }
    );
}

#[test]
fn lsp_popup_size_percent_can_resolve_against_editor_grid() {
    let context = PopupSizingContext {
        terminal_width: 100,
        terminal_height: 40,
        parent_window_rect: crate::presentation::screen_model::PaneRect {
            x: 0,
            y: 0,
            width: 60,
            height: 20,
        },
    };

    let limit = resolve_lsp_popup_size_limit(
        PopupSizeSpec {
            width: PopupSizeValue::Percent(50),
            height: PopupSizeValue::Percent(50),
            basis: PopupSizeBasis::Editor,
        },
        &context,
    );

    assert_eq!(
        limit,
        ResolvedPopupSizeLimit {
            max_width: 50,
            max_height: 20,
        }
    );
}

#[test]
fn lsp_popup_size_payload_rejects_invalid_percentages() {
    let error = parse_lsp_popup_size_spec(
        Some(&serde_json::json!({ "width": "0%", "height": "101%" })),
        PopupSizeBasis::Window,
        "lsp.floatHover.ui",
    )
    .expect_err("invalid percentage should fail");

    assert!(
        error.contains("percentage string from 1% through 100%"),
        "error should explain percentage bounds: {error}"
    );
}

#[test]
fn lsp_hover_payload_kind_accepts_signature_help_for_dedicated_ui_size() {
    assert_eq!(
        parse_lsp_hover_popup_kind(&serde_json::json!({ "kind": "signatureHelp" }))
            .expect("signatureHelp kind should be accepted"),
        LspHoverPopupKind::SignatureHelp
    );
    assert!(
        parse_lsp_hover_popup_kind(&serde_json::json!({ "kind": "typo" })).is_err(),
        "unknown hover kind should be rejected"
    );
}

#[test]
fn runtime_lsp_hover_any_prefers_diagnostic_at_cursor() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    let mut floating_window_manager = FloatingWindowManager::default();
    let mut lsp_diagnostic_store = LspDiagnosticStore::default();

    execute_runtime_host_command_with_floats(
        r#"lsp.publishDiagnostics {"params":{"uri":"file:///Users/skudo/.config/saya/init.ts","diagnostics":[{"severity":1,"message":"Property 'lineNumber' does not exist on type 'SayaStartupOptionsSurface'.","range":{"start":{"line":0,"character":0},"end":{"line":0,"character":10}}}]}}"#,
        &mut outcome,
        &mut session_state,
        Some(&mut floating_window_manager),
        None,
        Some(&mut lsp_diagnostic_store),
        None,
    )
    .expect("diagnostics should publish");

    execute_runtime_host_command_with_floats(
        r#"lsp.floatHover {"response":{"source":"lsp","method":"textDocument/hover","result":{"contents":"\n```typescript\nany\n```\n","range":{"start":{"line":0,"character":0},"end":{"line":0,"character":10}}}}}"#,
        &mut outcome,
        &mut session_state,
        Some(&mut floating_window_manager),
        None,
        Some(&mut lsp_diagnostic_store),
        None,
    )
    .expect("hover should prefer diagnostic");

    let active_window_id = outcome
        .core_bridge
        .light_snapshot()
        .active_window_id()
        .unwrap_or(1);
    let floats = floating_window_manager.resolve_screen_models_with_cursors(
        80,
        24,
        &[(
            active_window_id,
            crate::presentation::screen_model::PaneRect {
                x: 0,
                y: 0,
                width: 80,
                height: 24,
            },
        )],
        &[(active_window_id, 3, 7)],
        Some(active_window_id),
    );

    assert_eq!(floats.len(), 1);
    assert!(
        floats[0].lines.join(" ").contains(
            "Error: Property 'lineNumber' does not exist on type 'SayaStartupOptionsSurface'."
        ),
        "diagnostic hover should render the TypeScript error: {:?}",
        floats[0].lines
    );
    assert_eq!((floats[0].rect.x, floats[0].rect.y), (7, 4));
}

#[test]
fn runtime_lsp_feature_host_commands_render_lists_and_navigate_definition() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("lsp-definition-target").with_extension("rs");
    std::fs::write(&target_path, "fn target() {}\nfn caller() {}\n")
        .expect("definition target should be written");
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    let mut floating_window_manager = FloatingWindowManager::default();

    execute_runtime_host_command_with_floats(
        &format!(
            r#"lsp.gotoDefinition {{"response":{{"result":{{"uri":"file://{}","range":{{"start":{{"line":1,"character":0}}}}}}}}}}"#,
            target_path.display()
        ),
        &mut outcome,
        &mut session_state,
        Some(&mut floating_window_manager),
        None,
        None,
        None,
    )
    .expect("definition navigation should apply");
    assert_eq!(session_state.target_path(), Some(&target_path));
    assert_eq!(outcome.core_bridge.light_snapshot().cursor_row, 1);

    execute_runtime_host_command_with_floats(
        r#"lsp.floatLocations {"title":"References","response":{"result":[{"uri":"file:///workspace/src/main.rs","range":{"start":{"line":4,"character":1}}},{"uri":"file:///workspace/src/lib.rs","range":{"start":{"line":9,"character":3}}}]}}"#,
        &mut outcome,
        &mut session_state,
        Some(&mut floating_window_manager),
        None,
        None,
        None,
    )
    .expect("references list should open");
    execute_runtime_host_command_with_floats(
        r#"lsp.floatSymbols {"response":{"result":[{"name":"main","kind":12,"range":{"start":{"line":0,"character":0}},"children":[{"name":"child","kind":6,"range":{"start":{"line":2,"character":2}}}]}]}}"#,
        &mut outcome,
        &mut session_state,
        Some(&mut floating_window_manager),
        None,
        None,
        None,
    )
    .expect("symbol outline should open");
    execute_runtime_host_command_with_floats(
        r#"lsp.previewWorkspaceEdit {"title":"Rename preview","response":{"result":{"changes":{"file:///workspace/src/main.rs":[{"range":{"start":{"line":4,"character":1},"end":{"line":4,"character":5}},"newText":"renamed"}]}}}}"#,
        &mut outcome,
        &mut session_state,
        Some(&mut floating_window_manager),
        None,
        None,
        None,
    )
    .expect("workspace edit preview should open");
    execute_runtime_host_command_with_floats(
        r#"lsp.floatCodeActions {"response":{"result":[{"title":"Organize Imports","kind":"source.organizeImports"},{"title":"Fix issue","kind":"quickfix"}]}}"#,
        &mut outcome,
        &mut session_state,
        Some(&mut floating_window_manager),
        None,
        None,
        None,
    )
    .expect("code action float should open");

    let active_window_id = outcome
        .core_bridge
        .light_snapshot()
        .active_window_id()
        .unwrap_or(1);
    let floats = floating_window_manager.resolve_screen_models(
        100,
        30,
        &[(
            active_window_id,
            crate::presentation::screen_model::PaneRect {
                x: 0,
                y: 0,
                width: 100,
                height: 30,
            },
        )],
        Some(active_window_id),
    );
    let rendered_lines = floats
        .iter()
        .flat_map(|float| float.lines.iter().cloned())
        .collect::<Vec<_>>();
    assert!(
        rendered_lines
            .iter()
            .any(|line| line.contains("src/main.rs:5:2")),
        "references should render line and column in a headless float: {rendered_lines:?}"
    );
    assert!(
        rendered_lines
            .iter()
            .any(|line| line.contains("main") && line.contains("Function")),
        "document symbols should render a selectable outline: {rendered_lines:?}"
    );
    assert!(
        rendered_lines.iter().any(|line| line.contains("child")),
        "nested document symbols should be rendered: {rendered_lines:?}"
    );
    assert!(
        rendered_lines
            .iter()
            .any(|line| line.contains("source.organizeImports: Organize Imports")),
        "code actions should render actionable titles: {rendered_lines:?}"
    );

    std::fs::remove_file(target_path).expect("cleanup definition target");
}

#[test]
fn runtime_lsp_diagnostics_publish_and_cycle_open_diagnostic_floats() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    let mut floating_window_manager = FloatingWindowManager::default();
    let mut lsp_diagnostic_store = LspDiagnosticStore::default();

    execute_runtime_host_command_with_floats(
        r#"lsp.publishDiagnostics {"params":{"uri":"file:///workspace/src/main.rs","diagnostics":[{"severity":1,"message":"first error","range":{"start":{"line":2,"character":4}}},{"severity":2,"message":"second warning","range":{"start":{"line":5,"character":1}}}]}}"#,
        &mut outcome,
        &mut session_state,
        Some(&mut floating_window_manager),
        None,
        Some(&mut lsp_diagnostic_store),
        None,
    )
    .expect("diagnostics should publish");
    assert!(!lsp_diagnostic_store.is_empty());
    assert_eq!(
        floating_window_manager
            .resolve_screen_models(80, 24, &[], None)
            .len(),
        0,
        "publishing diagnostics should update the store without stealing focus"
    );

    execute_runtime_host_command_with_floats(
        "lsp.nextDiagnostic",
        &mut outcome,
        &mut session_state,
        Some(&mut floating_window_manager),
        None,
        Some(&mut lsp_diagnostic_store),
        None,
    )
    .expect("next diagnostic should open");
    execute_runtime_host_command_with_floats(
        "lsp.nextDiagnostic",
        &mut outcome,
        &mut session_state,
        Some(&mut floating_window_manager),
        None,
        Some(&mut lsp_diagnostic_store),
        None,
    )
    .expect("next diagnostic should cycle");

    let active_window_id = outcome
        .core_bridge
        .light_snapshot()
        .active_window_id()
        .unwrap_or(1);
    let floats = floating_window_manager.resolve_screen_models(
        80,
        24,
        &[(
            active_window_id,
            crate::presentation::screen_model::PaneRect {
                x: 0,
                y: 0,
                width: 80,
                height: 24,
            },
        )],
        Some(active_window_id),
    );
    assert_eq!(floats.len(), 1);
    assert_eq!(floats[0].lines, vec!["Warning: second warning"]);
    assert!(matches!(
        floating_window_manager
            .debug_window(floats[0].id)
            .expect("diagnostic float should exist")
            .placement
            .relative_to,
        FloatingRelativeTo::BufferPosition {
            line: 5,
            column: 1,
            ..
        }
    ));
}

#[test]
fn runtime_typed_completion_accept_applies_replace_range_insert_text() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    outcome
        .core_bridge
        .replace_buffer_text("pri\n")
        .expect("seed buffer text");
    outcome
        .core_bridge
        .dispatch_key("A")
        .expect("enter insert mode at line end");
    let mut floating_window_manager = FloatingWindowManager::default();
    let mut completion_float_manager = CompletionFloatManager::default();

    assert!(completion_float_manager.show_typed(
        &mut floating_window_manager,
        1,
        0,
        0,
        CompletionShowRequest {
            session_id: "test-session".to_string(),
            request_id: 1,
            replace_range: crate::features::completion::session::CompletionRange {
                start: crate::features::completion::session::CompletionPosition {
                    line: 0,
                    character: 0,
                },
                end: crate::features::completion::session::CompletionPosition {
                    line: 0,
                    character: 3,
                },
            },
            candidates: vec![
                crate::features::completion::session::HostCompletionCandidate {
                    label: "println!".to_string(),
                    insert_text: Some("println!($0);".to_string()),
                    kind: Some("Function".to_string()),
                    detail: Some("macro".to_string()),
                    documentation: Vec::new(),
                    source: Some("rust-analyzer".to_string()),
                    metadata: None,
                },
            ],
            selected_index: 0,
            max_visible_items: 8,
            documentation_max_width: 72,
            documentation_max_height: 12,
            keys: Some(
                crate::features::completion::session::CompletionKeyBindingsRequest {
                    confirm: Some(vec!["<Enter>".to_string()]),
                    close: None,
                    next: None,
                    previous: None,
                    page_next: None,
                    page_previous: None,
                }
            ),
        },
    ));
    let active_window_id = outcome
        .core_bridge
        .light_snapshot()
        .active_window_id()
        .unwrap_or(1);
    let _menu_id = floating_window_manager
        .windows()
        .iter()
        .find(|window| {
            matches!(
                window.content,
                crate::presentation::floating_window::FloatingContentRef::CompletionMenu { .. }
            )
        })
        .map(|window| window.id)
        .expect("completion menu should exist");
    assert_eq!(floating_window_manager.focused_float_id(), None);

    assert!(matches!(
        handle_completion_float_key(
            &mut completion_float_manager,
            &mut floating_window_manager,
            &mut outcome.core_bridge,
            &KeyInput::Enter,
            active_window_id,
        ),
        Some(FloatingWindowKeyHandling::Closed { .. })
    ));
    assert_eq!(outcome.core_bridge.buffer_text(), "println!($0);\n");
    let snapshot = outcome.core_bridge.light_snapshot();
    assert_eq!(snapshot.mode, vim_core_rs::CoreMode::Insert);
    assert_eq!(snapshot.cursor_row, 0);
    assert_eq!(
        snapshot.cursor_col,
        "println!($0);".len(),
        "typed completion confirmation should move the insert cursor to the replacement end"
    );
}

#[test]
fn runtime_typed_completion_accept_applies_lsp_additional_text_edits() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    outcome
        .core_bridge
        .replace_buffer_text("package main\n\nfunc main() {\n\tlog.Pri\n}\n")
        .expect("seed buffer text");
    outcome
        .core_bridge
        .dispatch_key("GkA")
        .expect("enter insert mode at completion line end");
    let mut floating_window_manager = FloatingWindowManager::default();
    let mut completion_float_manager = CompletionFloatManager::default();

    assert!(completion_float_manager.show_typed(
        &mut floating_window_manager,
        1,
        3,
        8,
        CompletionShowRequest {
            session_id: "test-session".to_string(),
            request_id: 1,
            replace_range: crate::features::completion::session::CompletionRange {
                start: crate::features::completion::session::CompletionPosition {
                    line: 3,
                    character: 5,
                },
                end: crate::features::completion::session::CompletionPosition {
                    line: 3,
                    character: 8,
                },
            },
            candidates: vec![
                crate::features::completion::session::HostCompletionCandidate {
                    label: "Printf".to_string(),
                    insert_text: Some("Printf".to_string()),
                    kind: Some("Function".to_string()),
                    detail: Some("func(format string, v ...any)".to_string()),
                    documentation: Vec::new(),
                    source: Some("lsp".to_string()),
                    metadata: Some(serde_json::json!({
                        "additionalTextEdits": [{
                            "range": {
                                "start": { "line": 2, "character": 0 },
                                "end": { "line": 2, "character": 0 }
                            },
                            "newText": "import \"log\"\n\n"
                        }]
                    })),
                },
            ],
            selected_index: 0,
            max_visible_items: 8,
            documentation_max_width: 72,
            documentation_max_height: 12,
            keys: Some(
                crate::features::completion::session::CompletionKeyBindingsRequest {
                    confirm: Some(vec!["<Enter>".to_string()]),
                    close: None,
                    next: None,
                    previous: None,
                    page_next: None,
                    page_previous: None,
                }
            ),
        },
    ));
    let active_window_id = outcome
        .core_bridge
        .light_snapshot()
        .active_window_id()
        .unwrap_or(1);
    assert!(matches!(
        handle_completion_float_key(
            &mut completion_float_manager,
            &mut floating_window_manager,
            &mut outcome.core_bridge,
            &KeyInput::Enter,
            active_window_id,
        ),
        Some(FloatingWindowKeyHandling::Closed { .. })
    ));
    assert_eq!(
        outcome.core_bridge.buffer_text(),
        "package main\n\nimport \"log\"\n\nfunc main() {\n\tlog.Printf\n}\n"
    );
    let snapshot = outcome.core_bridge.light_snapshot();
    assert_eq!(snapshot.mode, vim_core_rs::CoreMode::Insert);
    assert_eq!(snapshot.cursor_row, 5);
    assert_eq!(snapshot.cursor_col, "\tlog.Printf".len());
}

#[tokio::test(flavor = "current_thread")]
async fn startup_completion_keymap_opens_pum_and_enter_confirms_candidate() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("completion-keymap-target").with_extension("txt");
    let config_path = unique_path("completion-keymap-init").with_extension("ts");
    let completion_path =
        crate::support::paths::dev_ts_plugins_dir().join("bundled/completion/index.ts");
    std::fs::write(&target_path, "ty\ntype\n").expect("target file");
    std::fs::write(
        &config_path,
        format!(
            r#"
                import {{ createBufferWordSource, setupSayaCompletion }} from "{}";
                setupSayaCompletion({{
                    key: "<C-x>",
                    keys: {{ confirm: ["<Enter>"] }},
                    minPrefixLength: 2,
                    sourceTimeoutMs: 0,
                    sources: [createBufferWordSource()],
                }});
            "#,
            completion_path.to_string_lossy()
        ),
    )
    .expect("config file");

    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::File(target_path.clone()),
        config_source: crate::app::cli::ConfigSource::File(config_path.clone()),
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
        .expect("runtime session should initialize");
    let mut floating_window_manager = FloatingWindowManager::default();
    let mut completion_float_manager = CompletionFloatManager::default();
    let mut lsp_diagnostic_store = LspDiagnosticStore::default();
    let mut terminal_float_manager = TerminalFloatManager::default();
    let mut panel_manager = PanelManager::default();
    let mut transient_msg = None;
    let mut need_redraw = false;
    let mut runtime_presentation_intents = Vec::new();

    outcome.core_bridge.dispatch_key("A").expect("enter insert");
    // ADR 0006 Phase 2/3: legacy wrapper を廃止し、下位純粋関数で単キー解決する
    // （本番スモーク `run_binary_completion_smoke` と同一経路）。
    let action = startup_keymap_action_for_input(
        &outcome.startup_registry.keymaps,
        outcome.core_bridge.mode(),
        &KeyInput::Ctrl('x'),
    )
    .unwrap_or_else(|| {
        panic!(
            "insert completion keymap should resolve; keymaps={:?}, mode={:?}, warnings={:?}",
            outcome.startup_registry.keymaps,
            outcome.core_bridge.mode(),
            outcome.warnings
        )
    });
    let StartupKeymapAction::RegisteredCommand(command_name) = action else {
        panic!("completion keymap should point at a registered command");
    };
    assert_eq!(command_name, "completion.trigger");

    let shutdown = execute_startup_keymap_registered_command(
        Some(&mut runtime_session),
        &command_name,
        &mut outcome,
        &mut session_state,
        &mut floating_window_manager,
        &mut completion_float_manager,
        &mut lsp_diagnostic_store,
        &mut terminal_float_manager,
        &mut panel_manager,
        None,
        &mut transient_msg,
        &mut need_redraw,
        &mut runtime_presentation_intents,
        None,
    )
    .await;
    assert_eq!(shutdown, None);
    assert_eq!(
        transient_msg, None,
        "dired startup command should not surface swap or pager messages"
    );
    assert!(
        floating_window_manager.windows().iter().any(|window| {
            matches!(
                window.content,
                crate::presentation::floating_window::FloatingContentRef::CompletionMenu { .. }
            )
        }),
        "completion trigger should open a completion menu"
    );
    assert_eq!(floating_window_manager.focused_float_id(), None);

    let active_window_id = outcome
        .core_bridge
        .light_snapshot()
        .active_window_id()
        .unwrap_or(1);
    assert!(matches!(
        handle_completion_float_key(
            &mut completion_float_manager,
            &mut floating_window_manager,
            &mut outcome.core_bridge,
            &KeyInput::Enter,
            active_window_id,
        ),
        Some(FloatingWindowKeyHandling::Closed { .. })
    ));
    assert_eq!(outcome.core_bridge.buffer_text(), "type\ntype\n");
    let snapshot = outcome.core_bridge.light_snapshot();
    assert_eq!(snapshot.cursor_row, 0);
    assert_eq!(
        snapshot.cursor_col, 4,
        "startup completion keymap confirmation should move the insert cursor after the candidate"
    );

    std::fs::remove_file(target_path).expect("cleanup target");
    std::fs::remove_file(config_path).expect("cleanup config");
}

#[tokio::test(flavor = "current_thread")]
async fn startup_completion_auto_trigger_opens_menu_from_buffer_changed_event() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("completion-auto-target").with_extension("txt");
    let config_path = unique_path("completion-auto-init").with_extension("ts");
    let completion_path =
        crate::support::paths::dev_ts_plugins_dir().join("bundled/completion/index.ts");
    std::fs::write(&target_path, "t\ntype\n").expect("target file");
    std::fs::write(
        &config_path,
        format!(
            r#"
                import {{ createBufferWordSource, setupSayaCompletion }} from "{}";
                setupSayaCompletion({{
                    key: "<C-x>",
                    autoTrigger: true,
                    autoTriggerDelayMs: 0,
                    sourceTimeoutMs: 0,
                    sources: [createBufferWordSource()],
                }});
            "#,
            completion_path.to_string_lossy()
        ),
    )
    .expect("config file");

    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::File(target_path.clone()),
        config_source: crate::app::cli::ConfigSource::File(config_path.clone()),
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
        .expect("runtime session should initialize");
    let mut floating_window_manager = FloatingWindowManager::default();
    let mut completion_float_manager = CompletionFloatManager::default();
    let mut lsp_diagnostic_store = LspDiagnosticStore::default();
    let mut terminal_float_manager = TerminalFloatManager::default();
    let mut panel_manager = PanelManager::default();
    let mut transient_msg = None;
    let mut need_redraw = false;
    let mut runtime_presentation_intents = Vec::new();

    outcome.core_bridge.dispatch_key("A").expect("enter insert");
    let shutdown = dispatch_buffer_changed_with_runtime(
        Some(&mut runtime_session),
        &mut outcome,
        &mut session_state,
        &mut transient_msg,
        &mut need_redraw,
        &mut runtime_presentation_intents,
        &mut floating_window_manager,
        &mut completion_float_manager,
        &mut lsp_diagnostic_store,
        &mut terminal_float_manager,
        &mut panel_manager,
        None,
    )
    .await;
    assert_eq!(shutdown, None);
    assert_eq!(
        transient_msg, None,
        "auto completion must not surface runtime callback errors"
    );
    assert!(need_redraw, "auto completion should request redraw");
    assert!(
        floating_window_manager.windows().iter().any(|window| {
            matches!(
                window.content,
                crate::presentation::floating_window::FloatingContentRef::CompletionMenu { .. }
            )
        }),
        "bufferChanged auto trigger should open a completion menu after one character"
    );
    let active_window_id = outcome
        .core_bridge
        .light_snapshot()
        .active_window_id()
        .unwrap_or(1);
    assert!(matches!(
        handle_completion_float_key(
            &mut completion_float_manager,
            &mut floating_window_manager,
            &mut outcome.core_bridge,
            &KeyInput::Escape,
            active_window_id,
        ),
        Some(FloatingWindowKeyHandling::Closed { .. })
    ));
    assert!(
        !floating_window_manager.windows().iter().any(|window| {
            matches!(
                window.content,
                crate::presentation::floating_window::FloatingContentRef::CompletionMenu { .. }
            )
        }),
        "Esc should close the completion menu before leaving insert mode"
    );
    assert_eq!(
        outcome.core_bridge.light_snapshot().mode,
        vim_core_rs::CoreMode::Normal,
        "Esc should leave insert mode after closing the completion menu"
    );

    outcome
        .core_bridge
        .dispatch_key("A")
        .expect("re-enter insert");
    let shutdown = dispatch_buffer_changed_with_runtime(
        Some(&mut runtime_session),
        &mut outcome,
        &mut session_state,
        &mut transient_msg,
        &mut need_redraw,
        &mut runtime_presentation_intents,
        &mut floating_window_manager,
        &mut completion_float_manager,
        &mut lsp_diagnostic_store,
        &mut terminal_float_manager,
        &mut panel_manager,
        None,
    )
    .await;
    assert_eq!(shutdown, None);
    assert!(
        floating_window_manager.windows().iter().any(|window| {
            matches!(
                window.content,
                crate::presentation::floating_window::FloatingContentRef::CompletionMenu { .. }
            )
        }),
        "bufferChanged auto trigger should reopen the one-character completion menu"
    );

    outcome
        .core_bridge
        .replace_buffer_text("\n")
        .expect("empty prefix buffer text");
    let shutdown = dispatch_buffer_changed_with_runtime(
        Some(&mut runtime_session),
        &mut outcome,
        &mut session_state,
        &mut transient_msg,
        &mut need_redraw,
        &mut runtime_presentation_intents,
        &mut floating_window_manager,
        &mut completion_float_manager,
        &mut lsp_diagnostic_store,
        &mut terminal_float_manager,
        &mut panel_manager,
        None,
    )
    .await;
    assert_eq!(shutdown, None);
    assert_eq!(
        transient_msg, None,
        "auto completion close must not surface runtime callback errors"
    );
    assert!(
        !floating_window_manager.windows().iter().any(|window| {
            matches!(
                window.content,
                crate::presentation::floating_window::FloatingContentRef::CompletionMenu { .. }
            )
        }),
        "bufferChanged auto trigger should close the stale menu when the prefix is too short"
    );

    std::fs::remove_file(target_path).expect("cleanup target");
    std::fs::remove_file(config_path).expect("cleanup config");
}

#[tokio::test(flavor = "current_thread")]
async fn startup_completion_auto_trigger_opens_menu_from_trigger_character_without_prefix() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("completion-auto-trigger-char-target").with_extension("txt");
    let config_path = unique_path("completion-auto-trigger-char-init").with_extension("ts");
    let completion_path =
        crate::support::paths::dev_ts_plugins_dir().join("bundled/completion/index.ts");
    std::fs::write(&target_path, "fmt\nPrintln\n").expect("target file");
    std::fs::write(
        &config_path,
        format!(
            r#"
                import {{ createBufferWordSource, setupSayaCompletion }} from "{}";
                setupSayaCompletion({{
                    key: "<C-x>",
                    autoTrigger: true,
                    autoTriggerDelayMs: 0,
                    minPrefixLength: 2,
                    sourceTimeoutMs: 0,
                    sources: [createBufferWordSource({{ triggerCharacters: ["."] }})],
                }});
            "#,
            completion_path.to_string_lossy()
        ),
    )
    .expect("config file");

    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::File(target_path.clone()),
        config_source: crate::app::cli::ConfigSource::File(config_path.clone()),
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
        .expect("runtime session should initialize");
    let mut floating_window_manager = FloatingWindowManager::default();
    let mut completion_float_manager = CompletionFloatManager::default();
    let mut lsp_diagnostic_store = LspDiagnosticStore::default();
    let mut terminal_float_manager = TerminalFloatManager::default();
    let mut panel_manager = PanelManager::default();
    let mut transient_msg = None;
    let mut need_redraw = false;
    let mut runtime_presentation_intents = Vec::new();

    outcome
        .core_bridge
        .dispatch_key("A.")
        .expect("type trigger");
    let shutdown = dispatch_buffer_changed_with_runtime(
        Some(&mut runtime_session),
        &mut outcome,
        &mut session_state,
        &mut transient_msg,
        &mut need_redraw,
        &mut runtime_presentation_intents,
        &mut floating_window_manager,
        &mut completion_float_manager,
        &mut lsp_diagnostic_store,
        &mut terminal_float_manager,
        &mut panel_manager,
        None,
    )
    .await;
    assert_eq!(shutdown, None);
    assert_eq!(
        transient_msg, None,
        "trigger-character auto completion must not surface runtime callback errors"
    );
    assert!(
        floating_window_manager.windows().iter().any(|window| {
            matches!(
                window.content,
                crate::presentation::floating_window::FloatingContentRef::CompletionMenu { .. }
            )
        }),
        "bufferChanged auto trigger should open a completion menu after a trigger character even when the word prefix is empty"
    );

    std::fs::remove_file(target_path).expect("cleanup target");
    std::fs::remove_file(config_path).expect("cleanup config");
}

#[tokio::test(flavor = "current_thread")]
async fn startup_completion_auto_trigger_disabled_does_not_open_from_buffer_changed_event() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("completion-auto-disabled-target").with_extension("txt");
    let config_path = unique_path("completion-auto-disabled-init").with_extension("ts");
    let completion_path =
        crate::support::paths::dev_ts_plugins_dir().join("bundled/completion/index.ts");
    std::fs::write(&target_path, "ty\ntype\n").expect("target file");
    std::fs::write(
        &config_path,
        format!(
            r#"
                import {{ createBufferWordSource, setupSayaCompletion }} from "{}";
                setupSayaCompletion({{
                    key: "<C-x>",
                    autoTrigger: false,
                    autoTriggerDelayMs: 0,
                    minPrefixLength: 2,
                    sourceTimeoutMs: 0,
                    sources: [createBufferWordSource()],
                }});
            "#,
            completion_path.to_string_lossy()
        ),
    )
    .expect("config file");

    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::File(target_path.clone()),
        config_source: crate::app::cli::ConfigSource::File(config_path.clone()),
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
        .expect("runtime session should initialize");
    let mut floating_window_manager = FloatingWindowManager::default();
    let mut completion_float_manager = CompletionFloatManager::default();
    let mut lsp_diagnostic_store = LspDiagnosticStore::default();
    let mut terminal_float_manager = TerminalFloatManager::default();
    let mut panel_manager = PanelManager::default();
    let mut transient_msg = None;
    let mut need_redraw = false;
    let mut runtime_presentation_intents = Vec::new();

    outcome.core_bridge.dispatch_key("A").expect("enter insert");
    let shutdown = dispatch_buffer_changed_with_runtime(
        Some(&mut runtime_session),
        &mut outcome,
        &mut session_state,
        &mut transient_msg,
        &mut need_redraw,
        &mut runtime_presentation_intents,
        &mut floating_window_manager,
        &mut completion_float_manager,
        &mut lsp_diagnostic_store,
        &mut terminal_float_manager,
        &mut panel_manager,
        None,
    )
    .await;

    assert_eq!(shutdown, None);
    assert_eq!(transient_msg, None);
    assert!(
        !floating_window_manager.windows().iter().any(|window| {
            matches!(
                window.content,
                crate::presentation::floating_window::FloatingContentRef::CompletionMenu { .. }
            )
        }),
        "disabled auto trigger must not open a completion menu from bufferChanged"
    );

    std::fs::remove_file(target_path).expect("cleanup target");
    std::fs::remove_file(config_path).expect("cleanup config");
}

#[test]
fn runtime_buffer_float_host_command_opens_core_window_float_and_renders_buffer_lines() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("buffer-float-render").with_extension("txt");
    std::fs::write(&target_path, "alpha\nbeta\ngamma\n").expect("target file");
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::File(target_path),
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    let mut floating_window_manager = FloatingWindowManager::default();
    let active_window_id = outcome
        .core_bridge
        .light_snapshot()
        .active_window_id()
        .expect("active window");

    execute_runtime_host_command_with_floats(
        r#"buffer.floatWindow {"width":24,"height":4,"border":"none"}"#,
        &mut outcome,
        &mut session_state,
        Some(&mut floating_window_manager),
        None,
        None,
        None,
    )
    .expect("buffer float should open");

    assert_eq!(
        floating_window_manager.focused_core_window_id(),
        Some(active_window_id)
    );
    refresh_buffer_backed_float_lines(
        &mut floating_window_manager,
        &outcome.core_bridge,
        &outcome.core_bridge.light_snapshot(),
    );
    let floats = floating_window_manager.resolve_screen_models(
        80,
        24,
        &[(
            active_window_id,
            crate::presentation::screen_model::PaneRect {
                x: 0,
                y: 0,
                width: 80,
                height: 24,
            },
        )],
        Some(active_window_id),
    );

    assert_eq!(floats.len(), 1);
    assert_eq!(
        floats[0].content,
        crate::presentation::floating_window::FloatingContentRef::CoreWindow {
            window_id: active_window_id
        }
    );
    assert_eq!(floats[0].lines, vec!["alpha", "beta", "gamma"]);
}

#[test]
fn runtime_window_open_float_api_opens_static_lines_float_through_application_host() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut floating_window_manager = FloatingWindowManager::default();
    let request = RuntimeFloatOpenRequest {
        content: RuntimeFloatContentRequest::Lines {
            lines: vec!["phase8".to_string(), "runtime-api".to_string()],
        },
        relative_to: Some(RuntimeFloatRelativeToRequest::Editor),
        width: Some(24),
        height: Some(4),
        row: Some(1),
        col: Some(2),
        anchor: Some("nw".to_string()),
        focusable: Some(true),
        border: Some("single".to_string()),
        z_index: Some(RuntimeFloatZIndexRequest::Named("user".to_string())),
        lifecycle: Some("manual".to_string()),
        group: Some("phase8:api".to_string()),
    };

    let snapshot = execute_runtime_window_open_float(
        request,
        &mut outcome,
        Some(&mut floating_window_manager),
        None,
    )
    .expect("runtime window openFloat should open a float");

    assert_eq!(snapshot.kind, "lines");
    assert!(snapshot.focused);
    assert_eq!(snapshot.replacement_group.as_deref(), Some("phase8:api"));
    assert_eq!(
        floating_window_manager
            .debug_window(FloatingWindowId(snapshot.id))
            .expect("float should exist")
            .lines,
        vec!["phase8", "runtime-api"]
    );
}

#[test]
fn runtime_buffer_float_uses_existing_backing_window_without_switching_active_window() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let first_path = unique_path("buffer-float-backed-first").with_extension("txt");
    std::fs::write(&first_path, "first\n").expect("first target file");
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::File(first_path.clone()),
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut floating_window_manager = FloatingWindowManager::default();
    let first_snapshot = outcome.core_bridge.light_snapshot();
    let first_buffer_id = first_snapshot
        .active_window()
        .expect("first active window")
        .buf_id;

    outcome
        .core_bridge
        .apply_ex_command(":split")
        .expect("split should create a backing window");
    outcome
        .core_bridge
        .apply_ex_command(":enew")
        .expect("enew should create a second active buffer");
    let second_snapshot = outcome.core_bridge.light_snapshot();
    let active_window_id = second_snapshot
        .active_window_id()
        .expect("second active window");
    let active_buffer_id = second_snapshot
        .active_window()
        .expect("second active window info")
        .buf_id;
    assert_ne!(
        first_buffer_id, active_buffer_id,
        "test setup requires a non-active buffer"
    );
    let backing_window_id = second_snapshot
        .windows
        .iter()
        .find(|window| window.buf_id == first_buffer_id)
        .expect("first buffer should still have an inactive backing window")
        .id;
    assert_ne!(
        backing_window_id, active_window_id,
        "test setup requires an inactive backing window"
    );

    let request = RuntimeFloatOpenRequest {
        content: RuntimeFloatContentRequest::Buffer {
            buffer_id: Some(first_buffer_id as u64),
            window_id: None,
        },
        relative_to: Some(RuntimeFloatRelativeToRequest::Editor),
        width: Some(24),
        height: Some(4),
        row: Some(1),
        col: Some(2),
        anchor: Some("nw".to_string()),
        focusable: Some(true),
        border: Some("single".to_string()),
        z_index: Some(RuntimeFloatZIndexRequest::Named("user".to_string())),
        lifecycle: Some("manual".to_string()),
        group: Some("phase10:buffer".to_string()),
    };

    let snapshot = execute_runtime_window_open_float(
        request,
        &mut outcome,
        Some(&mut floating_window_manager),
        None,
    )
    .expect("backed buffer float should use the existing core window");

    assert_eq!(snapshot.kind, "buffer");
    assert_eq!(
        floating_window_manager.focused_core_window_id(),
        Some(backing_window_id)
    );
    let after = outcome.core_bridge.light_snapshot();
    assert_eq!(after.active_window_id(), Some(active_window_id));
    assert_eq!(
        after.active_window().map(|window| window.buf_id),
        Some(active_buffer_id),
        "buffer float opening must not switch the active core window buffer"
    );
}

#[test]
fn runtime_buffer_float_rejects_unbacked_buffer_without_partial_float() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut floating_window_manager = FloatingWindowManager::default();
    let before = outcome.core_bridge.light_snapshot();
    let active_window_id = before.active_window_id();
    let active_buffer_id = before.active_window().map(|window| window.buf_id);
    let unbacked_buffer_id = before
        .buffers
        .iter()
        .map(|buffer| buffer.id)
        .max()
        .unwrap_or(0)
        + 10_000;

    let request = RuntimeFloatOpenRequest {
        content: RuntimeFloatContentRequest::Buffer {
            buffer_id: Some(unbacked_buffer_id as u64),
            window_id: None,
        },
        relative_to: Some(RuntimeFloatRelativeToRequest::Editor),
        width: Some(24),
        height: Some(4),
        row: Some(1),
        col: Some(2),
        anchor: Some("nw".to_string()),
        focusable: Some(true),
        border: Some("single".to_string()),
        z_index: Some(RuntimeFloatZIndexRequest::Named("user".to_string())),
        lifecycle: Some("manual".to_string()),
        group: Some("phase10:unbacked-buffer".to_string()),
    };

    let error = execute_runtime_window_open_float(
        request,
        &mut outcome,
        Some(&mut floating_window_manager),
        None,
    )
    .expect_err("unbacked buffer float should be rejected");

    assert!(matches!(
        error,
        RuntimeCommandError::CommandFailed { ref name, ref message }
            if name == "window.openFloat"
                && message.contains("hidden core-window creation is not available")
    ));
    let after = outcome.core_bridge.light_snapshot();
    assert_eq!(after.active_window_id(), active_window_id);
    assert_eq!(
        after.active_window().map(|window| window.buf_id),
        active_buffer_id
    );
    assert!(floating_window_manager.is_empty());
}

#[test]
fn runtime_window_open_float_api_opens_pty_terminal_float_and_renders_output() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut floating_window_manager = FloatingWindowManager::default();
    let mut terminal_float_manager = TerminalFloatManager::default();
    let request = RuntimeFloatOpenRequest {
        content: RuntimeFloatContentRequest::Terminal {
            command: vec![
                "sh".to_string(),
                "-lc".to_string(),
                "printf 'phase9-runtime-terminal\\n'".to_string(),
            ],
            close_behavior: Some("killOnClose".to_string()),
        },
        relative_to: Some(RuntimeFloatRelativeToRequest::Editor),
        width: Some(34),
        height: Some(6),
        row: Some(1),
        col: Some(2),
        anchor: Some("nw".to_string()),
        focusable: Some(true),
        border: Some("single".to_string()),
        z_index: Some(RuntimeFloatZIndexRequest::Named("user".to_string())),
        lifecycle: Some("manual".to_string()),
        group: Some("phase9:pty-api".to_string()),
    };

    let snapshot = execute_runtime_window_open_float(
        request,
        &mut outcome,
        Some(&mut floating_window_manager),
        Some(&mut terminal_float_manager),
    )
    .expect("runtime window openFloat should open a PTY terminal float");

    assert_eq!(snapshot.kind, "terminal");
    assert!(snapshot.focused);
    wait_for_test_condition(|| {
        refresh_terminal_float_lines(&mut floating_window_manager, &mut terminal_float_manager);
        floating_window_manager
            .debug_window(FloatingWindowId(snapshot.id))
            .expect("terminal float should exist")
            .lines
            .iter()
            .any(|line| line.contains("phase9-runtime-terminal"))
    });

    assert!(
        execute_runtime_window_close_float(
            snapshot.id,
            Some(&mut floating_window_manager),
            Some(&mut terminal_float_manager),
        )
        .expect("terminal float should close")
    );
}

#[test]
fn runtime_terminal_float_host_command_opens_pty_float_and_renders_output() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    let mut floating_window_manager = FloatingWindowManager::default();
    let mut terminal_float_manager = TerminalFloatManager::default();

    execute_runtime_host_command_with_floats(
        r#"terminal.float {"command":"sh","args":["-lc","printf 'phase7-main-terminal\n'"],"width":30,"height":4,"border":"single"}"#,
        &mut outcome,
        &mut session_state,
        Some(&mut floating_window_manager),
        None,
        None,
        Some(&mut terminal_float_manager),
    )
    .expect("terminal float should open");

    let terminal_id = floating_window_manager
        .focused_terminal_id()
        .expect("terminal float should take focus");
    wait_for_test_condition(|| {
        refresh_terminal_float_lines(&mut floating_window_manager, &mut terminal_float_manager);
        floating_window_manager
            .debug_window(
                floating_window_manager
                    .focused_float_id()
                    .expect("terminal float should stay focused"),
            )
            .expect("terminal float should exist")
            .lines
            .iter()
            .any(|line| line.contains("phase7-main-terminal"))
    });

    assert_eq!(
        floating_window_manager.focused_terminal_id(),
        Some(terminal_id)
    );
}

#[test]
fn focused_terminal_float_routes_input_through_main_key_handler() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut floating_window_manager = FloatingWindowManager::default();
    let mut terminal_float_manager = TerminalFloatManager::default();
    let terminal_id = terminal_float_manager
        .spawn(TerminalFloatSpawnRequest {
            command: "sh".to_string(),
            args: vec![
                "-lc".to_string(),
                "read line; printf \"main-echo:%s\\n\" \"$line\"; sleep 30".to_string(),
            ],
            width: 32,
            height: 4,
            close_behavior: TerminalFloatCloseBehavior::KillOnClose,
        })
        .expect("terminal session should spawn");
    let float_id = floating_window_manager.open_terminal(
        terminal_id,
        FloatingPlacement::editor_at(1, 2),
        FloatingSize {
            width: 36,
            height: 6,
        },
        FloatingChrome {
            border: FloatingBorder::Single,
        },
        FloatingZIndex::User,
        true,
    );
    floating_window_manager.focus_float(float_id);

    assert_eq!(
        handle_terminal_float_key(
            &floating_window_manager,
            &mut terminal_float_manager,
            &KeyInput::Char('o'),
        ),
        Some(FloatingWindowKeyHandling::Consumed)
    );
    assert_eq!(
        handle_terminal_float_key(
            &floating_window_manager,
            &mut terminal_float_manager,
            &KeyInput::Char('k'),
        ),
        Some(FloatingWindowKeyHandling::Consumed)
    );
    assert_eq!(
        handle_terminal_float_key(
            &floating_window_manager,
            &mut terminal_float_manager,
            &KeyInput::Enter,
        ),
        Some(FloatingWindowKeyHandling::Consumed)
    );

    wait_for_test_condition(|| {
        refresh_terminal_float_lines(&mut floating_window_manager, &mut terminal_float_manager);
        floating_window_manager
            .debug_window(float_id)
            .expect("terminal float should exist")
            .lines
            .iter()
            .any(|line| line.contains("main-echo:ok"))
    });
    terminal_float_manager
        .kill(terminal_id)
        .expect("cleanup terminal session");
}

#[test]
fn focused_buffer_float_routes_edit_keys_through_core_and_preserves_dirty_state() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("buffer-float-edit").with_extension("txt");
    std::fs::write(&target_path, "hello\n").expect("target file");
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::File(target_path),
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut manager = FloatingWindowManager::default();
    let active_window_id = outcome
        .core_bridge
        .light_snapshot()
        .active_window_id()
        .expect("active window");
    let id = manager.open_core_window(
        active_window_id,
        FloatingPlacement::editor_at(1, 1),
        FloatingSize {
            width: 20,
            height: 4,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::User,
        true,
    );
    assert!(manager.focus_float(id));

    assert_eq!(
        handle_core_window_float_key(&mut manager, &mut outcome.core_bridge, &KeyInput::Char('i')),
        Some(FloatingWindowKeyHandling::Consumed)
    );
    assert_eq!(
        handle_core_window_float_key(&mut manager, &mut outcome.core_bridge, &KeyInput::Char('Z')),
        Some(FloatingWindowKeyHandling::Consumed)
    );
    assert_eq!(
        handle_core_window_float_key(&mut manager, &mut outcome.core_bridge, &KeyInput::Escape),
        Some(FloatingWindowKeyHandling::Consumed)
    );

    let snapshot = outcome.core_bridge.snapshot();
    assert!(
        snapshot.dirty,
        "buffer-float edits must preserve dirty state"
    );
    assert!(
        snapshot.text.starts_with("Zhello"),
        "edit key should be routed through vim-core-rs: {:?}",
        snapshot.text
    );
    assert_eq!(
        manager.focus(),
        Some(crate::presentation::floating_window::WorkspaceFocus::Float { float_id: id }),
        "editing Escape should leave insert mode through core, not close the buffer float"
    );
}

#[test]
fn focused_buffer_float_routes_normal_movement_and_page_scroll_through_core_window() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("buffer-float-scroll").with_extension("txt");
    let text = (0..40)
        .map(|index| format!("line-{index:02}"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(&target_path, text).expect("target file");
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::File(target_path),
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    outcome.core_bridge.set_screen_size(6, 80);
    let mut manager = FloatingWindowManager::default();
    let active_window_id = outcome
        .core_bridge
        .light_snapshot()
        .active_window_id()
        .expect("active window");
    let id = manager.open_core_window(
        active_window_id,
        FloatingPlacement::editor_at(1, 1),
        FloatingSize {
            width: 20,
            height: 4,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::User,
        true,
    );
    assert!(manager.focus_float(id));

    assert_eq!(
        handle_core_window_float_key(&mut manager, &mut outcome.core_bridge, &KeyInput::Char('j')),
        Some(FloatingWindowKeyHandling::Consumed)
    );
    let after_move = outcome.core_bridge.light_snapshot();
    assert_eq!(after_move.cursor_row, 1);
    let before_scroll_topline = after_move
        .window(active_window_id)
        .expect("active window after movement")
        .topline;

    assert_eq!(
        handle_core_window_float_key(&mut manager, &mut outcome.core_bridge, &KeyInput::Ctrl('f')),
        Some(FloatingWindowKeyHandling::Consumed)
    );
    let after_scroll = outcome.core_bridge.light_snapshot();
    let after_scroll_topline = after_scroll
        .window(active_window_id)
        .expect("active window after scroll")
        .topline;

    assert!(
        after_scroll_topline > before_scroll_topline,
        "focused buffer-float page scroll should update the backing core window viewport: {before_scroll_topline} -> {after_scroll_topline}"
    );
}

#[test]
fn startup_keymap_action_for_input_resolves_registered_command_before_core_dispatch() {
    let keymaps = vec![crate::app::bootstrap::StartupKeymapSnapshot {
        mode: StartupKeymapMode::Normal,
        lhs: "-".to_string(),
        action: StartupKeymapAction::RegisteredCommand("dired.open".to_string()),
    }];

    assert_eq!(
        startup_keymap_action_for_input(&keymaps, CoreMode::Normal, &KeyInput::Char('-')),
        Some(StartupKeymapAction::RegisteredCommand(
            "dired.open".to_string()
        ))
    );
}

#[test]
fn startup_registered_command_name_for_ex_command_matches_runtime_command_names() {
    let registry =
        crate::runtime::callback_registry_seed::CallbackRegistrySeed::from_startup_entries(vec![
            crate::runtime::config::StartupRegistryEntry::Command {
                name: "panel.toggle".to_string(),
                callback_source: "() => {}".to_string(),
            },
        ]);

    assert_eq!(
        startup_registered_command_name_for_ex_command(":panel.toggle", &registry),
        Some("panel.toggle".to_string())
    );
    assert_eq!(
        startup_registered_command_name_for_ex_command("panel.toggle", &registry),
        Some("panel.toggle".to_string())
    );
    assert_eq!(
        startup_registered_command_name_for_ex_command(":panel.missing", &registry),
        None
    );
}

#[test]
fn focused_terminal_panel_ctrl_w_returns_focus_to_editor() {
    let mut panel_manager = PanelManager::default();
    let mut terminal_float_manager = TerminalFloatManager::default();
    panel_manager.open(PanelOpenRequest {
        id: "ai-agent".to_string(),
        position: PanelPosition::Right,
        size: PanelSize::Percent(35),
        content: PanelContent::Terminal {
            terminal_id: 77,
            close_behavior: PanelCloseBehavior::Detach,
        },
        focus: true,
    });

    assert_eq!(panel_manager.focused_terminal_id(), Some(77));
    assert_eq!(
        handle_terminal_panel_key(
            &mut panel_manager,
            &mut terminal_float_manager,
            &KeyInput::Ctrl('w')
        ),
        Some(FloatingWindowKeyHandling::Consumed)
    );
    assert_eq!(panel_manager.focused_panel_id(), None);
    assert_eq!(panel_manager.focused_terminal_id(), None);
}

#[test]
fn focused_terminal_panel_colon_enters_editor_command_line() {
    let mut panel_manager = PanelManager::default();
    panel_manager.open(PanelOpenRequest {
        id: "ai-agent".to_string(),
        position: PanelPosition::Right,
        size: PanelSize::Percent(35),
        content: PanelContent::Terminal {
            terminal_id: 77,
            close_behavior: PanelCloseBehavior::Detach,
        },
        focus: true,
    });

    assert_eq!(panel_manager.focused_terminal_id(), Some(77));
    assert_eq!(
        begin_command_line_from_focused_panel(
            &mut panel_manager,
            &KeyInput::Char(':'),
            CoreMode::Normal
        ),
        Some(':')
    );
    assert_eq!(panel_manager.focused_panel_id(), None);
    assert_eq!(panel_manager.focused_terminal_id(), None);
}

#[test]
fn focused_terminal_panel_search_enters_editor_command_line() {
    let mut panel_manager = PanelManager::default();
    panel_manager.open(PanelOpenRequest {
        id: "ai-agent".to_string(),
        position: PanelPosition::Right,
        size: PanelSize::Percent(35),
        content: PanelContent::Terminal {
            terminal_id: 77,
            close_behavior: PanelCloseBehavior::Detach,
        },
        focus: true,
    });

    assert_eq!(
        begin_command_line_from_focused_panel(
            &mut panel_manager,
            &KeyInput::Char('/'),
            CoreMode::Normal
        ),
        Some('/')
    );
    assert_eq!(panel_manager.focused_terminal_id(), None);
}

#[test]
fn focused_terminal_panel_plain_text_stays_terminal_input() {
    let mut panel_manager = PanelManager::default();
    panel_manager.open(PanelOpenRequest {
        id: "ai-agent".to_string(),
        position: PanelPosition::Right,
        size: PanelSize::Percent(35),
        content: PanelContent::Terminal {
            terminal_id: 77,
            close_behavior: PanelCloseBehavior::Detach,
        },
        focus: true,
    });

    assert_eq!(
        begin_command_line_from_focused_panel(
            &mut panel_manager,
            &KeyInput::Char('x'),
            CoreMode::Normal
        ),
        None
    );
    assert_eq!(panel_manager.focused_terminal_id(), Some(77));
}

#[test]
fn focused_view_panel_does_not_enter_terminal_input_semantics() {
    let mut panel_manager = PanelManager::default();
    let mut terminal_float_manager = TerminalFloatManager::default();
    panel_manager.open(PanelOpenRequest {
        id: "dashboard".to_string(),
        position: PanelPosition::Right,
        size: PanelSize::Percent(35),
        content: PanelContent::View {
            nodes: vec![PanelNode::Text {
                text: "status".to_string(),
            }],
        },
        focus: true,
    });

    assert_eq!(panel_manager.focused_panel_id(), Some("dashboard"));
    assert_eq!(panel_manager.focused_terminal_id(), None);
    assert_eq!(
        handle_terminal_panel_key(
            &mut panel_manager,
            &mut terminal_float_manager,
            &KeyInput::Ctrl('w')
        ),
        None
    );
    assert_eq!(
        begin_command_line_from_focused_panel(
            &mut panel_manager,
            &KeyInput::Char(':'),
            CoreMode::Normal
        ),
        None
    );
    assert_eq!(panel_manager.focused_panel_id(), Some("dashboard"));
}

#[test]
fn runtime_panel_open_accepts_view_content_and_lists_rendered_view_panel() {
    let mut panel_manager = PanelManager::default();
    let snapshot = execute_runtime_panel_open(
        RuntimePanelOpenRequest {
            id: "dashboard".to_string(),
            position: "right".to_string(),
            size: "35%".to_string(),
            content: RuntimePanelContentRequest {
                kind: "view".to_string(),
                command: Vec::new(),
                lines: Vec::new(),
                nodes: vec![
                    RuntimePanelNodeRequest {
                        node_type: "heading".to_string(),
                        text: Some("Weather".to_string()),
                        label: None,
                        src: None,
                        alt: None,
                        value: None,
                    },
                    RuntimePanelNodeRequest {
                        node_type: "progress".to_string(),
                        text: None,
                        label: Some("build".to_string()),
                        src: None,
                        alt: None,
                        value: Some(140),
                    },
                ],
                close_behavior: None,
            },
            focus: true,
        },
        Some(&mut panel_manager),
        None,
    )
    .expect("runtime panel open should accept view content");

    assert_eq!(snapshot.id, "dashboard");
    assert_eq!(snapshot.kind, "view");
    assert!(snapshot.focused);
    assert_eq!(panel_manager.focused_terminal_id(), None);
    assert_eq!(panel_manager.snapshots()[0].kind, "view");
    assert_eq!(
        panel_manager.resolve_screen_models(100, 30)[0].lines,
        vec!["Weather".to_string(), "build [##########] 100%".to_string()]
    );
}

// ADR 0006 Phase 3 整理: `startup_keymap_action_for_snapshot_input` の 2 キー
// prefix 解決を検証していた
// `startup_keymap_action_for_input_resolves_pending_two_key_sequence` /
// `startup_keymap_action_for_input_tracks_custom_two_key_prefix` は削除した。
//
// 理由: 同関数の 2 キー解決能力は本番で未使用である。本番の唯一の呼び出し元
// `run_binary_completion_smoke`（main.rs）は単キー `<C-x>` を解決するだけで、
// `g` 始まり等の 2 キー mapping 解決は単一パイプライン
// （`resolve_pipeline_command_buffered`）が担う。2 キー解決の本番回帰は
// integration テスト `tests/integration_input_pipeline.rs`
// （`production_pipeline_gg_jumps_to_row0_with_g_prefixed_keymap` /
// `production_pipeline_gd_resolves_registered_host_command`）が担保する。
// 本番未使用の 2 キー能力を緑で誤表現しないため、これらのユニットテストは撤去した。

#[test]
fn runtime_current_buffer_snapshot_includes_cursor_line_for_dired_navigation() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("runtime-buffer-snapshot-current-line");
    std::fs::write(&target_path, "README.md\nsrc/\n").expect("test file");
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::File(target_path.clone()),
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    outcome.core_bridge.dispatch_key("j").expect("move to src");
    let mut host_session = MainRuntimeHostSession::new(&mut outcome, &mut session_state);

    let snapshot = host_session.current_buffer_snapshot();

    assert_eq!(snapshot.cursor_row, 1);
    assert_eq!(snapshot.cursor_col, 0);
    assert_eq!(snapshot.current_line, "src/");
    std::fs::remove_file(target_path).expect("cleanup");
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_current_filer_entry_uses_directory_metadata_not_rendered_text() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("runtime-current-filer-entry-root");
    let nested_path = root_path.join("src");
    let readme_path = root_path.join("README.md");
    std::fs::create_dir_all(&nested_path).expect("nested directory");
    std::fs::write(&readme_path, "hello\n").expect("readme file");
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();

    execute_runtime_host_command(
        &format!("edit {}", root_path.display()),
        &mut outcome,
        &mut session_state,
    )
    .expect("open root listing");
    outcome
        .core_bridge
        .dispatch_key("i")
        .expect("enter insert mode");
    outcome
        .core_bridge
        .dispatch_key("BROKEN-")
        .expect("mutate rendered listing text");
    outcome
        .core_bridge
        .dispatch_key("\x1b")
        .expect("normal mode");
    let mut host_session = MainRuntimeHostSession::new(&mut outcome, &mut session_state);

    let entry = host_session
        .current_filer_entry()
        .expect("directory metadata should resolve current entry")
        .expect("cursor row should map to directory entry");

    assert_eq!(entry.name, "src");
    assert_eq!(entry.path, nested_path.to_string_lossy());
    assert_eq!(
        entry.kind,
        crate::runtime::live::RuntimeFilerEntryKind::Directory
    );

    std::fs::remove_dir_all(root_path).expect("cleanup root directory");
}

#[test]
fn runtime_current_filer_entry_is_none_for_regular_file_buffer() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("runtime-current-filer-entry-file");
    std::fs::write(&target_path, "hello\n").expect("test file");
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::File(target_path.clone()),
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    let mut host_session = MainRuntimeHostSession::new(&mut outcome, &mut session_state);

    let entry = host_session
        .current_filer_entry()
        .expect("regular file should not fail metadata lookup");

    assert_eq!(entry, None);
    std::fs::remove_file(target_path).expect("cleanup file");
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_host_command_executor_routes_quit_family_through_coordinator() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("runtime-host-command");
    std::fs::write(&target_path, "initial\n").expect("test file");

    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::File(target_path.clone()),
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();

    outcome.core_bridge.dispatch_key("i").unwrap();
    outcome.core_bridge.dispatch_key("X").unwrap();
    outcome.core_bridge.dispatch_key("\x1b").unwrap();
    sync_session_dirty_from_core(&mut session_state, &outcome.core_bridge);

    let effect = execute_runtime_host_command("exit", &mut outcome, &mut session_state)
        .expect("runtime quit-family command should succeed");

    assert_eq!(
        effect.transient_message,
        Some("Saved successfully".to_string())
    );
    assert_eq!(
        effect.shutdown_intent,
        Some(RuntimeShutdownIntent::UserQuit)
    );
    assert!(matches!(
        effect.follow_up_events.as_slice(),
        [crate::runtime::live::RuntimeEventPayload::BufferWritePost(
            _
        )]
    ));
    assert_eq!(
        std::fs::read_to_string(&target_path).expect("saved file should exist"),
        "Xinitial\n"
    );

    std::fs::remove_file(&target_path).expect("cleanup");
}

#[tokio::test(flavor = "current_thread")]
async fn save_then_dirty_sync_keeps_normal_quit_allowed() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("save-then-quit-clean");
    std::fs::write(&target_path, "initial\n").expect("test file");

    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::File(target_path.clone()),
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    let mut outcome_accumulator = MainOutcomeAccumulator::default();
    let mut transient_msg = None;
    let mut system_warning = None;
    let mut host_action_runtime = HostActionRuntime::default();
    let mut runtime_presentation_intents = Vec::new();
    let mut need_redraw = false;

    outcome.core_bridge.dispatch_key("i").expect("enter insert");
    outcome.core_bridge.dispatch_key("X").expect("insert text");
    outcome
        .core_bridge
        .dispatch_key("\x1b")
        .expect("leave insert");
    consume_core_outcomes_from_core(
        &mut outcome.core_bridge,
        &mut outcome_accumulator,
        &mut need_redraw,
    );
    sync_session_dirty_from_core(&mut session_state, &outcome.core_bridge);
    assert!(session_state.is_dirty(), "edit should make session dirty");

    outcome
        .core_bridge
        .apply_ex_command(":write")
        .expect(":write should be accepted");
    consume_core_outcomes_from_core(
        &mut outcome.core_bridge,
        &mut outcome_accumulator,
        &mut need_redraw,
    );
    let save_shutdown = process_pending_host_actions_with_runtime(
        &mut outcome,
        &mut outcome_accumulator,
        &mut session_state,
        &mut transient_msg,
        &mut system_warning,
        &mut host_action_runtime,
        None,
        &mut need_redraw,
        &mut runtime_presentation_intents,
        None,
    )
    .await;
    assert_eq!(save_shutdown, None);
    assert_eq!(transient_msg, Some("Saved successfully".to_string()));
    assert_eq!(
        std::fs::read_to_string(&target_path).expect("saved file"),
        "Xinitial\n"
    );

    sync_session_dirty_from_core(&mut session_state, &outcome.core_bridge);
    assert!(
        !session_state.is_dirty(),
        "stale core dirty at the saved revision must not re-dirty the session"
    );

    outcome
        .core_bridge
        .apply_ex_command(":quit")
        .expect(":quit should be accepted");
    consume_core_outcomes_from_core(
        &mut outcome.core_bridge,
        &mut outcome_accumulator,
        &mut need_redraw,
    );
    sync_session_dirty_from_core(&mut session_state, &outcome.core_bridge);
    let quit_shutdown = process_pending_host_actions_with_runtime(
        &mut outcome,
        &mut outcome_accumulator,
        &mut session_state,
        &mut transient_msg,
        &mut system_warning,
        &mut host_action_runtime,
        None,
        &mut need_redraw,
        &mut runtime_presentation_intents,
        None,
    )
    .await;

    assert_eq!(quit_shutdown, Some(ShutdownReason::UserQuit));
    assert_eq!(system_warning, None);
    std::fs::remove_file(target_path).expect("cleanup");
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_host_command_executor_drains_vfs_until_directory_listing_loads() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("runtime-host-command-directory");
    let nested_path = root_path.join("src");
    let readme_path = root_path.join("README.md");
    std::fs::create_dir_all(&nested_path).expect("test directory");
    std::fs::write(&readme_path, "hello\n").expect("test file");

    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();

    execute_runtime_host_command(
        &format!("edit {}", root_path.display()),
        &mut outcome,
        &mut session_state,
    )
    .expect("runtime edit command should load directory listing");

    assert_eq!(outcome.core_bridge.snapshot().text, "src/\nREADME.md\n");

    std::fs::remove_file(readme_path).expect("cleanup file");
    std::fs::remove_dir(nested_path).expect("cleanup nested directory");
    std::fs::remove_dir(root_path).expect("cleanup root directory");
}
