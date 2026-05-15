use saya::selector_host_adapter::{
    HeadlessSelectorUiProjectionSink, SelectorHostViewAdapter, SelectorUiIntent,
};
use saya::selector_runtime::{
    RuntimeRenderedSelectorItem, RuntimeSelectorCollectStatus, RuntimeSelectorHighlight,
    RuntimeSelectorMatchStatus, RuntimeSelectorStatus, RuntimeSelectorStorageMode,
    RuntimeSelectorStoreStatus, RuntimeSelectorWorkState, SelectorViewBackend,
    SelectorViewBackendInput,
};
use std::sync::Arc;

#[test]
fn host_adapter_projects_backend_input_into_visible_ui_rows() {
    let sink = Arc::new(HeadlessSelectorUiProjectionSink::new());
    let adapter = SelectorHostViewAdapter::new(sink.clone(), 3);

    adapter.render(selector_input(
        4,
        3,
        false,
        false,
        RuntimeSelectorWorkState::Completed,
    ));

    let projections = sink.projections();
    assert_eq!(projections.len(), 1);
    let projection = &projections[0];

    assert_eq!(projection.session_id, 7);
    assert_eq!(projection.query, "needle");
    assert_eq!(projection.cursor, 4);
    assert_eq!(projection.offset, 3);
    assert!(!projection.hidden);
    assert!(!projection.cancelled);
    assert_eq!(projection.intent, SelectorUiIntent::Render);
    assert!(!projection.should_dispose_session);
    assert_eq!(
        projection
            .visible_rows
            .iter()
            .map(|row| (row.index, row.item.id.as_str(), row.selected))
            .collect::<Vec<_>>(),
        vec![(3, "row-3", false), (4, "row-4", true), (5, "row-5", false)]
    );
    assert_eq!(
        projection
            .selected_row
            .as_ref()
            .map(|row| (row.index, row.item.id.as_str(), row.selected)),
        Some((4, "row-4", true))
    );
    assert_eq!(
        projection
            .selected_item
            .as_ref()
            .map(|item| item.id.as_str()),
        Some("row-4")
    );
    assert_eq!(projection.status.collect.total_stored, 8);
    assert_eq!(projection.status.match_status.total_matched, 8);
    assert_eq!(projection.status.store.total_stored, 8);
}

#[test]
fn host_adapter_projects_hide_without_session_dispose_intent() {
    let sink = Arc::new(HeadlessSelectorUiProjectionSink::new());
    let adapter = SelectorHostViewAdapter::new(sink.clone(), 5);

    adapter.render(selector_input(
        2,
        0,
        true,
        false,
        RuntimeSelectorWorkState::Completed,
    ));

    let projection = sink.projections().pop().expect("projection is recorded");
    assert_eq!(projection.intent, SelectorUiIntent::Hide);
    assert!(projection.hidden);
    assert!(!projection.cancelled);
    assert!(!projection.should_dispose_session);
    assert_eq!(
        projection.status.match_status.state,
        RuntimeSelectorWorkState::Completed
    );
}

#[test]
fn host_adapter_projects_cancel_as_closed_cancelled_status() {
    let sink = Arc::new(HeadlessSelectorUiProjectionSink::new());
    let adapter = SelectorHostViewAdapter::new(sink.clone(), 5);

    adapter.render(selector_input(
        2,
        0,
        true,
        true,
        RuntimeSelectorWorkState::Cancelled,
    ));

    let projection = sink.projections().pop().expect("projection is recorded");
    assert_eq!(projection.intent, SelectorUiIntent::Cancel);
    assert!(projection.hidden);
    assert!(projection.cancelled);
    assert!(!projection.should_dispose_session);
    assert_eq!(
        projection.status.match_status.state,
        RuntimeSelectorWorkState::Cancelled
    );
}

fn selector_input(
    cursor: usize,
    offset: usize,
    hidden: bool,
    cancelled: bool,
    match_state: RuntimeSelectorWorkState,
) -> SelectorViewBackendInput {
    let rendered_items = (0..8)
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
        session_id: 7,
        query: "needle".to_string(),
        rendered_items,
        selected_item,
        cursor,
        offset,
        hidden,
        cancelled,
        status: RuntimeSelectorStatus {
            collect: RuntimeSelectorCollectStatus {
                state: RuntimeSelectorWorkState::Completed,
                total_seen: 8,
                total_stored: 8,
                storage: RuntimeSelectorStorageMode::Memory,
                error_message: None,
            },
            match_status: RuntimeSelectorMatchStatus {
                state: match_state,
                total_matched: 8,
                total_rendered: 8,
                error_message: None,
            },
            store: RuntimeSelectorStoreStatus {
                storage: RuntimeSelectorStorageMode::Memory,
                total_stored: 8,
                estimated_bytes: Some(64),
                temp_file_path: None,
            },
        },
    }
}
