use saya::floating_window::{
    FloatingLifecycle, FloatingLifecycleEvent, FloatingLifecycleOutcome, FloatingRelativeTo,
    FloatingWindowManager, WorkspaceFocus,
};
use saya::lsp_float::{
    LspDiagnosticFloatRequest, LspHoverFloatRequest, hover_lines_from_lsp_value,
    open_lsp_diagnostic_float, open_lsp_hover_float,
};
use saya::screen_model::PaneRect;
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
    assert_eq!(
        hover_lines_from_lsp_value(&json!({
            "contents": [
                "first",
                { "language": "rust", "value": "let value = 1;" }
            ]
        })),
        vec![
            "first".to_string(),
            "rust:".to_string(),
            "let value = 1;".to_string()
        ]
    );
}

#[test]
fn lsp_hover_float_opens_near_cursor_replaces_previous_hover_and_closes_on_cursor_move() {
    let mut manager = FloatingWindowManager::default();
    let first = open_lsp_hover_float(
        &mut manager,
        LspHoverFloatRequest {
            window_id: 7,
            cursor_row: 4,
            cursor_col: 9,
            response: json!({ "result": { "contents": "old hover" } }),
        },
    )
    .expect("first hover should open");
    assert!(manager.focus_float(first));

    let second = open_lsp_hover_float(
        &mut manager,
        LspHoverFloatRequest {
            window_id: 7,
            cursor_row: 4,
            cursor_col: 9,
            response: json!({ "result": { "contents": "new hover\nsecond line" } }),
        },
    )
    .expect("second hover should replace first");

    let floats =
        manager.resolve_screen_models_with_cursors(80, 24, &[pane(7)], &[(7, 4, 9)], Some(7));
    assert_eq!(floats.len(), 1);
    assert_eq!(floats[0].id, second);
    assert_eq!(floats[0].lines, vec!["new hover", "second line"]);
    assert_eq!(floats[0].rect.x, 11);
    assert_eq!(floats[0].rect.y, 8);
    assert_eq!(manager.focus(), None);
    assert!(manager.focus_float(second));

    assert_eq!(
        manager.apply_lifecycle_event(
            FloatingLifecycleEvent::CursorMoved {
                window_id: 7,
                row: 5,
                col: 9,
            },
            Some(7),
        ),
        FloatingLifecycleOutcome {
            closed: vec![second]
        }
    );
    assert_eq!(manager.focus(), Some(WorkspaceFocus::Pane { window_id: 7 }));
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
fn lsp_hover_uses_close_on_cursor_move_lifecycle_with_independent_replacement_group() {
    let mut manager = FloatingWindowManager::default();
    let id = open_lsp_hover_float(
        &mut manager,
        LspHoverFloatRequest {
            window_id: 3,
            cursor_row: 1,
            cursor_col: 1,
            response: json!({ "result": { "contents": "hover" } }),
        },
    )
    .expect("hover should open");

    let window = manager.debug_window(id).expect("hover float should exist");
    assert_eq!(window.lifecycle, FloatingLifecycle::CloseOnCursorMove);
    assert_eq!(window.replacement_group.as_deref(), Some("lsp:hover"));
}
