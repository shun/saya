use crate::presentation::screen_model::PaneRect;

use crate::input::router::KeyInput;

mod geometry;
mod types;

use geometry::*;
use types::default_close_keys;
pub use types::*;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FloatingWindowManager {
    windows: Vec<FloatingWindow>,
    next_id: u64,
    next_creation_order: u64,
    focus: Option<WorkspaceFocus>,
}

impl FloatingWindowManager {
    pub fn open_static_lines(
        &mut self,
        lines: Vec<String>,
        placement: FloatingPlacement,
        size: FloatingSize,
        chrome: FloatingChrome,
        zindex: FloatingZIndex,
        focusable: bool,
    ) -> FloatingWindowId {
        self.open_static_lines_with_lifecycle(
            lines,
            FloatingLifecycle::Manual,
            placement,
            size,
            chrome,
            zindex,
            focusable,
        )
    }

    pub fn open_static_lines_with_lifecycle(
        &mut self,
        lines: Vec<String>,
        lifecycle: FloatingLifecycle,
        placement: FloatingPlacement,
        size: FloatingSize,
        chrome: FloatingChrome,
        zindex: FloatingZIndex,
        focusable: bool,
    ) -> FloatingWindowId {
        let replacement_group = match lifecycle {
            FloatingLifecycle::ReplaceByGroup(group) => Some(group.to_string()),
            _ => None,
        };
        self.open_static_lines_with_lifecycle_and_replacement_group(
            lines,
            lifecycle,
            replacement_group,
            placement,
            size,
            chrome,
            zindex,
            focusable,
        )
    }

    pub fn open_static_lines_with_lifecycle_and_replacement_group(
        &mut self,
        lines: Vec<String>,
        lifecycle: FloatingLifecycle,
        replacement_group: Option<String>,
        placement: FloatingPlacement,
        size: FloatingSize,
        chrome: FloatingChrome,
        zindex: FloatingZIndex,
        focusable: bool,
    ) -> FloatingWindowId {
        self.open_rendered_lines_with_lifecycle_and_replacement_group(
            lines,
            |id| FloatingContentRef::StaticLines { content_id: id.0 },
            lifecycle,
            replacement_group,
            placement,
            size,
            chrome,
            zindex,
            focusable,
        )
    }

    pub fn open_core_window(
        &mut self,
        window_id: i32,
        placement: FloatingPlacement,
        size: FloatingSize,
        chrome: FloatingChrome,
        zindex: FloatingZIndex,
        focusable: bool,
    ) -> FloatingWindowId {
        self.open_core_window_with_lifecycle(
            window_id,
            FloatingLifecycle::Manual,
            None,
            placement,
            size,
            chrome,
            zindex,
            focusable,
        )
    }

    pub fn open_core_window_with_lifecycle(
        &mut self,
        window_id: i32,
        lifecycle: FloatingLifecycle,
        replacement_group: Option<String>,
        placement: FloatingPlacement,
        size: FloatingSize,
        chrome: FloatingChrome,
        zindex: FloatingZIndex,
        focusable: bool,
    ) -> FloatingWindowId {
        self.open_rendered_lines_with_lifecycle_and_replacement_group(
            Vec::new(),
            |_| FloatingContentRef::CoreWindow { window_id },
            lifecycle,
            replacement_group,
            placement,
            size,
            chrome,
            zindex,
            focusable,
        )
    }

    pub fn open_terminal(
        &mut self,
        terminal_id: u64,
        placement: FloatingPlacement,
        size: FloatingSize,
        chrome: FloatingChrome,
        zindex: FloatingZIndex,
        focusable: bool,
    ) -> FloatingWindowId {
        self.open_terminal_with_lifecycle(
            terminal_id,
            FloatingLifecycle::Manual,
            None,
            placement,
            size,
            chrome,
            zindex,
            focusable,
        )
    }

    pub fn open_terminal_with_lifecycle(
        &mut self,
        terminal_id: u64,
        lifecycle: FloatingLifecycle,
        replacement_group: Option<String>,
        placement: FloatingPlacement,
        size: FloatingSize,
        chrome: FloatingChrome,
        zindex: FloatingZIndex,
        focusable: bool,
    ) -> FloatingWindowId {
        self.open_rendered_lines_with_lifecycle_and_replacement_group(
            Vec::new(),
            |_| FloatingContentRef::Terminal { terminal_id },
            lifecycle,
            replacement_group,
            placement,
            size,
            chrome,
            zindex,
            focusable,
        )
    }

    pub fn open_rendered_lines_with_lifecycle_and_replacement_group(
        &mut self,
        lines: Vec<String>,
        content: impl FnOnce(FloatingWindowId) -> FloatingContentRef,
        lifecycle: FloatingLifecycle,
        replacement_group: Option<String>,
        placement: FloatingPlacement,
        size: FloatingSize,
        chrome: FloatingChrome,
        zindex: FloatingZIndex,
        focusable: bool,
    ) -> FloatingWindowId {
        let id = self.allocate_id();
        let content = content(id);
        let creation_order = self.allocate_creation_order();
        self.replace_existing_group(replacement_group.as_deref(), lifecycle, id);
        log::debug!(
            "[floating_window] opening float: id={}, content_kind={}, lifecycle={:?}, replacement_group={:?}, relative_to={:?}, anchor={:?}, row={}, col={}, size=({},{}), focusable={}, zindex={}, creation_order={}",
            id.0,
            content.kind_name(),
            lifecycle,
            replacement_group,
            placement.relative_to,
            placement.anchor,
            placement.row,
            placement.col,
            size.width,
            size.height,
            focusable,
            zindex.value(),
            creation_order
        );
        self.windows.push(FloatingWindow {
            id,
            content,
            placement,
            size,
            focusable,
            mouse: focusable,
            chrome,
            zindex: zindex.value(),
            lifecycle,
            replacement_group,
            focus_id: None,
            anchor_signature: None,
            close_keys: default_close_keys(),
            inline_styles: Vec::new(),
            creation_order,
            scroll_offset: 0,
            lines,
            images: Vec::new(),
        });
        id
    }

    /// `focus_id` + `anchor_signature` を持つ既存 float があり、現 focus が
    /// パネル側ならば、新規 float を生成せず既存 float に focus を移して
    /// `FocusedExisting` を返す。focus が既に float 側にあるか、anchor が
    /// 違うか、既存 float が存在しなければ通常 open + 同 `focus_id` の旧
    /// float を内部 replace して `Opened` を返す。
    ///
    /// この API は LSP hover の "2 回目の K で float に focus" UX を
    /// 一般化したもので、`completion menu` / `signature help` 等にも適用
    /// 可能。`replacement_group` は同名グループによる "強制 replace" 用で
    /// あり、`focus_id` は "同一性 + focus toggle" 用に役割を分けている。
    #[allow(clippy::too_many_arguments)]
    pub fn open_static_lines_with_focus_toggle(
        &mut self,
        lines: Vec<String>,
        focus_id: FloatingFocusId,
        anchor_signature: FloatingAnchorSignature,
        lifecycle: FloatingLifecycle,
        placement: FloatingPlacement,
        size: FloatingSize,
        chrome: FloatingChrome,
        zindex: FloatingZIndex,
        focusable: bool,
    ) -> FloatingOpenWithFocusOutcome {
        let existing_match = self.windows.iter().find(|window| {
            window.focus_id.as_ref() == Some(&focus_id)
                && window.anchor_signature == Some(anchor_signature)
        });

        if let Some(existing) = existing_match {
            let existing_id = existing.id;
            let existing_focusable = existing.focusable;
            let already_focused = matches!(
                self.focus,
                Some(WorkspaceFocus::Float { float_id }) if float_id == existing_id
            );
            if !already_focused && existing_focusable {
                self.focus_float(existing_id);
                log::debug!(
                    "[floating_window] focus toggle reused existing float: id={}, focus_id={}, anchor={:?}",
                    existing_id.0,
                    focus_id.as_str(),
                    anchor_signature
                );
                return FloatingOpenWithFocusOutcome::FocusedExisting { id: existing_id };
            }
            log::debug!(
                "[floating_window] focus toggle fell through to replace: existing_id={}, focus_id={}, already_focused={}, existing_focusable={}",
                existing_id.0,
                focus_id.as_str(),
                already_focused,
                existing_focusable
            );
        }

        let replaced: Vec<FloatingWindowId> = self
            .windows
            .iter()
            .filter(|window| window.focus_id.as_ref() == Some(&focus_id))
            .map(|window| window.id)
            .collect();
        if !replaced.is_empty() {
            log::debug!(
                "[floating_window] focus toggle replacing prior floats with same focus_id: focus_id={}, replaced={:?}",
                focus_id.as_str(),
                replaced.iter().map(|id| id.0).collect::<Vec<_>>()
            );
            self.windows.retain(|window| !replaced.contains(&window.id));
            if let Some(WorkspaceFocus::Float { float_id }) = self.focus
                && replaced.contains(&float_id)
            {
                self.focus = None;
            }
        }

        let id = self.open_static_lines_with_lifecycle_and_replacement_group(
            lines, lifecycle, None, placement, size, chrome, zindex, focusable,
        );
        if let Some(window) = self.windows.iter_mut().find(|window| window.id == id) {
            window.focus_id = Some(focus_id.clone());
            window.anchor_signature = Some(anchor_signature);
        }
        log::debug!(
            "[floating_window] focus toggle opened new float: id={}, focus_id={}, anchor={:?}",
            id.0,
            focus_id.as_str(),
            anchor_signature
        );
        FloatingOpenWithFocusOutcome::Opened { id }
    }

    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }

    pub fn debug_window(&self, id: FloatingWindowId) -> Option<&FloatingWindow> {
        self.windows.iter().find(|window| window.id == id)
    }

    pub fn windows(&self) -> &[FloatingWindow] {
        &self.windows
    }

    pub fn focused_float_id(&self) -> Option<FloatingWindowId> {
        match self.focus {
            Some(WorkspaceFocus::Float { float_id }) => Some(float_id),
            _ => None,
        }
    }

    pub fn focused_core_window_id(&self) -> Option<i32> {
        let float_id = self.focused_float_id()?;
        self.windows
            .iter()
            .find(|window| window.id == float_id)
            .and_then(|window| match window.content {
                FloatingContentRef::CoreWindow { window_id } => Some(window_id),
                _ => None,
            })
    }

    /// 現在 focus されている float が、ローカルでキー処理可能な static-lines
    /// （ホバー等のスクロール可能フロート）かどうかを返す。
    ///
    /// `dispatch_floating_window_key` が実際に処理できる float に絞った focus
    /// 述語であり、`focused_input_target_for_key` の振り分け判断に用いる。
    /// core-window / terminal / completion-menu などの float が focus されて
    /// いる場合は `None` を返す（それらは別ラッパが先に処理する）。
    pub fn focused_static_lines_id(&self) -> Option<FloatingWindowId> {
        let float_id = self.focused_float_id()?;
        self.windows
            .iter()
            .find(|window| window.id == float_id)
            .filter(|window| matches!(window.content, FloatingContentRef::StaticLines { .. }))
            .map(|window| window.id)
    }

    pub fn focused_terminal_id(&self) -> Option<u64> {
        let float_id = self.focused_float_id()?;
        self.windows
            .iter()
            .find(|window| window.id == float_id)
            .and_then(|window| match window.content {
                FloatingContentRef::Terminal { terminal_id } => Some(terminal_id),
                _ => None,
            })
    }

    pub fn window_content(&self, id: FloatingWindowId) -> Option<&FloatingContentRef> {
        self.windows
            .iter()
            .find(|window| window.id == id)
            .map(|window| &window.content)
    }

    pub fn replace_rendered_lines(
        &mut self,
        id: FloatingWindowId,
        lines: Vec<String>,
        size: FloatingSize,
    ) -> bool {
        let Some(window) = self.windows.iter_mut().find(|window| window.id == id) else {
            log::debug!(
                "[floating_window] rendered-line update ignored for missing float: id={}",
                id.0
            );
            return false;
        };
        let old_line_count = window.lines.len();
        window.lines = lines;
        window.size = size;
        window.scroll_offset = 0;
        log::debug!(
            "[floating_window] rendered lines updated: id={}, content_kind={}, old_lines={}, new_lines={}, size=({},{})",
            id.0,
            window.content.kind_name(),
            old_line_count,
            window.lines.len(),
            size.width,
            size.height
        );
        true
    }

    pub fn replace_core_window_lines(&mut self, id: FloatingWindowId, lines: Vec<String>) -> bool {
        let Some(size) = self
            .windows
            .iter()
            .find(|window| {
                window.id == id && matches!(window.content, FloatingContentRef::CoreWindow { .. })
            })
            .map(|window| window.size)
        else {
            log::debug!(
                "[floating_window] core-window rendered-line update ignored for non-core or missing float: id={}",
                id.0
            );
            return false;
        };
        self.replace_rendered_lines(id, lines, size)
    }

    pub fn replace_terminal_lines(&mut self, id: FloatingWindowId, lines: Vec<String>) -> bool {
        let Some(size) = self
            .windows
            .iter()
            .find(|window| {
                window.id == id && matches!(window.content, FloatingContentRef::Terminal { .. })
            })
            .map(|window| window.size)
        else {
            log::debug!(
                "[floating_window] terminal rendered-line update ignored for non-terminal or missing float: id={}",
                id.0
            );
            return false;
        };
        self.replace_rendered_lines(id, lines, size)
    }

    pub fn core_window_float_view_requests(&self) -> Vec<CoreWindowFloatViewRequest> {
        self.windows
            .iter()
            .filter_map(|window| match window.content {
                FloatingContentRef::CoreWindow { window_id } => Some(CoreWindowFloatViewRequest {
                    float_id: window.id,
                    window_id,
                    content_height: visible_content_height_for(window.size, window.chrome),
                }),
                _ => None,
            })
            .collect()
    }

    pub fn terminal_float_view_requests(&self) -> Vec<TerminalFloatViewRequest> {
        self.windows
            .iter()
            .filter_map(|window| match window.content {
                FloatingContentRef::Terminal { terminal_id } => Some(TerminalFloatViewRequest {
                    float_id: window.id,
                    terminal_id,
                    content_width: visible_content_width_for(window.size, window.chrome),
                    content_height: visible_content_height_for(window.size, window.chrome),
                }),
                _ => None,
            })
            .collect()
    }

    pub fn focus(&self) -> Option<WorkspaceFocus> {
        self.focus
    }

    pub fn focus_float(&mut self, id: FloatingWindowId) -> bool {
        let can_focus = self
            .windows
            .iter()
            .any(|window| window.id == id && window.focusable);
        if can_focus {
            self.focus = Some(WorkspaceFocus::Float { float_id: id });
        }
        log::debug!(
            "[floating_window] focus float requested: id={}, focused={}, focus={:?}",
            id.0,
            can_focus,
            self.focus
        );
        can_focus
    }

    pub fn clear_focus_to_pane(&mut self, window_id: i32) {
        self.focus = Some(WorkspaceFocus::Pane { window_id });
        log::debug!(
            "[floating_window] focus restored to pane: window_id={}",
            window_id
        );
    }

    pub fn close(&mut self, id: FloatingWindowId) -> bool {
        let before = self.windows.len();
        self.windows.retain(|window| window.id != id);
        let closed = self.windows.len() != before;
        if self.focus == Some(WorkspaceFocus::Float { float_id: id }) {
            self.focus = None;
        }
        log::debug!(
            "[floating_window] close requested: id={}, closed={closed}, remaining={}, focus={:?}",
            id.0,
            self.windows.len(),
            self.focus
        );
        closed
    }

    pub fn close_focused(&mut self, restore_window_id: i32) -> Option<FloatingWindowId> {
        let Some(WorkspaceFocus::Float { float_id }) = self.focus else {
            log::debug!(
                "[floating_window] close focused ignored because focus is not a float: focus={:?}",
                self.focus
            );
            return None;
        };

        if self.close(float_id) {
            self.clear_focus_to_pane(restore_window_id);
            log::debug!(
                "[floating_window] focused float closed and pane focus restored: id={}, window_id={}",
                float_id.0,
                restore_window_id
            );
            Some(float_id)
        } else {
            self.clear_focus_to_pane(restore_window_id);
            None
        }
    }

    pub fn apply_lifecycle_event(
        &mut self,
        event: FloatingLifecycleEvent,
        restore_window_id: Option<i32>,
    ) -> FloatingLifecycleOutcome {
        let closing = self
            .windows
            .iter()
            .filter(|window| lifecycle_matches_event(window, event))
            .map(|window| window.id)
            .collect::<Vec<_>>();

        if closing.is_empty() {
            log::debug!(
                "[floating_window] lifecycle event kept floats: trigger={}, focus_target={}, remaining={}, focus={:?}",
                event.trigger_name(),
                focus_target_name(restore_window_id),
                self.windows.len(),
                self.focus
            );
            return FloatingLifecycleOutcome { closed: vec![] };
        }

        for id in &closing {
            if let Some(window) = self.windows.iter().find(|window| window.id == *id) {
                log::debug!(
                    "[floating_window] lifecycle closing float: id={}, content_kind={}, lifecycle={:?}, trigger={}, focus_target={}, focus_before={:?}",
                    window.id.0,
                    window.content.kind_name(),
                    window.lifecycle,
                    event.trigger_name(),
                    focus_target_name(restore_window_id),
                    self.focus
                );
            }
        }

        self.windows.retain(|window| !closing.contains(&window.id));
        if let Some(WorkspaceFocus::Float { float_id }) = self.focus {
            if closing.contains(&float_id) {
                self.focus = restore_window_id.map(|window_id| WorkspaceFocus::Pane { window_id });
            }
        }

        log::debug!(
            "[floating_window] lifecycle event applied: trigger={}, closed={:?}, remaining={}, focus={:?}",
            event.trigger_name(),
            closing.iter().map(|id| id.0).collect::<Vec<_>>(),
            self.windows.len(),
            self.focus
        );

        FloatingLifecycleOutcome { closed: closing }
    }

    pub fn handle_focused_static_lines_key(&mut self, key: &KeyInput) -> FloatingInputOutcome {
        self.handle_focused_static_lines_key_with_restore(key, None)
    }

    pub fn handle_focused_static_lines_key_with_restore(
        &mut self,
        key: &KeyInput,
        restore_window_id: Option<i32>,
    ) -> FloatingInputOutcome {
        let Some(WorkspaceFocus::Float { float_id }) = self.focus else {
            log::debug!(
                "[floating_window] focused key ignored because focus is not a float: key={:?}, focus={:?}",
                key,
                self.focus
            );
            return FloatingInputOutcome::Ignored;
        };

        let Some(index) = self.windows.iter().position(|window| window.id == float_id) else {
            self.focus = None;
            log::debug!(
                "[floating_window] focused key ignored because focused float is missing: key={:?}, float_id={}",
                key,
                float_id.0
            );
            return FloatingInputOutcome::Ignored;
        };

        if !matches!(
            self.windows[index].content,
            FloatingContentRef::StaticLines { .. }
        ) {
            log::debug!(
                "[floating_window] focused key ignored because float content is not locally scrollable: key={:?}, float_id={}, content_kind={}",
                key,
                float_id.0,
                self.windows[index].content.kind_name()
            );
            return FloatingInputOutcome::Ignored;
        }

        if self.windows[index].close_keys.contains(key) {
            self.close(float_id);
            if let Some(window_id) = restore_window_id {
                self.clear_focus_to_pane(window_id);
            }
            log::debug!(
                "[floating_window] focused static-lines float closed by declarative close key: key={:?}, float_id={}",
                key,
                float_id.0
            );
            return FloatingInputOutcome::Closed { id: float_id };
        }
        match key {
            KeyInput::Up | KeyInput::Char('k') => {
                self.scroll_focused(index, -1);
                FloatingInputOutcome::Consumed
            }
            KeyInput::Down | KeyInput::Char('j') => {
                self.scroll_focused(index, 1);
                FloatingInputOutcome::Consumed
            }
            KeyInput::PageUp | KeyInput::Ctrl('b') | KeyInput::Ctrl('B') => {
                let page = self.visible_content_height(index);
                self.scroll_focused(index, -i32::from(page));
                FloatingInputOutcome::Consumed
            }
            KeyInput::PageDown | KeyInput::Ctrl('f') | KeyInput::Ctrl('F') => {
                let page = self.visible_content_height(index);
                self.scroll_focused(index, i32::from(page));
                FloatingInputOutcome::Consumed
            }
            _ => {
                log::debug!(
                    "[floating_window] focused static-lines key ignored: key={:?}, float_id={}",
                    key,
                    float_id.0
                );
                FloatingInputOutcome::Ignored
            }
        }
    }

    /// 指定 float の close_keys を全置換する。focus 中のときに
    /// `handle_focused_static_lines_key` がこれを参照して close 判定する。
    /// 既定値（`[Escape, Ctrl('[')]`）を hover float のように `q` を加えた
    /// セットへ拡張したい場合に使う。empty を渡すとキーで閉じられなくなる。
    pub fn set_close_keys(&mut self, id: FloatingWindowId, keys: Vec<KeyInput>) -> bool {
        let Some(window) = self.windows.iter_mut().find(|window| window.id == id) else {
            log::debug!(
                "[floating_window] close_keys update ignored for missing float: id={}",
                id.0
            );
            return false;
        };
        log::debug!(
            "[floating_window] close_keys updated: id={}, old={:?}, new={:?}",
            id.0,
            window.close_keys,
            keys
        );
        window.close_keys = keys;
        true
    }

    /// 指定 float の `inline_styles` を全置換する。`tui_renderer` は
    /// `FloatingScreenModel.inline_styles` を参照して Span 単位の theme
    /// スタイルを適用する。markdown レンダリング結果を視覚的に反映する
    /// 際の公式入口。
    pub fn set_inline_styles(
        &mut self,
        id: FloatingWindowId,
        styles: Vec<FloatingInlineStyle>,
    ) -> bool {
        let Some(window) = self.windows.iter_mut().find(|window| window.id == id) else {
            log::debug!(
                "[floating_window] inline_styles update ignored for missing float: id={}",
                id.0
            );
            return false;
        };
        log::debug!(
            "[floating_window] inline_styles updated: id={}, old_count={}, new_count={}",
            id.0,
            window.inline_styles.len(),
            styles.len()
        );
        window.inline_styles = styles;
        true
    }

    pub fn set_images(&mut self, id: FloatingWindowId, images: Vec<FloatingImage>) -> bool {
        let Some(window) = self.windows.iter_mut().find(|window| window.id == id) else {
            log::debug!(
                "[floating_window] image update ignored for missing float: id={}",
                id.0
            );
            return false;
        };
        log::debug!(
            "[floating_window] images updated: id={}, old_count={}, new_count={}",
            id.0,
            window.images.len(),
            images.len()
        );
        window.images = images;
        true
    }

    pub fn set_mouse_enabled(&mut self, id: FloatingWindowId, mouse: bool) -> bool {
        let Some(window) = self.windows.iter_mut().find(|window| window.id == id) else {
            log::debug!(
                "[floating_window] mouse policy update ignored for missing float: id={}, mouse={}",
                id.0,
                mouse
            );
            return false;
        };
        window.mouse = mouse;
        log::debug!(
            "[floating_window] mouse policy updated: id={}, mouse={}",
            id.0,
            mouse
        );
        true
    }

    pub fn resolve_screen_models(
        &self,
        terminal_width: u16,
        terminal_height: u16,
        panes: &[(i32, PaneRect)],
        active_window_id: Option<i32>,
    ) -> Vec<FloatingScreenModel> {
        self.resolve_screen_models_with_cursors(
            terminal_width,
            terminal_height,
            panes,
            &[],
            active_window_id,
        )
    }

    pub fn resolve_screen_models_with_cursors(
        &self,
        terminal_width: u16,
        terminal_height: u16,
        panes: &[(i32, PaneRect)],
        cursors: &[(i32, u16, u16)],
        active_window_id: Option<i32>,
    ) -> Vec<FloatingScreenModel> {
        let mut resolved = self
            .windows
            .iter()
            .filter_map(|window| {
                let rect = resolve_float_rect(
                    window,
                    terminal_width,
                    terminal_height,
                    panes,
                    cursors,
                    active_window_id,
                )?;
                log::debug!(
                    "[floating_window] resolved float: id={}, content_kind={}, rect=({},{},{},{}), focusable={}, zindex={}, creation_order={}",
                    window.id.0,
                    window.content.kind_name(),
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                    window.focusable,
                    window.zindex,
                    window.creation_order
                );
                let scroll_offset = usize::from(window.scroll_offset);
                Some(FloatingScreenModel {
                    id: window.id,
                    content: window.content.clone(),
                    rect,
                    lines: window
                        .lines
                        .iter()
                        .skip(scroll_offset)
                        .cloned()
                        .collect(),
                    inline_styles: window
                        .inline_styles
                        .iter()
                        .filter_map(|style| {
                            if style.line < scroll_offset {
                                None
                            } else {
                                Some(FloatingInlineStyle {
                                    kind: style.kind,
                                    line: style.line - scroll_offset,
                                    column_start: style.column_start,
                                    column_end: style.column_end,
                                })
                            }
                        })
                        .collect(),
                    images: window
                        .images
                        .iter()
                        .filter_map(|image| {
                            let scroll_offset = u16::try_from(scroll_offset).unwrap_or(u16::MAX);
                            if image.line < scroll_offset {
                                return None;
                            }
                            let mut image = image.clone();
                            image.line = image.line.saturating_sub(scroll_offset);
                            Some(image)
                        })
                        .collect(),
                    cursor: None,
                    focusable: window.focusable,
                    mouse: window.mouse,
                    chrome: window.chrome,
                    zindex: window.zindex,
                    creation_order: window.creation_order,
                })
            })
            .collect::<Vec<_>>();
        resolved.sort_by_key(|float| (float.zindex, float.creation_order));
        resolved
    }

    pub fn hit_test(
        &self,
        x: u16,
        y: u16,
        terminal_width: u16,
        terminal_height: u16,
        panes: &[(i32, PaneRect)],
        active_window_id: Option<i32>,
    ) -> Option<FloatingWindowId> {
        let hit = self
            .resolve_screen_models(terminal_width, terminal_height, panes, active_window_id)
            .into_iter()
            .rev()
            .find(|float| float.focusable && rect_contains(float.rect, x, y))
            .map(|float| float.id);
        log::debug!(
            "[floating_window] hit test: cell=({},{}), hit={:?}",
            x,
            y,
            hit.map(|id| id.0)
        );
        hit
    }

    pub fn focus_topmost_at(
        &mut self,
        x: u16,
        y: u16,
        terminal_width: u16,
        terminal_height: u16,
        panes: &[(i32, PaneRect)],
        active_window_id: Option<i32>,
    ) -> FloatingMouseOutcome {
        let hit = self
            .resolve_screen_models(terminal_width, terminal_height, panes, active_window_id)
            .into_iter()
            .rev()
            .find(|float| float.mouse && float.focusable && rect_contains(float.rect, x, y))
            .map(|float| float.id);

        let Some(id) = hit else {
            log::debug!(
                "[floating_window] mouse focus passed through: cell=({},{}), active_window_id={:?}",
                x,
                y,
                active_window_id
            );
            return FloatingMouseOutcome::PassThrough;
        };

        if self.focus_float(id) {
            log::debug!(
                "[floating_window] mouse focused float: cell=({},{}), id={}",
                x,
                y,
                id.0
            );
            FloatingMouseOutcome::Focused { id }
        } else {
            FloatingMouseOutcome::PassThrough
        }
    }

    fn allocate_id(&mut self) -> FloatingWindowId {
        self.next_id = self.next_id.saturating_add(1);
        FloatingWindowId(self.next_id)
    }

    fn allocate_creation_order(&mut self) -> u64 {
        self.next_creation_order = self.next_creation_order.saturating_add(1);
        self.next_creation_order
    }

    fn replace_existing_group(
        &mut self,
        replacement_group: Option<&str>,
        lifecycle: FloatingLifecycle,
        replacement_id: FloatingWindowId,
    ) {
        let Some(group) = replacement_group else {
            return;
        };

        let replaced = self
            .windows
            .iter()
            .filter(|window| window.replacement_group.as_deref() == Some(group))
            .map(|window| window.id)
            .collect::<Vec<_>>();
        if replaced.is_empty() {
            log::debug!(
                "[floating_window] replace-by-group kept existing floats: group={}, lifecycle={:?}, replacement_id={}, focus={:?}",
                group,
                lifecycle,
                replacement_id.0,
                self.focus
            );
            return;
        }

        for id in &replaced {
            if let Some(window) = self.windows.iter().find(|window| window.id == *id) {
                log::debug!(
                    "[floating_window] lifecycle replacing float: id={}, content_kind={}, lifecycle={:?}, trigger=ReplaceByGroup({}), focus_target=None, replacement_id={}, focus_before={:?}",
                    window.id.0,
                    window.content.kind_name(),
                    window.lifecycle,
                    group,
                    replacement_id.0,
                    self.focus
                );
            }
        }

        self.windows.retain(|window| !replaced.contains(&window.id));
        if let Some(WorkspaceFocus::Float { float_id }) = self.focus {
            if replaced.contains(&float_id) {
                self.focus = None;
            }
        }
        log::debug!(
            "[floating_window] replace-by-group applied: group={}, replaced={:?}, replacement_id={}, remaining={}, focus={:?}",
            group,
            replaced.iter().map(|id| id.0).collect::<Vec<_>>(),
            replacement_id.0,
            self.windows.len(),
            self.focus
        );
    }

    fn visible_content_height(&self, index: usize) -> u16 {
        let window = &self.windows[index];
        visible_content_height_for(window.size, window.chrome)
    }

    fn scroll_focused(&mut self, index: usize, delta: i32) {
        let visible_height = usize::from(self.visible_content_height(index));
        let max_offset = self.windows[index]
            .lines
            .len()
            .saturating_sub(visible_height);
        let current = i32::from(self.windows[index].scroll_offset);
        let next = current
            .saturating_add(delta)
            .clamp(0, i32::try_from(max_offset).unwrap_or(i32::MAX));
        self.windows[index].scroll_offset = u16::try_from(next).unwrap_or(u16::MAX);
        log::debug!(
            "[floating_window] scrolled focused static-lines float: id={}, delta={}, visible_height={}, max_offset={}, offset={}",
            self.windows[index].id.0,
            delta,
            visible_height,
            max_offset,
            self.windows[index].scroll_offset
        );
    }
}
