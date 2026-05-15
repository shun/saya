use std::sync::Mutex;

use crate::features::selector::host_adapter::{
    SelectorUiIntent, SelectorUiProjection, SelectorUiProjectionSink, SelectorUiRow,
};
use crate::features::selector::runtime::RuntimeSelectorStatus;
use crate::{
    presentation::floating_window::{
        FloatingBorder, FloatingChrome, FloatingContentRef, FloatingInlineStyle,
        FloatingScreenModel, FloatingWindowId,
    },
    presentation::screen_model::PaneRect,
};
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorTuiViewModel {
    pub session_id: u64,
    pub query: String,
    pub visible_rows: Vec<SelectorUiRow>,
    pub selected_row: Option<SelectorUiRow>,
    pub status: RuntimeSelectorStatus,
    pub status_text: String,
    pub hidden: bool,
    pub cancelled: bool,
    pub intent: SelectorUiIntent,
    pub should_dispose_session: bool,
}

impl From<SelectorUiProjection> for SelectorTuiViewModel {
    fn from(projection: SelectorUiProjection) -> Self {
        Self {
            session_id: projection.session_id,
            query: projection.query,
            visible_rows: projection.visible_rows,
            selected_row: projection.selected_row,
            status: projection.status,
            status_text: projection.status_text,
            hidden: projection.hidden,
            cancelled: projection.cancelled,
            intent: projection.intent,
            should_dispose_session: projection.should_dispose_session,
        }
    }
}

#[derive(Debug, Default)]
struct SelectorTuiProjectionSinkState {
    current_model: Option<SelectorTuiViewModel>,
    projection_count: usize,
    visible: bool,
}

#[derive(Debug, Default)]
pub struct SelectorTuiProjectionSink {
    state: Mutex<SelectorTuiProjectionSinkState>,
}

impl SelectorTuiProjectionSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn current_model(&self) -> Option<SelectorTuiViewModel> {
        self.state
            .lock()
            .expect("selector TUI projection sink poisoned")
            .current_model
            .clone()
    }

    pub fn is_visible(&self) -> bool {
        self.state
            .lock()
            .expect("selector TUI projection sink poisoned")
            .visible
    }

    pub fn projection_count(&self) -> usize {
        self.state
            .lock()
            .expect("selector TUI projection sink poisoned")
            .projection_count
    }

    pub fn float_launch_count(&self) -> usize {
        0
    }
}

impl SelectorUiProjectionSink for SelectorTuiProjectionSink {
    fn render_projection(&self, projection: SelectorUiProjection) {
        log::debug!(
            "[selector_tui_state] apply selector projection: id={}, intent={:?}, rows={}, hidden={}, cancelled={}, dispose={}",
            projection.session_id,
            projection.intent,
            projection.visible_rows.len(),
            projection.hidden,
            projection.cancelled,
            projection.should_dispose_session
        );

        let visible = matches!(projection.intent, SelectorUiIntent::Render);
        let model = SelectorTuiViewModel::from(projection);
        let mut state = self
            .state
            .lock()
            .expect("selector TUI projection sink poisoned");
        state.current_model = Some(model);
        state.visible = visible;
        state.projection_count += 1;
    }
}

pub fn selector_tui_model_to_workspace_float(
    model: &SelectorTuiViewModel,
    terminal_width: u16,
    terminal_height: u16,
) -> Option<FloatingScreenModel> {
    if !matches!(model.intent, SelectorUiIntent::Render) || model.hidden || model.cancelled {
        log::debug!(
            "[selector_tui_state] skip selector workspace float projection: id={}, intent={:?}, hidden={}, cancelled={}",
            model.session_id,
            model.intent,
            model.hidden,
            model.cancelled
        );
        return None;
    }

    let lines = selector_tui_static_lines(model);
    let content_width = lines
        .iter()
        .map(|line| UnicodeWidthStr::width(line.as_str()))
        .max()
        .unwrap_or(1);
    let border_padding = 2usize;
    let width = u16::try_from(content_width.saturating_add(border_padding))
        .unwrap_or(u16::MAX)
        .min(terminal_width.max(1))
        .max(1);
    let height = u16::try_from(lines.len().saturating_add(border_padding))
        .unwrap_or(u16::MAX)
        .min(terminal_height.max(1))
        .max(1);
    let x = terminal_width.saturating_sub(width) / 2;
    let y = terminal_height.saturating_sub(height) / 3;
    let float_id = selector_tui_float_id(model.session_id);

    log::debug!(
        "[selector_tui_state] project selector model to workspace float: id={}, float_id={}, lines={}, rect=({}, {}, {}, {})",
        model.session_id,
        float_id.0,
        lines.len(),
        x,
        y,
        width,
        height
    );

    Some(FloatingScreenModel {
        id: float_id,
        content: FloatingContentRef::StaticLines {
            content_id: model.session_id,
        },
        rect: PaneRect {
            x,
            y,
            width,
            height,
        },
        lines,
        inline_styles: Vec::<FloatingInlineStyle>::new(),
        focusable: false,
        mouse: false,
        chrome: FloatingChrome {
            border: FloatingBorder::Single,
        },
        zindex: 180,
        creation_order: u64::MAX.saturating_sub(model.session_id),
    })
}

fn selector_tui_static_lines(model: &SelectorTuiViewModel) -> Vec<String> {
    let mut lines = Vec::with_capacity(model.visible_rows.len().saturating_add(2));
    lines.push(format!("query: {}", model.query));
    lines.extend(model.visible_rows.iter().map(|row| {
        let marker = if row.selected { '>' } else { ' ' };
        format!("{marker} {}", row.item.label)
    }));
    lines.push(model.status_text.clone());
    lines
}

fn selector_tui_float_id(session_id: u64) -> FloatingWindowId {
    FloatingWindowId(u64::MAX.saturating_sub(session_id))
}
