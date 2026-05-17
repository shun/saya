use std::sync::Arc;

use saya::features::selector::host_adapter::{SelectorHostViewAdapter, SelectorUiIntent};
use saya::features::selector::runtime::{
    RuntimeRenderedSelectorItem, RuntimeSelectorCollectStatus, RuntimeSelectorHighlight,
    RuntimeSelectorMatchStatus, RuntimeSelectorStatus, RuntimeSelectorStorageMode,
    RuntimeSelectorStoreStatus, RuntimeSelectorUiOptions, RuntimeSelectorWindowPercent,
    RuntimeSelectorWindowSizeValue, RuntimeSelectorWindowUiOptions, RuntimeSelectorWorkState,
    SelectorViewBackend, SelectorViewBackendInput,
};
use saya::features::selector::tui_state::{
    SelectorTuiProjectionSink, selector_tui_model_to_workspace_float,
};

#[test]
fn tui_selector_state_keeps_render_projection_as_draw_ready_model() {
    let tui_state = Arc::new(SelectorTuiProjectionSink::new());
    let adapter = SelectorHostViewAdapter::new(tui_state.clone(), 3);

    adapter.render(selector_input(
        "needle",
        3,
        2,
        false,
        false,
        RuntimeSelectorWorkState::Completed,
    ));

    let model = tui_state
        .current_model()
        .expect("render projection should produce TUI selector model");
    assert!(tui_state.is_visible());
    assert_eq!(model.session_id, 42);
    assert_eq!(model.query, "needle");
    assert_eq!(
        model
            .visible_rows
            .iter()
            .map(|row| (row.index, row.item.id.as_str(), row.selected))
            .collect::<Vec<_>>(),
        vec![(2, "row-2", false), (3, "row-3", true), (4, "row-4", false)]
    );
    assert_eq!(
        model
            .selected_row
            .as_ref()
            .map(|row| (row.index, row.item.id.as_str())),
        Some((3, "row-3"))
    );
    assert_eq!(
        model.status.match_status.state,
        RuntimeSelectorWorkState::Completed
    );
    assert!(!model.cancelled);
    assert_eq!(model.intent, SelectorUiIntent::Render);
    assert!(!model.should_dispose_session);
    assert_eq!(tui_state.projection_count(), 1);
    assert_eq!(
        tui_state.float_launch_count(),
        0,
        "TUI state sink must not start a floating or split UI"
    );
}

#[test]
fn tui_selector_state_hides_without_disposing_session() {
    let tui_state = Arc::new(SelectorTuiProjectionSink::new());
    let adapter = SelectorHostViewAdapter::new(tui_state.clone(), 5);

    adapter.render(selector_input(
        "needle",
        1,
        0,
        false,
        false,
        RuntimeSelectorWorkState::Completed,
    ));
    adapter.render(selector_input(
        "needle",
        1,
        0,
        true,
        false,
        RuntimeSelectorWorkState::Completed,
    ));

    let model = tui_state
        .current_model()
        .expect("hide projection should retain the last selector model");
    assert!(!tui_state.is_visible());
    assert_eq!(model.intent, SelectorUiIntent::Hide);
    assert!(model.hidden);
    assert!(!model.cancelled);
    assert!(!model.should_dispose_session);
    assert_eq!(
        model.status.match_status.state,
        RuntimeSelectorWorkState::Completed
    );
    assert_eq!(tui_state.projection_count(), 2);
}

#[test]
fn tui_selector_state_cancels_as_hidden_cancelled_model_without_dispose() {
    let tui_state = Arc::new(SelectorTuiProjectionSink::new());
    let adapter = SelectorHostViewAdapter::new(tui_state.clone(), 5);

    adapter.render(selector_input(
        "needle",
        1,
        0,
        true,
        true,
        RuntimeSelectorWorkState::Cancelled,
    ));

    let model = tui_state
        .current_model()
        .expect("cancel projection should retain cancelled selector model");
    assert!(!tui_state.is_visible());
    assert_eq!(model.intent, SelectorUiIntent::Cancel);
    assert!(model.hidden);
    assert!(model.cancelled);
    assert!(!model.should_dispose_session);
    assert_eq!(
        model.status.match_status.state,
        RuntimeSelectorWorkState::Cancelled
    );
    assert_eq!(tui_state.projection_count(), 1);
}

#[test]
fn visible_tui_selector_model_projects_to_static_workspace_float() {
    let tui_state = Arc::new(SelectorTuiProjectionSink::new());
    let adapter = SelectorHostViewAdapter::new(tui_state.clone(), 3);

    adapter.render(selector_input(
        "needle",
        3,
        2,
        false,
        false,
        RuntimeSelectorWorkState::Completed,
    ));

    let model = tui_state
        .current_model()
        .expect("render projection should produce TUI selector model");
    let float = selector_tui_model_to_workspace_float(&model, 80, 24)
        .expect("visible selector model should project to workspace float");

    assert_eq!(float.lines[0], "query: needle");
    assert_eq!(
        &float.lines[1..4],
        ["  row 2", "> row 3", "  row 4"],
        "selected row should be visible in the static selector float"
    );
    assert!(
        float
            .lines
            .last()
            .expect("status summary line")
            .contains("matched=6 rendered=6"),
        "status summary should include runtime selector counts"
    );
    assert!(!float.focusable);
}

#[test]
fn selector_window_ui_options_control_float_rect_and_derives_rows_from_height() {
    let tui_state = Arc::new(SelectorTuiProjectionSink::new());
    let adapter = SelectorHostViewAdapter::new(tui_state.clone(), 10);

    let mut input = selector_input(
        "needle",
        0,
        0,
        false,
        false,
        RuntimeSelectorWorkState::Completed,
    );
    input.ui.window = Some(RuntimeSelectorWindowUiOptions {
        width: Some(RuntimeSelectorWindowSizeValue::Percent(
            RuntimeSelectorWindowPercent(75),
        )),
        height: Some(RuntimeSelectorWindowSizeValue::Cells(9)),
    });
    adapter.render(input);

    let model = tui_state
        .current_model()
        .expect("render projection should produce TUI selector model");
    let float = selector_tui_model_to_workspace_float(&model, 120, 30)
        .expect("visible selector model should project to workspace float");
    assert_eq!(float.rect.width, 90);
    assert_eq!(float.rect.height, 9);
    assert_eq!(float.rect.x, 15);
    assert_eq!(float.rect.y, 7);
    assert_eq!(
        float.lines.len(),
        7,
        "height 9 leaves 5 content rows after border/query/status"
    );
}

#[test]
fn selector_window_height_keeps_cursor_visible_without_ten_row_page_jump() {
    let tui_state = Arc::new(SelectorTuiProjectionSink::new());
    let adapter = SelectorHostViewAdapter::new(tui_state.clone(), 10);

    let mut input = selector_input_with_len(
        "needle",
        10,
        10,
        50,
        false,
        false,
        RuntimeSelectorWorkState::Completed,
    );
    input.ui.window = Some(RuntimeSelectorWindowUiOptions {
        width: None,
        height: Some(RuntimeSelectorWindowSizeValue::Cells(20)),
    });
    adapter.render(input);

    let model = tui_state
        .current_model()
        .expect("render projection should produce TUI selector model");
    let float = selector_tui_model_to_workspace_float(&model, 120, 30)
        .expect("visible selector model should project to workspace float");

    assert_eq!(float.rect.height, 20);
    assert_eq!(
        float.lines[1], "  row 0",
        "height-derived rows should not treat the fixed controller offset as a page reset"
    );
    assert_eq!(
        float.lines[11], "> row 10",
        "cursor should remain at its natural row instead of jumping to the top"
    );
}

#[test]
fn hidden_or_cancelled_tui_selector_model_projects_to_no_workspace_float() {
    let tui_state = Arc::new(SelectorTuiProjectionSink::new());
    let adapter = SelectorHostViewAdapter::new(tui_state.clone(), 3);

    adapter.render(selector_input(
        "needle",
        3,
        2,
        true,
        false,
        RuntimeSelectorWorkState::Completed,
    ));
    let hidden = tui_state
        .current_model()
        .expect("hide projection should retain TUI selector state");
    assert!(selector_tui_model_to_workspace_float(&hidden, 80, 24).is_none());

    adapter.render(selector_input(
        "needle",
        3,
        2,
        true,
        true,
        RuntimeSelectorWorkState::Cancelled,
    ));
    let cancelled = tui_state
        .current_model()
        .expect("cancel projection should retain TUI selector state");
    assert!(cancelled.cancelled);
    assert!(!cancelled.should_dispose_session);
    assert!(selector_tui_model_to_workspace_float(&cancelled, 80, 24).is_none());
}

fn selector_input(
    query: &str,
    cursor: usize,
    offset: usize,
    hidden: bool,
    cancelled: bool,
    match_state: RuntimeSelectorWorkState,
) -> SelectorViewBackendInput {
    selector_input_with_len(query, cursor, offset, 6, hidden, cancelled, match_state)
}

fn selector_input_with_len(
    query: &str,
    cursor: usize,
    offset: usize,
    item_len: usize,
    hidden: bool,
    cancelled: bool,
    match_state: RuntimeSelectorWorkState,
) -> SelectorViewBackendInput {
    let rendered_items = (0..item_len)
        .map(|index| RuntimeRenderedSelectorItem {
            id: format!("row-{index}"),
            label: format!("row {index}"),
            kind: "test".to_string(),
            detail: serde_json::json!({ "index": index }),
            highlights: Vec::<RuntimeSelectorHighlight>::new(),
        })
        .collect::<Vec<_>>();
    let selected_item = rendered_items.get(cursor).cloned();

    SelectorViewBackendInput {
        session_id: 42,
        query: query.to_string(),
        rendered_items,
        selected_item,
        cursor,
        offset,
        hidden,
        cancelled,
        status: RuntimeSelectorStatus {
            collect: RuntimeSelectorCollectStatus {
                state: RuntimeSelectorWorkState::Completed,
                total_seen: item_len,
                total_stored: item_len,
                storage: RuntimeSelectorStorageMode::Memory,
                error_message: None,
            },
            match_status: RuntimeSelectorMatchStatus {
                state: match_state,
                total_matched: item_len,
                total_rendered: item_len,
                error_message: None,
            },
            store: RuntimeSelectorStoreStatus {
                storage: RuntimeSelectorStorageMode::Memory,
                total_stored: item_len,
                estimated_bytes: Some(48),
                temp_file_path: None,
            },
        },
        ui: RuntimeSelectorUiOptions::default(),
    }
}
