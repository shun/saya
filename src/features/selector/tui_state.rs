use std::sync::Mutex;

use crate::features::selector::host_adapter::{
    SelectorUiIntent, SelectorUiProjection, SelectorUiProjectionSink, SelectorUiRow,
};
use crate::features::selector::runtime::RuntimeSelectorStatus;
use crate::features::selector::runtime::{
    RuntimeSelectorHighlightKind, RuntimeSelectorUiOptions, RuntimeSelectorWindowSizeValue,
};
use crate::{
    presentation::floating_window::{
        FloatingBorder, FloatingChrome, FloatingContentRef, FloatingCursor, FloatingInlineStyle,
        FloatingInlineStyleKind, FloatingScreenModel, FloatingWindowId,
    },
    presentation::screen_model::PaneRect,
};
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorTuiViewModel {
    pub session_id: u64,
    pub query: String,
    pub mode: SelectorMode,
    pub focused_part: SelectorTuiPart,
    pub rendered_items: Vec<crate::features::selector::runtime::RuntimeRenderedSelectorItem>,
    pub visible_rows: Vec<SelectorUiRow>,
    pub selected_row: Option<SelectorUiRow>,
    pub cursor: usize,
    pub offset: usize,
    pub status: RuntimeSelectorStatus,
    pub ui: RuntimeSelectorUiOptions,
    pub status_text: String,
    pub hidden: bool,
    pub cancelled: bool,
    pub intent: SelectorUiIntent,
    pub should_dispose_session: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorMode {
    Insert,
    Normal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorTuiPart {
    FilterInput,
    CandidateList,
}

impl From<SelectorUiProjection> for SelectorTuiViewModel {
    fn from(projection: SelectorUiProjection) -> Self {
        Self {
            session_id: projection.session_id,
            query: projection.query,
            mode: SelectorMode::Insert,
            focused_part: SelectorTuiPart::FilterInput,
            rendered_items: projection.rendered_items,
            visible_rows: projection.visible_rows,
            selected_row: projection.selected_row,
            cursor: projection.cursor,
            offset: projection.offset,
            status: projection.status,
            ui: projection.ui,
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
    mode: Option<SelectorTuiModeState>,
    viewport: Option<SelectorTuiViewportState>,
    projection_count: usize,
    visible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SelectorTuiModeState {
    session_id: u64,
    mode: SelectorMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SelectorTuiViewportState {
    session_id: u64,
    row_limit: usize,
    start: usize,
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

    pub fn set_mode(&self, session_id: u64, mode: SelectorMode) -> Result<(), SelectorModeError> {
        let mut state = self
            .state
            .lock()
            .expect("selector TUI projection sink poisoned");
        let Some(model) = state.current_model.as_mut() else {
            return Err(SelectorModeError::NoActiveSelector);
        };
        if model.session_id != session_id {
            return Err(SelectorModeError::SessionMismatch {
                expected: model.session_id,
                actual: session_id,
            });
        }
        log::info!(
            "[selector_tui_state] switch selector mode: id={}, from={:?}, to={:?}",
            session_id,
            model.mode,
            mode
        );
        model.mode = mode;
        model.focused_part = selector_tui_part_for_mode(mode);
        state.mode = Some(SelectorTuiModeState { session_id, mode });
        state.projection_count += 1;
        Ok(())
    }

    pub fn workspace_float(
        &self,
        terminal_width: u16,
        terminal_height: u16,
    ) -> Option<FloatingScreenModel> {
        let mut state = self
            .state
            .lock()
            .expect("selector TUI projection sink poisoned");
        let model = state.current_model.clone()?;
        let row_limit = selector_tui_content_row_limit(&model, terminal_height);
        let previous_start = state.viewport.and_then(|viewport| {
            (viewport.session_id == model.session_id && viewport.row_limit == row_limit)
                .then_some(viewport.start)
        });
        let start = selector_tui_visible_start_with_previous(&model, row_limit, previous_start);
        state.viewport = Some(SelectorTuiViewportState {
            session_id: model.session_id,
            row_limit,
            start,
        });
        selector_tui_model_to_workspace_float_with_start(
            &model,
            terminal_width,
            terminal_height,
            row_limit,
            start,
        )
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
        let mut model = SelectorTuiViewModel::from(projection);
        let mut state = self
            .state
            .lock()
            .expect("selector TUI projection sink poisoned");
        if !visible {
            state.viewport = None;
        }
        let mode = state
            .mode
            .filter(|mode| mode.session_id == model.session_id)
            .map(|mode| mode.mode)
            .unwrap_or(SelectorMode::Insert);
        model.mode = mode;
        model.focused_part = selector_tui_part_for_mode(mode);
        state.current_model = Some(model);
        state.visible = visible;
        state.projection_count += 1;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorModeError {
    NoActiveSelector,
    SessionMismatch { expected: u64, actual: u64 },
}

fn selector_tui_part_for_mode(mode: SelectorMode) -> SelectorTuiPart {
    match mode {
        SelectorMode::Insert => SelectorTuiPart::FilterInput,
        SelectorMode::Normal => SelectorTuiPart::CandidateList,
    }
}

pub fn selector_tui_model_to_workspace_float(
    model: &SelectorTuiViewModel,
    terminal_width: u16,
    terminal_height: u16,
) -> Option<FloatingScreenModel> {
    let row_limit = selector_tui_content_row_limit(model, terminal_height);
    let start = selector_tui_visible_start(model, row_limit);
    selector_tui_model_to_workspace_float_with_start(
        model,
        terminal_width,
        terminal_height,
        row_limit,
        start,
    )
}

fn selector_tui_model_to_workspace_float_with_start(
    model: &SelectorTuiViewModel,
    terminal_width: u16,
    terminal_height: u16,
    row_limit: usize,
    start: usize,
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

    let window = model.ui.window;
    let configured_height = selector_tui_configured_height(model, terminal_height);
    let rows = selector_tui_visible_rows_from_start(model, row_limit, start);
    let lines = selector_tui_static_lines(model, &rows);
    let inline_styles = selector_tui_inline_styles(&rows);
    let cursor = selector_tui_cursor(model, &rows);
    let content_width = lines
        .iter()
        .map(|line| UnicodeWidthStr::width(line.as_str()))
        .max()
        .unwrap_or(1);
    let border_padding = 2usize;
    let content_based_width = u16::try_from(content_width.saturating_add(border_padding))
        .unwrap_or(u16::MAX)
        .min(terminal_width.max(1))
        .max(1);
    let content_based_height = u16::try_from(lines.len().saturating_add(border_padding))
        .unwrap_or(u16::MAX)
        .min(terminal_height.max(1))
        .max(1);
    let width = window
        .and_then(|window| window.width)
        .map(|value| resolve_selector_window_size_value(value, terminal_width))
        .unwrap_or(content_based_width)
        .min(terminal_width.max(1))
        .max(1);
    let height = configured_height
        .unwrap_or(content_based_height)
        .min(terminal_height.max(1))
        .max(1);
    let x = terminal_width.saturating_sub(width) / 2;
    let y = terminal_height.saturating_sub(height) / 3;
    let float_id = selector_tui_float_id(model.session_id);

    log::debug!(
        "[selector_tui_state] project selector model to workspace float: id={}, float_id={}, lines={}, inline_styles={}, row_limit={}, start={}, cursor={}, offset={}, rect=({}, {}, {}, {})",
        model.session_id,
        float_id.0,
        lines.len(),
        inline_styles.len(),
        row_limit,
        start,
        model.cursor,
        model.offset,
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
        inline_styles,
        cursor,
        focusable: false,
        mouse: false,
        chrome: FloatingChrome {
            border: FloatingBorder::Single,
        },
        zindex: 180,
        creation_order: u64::MAX.saturating_sub(model.session_id),
    })
}

fn selector_tui_configured_height(
    model: &SelectorTuiViewModel,
    terminal_height: u16,
) -> Option<u16> {
    model
        .ui
        .window
        .and_then(|window| window.height)
        .map(|value| resolve_selector_window_size_value(value, terminal_height))
}

fn selector_tui_content_row_limit(model: &SelectorTuiViewModel, terminal_height: u16) -> usize {
    selector_tui_configured_height(model, terminal_height)
        .map(selector_content_row_limit_for_height)
        .unwrap_or(model.visible_rows.len().max(1))
}

fn selector_content_row_limit_for_height(height: u16) -> usize {
    usize::from(height.saturating_sub(2))
        .saturating_sub(2)
        .max(1)
}

fn selector_tui_visible_rows_from_start(
    model: &SelectorTuiViewModel,
    row_limit: usize,
    start: usize,
) -> Vec<SelectorUiRow> {
    model
        .rendered_items
        .iter()
        .cloned()
        .enumerate()
        .skip(start)
        .take(row_limit)
        .map(|(index, item)| SelectorUiRow {
            index,
            item,
            selected: model.cursor == index,
        })
        .collect()
}

fn selector_tui_visible_start_with_previous(
    model: &SelectorTuiViewModel,
    row_limit: usize,
    previous_start: Option<usize>,
) -> usize {
    let Some(previous_start) = previous_start else {
        return selector_tui_visible_start(model, row_limit);
    };

    let row_limit = row_limit.max(1);
    let item_len = model.rendered_items.len();
    if item_len <= row_limit {
        return 0;
    }

    let max_start = item_len.saturating_sub(row_limit);
    let cursor = model.cursor.min(item_len.saturating_sub(1));
    let mut start = previous_start.min(max_start);
    if cursor < start {
        start = cursor;
    } else if cursor >= start.saturating_add(row_limit) {
        start = cursor.saturating_add(1).saturating_sub(row_limit);
    }
    start.min(max_start)
}

fn selector_tui_visible_start(model: &SelectorTuiViewModel, row_limit: usize) -> usize {
    let row_limit = row_limit.max(1);
    let item_len = model.rendered_items.len();
    if item_len <= row_limit {
        return 0;
    }

    let max_start = item_len.saturating_sub(row_limit);
    let cursor = model.cursor.min(item_len.saturating_sub(1));

    if row_limit > model.visible_rows.len().max(1) {
        return cursor
            .saturating_add(1)
            .saturating_sub(row_limit)
            .min(max_start);
    }

    let offset = model.offset.min(max_start);
    if cursor < offset {
        return cursor;
    }
    if cursor < offset.saturating_add(row_limit) {
        return offset;
    }

    cursor
        .saturating_add(1)
        .saturating_sub(row_limit)
        .min(max_start)
}

fn resolve_selector_window_size_value(
    value: RuntimeSelectorWindowSizeValue,
    terminal_dimension: u16,
) -> u16 {
    match value {
        RuntimeSelectorWindowSizeValue::Cells(cells) => cells.max(1),
        RuntimeSelectorWindowSizeValue::Percent(percent) => {
            let dimension = usize::from(terminal_dimension.max(1));
            let resolved = dimension.saturating_mul(usize::from(percent.0)) / 100;
            u16::try_from(resolved.max(1)).unwrap_or(u16::MAX)
        }
    }
}

fn selector_tui_static_lines(model: &SelectorTuiViewModel, rows: &[SelectorUiRow]) -> Vec<String> {
    let mut lines = Vec::with_capacity(rows.len().saturating_add(2));
    lines.push(format!("query: {}", model.query));
    lines.extend(rows.iter().map(|row| {
        let marker = if row.selected { '>' } else { ' ' };
        format!("{marker} {}", row.item.label)
    }));
    lines.push(model.status_text.clone());
    lines
}

fn selector_tui_cursor(
    model: &SelectorTuiViewModel,
    rows: &[SelectorUiRow],
) -> Option<FloatingCursor> {
    match model.mode {
        SelectorMode::Insert => Some(FloatingCursor {
            line: 0,
            column: "query: ".len().saturating_add(model.query.len()),
        }),
        SelectorMode::Normal => rows
            .iter()
            .position(|row| row.selected)
            .map(|visible_index| FloatingCursor {
                line: visible_index.saturating_add(1),
                column: 0,
            }),
    }
}

fn selector_tui_inline_styles(rows: &[SelectorUiRow]) -> Vec<FloatingInlineStyle> {
    rows.iter()
        .enumerate()
        .flat_map(|(visible_index, row)| {
            let line = visible_index + 1;
            row.item
                .highlights
                .iter()
                .filter(|highlight| highlight.kind == RuntimeSelectorHighlightKind::Match)
                .map(move |highlight| {
                    let column_start = 2usize.saturating_add(highlight.column);
                    FloatingInlineStyle {
                        kind: FloatingInlineStyleKind::Match,
                        line,
                        column_start,
                        column_end: column_start.saturating_add(highlight.width),
                    }
                })
        })
        .collect()
}

fn selector_tui_float_id(session_id: u64) -> FloatingWindowId {
    FloatingWindowId(u64::MAX.saturating_sub(session_id))
}
