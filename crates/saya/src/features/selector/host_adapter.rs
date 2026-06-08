use std::sync::{Arc, Mutex};

use crate::features::selector::runtime::{
    RuntimeRenderedSelectorItem, RuntimeSelectorStatus, RuntimeSelectorUiOptions,
    SelectorViewBackend, SelectorViewBackendInput,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorUiProjection {
    pub session_id: u64,
    pub query: String,
    pub rendered_items: Vec<RuntimeRenderedSelectorItem>,
    pub visible_rows: Vec<SelectorUiRow>,
    pub selected_row: Option<SelectorUiRow>,
    pub selected_item: Option<RuntimeRenderedSelectorItem>,
    pub cursor: usize,
    pub offset: usize,
    pub hidden: bool,
    pub cancelled: bool,
    pub status: RuntimeSelectorStatus,
    pub ui: RuntimeSelectorUiOptions,
    pub status_text: String,
    pub intent: SelectorUiIntent,
    pub should_dispose_session: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorUiRow {
    pub index: usize,
    pub item: RuntimeRenderedSelectorItem,
    pub selected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorUiIntent {
    Render,
    Hide,
    Cancel,
}

pub trait SelectorUiProjectionSink: Send + Sync + 'static {
    fn render_projection(&self, projection: SelectorUiProjection);
}

pub struct SelectorHostViewAdapter {
    sink: Arc<dyn SelectorUiProjectionSink>,
    visible_row_limit: usize,
}

impl SelectorHostViewAdapter {
    pub fn new(sink: Arc<dyn SelectorUiProjectionSink>, visible_row_limit: usize) -> Self {
        Self {
            sink,
            visible_row_limit: visible_row_limit.max(1),
        }
    }

    pub fn project(&self, input: SelectorViewBackendInput) -> SelectorUiProjection {
        project_selector_view_input(input, self.visible_row_limit)
    }
}

impl SelectorViewBackend for SelectorHostViewAdapter {
    fn render(&self, input: SelectorViewBackendInput) {
        let projection = self.project(input);
        log::debug!(
            "[selector_host_adapter] render projection: id={}, visible_rows={}, cursor={}, offset={}, hidden={}, cancelled={}, intent={:?}",
            projection.session_id,
            projection.visible_rows.len(),
            projection.cursor,
            projection.offset,
            projection.hidden,
            projection.cancelled,
            projection.intent
        );
        self.sink.render_projection(projection);
    }
}

#[derive(Debug, Default)]
pub struct HeadlessSelectorUiProjectionSink {
    projections: Mutex<Vec<SelectorUiProjection>>,
}

impl HeadlessSelectorUiProjectionSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn projections(&self) -> Vec<SelectorUiProjection> {
        self.projections
            .lock()
            .expect("headless selector ui projection sink poisoned")
            .clone()
    }
}

impl SelectorUiProjectionSink for HeadlessSelectorUiProjectionSink {
    fn render_projection(&self, projection: SelectorUiProjection) {
        log::debug!(
            "[selector_host_adapter] record headless projection: id={}, visible_rows={}, intent={:?}",
            projection.session_id,
            projection.visible_rows.len(),
            projection.intent
        );
        self.projections
            .lock()
            .expect("headless selector ui projection sink poisoned")
            .push(projection);
    }
}

fn project_selector_view_input(
    input: SelectorViewBackendInput,
    visible_row_limit: usize,
) -> SelectorUiProjection {
    let SelectorViewBackendInput {
        session_id,
        query,
        rendered_items,
        selected_item,
        cursor,
        offset,
        hidden,
        cancelled,
        status,
        ui,
    } = input;

    let visible_rows = rendered_items
        .iter()
        .cloned()
        .enumerate()
        .skip(offset)
        .take(visible_row_limit)
        .map(|(index, item)| SelectorUiRow {
            index,
            item,
            selected: index == cursor,
        })
        .collect::<Vec<_>>();
    let selected_row = visible_rows.iter().find(|row| row.selected).cloned();
    let intent = if cancelled {
        SelectorUiIntent::Cancel
    } else if hidden {
        SelectorUiIntent::Hide
    } else {
        SelectorUiIntent::Render
    };
    let status_text = selector_status_text(&status, cancelled);

    SelectorUiProjection {
        session_id,
        query,
        rendered_items,
        visible_rows,
        selected_row,
        selected_item,
        cursor,
        offset,
        hidden,
        cancelled,
        status,
        ui,
        status_text,
        intent,
        should_dispose_session: false,
    }
}

fn selector_status_text(status: &RuntimeSelectorStatus, cancelled: bool) -> String {
    if cancelled {
        return "cancelled".to_string();
    }

    format!(
        "collect={:?} seen={} stored={} match={:?} matched={} rendered={} store={}",
        status.collect.state,
        status.collect.total_seen,
        status.collect.total_stored,
        status.match_status.state,
        status.match_status.total_matched,
        status.match_status.total_rendered,
        status.store.total_stored
    )
}
