use super::program_test_support::*;
use super::*;

#[test]
fn message_pager_wraps_long_single_line_before_counting_visible_rows() {
    let wrapped =
        wrap_message_for_pager("unsupported startup option: saya.options.lineNumbers", 20);

    assert_eq!(
        wrapped.lines().collect::<Vec<_>>(),
        vec![
            "unsupported startup ",
            "option: saya.options",
            ".lineNumbers"
        ]
    );
}

#[test]
fn active_mermaid_block_appends_preview_float_without_changing_body_lines() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let source = "# Test\n```mermaid\ngraph TD\n  A-->B\n```\nafter\n";
    let bridge = crate::core::bridge::CoreBridge::new(source).expect("core bridge");
    let mut snapshot = bridge.snapshot();
    snapshot.cursor_row = 2;
    snapshot.windows[0].cursor_row = 2;
    snapshot.buffers[0].name = "diagram.md".to_string();
    let mut workspace = main_test_workspace();
    workspace.active_window_id = snapshot.windows[0].id;
    workspace.panes[0].window_id = snapshot.windows[0].id;
    workspace.panes[0].buffer_id = snapshot.windows[0].buf_id;
    workspace.panes[0].file_name = "diagram.md".to_string();
    workspace.panes[0].lines = vec![
        "# Test".to_string(),
        "```mermaid".to_string(),
        "graph TD".to_string(),
        "  A-->B".to_string(),
        "```".to_string(),
        "after".to_string(),
    ];
    let mut maps = BTreeMap::new();
    maps.insert(
        snapshot.windows[0].id,
        Arc::new(MarkdownDocumentMap::parse(source)),
    );
    let mut session_state = EditorSessionState::new(None);

    append_active_mermaid_preview_float(
        &mut workspace,
        &snapshot,
        source,
        &maps,
        80,
        24,
        &mut session_state,
    );

    assert_eq!(
        workspace.panes[0].lines,
        vec![
            "# Test",
            "```mermaid",
            "graph TD",
            "  A-->B",
            "```",
            "after"
        ],
        "preview must not collapse or reserve rows in the markdown body"
    );
    let preview = workspace
        .floats
        .iter()
        .find(|float| !float.images.is_empty())
        .expect("active Mermaid block should append an image preview float");
    let FloatingImageSource::Mermaid { source, row, .. } = &preview.images[0].source;
    assert_eq!(*row, 1);
    assert_eq!(source, "graph TD\n  A-->B");
}

#[test]
fn mermaid_preview_float_uses_roomy_terminal_relative_size() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let source = "```mermaid\ngraph TD\n  A-->B\n```\n";
    let bridge = crate::core::bridge::CoreBridge::new(source).expect("core bridge");
    let mut snapshot = bridge.snapshot();
    snapshot.cursor_row = 1;
    snapshot.windows[0].cursor_row = 1;
    snapshot.buffers[0].name = "diagram.md".to_string();
    let mut workspace = main_test_workspace();
    workspace.active_window_id = snapshot.windows[0].id;
    workspace.panes[0].window_id = snapshot.windows[0].id;
    workspace.panes[0].buffer_id = snapshot.windows[0].buf_id;
    let mut maps = BTreeMap::new();
    maps.insert(
        snapshot.windows[0].id,
        Arc::new(MarkdownDocumentMap::parse(source)),
    );
    let mut session_state = EditorSessionState::new(None);

    append_active_mermaid_preview_float(
        &mut workspace,
        &snapshot,
        source,
        &maps,
        200,
        80,
        &mut session_state,
    );

    let preview = workspace
        .floats
        .iter()
        .find(|float| !float.images.is_empty())
        .expect("active Mermaid block should append an image preview float");
    assert!(
        preview.rect.width >= 60,
        "Mermaid preview should use more than the old narrow 46-column cap"
    );
    assert!(
        preview.rect.height >= 20,
        "Mermaid preview should use more than the old short 16-row cap"
    );
    assert_eq!(preview.images[0].max_width, preview.rect.width - 2);
}

#[test]
fn mermaid_preview_float_size_uses_configured_window_percentages() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let source = "```mermaid\ngraph TD\n  A-->B\n```\n";
    let bridge = crate::core::bridge::CoreBridge::new(source).expect("core bridge");
    let mut snapshot = bridge.snapshot();
    snapshot.cursor_row = 1;
    snapshot.windows[0].cursor_row = 1;
    snapshot.buffers[0].name = "diagram.md".to_string();
    let mut workspace = main_test_workspace();
    workspace.active_window_id = snapshot.windows[0].id;
    workspace.panes[0].window_id = snapshot.windows[0].id;
    workspace.panes[0].buffer_id = snapshot.windows[0].buf_id;
    let mut maps = BTreeMap::new();
    maps.insert(
        snapshot.windows[0].id,
        Arc::new(MarkdownDocumentMap::parse(source)),
    );
    let mut session_state = EditorSessionState::new(None);
    session_state
        .apply_presentation_option(
            crate::runtime::options::SayaOptionName::MermaidPreviewWidth,
            crate::runtime::options::SayaOptionValue::Number(70),
        )
        .expect("width percent should apply");
    session_state
        .apply_presentation_option(
            crate::runtime::options::SayaOptionName::MermaidPreviewHeight,
            crate::runtime::options::SayaOptionValue::Number(60),
        )
        .expect("height percent should apply");

    append_active_mermaid_preview_float(
        &mut workspace,
        &snapshot,
        source,
        &maps,
        200,
        80,
        &mut session_state,
    );

    let preview = workspace
        .floats
        .iter()
        .find(|float| !float.images.is_empty())
        .expect("active Mermaid block should append an image preview float");
    assert_eq!(preview.rect.width, 140);
    assert_eq!(preview.rect.height, 48);
}

#[test]
fn mermaid_preview_float_source_includes_configured_background() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let source = "```mermaid\ngraph TD\n  A-->B\n```\n";
    let bridge = crate::core::bridge::CoreBridge::new(source).expect("core bridge");
    let mut snapshot = bridge.snapshot();
    snapshot.cursor_row = 1;
    snapshot.windows[0].cursor_row = 1;
    snapshot.buffers[0].name = "diagram.md".to_string();
    let mut workspace = main_test_workspace();
    workspace.active_window_id = snapshot.windows[0].id;
    workspace.panes[0].window_id = snapshot.windows[0].id;
    workspace.panes[0].buffer_id = snapshot.windows[0].buf_id;
    let mut maps = BTreeMap::new();
    maps.insert(
        snapshot.windows[0].id,
        Arc::new(MarkdownDocumentMap::parse(source)),
    );
    let mut session_state = EditorSessionState::new(None);
    session_state
        .apply_presentation_option(
            crate::runtime::options::SayaOptionName::MermaidPreviewBackground,
            crate::runtime::options::SayaOptionValue::String("#ffffff".to_string()),
        )
        .expect("background should apply");

    append_active_mermaid_preview_float(
        &mut workspace,
        &snapshot,
        source,
        &maps,
        120,
        40,
        &mut session_state,
    );

    let preview = workspace
        .floats
        .iter()
        .find(|float| !float.images.is_empty())
        .expect("active Mermaid block should append an image preview float");
    let FloatingImageSource::Mermaid { background, .. } = &preview.images[0].source;
    assert_eq!(background, "#ffffff");
}

#[test]
fn insert_mode_does_not_append_mermaid_preview_float() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let source = "```mermaid\ngraph TD\n  A-->B\n```\n";
    let bridge = crate::core::bridge::CoreBridge::new(source).expect("core bridge");
    let mut snapshot = bridge.snapshot();
    snapshot.mode = CoreMode::Insert;
    snapshot.cursor_row = 1;
    snapshot.windows[0].cursor_row = 1;
    snapshot.buffers[0].name = "diagram.md".to_string();
    let mut workspace = main_test_workspace();
    workspace.active_window_id = snapshot.windows[0].id;
    workspace.panes[0].window_id = snapshot.windows[0].id;
    workspace.panes[0].buffer_id = snapshot.windows[0].buf_id;
    let mut maps = BTreeMap::new();
    maps.insert(
        snapshot.windows[0].id,
        Arc::new(MarkdownDocumentMap::parse(source)),
    );
    let mut session_state = EditorSessionState::new(None);

    append_active_mermaid_preview_float(
        &mut workspace,
        &snapshot,
        source,
        &maps,
        80,
        24,
        &mut session_state,
    );

    assert!(
        workspace.floats.is_empty(),
        "Insert mode should keep editing responsive and avoid image preview rendering"
    );
}

#[test]
fn mermaid_preview_auto_off_skips_float_but_manual_request_appends_once() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let source = "```mermaid\ngraph TD\n  A-->B\n```\n";
    let bridge = crate::core::bridge::CoreBridge::new(source).expect("core bridge");
    let mut snapshot = bridge.snapshot();
    snapshot.cursor_row = 1;
    snapshot.windows[0].cursor_row = 1;
    snapshot.buffers[0].name = "diagram.md".to_string();
    let mut maps = BTreeMap::new();
    maps.insert(
        snapshot.windows[0].id,
        Arc::new(MarkdownDocumentMap::parse(source)),
    );

    let mut auto_workspace = main_test_workspace();
    auto_workspace.active_window_id = snapshot.windows[0].id;
    auto_workspace.panes[0].window_id = snapshot.windows[0].id;
    auto_workspace.panes[0].buffer_id = snapshot.windows[0].buf_id;
    let mut auto_session_state = EditorSessionState::new(None);
    auto_session_state
        .apply_presentation_option(
            crate::runtime::options::SayaOptionName::MermaidPreview,
            crate::runtime::options::SayaOptionValue::Boolean(false),
        )
        .expect("mermaidpreview off should apply");
    append_active_mermaid_preview_float(
        &mut auto_workspace,
        &snapshot,
        source,
        &maps,
        80,
        24,
        &mut auto_session_state,
    );
    assert!(auto_workspace.floats.is_empty());

    let mut manual_workspace = main_test_workspace();
    manual_workspace.active_window_id = snapshot.windows[0].id;
    manual_workspace.panes[0].window_id = snapshot.windows[0].id;
    manual_workspace.panes[0].buffer_id = snapshot.windows[0].buf_id;
    let mut manual_session_state = EditorSessionState::new(None);
    manual_session_state
        .apply_presentation_option(
            crate::runtime::options::SayaOptionName::MermaidPreview,
            crate::runtime::options::SayaOptionValue::Boolean(false),
        )
        .expect("mermaidpreview off should apply");
    manual_session_state.request_mermaid_preview();
    append_active_mermaid_preview_float(
        &mut manual_workspace,
        &snapshot,
        source,
        &maps,
        80,
        24,
        &mut manual_session_state,
    );
    assert_eq!(manual_workspace.floats.len(), 1);

    let mut next_frame_workspace = main_test_workspace();
    next_frame_workspace.active_window_id = snapshot.windows[0].id;
    next_frame_workspace.panes[0].window_id = snapshot.windows[0].id;
    next_frame_workspace.panes[0].buffer_id = snapshot.windows[0].buf_id;
    append_active_mermaid_preview_float(
        &mut next_frame_workspace,
        &snapshot,
        source,
        &maps,
        80,
        24,
        &mut manual_session_state,
    );
    assert_eq!(
        next_frame_workspace.floats.len(),
        1,
        "manual Mermaid preview should survive the async renderer completion redraw"
    );

    let mut outside_snapshot = snapshot.clone();
    outside_snapshot.cursor_row = 4;
    outside_snapshot.windows[0].cursor_row = 4;
    let mut outside_workspace = main_test_workspace();
    outside_workspace.active_window_id = outside_snapshot.windows[0].id;
    outside_workspace.panes[0].window_id = outside_snapshot.windows[0].id;
    outside_workspace.panes[0].buffer_id = outside_snapshot.windows[0].buf_id;
    append_active_mermaid_preview_float(
        &mut outside_workspace,
        &outside_snapshot,
        source,
        &maps,
        80,
        24,
        &mut manual_session_state,
    );
    assert!(outside_workspace.floats.is_empty());
    assert!(!manual_session_state.mermaid_preview_manual_active());
}

#[test]
fn focused_mermaid_preview_keys_zoom_pan_and_close_without_editor_passthrough() {
    let mut session_state = EditorSessionState::new(None);

    assert_eq!(
        handle_mermaid_preview_key(&mut session_state, &KeyInput::Char('j')),
        None,
        "non-focused preview must not steal normal j movement"
    );

    session_state.request_mermaid_preview();
    assert_eq!(
        handle_mermaid_preview_key(&mut session_state, &KeyInput::Char('+')),
        Some(FloatingWindowKeyHandling::Consumed)
    );
    assert_eq!(
        session_state.mermaid_preview_zoom(),
        MermaidPreviewZoom::Percent(125)
    );

    assert_eq!(
        handle_mermaid_preview_key(&mut session_state, &KeyInput::Char('j')),
        Some(FloatingWindowKeyHandling::Consumed)
    );
    assert_eq!(session_state.mermaid_preview_pan(), (0, 64));

    assert!(matches!(
        handle_mermaid_preview_key(&mut session_state, &KeyInput::Char('q')),
        Some(FloatingWindowKeyHandling::Closed { .. })
    ));
    assert!(!session_state.mermaid_preview_manual_active());
    assert!(!session_state.mermaid_preview_focused());
}

#[test]
fn focused_mermaid_preview_handles_full_zoom_and_pan_key_contract() {
    let mut session_state = EditorSessionState::new(None);
    session_state.request_mermaid_preview();

    assert_eq!(
        handle_mermaid_preview_key(&mut session_state, &KeyInput::Char('=')),
        Some(FloatingWindowKeyHandling::Consumed)
    );
    assert_eq!(
        session_state.mermaid_preview_zoom(),
        MermaidPreviewZoom::Percent(125)
    );

    assert_eq!(
        handle_mermaid_preview_key(&mut session_state, &KeyInput::Char('-')),
        Some(FloatingWindowKeyHandling::Consumed)
    );
    assert_eq!(
        session_state.mermaid_preview_zoom(),
        MermaidPreviewZoom::Percent(100)
    );

    assert_eq!(
        handle_mermaid_preview_key(&mut session_state, &KeyInput::Char('0')),
        Some(FloatingWindowKeyHandling::Consumed)
    );
    assert_eq!(
        session_state.mermaid_preview_zoom(),
        MermaidPreviewZoom::Fit
    );

    assert_eq!(
        handle_mermaid_preview_key(&mut session_state, &KeyInput::Char('1')),
        Some(FloatingWindowKeyHandling::Consumed)
    );
    assert_eq!(
        session_state.mermaid_preview_zoom(),
        MermaidPreviewZoom::Percent(100)
    );

    assert_eq!(
        handle_mermaid_preview_key(&mut session_state, &KeyInput::Ctrl('f')),
        Some(FloatingWindowKeyHandling::Consumed)
    );
    assert_eq!(session_state.mermaid_preview_pan(), (0, 256));

    assert_eq!(
        handle_mermaid_preview_key(&mut session_state, &KeyInput::Ctrl('b')),
        Some(FloatingWindowKeyHandling::Consumed)
    );
    assert_eq!(session_state.mermaid_preview_pan(), (0, 0));

    assert_eq!(
        handle_mermaid_preview_key(&mut session_state, &KeyInput::Char('L')),
        Some(FloatingWindowKeyHandling::Consumed)
    );
    assert_eq!(session_state.mermaid_preview_pan(), (256, 0));

    assert_eq!(
        handle_mermaid_preview_key(&mut session_state, &KeyInput::Char('H')),
        Some(FloatingWindowKeyHandling::Consumed)
    );
    assert_eq!(session_state.mermaid_preview_pan(), (0, 0));

    assert!(matches!(
        handle_mermaid_preview_key(&mut session_state, &KeyInput::Escape),
        Some(FloatingWindowKeyHandling::Closed { .. })
    ));
    assert!(!session_state.mermaid_preview_focused());
}

#[test]
fn mermaid_preview_mouse_wheel_pans_only_after_preview_focus() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let source = "```mermaid\ngraph TD\n  A-->B\n```\n";
    let bridge = crate::core::bridge::CoreBridge::new(source).expect("core bridge");
    let mut snapshot = bridge.snapshot();
    snapshot.cursor_row = 1;
    snapshot.windows[0].cursor_row = 1;
    snapshot.buffers[0].name = "diagram.md".to_string();
    let mut maps = BTreeMap::new();
    maps.insert(
        snapshot.windows[0].id,
        Arc::new(MarkdownDocumentMap::parse(source)),
    );
    let mut workspace = main_test_workspace();
    workspace.active_window_id = snapshot.windows[0].id;
    workspace.panes[0].window_id = snapshot.windows[0].id;
    workspace.panes[0].buffer_id = snapshot.windows[0].buf_id;
    let mut session_state = EditorSessionState::new(None);

    append_active_mermaid_preview_float(
        &mut workspace,
        &snapshot,
        source,
        &maps,
        80,
        24,
        &mut session_state,
    );
    let preview_rect = workspace
        .floats
        .iter()
        .find(|float| !float.images.is_empty())
        .expect("Mermaid preview float should be present")
        .rect;

    assert!(!handle_mermaid_preview_mouse_wheel(
        &mut session_state,
        Some(&workspace),
        preview_rect.x.saturating_add(1),
        preview_rect.y.saturating_add(1),
        0,
        1,
    ));
    assert_eq!(session_state.mermaid_preview_pan(), (0, 0));

    assert!(focus_mermaid_preview_from_mouse_click(
        &mut session_state,
        Some(&workspace),
        preview_rect.x.saturating_add(1),
        preview_rect.y.saturating_add(1),
    ));
    assert!(handle_mermaid_preview_mouse_wheel(
        &mut session_state,
        Some(&workspace),
        preview_rect.x.saturating_add(1),
        preview_rect.y.saturating_add(1),
        1,
        2,
    ));
    assert_eq!(session_state.mermaid_preview_pan(), (96, 192));
}

#[test]
fn workspace_render_ignores_saya_float_demo_env() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let previous = std::env::var_os("SAYA_FLOAT_DEMO");
    unsafe {
        std::env::set_var("SAYA_FLOAT_DEMO", "1");
    }
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    let mut viewport_store = WindowViewportStore::new();
    let mut search_refresh_store = WindowSearchRefreshStore::default();
    let mut markdown_metadata_cache = MarkdownMetadataCache::default();
    let mut floating_window_manager = FloatingWindowManager::default();

    let workspace = build_workspace_render_output(
        &mut outcome,
        &mut session_state,
        &mut viewport_store,
        ViewportSyncMode::Core,
        &mut search_refresh_store,
        &mut markdown_metadata_cache,
        None,
        "",
        0,
        None,
        None,
        None,
        None,
        None,
        80,
        24,
        Some(&mut floating_window_manager),
        None,
        None,
        None,
    )
    .expect("workspace should render");

    match previous {
        Some(value) => unsafe {
            std::env::set_var("SAYA_FLOAT_DEMO", value);
        },
        None => unsafe {
            std::env::remove_var("SAYA_FLOAT_DEMO");
        },
    }
    assert!(
        workspace.floats.is_empty(),
        "production render must not inject demo floats from SAYA_FLOAT_DEMO"
    );
    assert!(floating_window_manager.is_empty());
}

#[test]
fn workspace_render_highlights_substitute_matches_before_commit() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("substitute-preview").with_extension("txt");
    std::fs::write(&target_path, "foo foo\nbar foo\n").expect("test file");
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::File(target_path.clone()),
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    let mut viewport_store = WindowViewportStore::new();
    let mut search_refresh_store = WindowSearchRefreshStore::default();
    let mut markdown_metadata_cache = MarkdownMetadataCache::default();

    let workspace = build_workspace_render_output(
        &mut outcome,
        &mut session_state,
        &mut viewport_store,
        ViewportSyncMode::Core,
        &mut search_refresh_store,
        &mut markdown_metadata_cache,
        Some(':'),
        "%s/foo/baz",
        "%s/foo/baz".len(),
        None,
        None,
        None,
        None,
        None,
        80,
        24,
        None,
        None,
        None,
        None,
    )
    .expect("workspace should render");

    let active_pane = workspace
        .panes
        .iter()
        .find(|pane| pane.window_id == workspace.active_window_id)
        .expect("active pane should exist");
    assert_eq!(
        active_pane.search_overlays.len(),
        2,
        "substitute live preview should highlight each line's first replacement before Enter"
    );
    assert!(
        active_pane
            .lines
            .iter()
            .any(|line| line.contains("baz foo")),
        "substitute live preview should render the first visible line with replacement text: {:?}",
        active_pane.lines
    );
    assert!(
        active_pane
            .lines
            .iter()
            .any(|line| line.contains("bar baz")),
        "substitute live preview should render the second visible line with replacement text: {:?}",
        active_pane.lines
    );
    assert_eq!(
        outcome.core_bridge.snapshot().text,
        "foo foo\nbar foo\n",
        "substitute preview must not mutate the buffer before command commit"
    );
    std::fs::remove_file(target_path).expect("test file should be removed");
}

#[test]
fn generic_floating_window_ignored_key_does_not_consume_colon_command_prompt() {
    let mut manager = FloatingWindowManager::default();
    let id = manager.open_static_lines(
        vec!["demo".to_string()],
        FloatingPlacement::editor_at(1, 1),
        FloatingSize {
            width: 10,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::Hover,
        true,
    );
    assert!(manager.focus_float(id));

    let handling = handle_floating_window_key(&mut manager, &KeyInput::Char(':'), 1);

    assert_eq!(handling, None);
    assert_eq!(
        manager.focus(),
        Some(crate::presentation::floating_window::WorkspaceFocus::Float { float_id: id }),
        "ignored keys must leave float focus unchanged but pass through to later handlers"
    );
}

#[test]
fn floating_window_key_close_restores_active_pane_focus() {
    let mut manager = FloatingWindowManager::default();
    let id = manager.open_static_lines(
        vec!["demo".to_string()],
        FloatingPlacement::editor_at(1, 1),
        FloatingSize {
            width: 10,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::Hover,
        true,
    );
    assert!(manager.focus_float(id));

    let handling = handle_floating_window_key(&mut manager, &KeyInput::Escape, 9);

    assert_eq!(handling, Some(FloatingWindowKeyHandling::Closed { id }));
    assert_eq!(
        manager.focus(),
        Some(crate::presentation::floating_window::WorkspaceFocus::Pane { window_id: 9 }),
        "closed float should restore the active pane focus through the main routing helper"
    );
}

#[test]
fn floating_window_mouse_focus_helper_focuses_float_before_core_mouse_dispatch() {
    let workspace = main_test_workspace();
    let mut manager = FloatingWindowManager::default();
    let id = manager.open_static_lines(
        vec!["demo".to_string()],
        FloatingPlacement::editor_at(1, 1),
        FloatingSize {
            width: 10,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::Hover,
        true,
    );

    let outcome =
        focus_floating_window_from_mouse_click(&mut manager, Some(&workspace), 2, 2, 80, 24);

    assert_eq!(outcome, FloatingMouseOutcome::Focused { id });
    assert_eq!(
        manager.focus(),
        Some(crate::presentation::floating_window::WorkspaceFocus::Float { float_id: id })
    );
}

#[test]
fn floating_window_mouse_focus_helper_passes_through_non_mouse_float() {
    let workspace = main_test_workspace();
    let mut manager = FloatingWindowManager::default();
    let id = manager.open_static_lines(
        vec!["demo".to_string()],
        FloatingPlacement::editor_at(1, 1),
        FloatingSize {
            width: 10,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::Hover,
        true,
    );
    assert!(manager.set_mouse_enabled(id, false));

    let outcome =
        focus_floating_window_from_mouse_click(&mut manager, Some(&workspace), 2, 2, 80, 24);

    assert_eq!(outcome, FloatingMouseOutcome::PassThrough);
    assert_eq!(manager.focus(), None);
}

#[test]
fn viewport_sync_mode_uses_smooth_line_motion_only_for_single_line_vertical_inputs() {
    assert_eq!(
        viewport_sync_mode_for_input(&KeyInput::Char('j')),
        ViewportSyncMode::SmoothLineMotion
    );
    assert_eq!(
        viewport_sync_mode_for_input(&KeyInput::Char('k')),
        ViewportSyncMode::SmoothLineMotion
    );
    assert_eq!(
        viewport_sync_mode_for_input(&KeyInput::Down),
        ViewportSyncMode::SmoothLineMotion
    );
    assert_eq!(
        viewport_sync_mode_for_input(&KeyInput::Up),
        ViewportSyncMode::SmoothLineMotion
    );

    assert_eq!(
        viewport_sync_mode_for_input(&KeyInput::PageDown),
        ViewportSyncMode::Core
    );
    assert_eq!(
        viewport_sync_mode_for_input(&KeyInput::PageUp),
        ViewportSyncMode::Core
    );
    assert_eq!(
        viewport_sync_mode_for_input(&KeyInput::Ctrl('f')),
        ViewportSyncMode::Core
    );
    assert_eq!(
        viewport_sync_mode_for_input(&KeyInput::Char('H')),
        ViewportSyncMode::Core
    );
}

#[test]
fn editor_area_mouse_click_builds_one_based_sgr_sequence() {
    let workspace = main_test_workspace();

    let sequence = mouse_click_to_sgr_sequence(Some(&workspace), 0, 0);

    assert_eq!(sequence.as_deref(), Some("\x1b[<0;1;1M"));
}

#[test]
fn mouse_click_outside_editor_body_does_not_dispatch() {
    let workspace = main_test_workspace();

    let status_row = mouse_click_to_sgr_sequence(Some(&workspace), 0, 2);
    let command_row = mouse_click_to_sgr_sequence(Some(&workspace), 0, 3);
    let no_workspace = mouse_click_to_sgr_sequence(None, 0, 0);

    assert_eq!(status_row, None);
    assert_eq!(command_row, None);
    assert_eq!(no_workspace, None);
}

#[test]
fn mouse_click_sgr_coordinates_saturate_at_u16_max() {
    let workspace = WorkspaceScreenModel {
        panes: vec![crate::presentation::screen_model::ScreenModel {
            rect: crate::presentation::screen_model::PaneRect {
                x: u16::MAX,
                y: u16::MAX,
                width: 1,
                height: 1,
            },
            ..main_test_workspace().panes.remove(0)
        }],
        ..main_test_workspace()
    };

    let sequence = mouse_click_to_sgr_sequence(Some(&workspace), u16::MAX, u16::MAX);

    assert_eq!(sequence.as_deref(), Some("\x1b[<0;65535;65535M"));
}

#[test]
fn markdown_metadata_collection_is_skipped_when_markdown_render_is_disabled() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("markdown-render-off").with_extension("md");
    std::fs::write(&target_path, "# Title\n").expect("test markdown file");
    let outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::File(target_path.clone()),
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    session_state
        .apply_presentation_option(
            crate::runtime::options::SayaOptionName::MarkdownRender,
            crate::runtime::options::SayaOptionValue::Boolean(false),
        )
        .expect("markdownrender option should apply");
    let mut markdown_metadata_cache = MarkdownMetadataCache::default();

    let maps = collect_workspace_markdown_document_maps(
        &mut markdown_metadata_cache,
        &session_state,
        &outcome.core_bridge,
        &outcome.core_bridge.snapshot(),
    );

    assert!(
        maps.is_empty(),
        "raw Markdown mode should not collect render metadata"
    );
    std::fs::remove_file(&target_path).expect("test markdown file should be removed");
}

#[cfg(feature = "tree-sitter-syntax")]
#[test]
fn workspace_render_collects_tree_sitter_highlight_only_when_vim_syntax_is_on() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("syntax-toggle-main").with_extension("rs");
    let config_path = unique_path("syntax-toggle-empty-init").with_extension("ts");
    std::fs::write(&target_path, "fn main() {}\n").expect("test source file");
    std::fs::write(&config_path, "").expect("empty test config file");
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::File(target_path.clone()),
        config_source: crate::app::cli::ConfigSource::File(config_path.clone()),
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    outcome.core_bridge.set_screen_size(24, 80);
    let mut session_state = outcome.editor_session_state();
    let mut viewport_store = WindowViewportStore::new();
    let mut search_refresh_store = WindowSearchRefreshStore::default();
    let mut markdown_metadata_cache = MarkdownMetadataCache::default();

    let syntax_off_workspace = build_workspace_render_output(
        &mut outcome,
        &mut session_state,
        &mut viewport_store,
        ViewportSyncMode::Core,
        &mut search_refresh_store,
        &mut markdown_metadata_cache,
        None,
        "",
        0,
        None,
        None,
        None,
        None,
        None,
        80,
        24,
        None,
        None,
        None,
        None,
    )
    .expect("syntax-off workspace should render");
    assert!(
        syntax_off_workspace
            .panes
            .iter()
            .all(|pane| pane.syntax_chunks.is_empty()),
        "syntax off should skip Vim and Tree-sitter highlight collection"
    );

    outcome
        .core_bridge
        .apply_ex_command("syntax on")
        .expect("syntax on should enable highlight collection");
    assert!(
        outcome.core_bridge.is_syntax_enabled(),
        "syntax on should be visible before workspace render"
    );
    let snapshot = outcome.core_bridge.snapshot();
    assert!(
        snapshot
            .buffers
            .iter()
            .any(|buffer| buffer.name.ends_with(".rs")),
        "Rust source buffer name should be available for Tree-sitter language resolution: {:?}",
        snapshot.buffers
    );
    let mut syntax_on_workspace = None;
    for _ in 0..20 {
        let workspace = build_workspace_render_output(
            &mut outcome,
            &mut session_state,
            &mut viewport_store,
            ViewportSyncMode::Core,
            &mut search_refresh_store,
            &mut markdown_metadata_cache,
            None,
            "",
            0,
            None,
            None,
            None,
            None,
            None,
            80,
            24,
            None,
            None,
            None,
            None,
        )
        .expect("syntax-on workspace should render");
        if workspace_has_syntax_chunks(&workspace) {
            syntax_on_workspace = Some(workspace);
            break;
        }
        syntax_on_workspace = Some(workspace);
        std::thread::sleep(Duration::from_millis(10));
    }
    let syntax_on_workspace =
        syntax_on_workspace.expect("syntax-on workspace should render at least once");
    assert!(
        workspace_has_syntax_chunks(&syntax_on_workspace),
        "syntax on should allow Tree-sitter highlight chunks for Rust source"
    );
    std::fs::remove_file(&target_path).expect("test source file should be removed");
    std::fs::remove_file(&config_path).expect("test config file should be removed");
}

#[cfg(feature = "tree-sitter-syntax")]
fn workspace_has_syntax_chunks(workspace: &WorkspaceScreenModel) -> bool {
    workspace
        .panes
        .iter()
        .any(|pane| !pane.syntax_chunks.is_empty())
}

#[test]
fn pasted_text_dispatch_units_preserve_character_order_without_reinterpretation() {
    let units = pasted_text_to_dispatch_units("ab\n\r\nあ\x1b");

    assert_eq!(
        units,
        vec!["a", "b", "\n", "\r", "\n", "あ", "\x1b"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
    );
}
