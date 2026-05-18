use crate::presentation::screen_model::PaneRect;

use crate::input::router::KeyInput;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FloatingWindowId(pub u64);

/// 同じ `focus_id` で同じ `FloatingAnchorSignature` を持つ float が既に
/// 存在する場合、`open_static_lines_with_focus_toggle` は新規生成せず
/// 既存 float に focus を移す。LSP hover の "2 回目の K で float に
/// focus" のような UX を支える、汎用的な float identifier。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FloatingFocusId(String);

impl FloatingFocusId {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// 「同じ anchor 位置」を判定するための float の位置同一性キー。
/// `FloatingPlacement` は anchor / offset を含むため等価判定に向かないが、
/// このキーは「論理的に同じ場所か」を粒度を選んで表現できる。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FloatingAnchorSignature {
    Cursor {
        window_id: i32,
        row: usize,
        col: usize,
    },
    BufferPosition {
        window_id: i32,
        line: usize,
        column: usize,
    },
    Window {
        window_id: i32,
    },
    Editor,
}

impl FloatingAnchorSignature {
    pub fn cursor(window_id: i32, row: usize, col: usize) -> Self {
        Self::Cursor {
            window_id,
            row,
            col,
        }
    }

    pub fn buffer_position(window_id: i32, line: usize, column: usize) -> Self {
        Self::BufferPosition {
            window_id,
            line,
            column,
        }
    }

    pub fn window(window_id: i32) -> Self {
        Self::Window { window_id }
    }

    pub fn editor() -> Self {
        Self::Editor
    }
}

/// 行内テキストのスタイル種別。saya コア層に閉じた抽象表現で、
/// markdown のような特定プロトコル概念に依存しない。`tui_renderer` が
/// theme key にマップして実際の色 / 太字 / 下線を決定する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FloatingInlineStyleKind {
    /// 検索や selector の一致範囲
    Match,
    /// インラインコード相当（モノスペース強調）
    Code,
    /// 強調（太字 / 斜体相当）
    Emphasis,
    /// 見出し（レベルは別情報。階層強調用）
    Heading { level: u8 },
    /// リンクの可読テキスト部分
    LinkText,
    /// リンクの URL 部分
    LinkUrl,
}

/// float の特定行内に適用するインラインスタイル範囲。
/// `line` / `column_start` / `column_end` は `FloatingScreenModel.lines`
/// 配列上のバイト単位列範囲（end は exclusive）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FloatingInlineStyle {
    pub kind: FloatingInlineStyleKind,
    pub line: usize,
    pub column_start: usize,
    pub column_end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatingOpenWithFocusOutcome {
    Opened { id: FloatingWindowId },
    FocusedExisting { id: FloatingWindowId },
}

impl FloatingOpenWithFocusOutcome {
    pub fn id(&self) -> FloatingWindowId {
        match self {
            Self::Opened { id } | Self::FocusedExisting { id } => *id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceFocus {
    Pane { window_id: i32 },
    Float { float_id: FloatingWindowId },
    CommandLine,
    Prompt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FloatingContentRef {
    CoreWindow { window_id: i32 },
    ScratchBuffer { buffer_id: i32 },
    Terminal { terminal_id: u64 },
    StaticLines { content_id: u64 },
    CompletionMenu { menu_id: u64 },
}

impl FloatingContentRef {
    fn kind_name(&self) -> &'static str {
        match self {
            Self::CoreWindow { .. } => "core-window",
            Self::ScratchBuffer { .. } => "scratch-buffer",
            Self::Terminal { .. } => "terminal",
            Self::StaticLines { .. } => "static-lines",
            Self::CompletionMenu { .. } => "completion-menu",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatingRelativeTo {
    Editor,
    Window {
        window_id: i32,
    },
    Cursor {
        window_id: i32,
    },
    BufferPosition {
        window_id: i32,
        line: usize,
        column: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatingAnchor {
    NorthWest,
    NorthEast,
    SouthWest,
    SouthEast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatingFit {
    TruncateToGrid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FloatingPlacement {
    pub relative_to: FloatingRelativeTo,
    pub anchor: FloatingAnchor,
    pub row: i16,
    pub col: i16,
    pub fit: FloatingFit,
}

impl FloatingPlacement {
    pub fn editor_at(row: i16, col: i16) -> Self {
        Self {
            relative_to: FloatingRelativeTo::Editor,
            anchor: FloatingAnchor::NorthWest,
            row,
            col,
            fit: FloatingFit::TruncateToGrid,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FloatingSize {
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatingBorder {
    None,
    Single,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FloatingChrome {
    pub border: FloatingBorder,
}

impl FloatingChrome {
    pub fn borderless() -> Self {
        Self {
            border: FloatingBorder::None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatingZIndex {
    Hover,
    User,
    Completion,
    CompletionDocumentation,
    BlockingPrompt,
    Custom(i32),
}

impl FloatingZIndex {
    pub fn value(self) -> i32 {
        match self {
            Self::Hover => 40,
            Self::User => 80,
            Self::Completion => 100,
            Self::CompletionDocumentation => 120,
            Self::BlockingPrompt => 200,
            Self::Custom(value) => value,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorMode {
    Normal,
    Insert,
    Visual,
    Command,
    Replace,
    Terminal,
}

/// 複数の close トリガを宣言的にまとめる bitflag セット。
/// `CloseOnCursorMove` / `CloseOnInsert` / `CloseOnBufferChange` の単一
/// トリガでは表現できない「cursor 移動 / モード切替 / ウィンドウ離脱の
/// いずれでも閉じる」のような UX を 1 つの値で記述するために用意する。
///
/// 既存の単一トリガ enum 値は引き続きシンタックスシュガーとして残し、
/// `lifecycle_matches_event` がそれぞれを独立に判定する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FloatingCloseEvents {
    pub on_cursor_move: bool,
    pub on_mode_change: bool,
    pub on_buffer_change: bool,
    pub on_window_leave: bool,
}

impl FloatingCloseEvents {
    pub const fn none() -> Self {
        Self {
            on_cursor_move: false,
            on_mode_change: false,
            on_buffer_change: false,
            on_window_leave: false,
        }
    }

    pub const fn with_cursor_move(mut self) -> Self {
        self.on_cursor_move = true;
        self
    }

    pub const fn with_mode_change(mut self) -> Self {
        self.on_mode_change = true;
        self
    }

    pub const fn with_buffer_change(mut self) -> Self {
        self.on_buffer_change = true;
        self
    }

    pub const fn with_window_leave(mut self) -> Self {
        self.on_window_leave = true;
        self
    }

    fn matches(&self, event: FloatingLifecycleEvent) -> bool {
        match event {
            FloatingLifecycleEvent::CursorMoved { .. } => self.on_cursor_move,
            FloatingLifecycleEvent::InsertStarted { .. } => self.on_mode_change,
            FloatingLifecycleEvent::ModeChanged { from, to, .. } => {
                self.on_mode_change && from != to
            }
            FloatingLifecycleEvent::BufferChanged { .. } => self.on_buffer_change,
            FloatingLifecycleEvent::WindowLeft {
                from_window_id,
                to_window_id,
            } => self.on_window_leave && from_window_id != to_window_id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatingLifecycle {
    Manual,
    CloseOnCursorMove,
    CloseOnInsert,
    CloseOnBufferChange,
    /// 任意の close トリガを bitflag セットで宣言する。Neovim の
    /// `vim.lsp.util.open_floating_preview({ close_events = {...} })` 相当。
    CloseOnEvents(FloatingCloseEvents),
    ReplaceByGroup(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatingLifecycleEvent {
    CursorMoved {
        window_id: i32,
        row: usize,
        col: usize,
    },
    InsertStarted {
        window_id: i32,
    },
    /// 任意のモード遷移を表現する汎用イベント。`from == to` の場合は
    /// 遷移が起きていないとみなし、close 判定では発火しないことが期待。
    ModeChanged {
        window_id: i32,
        from: EditorMode,
        to: EditorMode,
    },
    BufferChanged {
        buffer_id: i32,
        revision: u64,
    },
    /// アクティブウィンドウの離脱を表す。`from_window_id != to_window_id`
    /// のときだけ「離脱した」と扱う。BufLeave 相当の UX を支える。
    WindowLeft {
        from_window_id: i32,
        to_window_id: i32,
    },
}

impl FloatingLifecycleEvent {
    fn trigger_name(self) -> &'static str {
        match self {
            Self::CursorMoved { .. } => "CursorMoved",
            Self::InsertStarted { .. } => "InsertStarted",
            Self::ModeChanged { .. } => "ModeChanged",
            Self::BufferChanged { .. } => "BufferChanged",
            Self::WindowLeft { .. } => "WindowLeft",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloatingLifecycleOutcome {
    pub closed: Vec<FloatingWindowId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloatingWindow {
    pub id: FloatingWindowId,
    pub content: FloatingContentRef,
    pub placement: FloatingPlacement,
    pub size: FloatingSize,
    pub focusable: bool,
    pub mouse: bool,
    pub chrome: FloatingChrome,
    pub zindex: i32,
    pub lifecycle: FloatingLifecycle,
    pub replacement_group: Option<String>,
    pub focus_id: Option<FloatingFocusId>,
    pub anchor_signature: Option<FloatingAnchorSignature>,
    pub close_keys: Vec<KeyInput>,
    pub inline_styles: Vec<FloatingInlineStyle>,
    pub creation_order: u64,
    pub scroll_offset: u16,
    pub lines: Vec<String>,
}

fn default_close_keys() -> Vec<KeyInput> {
    vec![KeyInput::Escape, KeyInput::Ctrl('[')]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloatingScreenModel {
    pub id: FloatingWindowId,
    pub content: FloatingContentRef,
    pub rect: PaneRect,
    pub lines: Vec<String>,
    pub inline_styles: Vec<FloatingInlineStyle>,
    pub cursor: Option<FloatingCursor>,
    pub focusable: bool,
    pub mouse: bool,
    pub chrome: FloatingChrome,
    pub zindex: i32,
    pub creation_order: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FloatingCursor {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreWindowFloatViewRequest {
    pub float_id: FloatingWindowId,
    pub window_id: i32,
    pub content_height: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalFloatViewRequest {
    pub float_id: FloatingWindowId,
    pub terminal_id: u64,
    pub content_height: u16,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatingInputOutcome {
    Ignored,
    Consumed,
    Closed { id: FloatingWindowId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatingMouseOutcome {
    PassThrough,
    Focused { id: FloatingWindowId },
}

fn visible_content_height_for(size: FloatingSize, chrome: FloatingChrome) -> u16 {
    match chrome.border {
        FloatingBorder::None => size.height.max(1),
        FloatingBorder::Single => size.height.saturating_sub(2).max(1),
    }
}

fn resolve_float_rect(
    window: &FloatingWindow,
    terminal_width: u16,
    terminal_height: u16,
    panes: &[(i32, PaneRect)],
    cursors: &[(i32, u16, u16)],
    active_window_id: Option<i32>,
) -> Option<PaneRect> {
    if terminal_width == 0
        || terminal_height == 0
        || window.size.width == 0
        || window.size.height == 0
    {
        log::debug!(
            "[floating_window] skipped float with empty grid or size: id={}, grid=({},{}), size=({},{})",
            window.id.0,
            terminal_width,
            terminal_height,
            window.size.width,
            window.size.height
        );
        return None;
    }

    let target = resolve_target_rect(
        window.placement.relative_to,
        terminal_width,
        terminal_height,
        panes,
        cursors,
        active_window_id,
    )?;
    let (base_x, base_y) = anchor_origin(target, window.placement.anchor, window.size);
    let x = base_x.saturating_add(i32::from(window.placement.col));
    let y = base_y.saturating_add(i32::from(window.placement.row));
    let x = x.max(0).min(i32::from(terminal_width.saturating_sub(1))) as u16;
    let y = y.max(0).min(i32::from(terminal_height.saturating_sub(1))) as u16;
    let width = window.size.width.min(terminal_width.saturating_sub(x));
    let height = window.size.height.min(terminal_height.saturating_sub(y));

    (width > 0 && height > 0).then_some(PaneRect {
        x,
        y,
        width,
        height,
    })
}

fn resolve_target_rect(
    relative_to: FloatingRelativeTo,
    terminal_width: u16,
    terminal_height: u16,
    panes: &[(i32, PaneRect)],
    cursors: &[(i32, u16, u16)],
    active_window_id: Option<i32>,
) -> Option<PaneRect> {
    match relative_to {
        FloatingRelativeTo::Editor => Some(PaneRect {
            x: 0,
            y: 0,
            width: terminal_width,
            height: terminal_height,
        }),
        FloatingRelativeTo::Window { window_id } => panes
            .iter()
            .find(|(id, _)| *id == window_id)
            .map(|(_, rect)| *rect),
        FloatingRelativeTo::Cursor { window_id } => {
            let window_id = active_window_id.unwrap_or(window_id);
            let pane = panes
                .iter()
                .find(|(id, _)| *id == window_id)
                .map(|(_, rect)| *rect)?;
            let (_, row, col) = cursors
                .iter()
                .find(|(id, _, _)| *id == window_id)
                .copied()
                .unwrap_or((window_id, 0, 0));
            Some(PaneRect {
                x: pane.x.saturating_add(col),
                y: pane.y.saturating_add(row),
                width: 1,
                height: 1,
            })
        }
        FloatingRelativeTo::BufferPosition { window_id, .. } => panes
            .iter()
            .find(|(id, _)| *id == window_id)
            .map(|(_, rect)| *rect),
    }
}

fn anchor_origin(target: PaneRect, anchor: FloatingAnchor, size: FloatingSize) -> (i32, i32) {
    let target_x = i32::from(target.x);
    let target_y = i32::from(target.y);
    let target_right = target_x + i32::from(target.width);
    let target_bottom = target_y + i32::from(target.height);
    let width = i32::from(size.width);
    let height = i32::from(size.height);

    match anchor {
        FloatingAnchor::NorthWest => (target_x, target_y),
        FloatingAnchor::NorthEast => (target_right - width, target_y),
        FloatingAnchor::SouthWest => (target_x, target_bottom - height),
        FloatingAnchor::SouthEast => (target_right - width, target_bottom - height),
    }
}

fn rect_contains(rect: PaneRect, x: u16, y: u16) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

fn lifecycle_matches_event(window: &FloatingWindow, event: FloatingLifecycleEvent) -> bool {
    match (window.lifecycle, event) {
        (FloatingLifecycle::Manual, _) | (FloatingLifecycle::ReplaceByGroup(_), _) => false,
        (
            FloatingLifecycle::CloseOnCursorMove,
            FloatingLifecycleEvent::CursorMoved { window_id, .. },
        ) => window_related_to_window(window, window_id),
        (FloatingLifecycle::CloseOnInsert, FloatingLifecycleEvent::InsertStarted { window_id }) => {
            window_related_to_window(window, window_id)
        }
        (FloatingLifecycle::CloseOnBufferChange, FloatingLifecycleEvent::BufferChanged { .. }) => {
            true
        }
        (FloatingLifecycle::CloseOnEvents(events), event) => {
            if !events.matches(event) {
                return false;
            }
            match event {
                FloatingLifecycleEvent::CursorMoved { window_id, .. }
                | FloatingLifecycleEvent::InsertStarted { window_id }
                | FloatingLifecycleEvent::ModeChanged { window_id, .. } => {
                    window_related_to_window(window, window_id)
                }
                FloatingLifecycleEvent::WindowLeft { from_window_id, .. } => {
                    window_related_to_window(window, from_window_id)
                }
                FloatingLifecycleEvent::BufferChanged { .. } => true,
            }
        }
        _ => false,
    }
}

fn window_related_to_window(window: &FloatingWindow, window_id: i32) -> bool {
    match window.placement.relative_to {
        FloatingRelativeTo::Editor => true,
        FloatingRelativeTo::Window {
            window_id: related_window_id,
        }
        | FloatingRelativeTo::Cursor {
            window_id: related_window_id,
        }
        | FloatingRelativeTo::BufferPosition {
            window_id: related_window_id,
            ..
        } => related_window_id == window_id,
    }
}

fn focus_target_name(restore_window_id: Option<i32>) -> &'static str {
    if restore_window_id.is_some() {
        "Pane"
    } else {
        "None"
    }
}
