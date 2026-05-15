use std::collections::HashMap;

use serde_json::Value;
use unicode_width::UnicodeWidthStr;

use crate::input::router::KeyInput;
use crate::presentation::floating_window::{
    FloatingAnchor, FloatingBorder, FloatingChrome, FloatingContentRef, FloatingFit,
    FloatingInputOutcome, FloatingLifecycle, FloatingPlacement, FloatingRelativeTo, FloatingSize,
    FloatingWindowId, FloatingWindowManager, FloatingZIndex,
};

const COMPLETION_MENU_GROUP: &str = "completion:menu";
const COMPLETION_DOCUMENTATION_GROUP: &str = "completion:documentation";
const DEFAULT_MENU_WIDTH: u16 = 24;
const MAX_MENU_WIDTH: u16 = 72;
const MAX_MENU_HEIGHT: u16 = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionCandidate {
    pub label: String,
    pub detail: Option<String>,
    pub kind: Option<String>,
    pub documentation: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionMenuFloatRequest {
    pub window_id: i32,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub candidates: Vec<CompletionCandidate>,
    pub selected_index: usize,
    pub max_visible_items: usize,
    pub documentation_max_width: u16,
    pub documentation_max_height: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompletionFloatOpenResult {
    pub menu_id: FloatingWindowId,
    pub documentation_id: Option<FloatingWindowId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionFloatInputOutcome {
    Ignored,
    Selected {
        menu_id: FloatingWindowId,
        selected_index: usize,
    },
    Accepted {
        menu_id: FloatingWindowId,
        label: String,
    },
    Closed {
        menu_id: FloatingWindowId,
    },
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CompletionFloatManager {
    menus: HashMap<FloatingWindowId, CompletionMenuState>,
    active_menu_id: Option<FloatingWindowId>,
    active_documentation_id: Option<FloatingWindowId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompletionMenuState {
    window_id: i32,
    candidates: Vec<CompletionCandidate>,
    selected_index: usize,
    scroll_offset: usize,
    max_visible_items: usize,
    documentation_max_width: u16,
    documentation_max_height: u16,
    menu_size: FloatingSize,
}

impl CompletionFloatManager {
    pub fn open_menu(
        &mut self,
        floats: &mut FloatingWindowManager,
        request: CompletionMenuFloatRequest,
    ) -> Option<CompletionFloatOpenResult> {
        if request.candidates.is_empty() {
            log::debug!(
                "[completion_float] completion menu request ignored because it has no candidates: window_id={}, cursor=({}, {})",
                request.window_id,
                request.cursor_row,
                request.cursor_col
            );
            return None;
        }

        self.close_active(floats, None);
        let selected_index = request
            .selected_index
            .min(request.candidates.len().saturating_sub(1));
        let max_visible_items = request
            .max_visible_items
            .max(1)
            .min(usize::from(MAX_MENU_HEIGHT.saturating_sub(2)).max(1));
        let scroll_offset = scroll_offset_for_selection(
            selected_index,
            0,
            max_visible_items,
            request.candidates.len(),
        );
        let visible_lines = render_completion_lines(
            &request.candidates,
            selected_index,
            scroll_offset,
            max_visible_items,
        );
        let menu_size = menu_size_for_lines(&visible_lines, max_visible_items);

        let menu_id = floats.open_rendered_lines_with_lifecycle_and_replacement_group(
            visible_lines,
            |id| FloatingContentRef::CompletionMenu { menu_id: id.0 },
            FloatingLifecycle::CloseOnInsert,
            Some(COMPLETION_MENU_GROUP.to_string()),
            FloatingPlacement {
                relative_to: FloatingRelativeTo::Cursor {
                    window_id: request.window_id,
                },
                anchor: FloatingAnchor::NorthWest,
                row: 1,
                col: 0,
                fit: FloatingFit::TruncateToGrid,
            },
            menu_size,
            FloatingChrome {
                border: FloatingBorder::Single,
            },
            FloatingZIndex::Completion,
            true,
        );
        let state = CompletionMenuState {
            window_id: request.window_id,
            candidates: request.candidates,
            selected_index,
            scroll_offset,
            max_visible_items,
            documentation_max_width: request.documentation_max_width,
            documentation_max_height: request.documentation_max_height,
            menu_size,
        };
        self.menus.insert(menu_id, state);
        self.active_menu_id = Some(menu_id);
        let documentation_id = self.refresh_documentation(floats, menu_id);
        log::debug!(
            "[completion_float] opened completion menu: menu_id={}, documentation_id={:?}, window_id={}, selected_index={}, visible_items={}, size=({},{})",
            menu_id.0,
            documentation_id.map(|id| id.0),
            request.window_id,
            selected_index,
            max_visible_items,
            menu_size.width,
            menu_size.height
        );
        Some(CompletionFloatOpenResult {
            menu_id,
            documentation_id,
        })
    }

    pub fn handle_key(
        &mut self,
        floats: &mut FloatingWindowManager,
        key: &KeyInput,
        restore_window_id: Option<i32>,
    ) -> CompletionFloatInputOutcome {
        let Some(menu_id) = floats.focused_float_id() else {
            log::debug!(
                "[completion_float] key ignored because focused target is not a float: key={:?}",
                key
            );
            return CompletionFloatInputOutcome::Ignored;
        };
        if !matches!(
            floats.window_content(menu_id),
            Some(FloatingContentRef::CompletionMenu { .. })
        ) {
            log::debug!(
                "[completion_float] key ignored because focused float is not a completion menu: key={:?}, menu_id={}",
                key,
                menu_id.0
            );
            return CompletionFloatInputOutcome::Ignored;
        }
        if !self.menus.contains_key(&menu_id) {
            log::debug!(
                "[completion_float] key ignored because completion state is missing: key={:?}, menu_id={}",
                key,
                menu_id.0
            );
            return CompletionFloatInputOutcome::Ignored;
        }

        match key {
            KeyInput::Escape | KeyInput::Ctrl('[') => {
                self.close_menu(floats, menu_id, restore_window_id);
                CompletionFloatInputOutcome::Closed { menu_id }
            }
            KeyInput::Enter | KeyInput::Tab | KeyInput::Ctrl('y') | KeyInput::Ctrl('Y') => {
                let label = self
                    .menus
                    .get(&menu_id)
                    .and_then(|state| state.candidates.get(state.selected_index))
                    .map(|candidate| candidate.label.clone())
                    .unwrap_or_default();
                self.close_menu(floats, menu_id, restore_window_id);
                log::debug!(
                    "[completion_float] accepted completion candidate: menu_id={}, label={:?}",
                    menu_id.0,
                    label
                );
                CompletionFloatInputOutcome::Accepted { menu_id, label }
            }
            KeyInput::Up | KeyInput::Ctrl('p') | KeyInput::Ctrl('P') => {
                self.move_selection(floats, menu_id, -1)
            }
            KeyInput::Down | KeyInput::Ctrl('n') | KeyInput::Ctrl('N') => {
                self.move_selection(floats, menu_id, 1)
            }
            KeyInput::PageUp => self.move_selection_by_page(floats, menu_id, -1),
            KeyInput::PageDown => self.move_selection_by_page(floats, menu_id, 1),
            _ => {
                log::debug!(
                    "[completion_float] key ignored by completion menu: key={:?}, menu_id={}",
                    key,
                    menu_id.0
                );
                CompletionFloatInputOutcome::Ignored
            }
        }
    }

    pub fn active_documentation_id(&self) -> Option<FloatingWindowId> {
        self.active_documentation_id
    }

    fn move_selection(
        &mut self,
        floats: &mut FloatingWindowManager,
        menu_id: FloatingWindowId,
        delta: i32,
    ) -> CompletionFloatInputOutcome {
        let (selected_index, scroll_offset) = {
            let Some(state) = self.menus.get_mut(&menu_id) else {
                return CompletionFloatInputOutcome::Ignored;
            };
            let max_index = state.candidates.len().saturating_sub(1);
            let current = i32::try_from(state.selected_index).unwrap_or(i32::MAX);
            let next = current
                .saturating_add(delta)
                .clamp(0, i32::try_from(max_index).unwrap_or(i32::MAX));
            state.selected_index = usize::try_from(next).unwrap_or(max_index);
            refresh_menu_lines(floats, menu_id, state);
            (state.selected_index, state.scroll_offset)
        };
        self.refresh_documentation(floats, menu_id);
        log::debug!(
            "[completion_float] moved completion selection: menu_id={}, selected_index={}, scroll_offset={}",
            menu_id.0,
            selected_index,
            scroll_offset
        );
        CompletionFloatInputOutcome::Selected {
            menu_id,
            selected_index,
        }
    }

    fn move_selection_by_page(
        &mut self,
        floats: &mut FloatingWindowManager,
        menu_id: FloatingWindowId,
        direction: i32,
    ) -> CompletionFloatInputOutcome {
        let page = self
            .menus
            .get(&menu_id)
            .map(|state| state.max_visible_items)
            .unwrap_or(1);
        self.move_selection(
            floats,
            menu_id,
            direction.saturating_mul(i32::try_from(page).unwrap_or(1)),
        )
    }

    fn refresh_documentation(
        &mut self,
        floats: &mut FloatingWindowManager,
        menu_id: FloatingWindowId,
    ) -> Option<FloatingWindowId> {
        if let Some(id) = self.active_documentation_id.take() {
            floats.close(id);
        }
        let Some(state) = self.menus.get(&menu_id) else {
            return None;
        };
        let lines = state
            .candidates
            .get(state.selected_index)
            .map(|candidate| candidate.documentation.clone())
            .unwrap_or_default()
            .into_iter()
            .filter(|line| !line.trim().is_empty())
            .collect::<Vec<_>>();
        if lines.is_empty() {
            log::debug!(
                "[completion_float] documentation float skipped because selected candidate has no docs: menu_id={}, selected_index={}",
                menu_id.0,
                state.selected_index
            );
            return None;
        }
        let size = documentation_size_for_lines(
            &lines,
            state.documentation_max_width,
            state.documentation_max_height,
        );
        let id = floats.open_static_lines_with_lifecycle_and_replacement_group(
            lines,
            FloatingLifecycle::CloseOnInsert,
            Some(COMPLETION_DOCUMENTATION_GROUP.to_string()),
            FloatingPlacement {
                relative_to: FloatingRelativeTo::Cursor {
                    window_id: state.window_id,
                },
                anchor: FloatingAnchor::NorthWest,
                row: 1,
                col: i16::try_from(state.menu_size.width.saturating_add(1)).unwrap_or(i16::MAX),
                fit: FloatingFit::TruncateToGrid,
            },
            size,
            FloatingChrome {
                border: FloatingBorder::Single,
            },
            FloatingZIndex::CompletionDocumentation,
            true,
        );
        self.active_documentation_id = Some(id);
        log::debug!(
            "[completion_float] refreshed documentation float: menu_id={}, documentation_id={}, selected_index={}, size=({},{})",
            menu_id.0,
            id.0,
            state.selected_index,
            size.width,
            size.height
        );
        Some(id)
    }

    fn close_active(
        &mut self,
        floats: &mut FloatingWindowManager,
        restore_window_id: Option<i32>,
    ) -> Option<FloatingWindowId> {
        let menu_id = self.active_menu_id?;
        self.close_menu(floats, menu_id, restore_window_id);
        Some(menu_id)
    }

    fn close_menu(
        &mut self,
        floats: &mut FloatingWindowManager,
        menu_id: FloatingWindowId,
        restore_window_id: Option<i32>,
    ) {
        self.menus.remove(&menu_id);
        if self.active_menu_id == Some(menu_id) {
            self.active_menu_id = None;
        }
        if let Some(id) = self.active_documentation_id.take() {
            floats.close(id);
        }
        floats.close(menu_id);
        if let Some(window_id) = restore_window_id {
            floats.clear_focus_to_pane(window_id);
        }
        log::debug!(
            "[completion_float] closed completion menu: menu_id={}, restore_window_id={:?}",
            menu_id.0,
            restore_window_id
        );
    }
}

pub fn completion_menu_request_from_json(
    window_id: i32,
    cursor_row: usize,
    cursor_col: usize,
    value: &Value,
) -> Result<CompletionMenuFloatRequest, String> {
    let candidates_value = value
        .get("candidates")
        .or_else(|| value.get("items"))
        .ok_or_else(|| "missing candidates".to_string())?;
    let candidates = candidates_value
        .as_array()
        .ok_or_else(|| "candidates must be an array".to_string())?
        .iter()
        .filter_map(candidate_from_json)
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Err("completion menu has no usable candidates".to_string());
    }
    Ok(CompletionMenuFloatRequest {
        window_id,
        cursor_row,
        cursor_col,
        candidates,
        selected_index: value
            .get("selectedIndex")
            .or_else(|| value.get("selected_index"))
            .and_then(Value::as_u64)
            .map(|index| index as usize)
            .unwrap_or(0),
        max_visible_items: value
            .get("maxVisibleItems")
            .or_else(|| value.get("max_visible_items"))
            .and_then(Value::as_u64)
            .map(|items| items as usize)
            .unwrap_or(8),
        documentation_max_width: value
            .get("documentationMaxWidth")
            .or_else(|| value.get("documentation_max_width"))
            .and_then(Value::as_u64)
            .and_then(|width| u16::try_from(width).ok())
            .unwrap_or(72),
        documentation_max_height: value
            .get("documentationMaxHeight")
            .or_else(|| value.get("documentation_max_height"))
            .and_then(Value::as_u64)
            .and_then(|height| u16::try_from(height).ok())
            .unwrap_or(12),
    })
}

fn candidate_from_json(value: &Value) -> Option<CompletionCandidate> {
    match value {
        Value::String(label) => {
            let label = label.trim();
            (!label.is_empty()).then(|| CompletionCandidate {
                label: label.to_string(),
                detail: None,
                kind: None,
                documentation: Vec::new(),
            })
        }
        Value::Object(object) => {
            let label = object
                .get("label")
                .or_else(|| object.get("word"))
                .or_else(|| object.get("insertText"))
                .and_then(Value::as_str)?
                .trim()
                .to_string();
            if label.is_empty() {
                return None;
            }
            Some(CompletionCandidate {
                label,
                detail: object
                    .get("detail")
                    .or_else(|| object.get("menu"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|detail| !detail.is_empty())
                    .map(ToString::to_string),
                kind: object
                    .get("kind")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|kind| !kind.is_empty())
                    .map(ToString::to_string),
                documentation: object
                    .get("documentation")
                    .or_else(|| object.get("docs"))
                    .or_else(|| object.get("info"))
                    .map(documentation_lines_from_json)
                    .unwrap_or_default(),
            })
        }
        _ => None,
    }
}

fn documentation_lines_from_json(value: &Value) -> Vec<String> {
    match value {
        Value::String(text) => normalize_documentation_lines(text),
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .flat_map(normalize_documentation_lines)
            .collect(),
        Value::Object(object) => object
            .get("value")
            .and_then(Value::as_str)
            .map(normalize_documentation_lines)
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn normalize_documentation_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .map(ToString::to_string)
        .collect()
}

fn refresh_menu_lines(
    floats: &mut FloatingWindowManager,
    menu_id: FloatingWindowId,
    state: &mut CompletionMenuState,
) {
    state.scroll_offset = scroll_offset_for_selection(
        state.selected_index,
        state.scroll_offset,
        state.max_visible_items,
        state.candidates.len(),
    );
    let lines = render_completion_lines(
        &state.candidates,
        state.selected_index,
        state.scroll_offset,
        state.max_visible_items,
    );
    state.menu_size = menu_size_for_lines(&lines, state.max_visible_items);
    floats.replace_rendered_lines(menu_id, lines, state.menu_size);
}

fn scroll_offset_for_selection(
    selected_index: usize,
    current_offset: usize,
    visible_items: usize,
    candidate_count: usize,
) -> usize {
    let visible_items = visible_items.max(1);
    let max_offset = candidate_count.saturating_sub(visible_items);
    if selected_index < current_offset {
        selected_index
    } else if selected_index >= current_offset.saturating_add(visible_items) {
        selected_index
            .saturating_add(1)
            .saturating_sub(visible_items)
    } else {
        current_offset
    }
    .min(max_offset)
}

fn render_completion_lines(
    candidates: &[CompletionCandidate],
    selected_index: usize,
    scroll_offset: usize,
    visible_items: usize,
) -> Vec<String> {
    candidates
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(visible_items.max(1))
        .map(|(index, candidate)| {
            let prefix = if index == selected_index { "> " } else { "  " };
            format!("{prefix}{}", render_candidate(candidate))
        })
        .collect()
}

fn render_candidate(candidate: &CompletionCandidate) -> String {
    let mut line = String::new();
    if let Some(kind) = candidate
        .kind
        .as_ref()
        .filter(|kind| !kind.trim().is_empty())
    {
        line.push('[');
        line.push_str(kind.trim());
        line.push_str("] ");
    }
    line.push_str(candidate.label.trim());
    if let Some(detail) = candidate
        .detail
        .as_ref()
        .filter(|detail| !detail.trim().is_empty())
    {
        line.push_str(" - ");
        line.push_str(detail.trim());
    }
    line
}

fn menu_size_for_lines(lines: &[String], max_visible_items: usize) -> FloatingSize {
    let width = lines
        .iter()
        .map(|line| UnicodeWidthStr::width(line.as_str()))
        .max()
        .unwrap_or(usize::from(DEFAULT_MENU_WIDTH))
        .clamp(usize::from(DEFAULT_MENU_WIDTH), usize::from(MAX_MENU_WIDTH));
    let content_height = lines.len().min(max_visible_items.max(1));
    FloatingSize {
        width: u16::try_from(width.saturating_add(2)).unwrap_or(MAX_MENU_WIDTH),
        height: u16::try_from(content_height.saturating_add(2)).unwrap_or(MAX_MENU_HEIGHT),
    }
}

fn documentation_size_for_lines(lines: &[String], max_width: u16, max_height: u16) -> FloatingSize {
    let width_limit = max_width.clamp(8, MAX_MENU_WIDTH);
    let height_limit = max_height.clamp(3, MAX_MENU_HEIGHT);
    let content_width = lines
        .iter()
        .map(|line| UnicodeWidthStr::width(line.as_str()))
        .max()
        .unwrap_or(1)
        .clamp(1, usize::from(width_limit.saturating_sub(2)));
    let content_height = lines
        .len()
        .clamp(1, usize::from(height_limit.saturating_sub(2)));
    FloatingSize {
        width: u16::try_from(content_width.saturating_add(2)).unwrap_or(width_limit),
        height: u16::try_from(content_height.saturating_add(2)).unwrap_or(height_limit),
    }
}

impl From<CompletionFloatInputOutcome> for FloatingInputOutcome {
    fn from(outcome: CompletionFloatInputOutcome) -> Self {
        match outcome {
            CompletionFloatInputOutcome::Ignored => FloatingInputOutcome::Ignored,
            CompletionFloatInputOutcome::Selected { .. }
            | CompletionFloatInputOutcome::Accepted { .. } => FloatingInputOutcome::Consumed,
            CompletionFloatInputOutcome::Closed { menu_id } => {
                FloatingInputOutcome::Closed { id: menu_id }
            }
        }
    }
}
