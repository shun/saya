use saya::features::lsp::float::{
    LspDiagnosticFloatRequest, LspHoverFloatRequest, LspHoverOpenOutcome, ResolvedPopupSizeLimit,
    hover_lines_from_lsp_value, open_lsp_diagnostic_float, open_lsp_hover_float,
};
use saya::input::router::KeyInput;
use saya::presentation::floating_window::{
    FloatingCloseEvents, FloatingLifecycle, FloatingLifecycleEvent, FloatingLifecycleOutcome,
    FloatingRelativeTo, FloatingWindowManager, WorkspaceFocus,
};
use saya::presentation::screen_model::PaneRect;
use serde_json::json;

fn pane(window_id: i32) -> (i32, PaneRect) {
    (
        window_id,
        PaneRect {
            x: 2,
            y: 3,
            width: 40,
            height: 12,
        },
    )
}

fn default_limit() -> ResolvedPopupSizeLimit {
    ResolvedPopupSizeLimit::lsp_default()
}

#[test]
fn hover_lines_accept_plaintext_markup_and_marked_string_shapes() {
    assert_eq!(
        hover_lines_from_lsp_value(&json!({
            "result": {
                "contents": { "kind": "plaintext", "value": "alpha\nbeta" }
            }
        })),
        vec!["alpha".to_string(), "beta".to_string()]
    );

    // Phase D 以降: 配列内の MarkedString は markdown_render を経由するため
    // 言語タグ用の "rust:" ヘッダ行は出力されない。複数 contents は
    // 段落区切りの空行で結合される。
    assert_eq!(
        hover_lines_from_lsp_value(&json!({
            "contents": [
                "first",
                { "language": "rust", "value": "let value = 1;" }
            ]
        })),
        vec![
            "first".to_string(),
            "".to_string(),
            "let value = 1;".to_string()
        ]
    );
}

#[test]
fn lsp_hover_at_a_different_cursor_position_replaces_previous_hover_and_closes_on_cursor_move() {
    let mut manager = FloatingWindowManager::default();
    let first = open_lsp_hover_float(
        &mut manager,
        LspHoverFloatRequest {
            window_id: 7,
            cursor_row: 4,
            cursor_col: 9,
            response: json!({ "result": { "contents": "old hover" } }),
            size_limit: default_limit(),
        },
    )
    .expect("first hover should open")
    .id();

    let second_outcome = open_lsp_hover_float(
        &mut manager,
        LspHoverFloatRequest {
            window_id: 7,
            cursor_row: 6,
            cursor_col: 0,
            response: json!({ "result": { "contents": "new hover\nsecond line" } }),
            size_limit: default_limit(),
        },
    )
    .expect("second hover should open");
    let LspHoverOpenOutcome::Opened { id: second } = second_outcome else {
        panic!("different anchor must produce a fresh float (Opened), got: {second_outcome:?}");
    };

    let floats =
        manager.resolve_screen_models_with_cursors(80, 24, &[pane(7)], &[(7, 6, 0)], Some(7));
    assert_eq!(floats.len(), 1);
    assert_eq!(floats[0].id, second);
    assert_eq!(floats[0].lines, vec!["new hover", "second line"]);

    assert_eq!(
        manager.apply_lifecycle_event(
            FloatingLifecycleEvent::CursorMoved {
                window_id: 7,
                row: 7,
                col: 0,
            },
            Some(7),
        ),
        FloatingLifecycleOutcome {
            closed: vec![second]
        }
    );
    assert_ne!(first, second);
}

#[test]
fn lsp_hover_at_same_cursor_position_focuses_existing_float_for_neovim_style_toggle() {
    let mut manager = FloatingWindowManager::default();
    let first_outcome = open_lsp_hover_float(
        &mut manager,
        LspHoverFloatRequest {
            window_id: 7,
            cursor_row: 4,
            cursor_col: 9,
            response: json!({ "result": { "contents": "hover content" } }),
            size_limit: default_limit(),
        },
    )
    .expect("first hover should open");
    let LspHoverOpenOutcome::Opened { id: first } = first_outcome else {
        panic!("first call must open a new float, got: {first_outcome:?}");
    };
    assert_eq!(
        manager.focus(),
        None,
        "first hover open must leave pane focused"
    );

    let second_outcome = open_lsp_hover_float(
        &mut manager,
        LspHoverFloatRequest {
            window_id: 7,
            cursor_row: 4,
            cursor_col: 9,
            response: json!({ "result": { "contents": "hover content" } }),
            size_limit: default_limit(),
        },
    )
    .expect("second hover at same anchor should focus existing float");
    assert_eq!(
        second_outcome,
        LspHoverOpenOutcome::FocusedExisting { id: first },
        "Neovim-style focus toggle requires the second call at the same cursor to reuse the float"
    );
    assert_eq!(
        manager.focus(),
        Some(WorkspaceFocus::Float { float_id: first }),
        "after focus toggle, focus must be on the existing hover float"
    );
}

#[test]
fn empty_lsp_hover_response_does_not_open_float() {
    let mut manager = FloatingWindowManager::default();

    assert_eq!(
        open_lsp_hover_float(
            &mut manager,
            LspHoverFloatRequest {
                window_id: 7,
                cursor_row: 0,
                cursor_col: 0,
                response: json!({ "result": null }),
                size_limit: default_limit(),
            },
        ),
        None
    );
    assert!(manager.is_empty());
}

#[test]
fn diagnostic_float_opens_at_buffer_position_and_replaces_previous_diagnostics() {
    let mut manager = FloatingWindowManager::default();
    let first = open_lsp_diagnostic_float(
        &mut manager,
        LspDiagnosticFloatRequest {
            window_id: 7,
            line: 10,
            column: 3,
            diagnostics: json!([
                { "severity": 1, "message": "first error" }
            ]),
            size_limit: default_limit(),
        },
    )
    .expect("first diagnostics should open");
    let second = open_lsp_diagnostic_float(
        &mut manager,
        LspDiagnosticFloatRequest {
            window_id: 7,
            line: 11,
            column: 2,
            diagnostics: json!([
                { "severity": 2, "message": "warning" },
                { "message": "hint" }
            ]),
            size_limit: default_limit(),
        },
    )
    .expect("second diagnostics should replace first");

    let floats = manager.resolve_screen_models(80, 24, &[pane(7)], Some(7));
    assert_eq!(floats.len(), 1);
    assert_eq!(floats[0].id, second);
    assert_eq!(floats[0].lines, vec!["Warning: warning", "hint"]);
    assert!(matches!(
        manager
            .debug_window(second)
            .expect("diagnostic float should exist")
            .placement
            .relative_to,
        FloatingRelativeTo::BufferPosition {
            window_id: 7,
            line: 11,
            column: 2
        }
    ));
    assert_ne!(first, second);
}

#[test]
fn lsp_hover_uses_close_on_events_lifecycle_with_cursor_move_and_mode_change_and_window_leave() {
    let mut manager = FloatingWindowManager::default();
    let id = open_lsp_hover_float(
        &mut manager,
        LspHoverFloatRequest {
            window_id: 3,
            cursor_row: 1,
            cursor_col: 1,
            response: json!({ "result": { "contents": "hover" } }),
            size_limit: default_limit(),
        },
    )
    .expect("hover should open")
    .id();

    let window = manager.debug_window(id).expect("hover float should exist");
    let expected_events = FloatingCloseEvents::none()
        .with_cursor_move()
        .with_mode_change()
        .with_window_leave();
    assert_eq!(
        window.lifecycle,
        FloatingLifecycle::CloseOnEvents(expected_events),
        "hover lifecycle must declare close-on cursor-move/mode-change/window-leave"
    );
    assert_eq!(
        window
            .focus_id
            .as_ref()
            .map(saya::presentation::floating_window::FloatingFocusId::as_str),
        Some("lsp:hover"),
        "hover float must carry focus_id `lsp:hover` for focus toggle"
    );
}

#[test]
fn lsp_hover_float_close_keys_include_q_for_neovim_style_dismiss() {
    let mut manager = FloatingWindowManager::default();
    let id = open_lsp_hover_float(
        &mut manager,
        LspHoverFloatRequest {
            window_id: 7,
            cursor_row: 1,
            cursor_col: 1,
            response: json!({ "result": { "contents": "hover" } }),
            size_limit: default_limit(),
        },
    )
    .expect("hover should open")
    .id();

    let close_keys = manager
        .debug_window(id)
        .expect("hover float should exist")
        .close_keys
        .clone();
    assert!(
        close_keys.contains(&KeyInput::Char('q')),
        "hover float close_keys must include `q`: {close_keys:?}"
    );
    assert!(
        close_keys.contains(&KeyInput::Escape),
        "hover float close_keys must still include Escape: {close_keys:?}"
    );
}

#[test]
fn lsp_hover_wraps_long_lines_to_max_float_width_to_keep_text_visible() {
    let mut manager = FloatingWindowManager::default();
    // 70 文字を大きく超える 1 行を作る（border 込み 72 セルに収まらない長さ）
    let long_line = "alpha ".repeat(40);
    let response = json!({
        "result": {
            "contents": { "kind": "plaintext", "value": long_line },
        }
    });
    let id = open_lsp_hover_float(
        &mut manager,
        LspHoverFloatRequest {
            window_id: 7,
            cursor_row: 1,
            cursor_col: 1,
            response,
            size_limit: default_limit(),
        },
    )
    .expect("hover should open")
    .id();
    let lines = manager
        .debug_window(id)
        .expect("hover float should exist")
        .lines
        .clone();
    assert!(
        lines.len() >= 2,
        "long single-line content must be wrapped into multiple rendered lines: {lines:?}"
    );
    for (index, line) in lines.iter().enumerate() {
        let width: usize = line
            .chars()
            .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0))
            .sum();
        assert!(
            width <= 70,
            "wrapped line[{index}] width {width} must not exceed inner float width 70: {line:?}"
        );
    }
}

#[test]
fn lsp_hover_respects_absolute_size_limit_for_wrapping_and_outer_size() {
    let mut manager = FloatingWindowManager::default();
    let response = json!({
        "result": {
            "contents": { "kind": "plaintext", "value": "x".repeat(80) },
        }
    });
    let id = open_lsp_hover_float(
        &mut manager,
        LspHoverFloatRequest {
            window_id: 7,
            cursor_row: 1,
            cursor_col: 1,
            response,
            size_limit: ResolvedPopupSizeLimit::bordered(20, 5),
        },
    )
    .expect("hover should open")
    .id();
    let window = manager.debug_window(id).expect("hover float should exist");
    assert_eq!(window.size.width, 20);
    assert_eq!(window.size.height, 5);

    let floats =
        manager.resolve_screen_models_with_cursors(80, 24, &[pane(7)], &[(7, 1, 1)], Some(7));
    let lines = &floats[0].lines;
    for (index, line) in lines.iter().enumerate() {
        let width: usize = line
            .chars()
            .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0))
            .sum();
        assert!(
            width <= 18,
            "wrapped line[{index}] width {width} must not exceed configured inner width 18: {line:?}"
        );
    }
}

#[test]
fn lsp_hover_renders_markdown_content_via_markdown_render_pipeline() {
    let mut manager = FloatingWindowManager::default();
    let response = json!({
        "result": {
            "contents": {
                "kind": "markdown",
                "value": "**foo** is a function\n\n```rust\nfn foo() {}\n```\n",
            }
        }
    });
    let id = open_lsp_hover_float(
        &mut manager,
        LspHoverFloatRequest {
            window_id: 7,
            cursor_row: 1,
            cursor_col: 1,
            response,
            size_limit: default_limit(),
        },
    )
    .expect("hover should open")
    .id();
    let lines = manager
        .debug_window(id)
        .expect("hover float should exist")
        .lines
        .clone();
    assert!(
        lines.iter().any(|line| line.contains("**foo**")),
        "markdown emphasis markers must be preserved in rendered lines: {lines:?}"
    );
    assert!(
        !lines.iter().any(|line| line.trim().starts_with("```")),
        "fenced code block markers must be stripped from rendered lines: {lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("fn foo()")),
        "code body must remain after stripping fence markers: {lines:?}"
    );
}
