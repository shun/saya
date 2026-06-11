use crate::input::router::KeyInput;
use crate::presentation::screen_model::PaneRect;
use crate::terminal::emulator::TerminalCellStyle;

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
    /// terminal emulator のセル属性を renderer へ渡すための host-owned style。
    TerminalCell(TerminalCellStyle),
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
    pub(super) fn kind_name(&self) -> &'static str {
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

    pub(super) fn matches(&self, event: FloatingLifecycleEvent) -> bool {
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
    pub(super) fn trigger_name(self) -> &'static str {
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
    pub images: Vec<FloatingImage>,
}

pub(super) fn default_close_keys() -> Vec<KeyInput> {
    vec![KeyInput::Escape, KeyInput::Ctrl('[')]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloatingScreenModel {
    pub id: FloatingWindowId,
    pub content: FloatingContentRef,
    pub rect: PaneRect,
    pub lines: Vec<String>,
    pub inline_styles: Vec<FloatingInlineStyle>,
    pub images: Vec<FloatingImage>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloatingImage {
    pub line: u16,
    pub column: u16,
    pub max_width: u16,
    pub max_height: u16,
    pub view: FloatingImageView,
    pub source: FloatingImageSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FloatingImageView {
    pub zoom_percent: Option<u16>,
    pub pan_x_px: u32,
    pub pan_y_px: u32,
}

impl FloatingImageView {
    pub fn fit() -> Self {
        Self {
            zoom_percent: None,
            pan_x_px: 0,
            pan_y_px: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FloatingImageSource {
    Mermaid {
        buffer_id: i32,
        row: usize,
        alt_text: String,
        background: String,
        source: String,
    },
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
    pub content_width: u16,
    pub content_height: u16,
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
