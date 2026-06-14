use std::collections::HashMap;

use unicode_width::UnicodeWidthStr;

use crate::features::completion::session::{
    CompletionKeyBindingsRequest, CompletionRange, CompletionSessionManager, CompletionShowRequest,
};
use crate::input::router::{KeyInput, NavigationKey};
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
    pub insert_text: Option<String>,
    pub detail: Option<String>,
    pub kind: Option<String>,
    pub documentation: Vec<String>,
    pub source: Option<String>,
    pub replace_range: Option<CompletionRange>,
}

impl CompletionCandidate {
    pub fn insert_text(&self) -> &str {
        self.insert_text
            .as_deref()
            .filter(|text| !text.is_empty())
            .unwrap_or(&self.label)
    }
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
    pub keys: Option<CompletionKeyBindingsRequest>,
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
        candidate: CompletionCandidate,
    },
    Closed {
        menu_id: FloatingWindowId,
        editor_key: Option<KeyInput>,
    },
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CompletionFloatManager {
    menus: HashMap<FloatingWindowId, CompletionMenuState>,
    active_menu_id: Option<FloatingWindowId>,
    active_documentation_id: Option<FloatingWindowId>,
    session_manager: CompletionSessionManager,
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
    keys: CompletionMenuKeyBindings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompletionMenuKeyBindings {
    confirm: Vec<KeyInput>,
    close: Vec<KeyInput>,
    next: Vec<KeyInput>,
    previous: Vec<KeyInput>,
    page_next: Vec<KeyInput>,
    page_previous: Vec<KeyInput>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompletionMenuKeyAction {
    Confirm,
    Close,
    Next,
    Previous,
    PageNext,
    PagePrevious,
}

impl Default for CompletionMenuKeyBindings {
    fn default() -> Self {
        Self {
            confirm: vec![KeyInput::Enter, KeyInput::Tab, KeyInput::Ctrl('y')],
            close: vec![KeyInput::Ctrl('e')],
            next: vec![KeyInput::Down, KeyInput::Ctrl('n')],
            previous: vec![KeyInput::Up, KeyInput::Ctrl('p')],
            page_next: vec![KeyInput::PageDown],
            page_previous: vec![KeyInput::PageUp],
        }
    }
}

impl CompletionMenuKeyBindings {
    fn from_request(request: Option<&CompletionKeyBindingsRequest>) -> Self {
        let Some(request) = request else {
            return Self::default();
        };
        let defaults = Self::default();
        Self {
            confirm: normalize_key_specs(request.confirm.as_ref(), defaults.confirm),
            close: normalize_key_specs(request.close.as_ref(), defaults.close),
            next: normalize_key_specs(request.next.as_ref(), defaults.next),
            previous: normalize_key_specs(request.previous.as_ref(), defaults.previous),
            page_next: normalize_key_specs(request.page_next.as_ref(), defaults.page_next),
            page_previous: normalize_key_specs(
                request.page_previous.as_ref(),
                defaults.page_previous,
            ),
        }
    }

    fn action_for(&self, key: &KeyInput) -> Option<CompletionMenuKeyAction> {
        if key_matches(&self.close, key) {
            return Some(CompletionMenuKeyAction::Close);
        }
        if key_matches(&self.confirm, key) {
            return Some(CompletionMenuKeyAction::Confirm);
        }
        if key_matches(&self.previous, key) {
            return Some(CompletionMenuKeyAction::Previous);
        }
        if key_matches(&self.next, key) {
            return Some(CompletionMenuKeyAction::Next);
        }
        if key_matches(&self.page_previous, key) {
            return Some(CompletionMenuKeyAction::PagePrevious);
        }
        if key_matches(&self.page_next, key) {
            return Some(CompletionMenuKeyAction::PageNext);
        }
        None
    }
}

impl CompletionFloatManager {
    /// 現在アクティブな completion メニューが存在するかどうか。
    ///
    /// `main` のイベントループが `focused_input_target_for_key` へ渡す focus
    /// 述語として用いる。メニューが開いていれば、そのキーは
    /// `dispatch_completion_float_key` 経由で leaf の `handle_key` へ流れる。
    pub fn has_active_menu(&self) -> bool {
        self.active_menu_id.is_some()
    }

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
            keys: CompletionMenuKeyBindings::from_request(request.keys.as_ref()),
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

    pub fn show_typed(
        &mut self,
        floats: &mut FloatingWindowManager,
        window_id: i32,
        cursor_row: usize,
        cursor_col: usize,
        request: CompletionShowRequest,
    ) -> bool {
        let accepted = match self.session_manager.accept_show_request(request) {
            Ok(accepted) => accepted,
            Err(stale) => {
                log::debug!(
                    "[completion_float] stale typed completion show ignored: session_id={}, request_id={}, current_request_id={}",
                    stale.session_id,
                    stale.request_id,
                    stale.current_request_id
                );
                return false;
            }
        };
        self.open_menu(
            floats,
            accepted.to_float_request(window_id, cursor_row, cursor_col),
        )
        .is_some()
    }

    pub fn close(
        &mut self,
        floats: &mut FloatingWindowManager,
        restore_window_id: Option<i32>,
    ) -> bool {
        self.close_active(floats, restore_window_id).is_some()
    }

    pub fn handle_key(
        &mut self,
        floats: &mut FloatingWindowManager,
        key: &KeyInput,
        restore_window_id: Option<i32>,
    ) -> CompletionFloatInputOutcome {
        let menu_id = match floats.focused_float_id() {
            Some(menu_id)
                if matches!(
                    floats.window_content(menu_id),
                    Some(FloatingContentRef::CompletionMenu { .. })
                ) =>
            {
                menu_id
            }
            Some(menu_id) => {
                log::debug!(
                    "[completion_float] key ignored because focused float is not a completion menu: key={:?}, menu_id={}",
                    key,
                    menu_id.0
                );
                return CompletionFloatInputOutcome::Ignored;
            }
            None => {
                let Some(menu_id) = self.active_menu_id else {
                    log::debug!(
                        "[completion_float] key ignored because no completion menu is active: key={:?}",
                        key
                    );
                    return CompletionFloatInputOutcome::Ignored;
                };
                menu_id
            }
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

        if completion_modal_escape_key(key) {
            self.close_menu(floats, menu_id, restore_window_id);
            return CompletionFloatInputOutcome::Closed {
                menu_id,
                editor_key: Some(key.clone()),
            };
        }

        let action = self
            .menus
            .get(&menu_id)
            .and_then(|state| state.keys.action_for(key));

        match action {
            Some(CompletionMenuKeyAction::Close) => {
                self.close_menu(floats, menu_id, restore_window_id);
                CompletionFloatInputOutcome::Closed {
                    menu_id,
                    editor_key: None,
                }
            }
            Some(CompletionMenuKeyAction::Confirm) => {
                let candidate = self
                    .menus
                    .get(&menu_id)
                    .and_then(|state| state.candidates.get(state.selected_index))
                    .cloned()
                    .unwrap_or_else(|| CompletionCandidate {
                        label: String::new(),
                        insert_text: None,
                        detail: None,
                        kind: None,
                        documentation: Vec::new(),
                        source: None,
                        replace_range: None,
                    });
                self.close_menu(floats, menu_id, restore_window_id);
                log::debug!(
                    "[completion_float] accepted completion candidate: menu_id={}, label={:?}, insert_text_len={}, source={:?}",
                    menu_id.0,
                    candidate.label,
                    candidate.insert_text().len(),
                    candidate.source
                );
                CompletionFloatInputOutcome::Accepted { menu_id, candidate }
            }
            Some(CompletionMenuKeyAction::Previous) => self.move_selection(floats, menu_id, -1),
            Some(CompletionMenuKeyAction::Next) => self.move_selection(floats, menu_id, 1),
            Some(CompletionMenuKeyAction::PagePrevious) => {
                self.move_selection_by_page(floats, menu_id, -1)
            }
            Some(CompletionMenuKeyAction::PageNext) => {
                self.move_selection_by_page(floats, menu_id, 1)
            }
            None => {
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

fn normalize_key_specs(specs: Option<&Vec<String>>, default: Vec<KeyInput>) -> Vec<KeyInput> {
    let Some(specs) = specs else {
        return default;
    };
    specs
        .iter()
        .filter_map(|spec| parse_completion_key_spec(spec))
        .fold(Vec::new(), |mut keys, key| {
            if !keys.iter().any(|existing| key_inputs_equal(existing, &key)) {
                keys.push(key);
            }
            keys
        })
}

fn key_matches(bindings: &[KeyInput], key: &KeyInput) -> bool {
    bindings
        .iter()
        .any(|binding| key_inputs_equal(binding, key))
}

fn key_inputs_equal(left: &KeyInput, right: &KeyInput) -> bool {
    match (left, right) {
        (KeyInput::Ctrl(left), KeyInput::Ctrl(right))
            if left.is_ascii_alphabetic() && right.is_ascii_alphabetic() =>
        {
            left.eq_ignore_ascii_case(right)
        }
        _ => left == right,
    }
}

fn parse_completion_key_spec(spec: &str) -> Option<KeyInput> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return None;
    }
    if !trimmed.starts_with('<') || !trimmed.ends_with('>') {
        let mut chars = trimmed.chars();
        let first = chars.next()?;
        return chars.next().is_none().then_some(KeyInput::Char(first));
    }

    let inner = trimmed.strip_prefix('<')?.strip_suffix('>')?.trim();
    let lower = inner.to_ascii_lowercase();
    match lower.as_str() {
        "tab" => Some(KeyInput::Tab),
        "s-tab" | "shift-tab" | "backtab" => Some(KeyInput::BackTab),
        "enter" | "return" | "cr" => Some(KeyInput::Enter),
        "s-enter" | "shift-enter" => Some(KeyInput::ShiftEnter),
        "esc" | "escape" => Some(KeyInput::Escape),
        "space" => Some(KeyInput::Char(' ')),
        "bs" | "backspace" => Some(KeyInput::Backspace),
        "del" | "delete" => Some(KeyInput::Delete),
        "insert" | "ins" => Some(KeyInput::Insert),
        "up" => Some(KeyInput::Up),
        "down" => Some(KeyInput::Down),
        "left" => Some(KeyInput::Left),
        "right" => Some(KeyInput::Right),
        "home" => Some(KeyInput::Home),
        "end" => Some(KeyInput::End),
        "pageup" | "page-up" => Some(KeyInput::PageUp),
        "pagedown" | "page-down" => Some(KeyInput::PageDown),
        _ => parse_modified_completion_key(inner),
    }
}

fn parse_modified_completion_key(inner: &str) -> Option<KeyInput> {
    let (modifier, key) = inner.split_once('-')?;
    let modifier = modifier.trim().to_ascii_lowercase();
    let key = key.trim();
    if modifier == "c" || modifier == "ctrl" {
        return parse_ctrl_completion_key(key);
    }
    if modifier == "a" || modifier == "alt" || modifier == "m" || modifier == "meta" {
        return parse_single_char_key(key).map(KeyInput::Alt);
    }
    None
}

fn parse_ctrl_completion_key(key: &str) -> Option<KeyInput> {
    match key.to_ascii_lowercase().as_str() {
        "space" => Some(KeyInput::Ctrl(' ')),
        "up" => Some(KeyInput::CtrlNav(NavigationKey::Up)),
        "down" => Some(KeyInput::CtrlNav(NavigationKey::Down)),
        "left" => Some(KeyInput::CtrlNav(NavigationKey::Left)),
        "right" => Some(KeyInput::CtrlNav(NavigationKey::Right)),
        "[" | "esc" | "escape" => Some(KeyInput::Ctrl('[')),
        _ => parse_single_char_key(key).map(|ch| KeyInput::Ctrl(ch.to_ascii_lowercase())),
    }
}

fn parse_single_char_key(key: &str) -> Option<char> {
    let mut chars = key.chars();
    let first = chars.next()?;
    chars.next().is_none().then_some(first)
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
            CompletionFloatInputOutcome::Closed { menu_id, .. } => {
                FloatingInputOutcome::Closed { id: menu_id }
            }
        }
    }
}

fn completion_modal_escape_key(key: &KeyInput) -> bool {
    matches!(key, KeyInput::Escape | KeyInput::Ctrl('['))
}
