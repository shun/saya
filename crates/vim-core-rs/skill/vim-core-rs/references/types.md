# vim-core-rs types and models

This document summarizes the public data model that host applications and tests
observe when working with `VimCoreSession`.

## Core snapshot and state
```rust
pub struct CoreSnapshot {
    pub text: String,
    pub revision: u64,
    pub dirty: bool,
    pub mode: CoreMode,
    pub pending_input: CorePendingInput,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub pending_host_actions: usize,
    pub buffers: Vec<CoreBufferInfo>,
    pub windows: Vec<CoreWindowInfo>,
    pub pum: Option<CorePumInfo>, // Pop-up menu (autocompletion)
}

pub enum CoreMode {
    Normal, Insert, Visual, VisualLine, VisualBlock,
    Replace, Select, SelectLine, SelectBlock,
    CommandLine, OperatorPending,
}

pub enum CorePendingInput {
    None, Char, Replace, MarkSet, MarkJump, Register,
}
```

`CoreSnapshot` is the most complete state capture. Prefer it when a test or
host update needs text, cursor, mode, window layout, and completion state from
one coherent read.

## Command outcomes and errors
```rust
pub enum CoreCommandOutcome {
    NoChange,
    BufferChanged { revision: u64 },
    CursorChanged { row: usize, col: usize },
    ModeChanged { mode: CoreMode },
    HostActionQueued, // Signals that host actions should be processed
}

pub enum CoreCommandError {
    InvalidInput,
    OperationFailed { reason_code: u32 },
    UnknownStatus { status: u32, reason_code: u32 },
}

pub enum CoreSessionError {
    SessionAlreadyActive,
    InitializationFailed { reason_code: &'static str },
    CommandFailed(CoreCommandError),
}
```

## Host actions
```rust
pub enum CoreHostAction {
    VfsRequest(CoreVfsRequest),
    Write { path: String, force: bool, issued_after_revision: u64 }, // Local save
    Quit { force: bool, issued_after_revision: u64 },
    Redraw { full: bool, clear_before_draw: bool },
    RequestInput { prompt: String, input_kind: CoreInputRequestKind, correlation_id: u64 },
    Bell,
    BufAdd { buf_id: i32 },
    WinNew { win_id: i32 },
    LayoutChanged,
    JobStart(CoreJobStartRequest),
    JobStop { job_id: i32 },
}

pub enum CoreInputRequestKind {
    CommandLine, Confirmation, Secret,
}
```

Treat `CoreHostAction` as part of the public API contract. Repository tests
assert both its presence and its payload details.

## Navigation, search, and rendering models
```rust
pub struct CoreMarkPosition {
    pub buf_id: i32,
    pub row: usize,
    pub col: usize,
}

pub struct CoreJumpListEntry {
    pub buf_id: i32,
    pub row: usize,
    pub col: usize,
}

pub struct CoreJumpList {
    pub current_index: usize,
    pub entries: Vec<CoreJumpListEntry>,
}

pub enum CoreSearchDirection {
    Forward,
    Backward,
}

pub enum CoreSearchHighlightMode {
    Disabled,
    HlSearch,
    IncSearch,
}

pub enum CoreMatchType {
    Regular,
    IncSearch,
    CurSearch,
}

pub struct CoreMatchRange {
    pub start_row: usize,
    pub start_col: usize,
    pub end_row: usize,
    pub end_col: usize,
    pub match_type: CoreMatchType,
}

pub struct CoreVisibleSearchState {
    pub window_id: i32,
    pub start_row: usize,
    pub end_row: usize,
    pub mode: CoreSearchHighlightMode,
    pub pattern: Option<String>,
    pub input_pattern: Option<String>,
    pub hlsearch_enabled: bool,
    pub hlsearch_suspended: bool,
    pub incsearch_active: bool,
    pub ranges: Vec<CoreMatchRange>,
}

pub struct CoreSearchCapabilityContract {
    pub live_state_query_available: bool,
    pub visible_rows_only: bool,
    pub start_col_inclusive: bool,
    pub end_col_exclusive: bool,
}

pub enum CoreSearchQueryError {
    NoActiveWindow,
    InvalidViewport { start_row: i32, end_row: i32 },
    WindowNotFound { window_id: i32 },
}
```

Search columns use byte offsets. For pane-local rendering, use the
window-targeted visible-search query APIs instead of recomputing match state in
the host.

## Undo, syntax, and completion
```rust
pub struct CoreUndoTree {
    pub nodes: Vec<CoreUndoNode>,
    pub synced: bool,
    pub seq_last: i32,
    pub save_last: i32,
    pub seq_cur: i32,
    pub time_cur: i64,
    pub save_cur: i32,
}

pub struct CoreUndoNode {
    pub seq: i32,
    pub time: i64,
    pub save_nr: i32,
    pub prev_seq: Option<i32>,
    pub next_seq: Option<i32>,
    pub alt_next_seq: Option<i32>,
    pub alt_prev_seq: Option<i32>,
    pub is_newhead: bool,
    pub is_curhead: bool,
}

pub struct CoreBufferRevision {
    pub value: u64,
}

pub struct CoreSyntaxChunk {
    pub start_col: usize,
    pub end_col: usize,
    pub syn_id: i32,
    pub name: Option<String>,
}

#[cfg(feature = "experimental-tree-sitter")]
pub struct CoreTreeSitterRangeSyntax {
    pub buffer_id: i32,
    pub source_revision: CoreBufferRevision,
    pub provenance: CoreTreeSitterProvenance,
    pub status: CoreTreeSitterStatus,
    pub has_error: bool,
    pub covered_ranges: Vec<CoreTextRange>,
    pub error_ranges: Vec<CoreTextRange>,
    pub budget_status: CoreTreeSitterBudgetStatus,
    pub chunks: Vec<CoreTreeSitterChunk>,
    pub embedded_regions: Vec<CoreEmbeddedRegion>,
}

#[cfg(feature = "experimental-tree-sitter")]
pub struct CoreTreeSitterChunk {
    pub range: CoreTextRange,
    pub capture_name: String,
    pub category: CoreSyntaxCategory,
    pub modifiers: Vec<CoreSyntaxModifier>,
}

#[cfg(feature = "experimental-tree-sitter")]
pub struct CoreTreeSitterPreparationRequest {
    pub buffer_id: i32,
    pub source_revision: Option<CoreBufferRevision>,
    pub range: CoreTextRange,
    pub vim_filetype: Option<String>,
    pub buffer_name: Option<String>,
    pub host_language_hint: Option<String>,
    pub snapshot_policy: CoreTreeSitterSnapshotPolicy,
}

#[cfg(feature = "experimental-tree-sitter")]
pub struct CoreTreeSitterSnapshotPolicy {
    pub retain_latest_per_buffer: usize,
    pub global_byte_budget: usize,
    pub max_snapshot_bytes: Option<usize>,
}

pub struct CorePumItem {
    pub word: String,
    pub abbr: String,
    pub menu: String,
    pub kind: String,
    pub info: String,
}

pub struct CorePumInfo {
    pub row: i32,
    pub col: i32,
    pub width: i32,
    pub height: i32,
    pub selected_index: Option<usize>,
    pub items: Vec<CorePumItem>,
}
```

## Search match metadata and messaging
```rust
pub struct CoreCursorMatchInfo {
    pub is_on_match: bool,
    pub current_match_index: usize,
    pub total_matches: MatchCountResult,
}

pub enum MatchCountResult {
    Calculated(usize), MaxReached(usize), TimedOut,
}

pub struct CoreMessageEvent {
    pub severity: CoreMessageSeverity,
    pub category: CoreMessageCategory,
    pub content: String,
}

pub enum CoreMessageSeverity {
    Info,
    Warning,
    Error,
}

pub enum CoreMessageCategory {
    UserVisible,
    CommandFeedback,
}
```

## Buffer, window, and option metadata
```rust
pub struct CoreBufferInfo {
    pub id: i32,
    pub name: String,
    pub dirty: bool,
    pub is_active: bool,
    pub source_kind: CoreBufferSourceKind, // Local or Vfs
    pub document_id: Option<String>,
    pub pending_vfs_operation: Option<CorePendingVfsOperation>,
    pub deferred_close: Option<CoreDeferredClose>,
    pub last_vfs_error: Option<CoreVfsError>,
}

pub struct CoreWindowInfo {
    pub id: i32,
    pub buf_id: i32,
    pub row: usize,
    pub col: usize,
    pub width: usize,
    pub height: usize,
    pub topline: usize,
    pub botline: usize,
    pub leftcol: usize,
    pub skipcol: usize,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub is_active: bool,
}

pub enum CoreOptionScope {
    Default,
    Global,
    Local,
}

pub enum CoreOptionType {
    Bool,
    Number,
    String,
}

pub enum CoreOptionError {
    UnknownOption { name: String },
    TypeMismatch {
        name: String,
        expected: CoreOptionType,
        actual: CoreOptionType,
    },
    SetFailed { name: String, reason: String },
    ScopeNotSupported { name: String, scope: CoreOptionScope },
    InternalError { name: String, detail: String },
}
```

`CoreWindowInfo` is the renderer-facing window contract. It exposes window
geometry, viewport state, active-window state, and the per-window cursor
without requiring the host to reimplement Vim window semantics.

## VFS and job-facing models

For VFS payloads, request status, deferred close variants, and job start
requests, read [vfs-vfd.md](vfs-vfd.md). Those types are operationally dense
enough that they are better understood with their flow semantics.
