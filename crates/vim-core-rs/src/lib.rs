use std::cell::RefCell;
use std::collections::VecDeque;
#[cfg(feature = "tree-sitter-syntax")]
use std::collections::{BTreeMap, HashMap};
use std::ffi::CString;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::rc::Rc;
use std::slice;
use std::str;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
#[cfg(feature = "tree-sitter-syntax")]
use tree_sitter::{Language, Node, Parser, Query, QueryCursor, StreamingIterator};

macro_rules! debug_log {
    ($($arg:tt)*) => {
        crate::debug_log::emit(format_args!($($arg)*))
    };
}

mod debug_log;
#[allow(
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    dead_code,
    unused_imports
)]
mod bindings {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

mod convert;
mod ex_command;
mod pending_input;
mod vfd;
mod vfs;

/// FFI 境界の POD 型を公開するモジュール。
/// bindgen で生成された型のうち、VFS POD 契約に必要なものを re-export する。
/// テストコードから FFI 契約の存在を検証するために使用する。
pub mod ffi {
    pub use crate::bindings::vim_core_buffer_commit_t;
    pub use crate::bindings::vim_core_buffer_info_t;

    // VFS operation kind 定数
    pub const VIM_CORE_VFS_OPERATION_NONE: u32 =
        crate::bindings::vim_core_vfs_operation_kind_VIM_CORE_VFS_OPERATION_NONE;
    pub const VIM_CORE_VFS_OPERATION_RESOLVE: u32 =
        crate::bindings::vim_core_vfs_operation_kind_VIM_CORE_VFS_OPERATION_RESOLVE;
    pub const VIM_CORE_VFS_OPERATION_EXISTS: u32 =
        crate::bindings::vim_core_vfs_operation_kind_VIM_CORE_VFS_OPERATION_EXISTS;
    pub const VIM_CORE_VFS_OPERATION_LOAD: u32 =
        crate::bindings::vim_core_vfs_operation_kind_VIM_CORE_VFS_OPERATION_LOAD;
    pub const VIM_CORE_VFS_OPERATION_SAVE: u32 =
        crate::bindings::vim_core_vfs_operation_kind_VIM_CORE_VFS_OPERATION_SAVE;

    // Buffer source kind 定数
    pub const VIM_CORE_BUFFER_SOURCE_LOCAL: u32 =
        crate::bindings::vim_core_buffer_source_kind_VIM_CORE_BUFFER_SOURCE_LOCAL;
    pub const VIM_CORE_BUFFER_SOURCE_VFS: u32 =
        crate::bindings::vim_core_buffer_source_kind_VIM_CORE_BUFFER_SOURCE_VFS;
}

use convert::{
    convert_command_result_with_snapshot, convert_event, convert_host_action, convert_jumplist,
    convert_light_snapshot, convert_mark_position, convert_native_pending_argument,
    convert_option_get_result, convert_option_scope, convert_option_set_result, convert_snapshot,
    convert_status, convert_undo_tree,
};
use ex_command::{
    find_first_trailing_quit_intent, parse_ex_intent, parse_single_ex_intent,
    rewrite_vfs_trailing_quit_intent, should_chain_trailing_quit_for_local_write,
    split_compound_ex,
};
use pending_input::{
    derive_direct_pending_input, derive_sequential_pending_input,
    mode_uses_normal_sequence_grammar, pending_for_dispatch_sequence,
};
use vfs::DocumentCoordinator;
pub use vfs::{
    CoreBufferBinding, CoreBufferSourceKind, CoreDeferredClose, CorePendingVfsOperation,
    CoreRequestEntry, CoreRequestStatus, CoreVfsError, CoreVfsErrorKind, CoreVfsOperationKind,
    CoreVfsRequest, CoreVfsResponse, VfsLogEntry, VfsLogEvent,
};

static ACTIVE_SESSION: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreMode {
    Normal,
    Insert,
    Visual,
    VisualLine,
    VisualBlock,
    Replace,
    Select,
    SelectLine,
    SelectBlock,
    CommandLine,
    OperatorPending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorePendingArgumentKind {
    Char,
    ReplaceChar,
    MarkSet,
    MarkJump,
    Register,
    MotionOrTextObject,
    NormalCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CorePendingInput {
    pub pending_keys: String,
    pub count: Option<usize>,
    pub awaited_argument: Option<CorePendingArgumentKind>,
}

impl CorePendingInput {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn is_pending(&self) -> bool {
        !self.pending_keys.is_empty() || self.count.is_some() || self.awaited_argument.is_some()
    }
}

/// ADR 0006 Phase 3: あるキー列を core に dispatch すると完成コマンドになるのか、
/// それともまだ追加キーを待つ pending 状態になるのかを表す予測結果。
///
/// host 側の入力パイプラインは、この予測を使って「pending の間は backend を呼ばず
/// キーをバッファし、完成した時だけ完成キー列を 1 回 dispatch する」越境制御を行う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreInputCompleteness {
    /// この時点で core に渡すと完成コマンドとして実行される（pending を残さない）。
    /// `pending_for_dispatch_sequence` が pending と判定しなかったキー列はここに入る。
    /// 予測器のカバー範囲外（Insert モード等の非 Normal 系文法）も「完成扱い」とし、
    /// 従来どおり即時 dispatch する（退行回避）。
    Complete,
    /// このキー列は未完成で、追加キーを待つ pending 状態になる。
    /// 例: operator `d`（motion 待ち）、`f`（文字待ち）、`g`（prefix）など。
    Pending,
}

impl CoreInputCompleteness {
    pub fn is_pending(self) -> bool {
        matches!(self, CoreInputCompleteness::Pending)
    }
}

/// ADR 0006 Phase 3: キー列が完成コマンドになるかを非破壊（状態を一切変えない）に
/// 予測する純粋関数。
///
/// `pending_input.rs` の `pending_for_dispatch_sequence` をそのまま再利用する。
/// この関数は `VimCoreSession` の状態を読まず書かないため、host から完成判定の
/// 予測器として安全に呼べる（重複文法を host 側に再実装しない、という ADR 0006 の
/// 方針に沿う）。
///
/// 注意:
/// - 対象は Normal 系の sequence 文法（`mode_uses_normal_sequence_grammar`）のみ。
///   Insert モードなど対象外のモードでは常に `Complete` を返し、host は従来どおり
///   即時 dispatch する。
/// - `pending_for_dispatch_sequence` は native typeahead を参照しない予測器なので、
///   native pending にしか現れない一部の待ち（FFI 側のみが知る状態）は判定できない。
///   その場合も `Complete` を返すため、host は「完成扱いで即送出」にフォールバックする
///   （pending を host で握りすぎて退行するより安全側）。
pub fn predict_input_completeness(sequence: &str, mode: CoreMode) -> CoreInputCompleteness {
    if sequence.is_empty() {
        return CoreInputCompleteness::Complete;
    }
    if !mode_uses_normal_sequence_grammar(mode) {
        return CoreInputCompleteness::Complete;
    }
    let predicted = pending_for_dispatch_sequence(sequence);
    if predicted.is_pending() {
        CoreInputCompleteness::Pending
    } else {
        CoreInputCompleteness::Complete
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreMarkPosition {
    pub buf_id: i32,
    pub row: usize,
    pub col: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreJumpListEntry {
    pub buf_id: i32,
    pub row: usize,
    pub col: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreJumpList {
    pub current_index: usize,
    pub entries: Vec<CoreJumpListEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreCommandOutcome {
    NoChange,
    BufferChanged { revision: u64 },
    CursorChanged { row: usize, col: usize },
    ModeChanged { mode: CoreMode },
    HostActionQueued,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreInputRequestKind {
    CommandLine,
    Confirmation,
    Secret,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreInputResponse {
    Submitted { correlation_id: u64, value: String },
    Cancelled { correlation_id: u64 },
}

impl CoreInputResponse {
    pub fn correlation_id(&self) -> u64 {
        match self {
            Self::Submitted { correlation_id, .. } | Self::Cancelled { correlation_id } => {
                *correlation_id
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreBackendIdentity {
    BridgeStub,
    UpstreamRuntime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CoreRuntimeMode {
    #[default]
    Embedded,
    Standalone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreOptionScope {
    Default,
    Global,
    Local,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreOptionType {
    Bool,
    Number,
    String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreOptionError {
    UnknownOption {
        name: String,
    },
    TypeMismatch {
        name: String,
        expected: CoreOptionType,
        actual: CoreOptionType,
    },
    SetFailed {
        name: String,
        reason: String,
    },
    ScopeNotSupported {
        name: String,
        scope: CoreOptionScope,
    },
    InternalError {
        name: String,
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreJobStartRequest {
    pub job_id: i32,
    pub argv: Vec<String>,
    pub cwd: Option<String>,
    pub vfd_in: i32,
    pub vfd_out: i32,
    pub vfd_err: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Running = 0,
    Finished = 1,
    Failed = 2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreHostAction {
    VfsRequest(CoreVfsRequest),
    Write {
        path: String,
        force: bool,
        issued_after_revision: u64,
    },
    Quit {
        force: bool,
        issued_after_revision: u64,
    },
    Suspend,
    Redraw {
        full: bool,
        clear_before_draw: bool,
    },
    RequestInput {
        prompt: String,
        input_kind: CoreInputRequestKind,
        correlation_id: u64,
    },
    Bell,
    JobStart(CoreJobStartRequest),
    JobWrite {
        vfd: i32,
        data: Vec<u8>,
    },
    JobStop {
        job_id: i32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CoreBufferRevision {
    pub value: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreBufferInfo {
    pub id: i32,
    pub name: String,
    pub source_revision: CoreBufferRevision,
    pub dirty: bool,
    pub is_active: bool,
    pub source_kind: CoreBufferSourceKind,
    pub document_id: Option<String>,
    pub pending_vfs_operation: Option<CorePendingVfsOperation>,
    pub deferred_close: Option<CoreDeferredClose>,
    pub last_vfs_error: Option<CoreVfsError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreUndoTree {
    pub nodes: Vec<CoreUndoNode>,
    pub synced: bool,
    pub seq_last: i32,
    pub save_last: i32,
    pub seq_cur: i32,
    pub time_cur: i64,
    pub save_cur: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreSyntaxChunk {
    pub start_col: usize,
    pub end_col: usize,
    pub syn_id: i32,
    pub name: Option<String>,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CoreTextPosition {
    pub row: usize,
    pub col: usize,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CoreTextRange {
    pub start: CoreTextPosition,
    pub end: CoreTextPosition,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreTreeSitterProvenance {
    pub language_id: String,
    pub package_id: String,
    pub package_version: String,
    pub parser_version: String,
    pub query_version: String,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreTreeSitterLanguagePackage {
    pub language_id: String,
    pub package_id: String,
    pub package_version: String,
    pub parser_version: String,
    pub query_version: String,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreTreeSitterStatus {
    Prepared,
    Stale,
    Unavailable,
    Unsupported,
    Partial,
    TimedOut,
    BudgetExceeded,
    TooLarge,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreTreeSitterBudgetStatus {
    WithinBudget,
    SnapshotTooLarge,
    GlobalBudgetExceeded,
    MatchLimitExceeded,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreSyntaxCategory {
    Attribute,
    Comment,
    Constant,
    Constructor,
    Function,
    Keyword,
    Label,
    Markup,
    Module,
    Number,
    Operator,
    Property,
    Punctuation,
    String,
    Tag,
    Text,
    Type,
    Variable,
    Unknown,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreSyntaxModifier {
    Async,
    Declaration,
    Definition,
    Deprecated,
    Documentation,
    Mutable,
    Readonly,
    Static,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreTreeSitterChunk {
    pub range: CoreTextRange,
    pub capture_name: String,
    pub category: CoreSyntaxCategory,
    pub modifiers: Vec<CoreSyntaxModifier>,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq)]
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

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CoreTreeSitterRequestId {
    pub value: u64,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreTreeSitterSnapshotPolicy {
    pub retain_latest_per_buffer: usize,
    pub global_byte_budget: usize,
    pub max_snapshot_bytes: Option<usize>,
}

#[cfg(feature = "tree-sitter-syntax")]
impl Default for CoreTreeSitterSnapshotPolicy {
    fn default() -> Self {
        Self {
            retain_latest_per_buffer: 4,
            global_byte_budget: 16 * 1024 * 1024,
            max_snapshot_bytes: Some(4 * 1024 * 1024),
        }
    }
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreTreeSitterPreparationRequest {
    pub buffer_id: i32,
    pub source_revision: Option<CoreBufferRevision>,
    pub range: CoreTextRange,
    pub vim_filetype: Option<String>,
    pub buffer_name: Option<String>,
    pub host_language_hint: Option<String>,
    pub snapshot_policy: CoreTreeSitterSnapshotPolicy,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreTreeSitterPreparation {
    pub request_id: CoreTreeSitterRequestId,
    pub buffer_id: i32,
    pub source_revision: CoreBufferRevision,
    pub status: CoreTreeSitterStatus,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreTreeSitterPreparationResult {
    pub request_id: CoreTreeSitterRequestId,
    pub syntax: CoreTreeSitterRangeSyntax,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreTreeSitterSnapshotStoreEntry {
    pub buffer_id: i32,
    pub source_revision: CoreBufferRevision,
    pub byte_len: usize,
    pub pin_count: usize,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreTreeSitterSnapshotStoreStats {
    pub snapshot_count: usize,
    pub pinned_snapshot_count: usize,
    pub total_unpinned_bytes: usize,
    pub snapshots: Vec<CoreTreeSitterSnapshotStoreEntry>,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreLanguageRole {
    RootDocument,
    EmbeddedRegion,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreLanguageResolutionSource {
    Registry,
    VimFiletype,
    BufferName,
    HostHint,
    MarkdownInfoString,
    MarkdownLinkTarget,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreResolutionConfidence {
    Exact,
    Alias,
    Heuristic,
    Unknown,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreLanguageResolutionStatus {
    Resolved,
    Unavailable,
    Unsupported,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreEmbeddedRegionSource {
    MarkdownFence,
    MarkdownLink,
    Unknown,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreDiagramKind {
    Mermaid,
    Drawio,
    Unknown,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreMediaKind {
    Svg,
    Png,
    Unknown,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreMediaFlavor {
    DrawioSvg,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreEmbeddedBlockKind {
    Syntax,
    Diagram {
        diagram_kind: CoreDiagramKind,
    },
    Media {
        media_kind: CoreMediaKind,
        flavor: Option<CoreMediaFlavor>,
    },
    Unknown,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreResolvedLanguage {
    pub range: CoreTextRange,
    pub role: CoreLanguageRole,
    pub status: CoreLanguageResolutionStatus,
    pub language_id: Option<String>,
    pub package_id: Option<String>,
    pub package_version: Option<String>,
    pub kind: CoreEmbeddedBlockKind,
    pub confidence: CoreResolutionConfidence,
    pub source: CoreLanguageResolutionSource,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreRootLanguageResolutionRequest {
    pub range: CoreTextRange,
    pub vim_filetype: Option<String>,
    pub buffer_name: Option<String>,
    pub host_language_hint: Option<String>,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreEmbeddedLanguageResolutionRequest {
    pub range: CoreTextRange,
    pub raw_info_string: Option<String>,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreEmbeddedRegion {
    pub range: CoreTextRange,
    pub content_range: CoreTextRange,
    pub source: CoreEmbeddedRegionSource,
    pub raw_info_string: Option<String>,
    pub normalized_info_string: Option<String>,
    pub normalized_kind: CoreEmbeddedBlockKind,
    pub resolved_language: Option<CoreResolvedLanguage>,
}

/// Vimメッセージの重要度
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreMessageSeverity {
    /// 情報メッセージ
    Info,
    /// 警告やガイダンス
    Warning,
    /// エラーメッセージ（Eから始まるメッセージ等）
    Error,
}

/// Vimメッセージの配信カテゴリ
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreMessageCategory {
    /// 埋め込みUIがユーザー向け通知として表示すべきメッセージ
    UserVisible,
    /// undo/redo などの操作進捗。UIは通常表示しない
    CommandFeedback,
}

impl CoreMessageCategory {
    pub fn is_user_visible(&self) -> bool {
        matches!(self, Self::UserVisible)
    }
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, Copy)]
struct BuiltInTreeSitterPackage {
    language_id: &'static str,
    package_id: &'static str,
    package_version: &'static str,
    parser_version: &'static str,
    query_version: &'static str,
    language: fn() -> Language,
    highlight_query: fn() -> &'static str,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq)]
struct CaptureMapping {
    category: CoreSyntaxCategory,
    modifiers: Vec<CoreSyntaxModifier>,
    priority: u16,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq)]
struct RawTreeSitterCapture {
    start_byte: usize,
    end_byte: usize,
    capture_name: String,
    category: CoreSyntaxCategory,
    modifiers: Vec<CoreSyntaxModifier>,
    priority: u16,
    query_order: usize,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TextRangeBytes {
    start: usize,
    end: usize,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TreeSitterSyntaxCacheKey {
    buffer_id: i32,
    source_revision: CoreBufferRevision,
    range: CoreTextRange,
    language_id: String,
    parser_version: String,
    query_version: String,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq)]
struct KnownTreeSitterLanguage {
    language_id: &'static str,
    package_id: &'static str,
    kind: CoreEmbeddedBlockKind,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq)]
enum BuiltInEmbeddedRegionKind {
    Syntax {
        language_id: &'static str,
        package_id: &'static str,
    },
    Diagram {
        diagram_kind: CoreDiagramKind,
    },
    Media {
        media_kind: CoreMediaKind,
        flavor: Option<CoreMediaFlavor>,
    },
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone, PartialEq, Eq)]
struct BuiltInEmbeddedRegionRule {
    aliases: &'static [&'static str],
    kind: BuiltInEmbeddedRegionKind,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Clone)]
struct TreeSitterTextSnapshot {
    buffer_id: i32,
    source_revision: CoreBufferRevision,
    text: String,
    pin_count: usize,
    last_used_tick: u64,
}

#[cfg(feature = "tree-sitter-syntax")]
#[derive(Debug, Default)]
struct TreeSitterSnapshotStore {
    snapshots: HashMap<(i32, CoreBufferRevision), TreeSitterTextSnapshot>,
    access_tick: u64,
}

#[cfg(feature = "tree-sitter-syntax")]
impl TreeSitterSnapshotStore {
    fn pin_existing(&mut self, buffer_id: i32, source_revision: CoreBufferRevision) -> bool {
        self.access_tick = self.access_tick.saturating_add(1);
        let Some(snapshot) = self.snapshots.get_mut(&(buffer_id, source_revision)) else {
            return false;
        };
        snapshot.pin_count = snapshot.pin_count.saturating_add(1);
        snapshot.last_used_tick = self.access_tick;
        true
    }

    fn pin_or_insert(
        &mut self,
        buffer_id: i32,
        source_revision: CoreBufferRevision,
        text: String,
        policy: &CoreTreeSitterSnapshotPolicy,
    ) -> Result<(), CoreTreeSitterStatus> {
        let byte_len = text.len();
        if policy
            .max_snapshot_bytes
            .map(|limit| byte_len > limit)
            .unwrap_or(false)
        {
            return Err(CoreTreeSitterStatus::TooLarge);
        }
        if byte_len > policy.global_byte_budget {
            return Err(CoreTreeSitterStatus::BudgetExceeded);
        }

        let key = (buffer_id, source_revision);
        self.access_tick = self.access_tick.saturating_add(1);
        if let Some(snapshot) = self.snapshots.get_mut(&key) {
            snapshot.pin_count = snapshot.pin_count.saturating_add(1);
            snapshot.last_used_tick = self.access_tick;
            return Ok(());
        }

        self.snapshots.insert(
            key,
            TreeSitterTextSnapshot {
                buffer_id,
                source_revision,
                text,
                pin_count: 1,
                last_used_tick: self.access_tick,
            },
        );
        match self.evict_unpinned(policy) {
            Ok(()) => Ok(()),
            Err(status) => {
                self.snapshots.remove(&key);
                Err(status)
            }
        }
    }

    fn unpin(&mut self, buffer_id: i32, source_revision: CoreBufferRevision) {
        if let Some(snapshot) = self.snapshots.get_mut(&(buffer_id, source_revision)) {
            snapshot.pin_count = snapshot.pin_count.saturating_sub(1);
        }
    }

    fn text(&self, buffer_id: i32, source_revision: CoreBufferRevision) -> Option<&str> {
        self.snapshots
            .get(&(buffer_id, source_revision))
            .map(|snapshot| snapshot.text.as_str())
    }

    fn evict_unpinned(
        &mut self,
        policy: &CoreTreeSitterSnapshotPolicy,
    ) -> Result<(), CoreTreeSitterStatus> {
        self.evict_latest_overflow(policy.retain_latest_per_buffer);
        self.evict_budget_overflow(policy.global_byte_budget);
        if self.total_unpinned_bytes() > policy.global_byte_budget {
            return Err(CoreTreeSitterStatus::BudgetExceeded);
        }
        Ok(())
    }

    fn evict_latest_overflow(&mut self, retain_latest_per_buffer: usize) {
        let mut revisions_by_buffer: HashMap<i32, Vec<CoreBufferRevision>> = HashMap::new();
        for snapshot in self.snapshots.values() {
            revisions_by_buffer
                .entry(snapshot.buffer_id)
                .or_default()
                .push(snapshot.source_revision);
        }

        let mut remove_keys = Vec::new();
        for (buffer_id, mut revisions) in revisions_by_buffer {
            revisions.sort();
            revisions.dedup();
            let keep_from = revisions.len().saturating_sub(retain_latest_per_buffer);
            for revision in &revisions[..keep_from] {
                let key = (buffer_id, *revision);
                if self
                    .snapshots
                    .get(&key)
                    .map(|snapshot| snapshot.pin_count == 0)
                    .unwrap_or(false)
                {
                    remove_keys.push(key);
                }
            }
        }

        for key in remove_keys {
            self.snapshots.remove(&key);
        }
    }

    fn evict_budget_overflow(&mut self, global_byte_budget: usize) {
        while self.total_unpinned_bytes() > global_byte_budget {
            let Some(key) = self
                .snapshots
                .iter()
                .filter(|(_, snapshot)| snapshot.pin_count == 0)
                .min_by_key(|(_, snapshot)| snapshot.last_used_tick)
                .map(|(key, _)| *key)
            else {
                break;
            };
            self.snapshots.remove(&key);
        }
    }

    fn total_unpinned_bytes(&self) -> usize {
        self.snapshots
            .values()
            .filter(|snapshot| snapshot.pin_count == 0)
            .map(|snapshot| snapshot.text.len())
            .sum()
    }

    fn stats(&self) -> CoreTreeSitterSnapshotStoreStats {
        let mut snapshots = self
            .snapshots
            .values()
            .map(|snapshot| CoreTreeSitterSnapshotStoreEntry {
                buffer_id: snapshot.buffer_id,
                source_revision: snapshot.source_revision,
                byte_len: snapshot.text.len(),
                pin_count: snapshot.pin_count,
            })
            .collect::<Vec<_>>();
        snapshots.sort_by_key(|snapshot| (snapshot.buffer_id, snapshot.source_revision));
        CoreTreeSitterSnapshotStoreStats {
            snapshot_count: snapshots.len(),
            pinned_snapshot_count: snapshots
                .iter()
                .filter(|snapshot| snapshot.pin_count > 0)
                .count(),
            total_unpinned_bytes: snapshots
                .iter()
                .filter(|snapshot| snapshot.pin_count == 0)
                .map(|snapshot| snapshot.byte_len)
                .sum(),
            snapshots,
        }
    }
}

#[cfg(feature = "tree-sitter-syntax")]
fn tree_sitter_language_packages() -> &'static [BuiltInTreeSitterPackage] {
    &[
        #[cfg(feature = "tree-sitter-markdown")]
        BuiltInTreeSitterPackage {
            language_id: "markdown",
            package_id: "tree-sitter-markdown",
            package_version: "tree-sitter-md-0.5.3",
            parser_version: "tree-sitter-md-block-0.5.3",
            query_version: "tree-sitter-md-block-highlights-0.5.3",
            language: markdown_tree_sitter_language,
            highlight_query: markdown_highlight_query,
        },
        #[cfg(feature = "tree-sitter-rust")]
        BuiltInTreeSitterPackage {
            language_id: "rust",
            package_id: "tree-sitter-rust",
            package_version: "tree-sitter-rust-0.24.2",
            parser_version: "tree-sitter-rust-0.24.2",
            query_version: "tree-sitter-rust-highlights-0.24.2",
            language: rust_tree_sitter_language,
            highlight_query: rust_highlight_query,
        },
        #[cfg(feature = "tree-sitter-go")]
        BuiltInTreeSitterPackage {
            language_id: "go",
            package_id: "tree-sitter-go",
            package_version: "tree-sitter-go-0.25.0",
            parser_version: "tree-sitter-go-0.25.0",
            query_version: "tree-sitter-go-highlights-0.25.0",
            language: go_tree_sitter_language,
            highlight_query: go_highlight_query,
        },
        #[cfg(feature = "tree-sitter-typescript")]
        BuiltInTreeSitterPackage {
            language_id: "typescript",
            package_id: "tree-sitter-typescript",
            package_version: "tree-sitter-typescript-0.23.2",
            parser_version: "tree-sitter-typescript-0.23.2",
            query_version: "tree-sitter-typescript-highlights-0.23.2",
            language: typescript_tree_sitter_language,
            highlight_query: typescript_highlight_query,
        },
        #[cfg(feature = "tree-sitter-typescript")]
        BuiltInTreeSitterPackage {
            language_id: "tsx",
            package_id: "tree-sitter-tsx",
            package_version: "tree-sitter-typescript-0.23.2",
            parser_version: "tree-sitter-tsx-0.23.2",
            query_version: "tree-sitter-typescript-highlights-0.23.2",
            language: tsx_tree_sitter_language,
            highlight_query: typescript_highlight_query,
        },
    ]
}

#[cfg(all(feature = "tree-sitter-syntax", feature = "tree-sitter-markdown"))]
fn markdown_tree_sitter_language() -> Language {
    tree_sitter_md::LANGUAGE.into()
}

#[cfg(all(feature = "tree-sitter-syntax", feature = "tree-sitter-markdown"))]
fn markdown_inline_tree_sitter_language() -> Language {
    tree_sitter_md::INLINE_LANGUAGE.into()
}

#[cfg(all(feature = "tree-sitter-syntax", feature = "tree-sitter-markdown"))]
fn markdown_highlight_query() -> &'static str {
    tree_sitter_md::HIGHLIGHT_QUERY_BLOCK
}

#[cfg(all(feature = "tree-sitter-syntax", feature = "tree-sitter-rust"))]
fn rust_tree_sitter_language() -> Language {
    tree_sitter_rust::LANGUAGE.into()
}

#[cfg(all(feature = "tree-sitter-syntax", feature = "tree-sitter-rust"))]
fn rust_highlight_query() -> &'static str {
    tree_sitter_rust::HIGHLIGHTS_QUERY
}

#[cfg(all(feature = "tree-sitter-syntax", feature = "tree-sitter-go"))]
fn go_tree_sitter_language() -> Language {
    tree_sitter_go::LANGUAGE.into()
}

#[cfg(all(feature = "tree-sitter-syntax", feature = "tree-sitter-go"))]
fn go_highlight_query() -> &'static str {
    tree_sitter_go::HIGHLIGHTS_QUERY
}

#[cfg(all(feature = "tree-sitter-syntax", feature = "tree-sitter-typescript"))]
fn typescript_tree_sitter_language() -> Language {
    tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
}

#[cfg(all(feature = "tree-sitter-syntax", feature = "tree-sitter-typescript"))]
fn tsx_tree_sitter_language() -> Language {
    tree_sitter_typescript::LANGUAGE_TSX.into()
}

#[cfg(all(feature = "tree-sitter-syntax", feature = "tree-sitter-typescript"))]
fn typescript_highlight_query() -> &'static str {
    tree_sitter_typescript::HIGHLIGHTS_QUERY
}

#[cfg(feature = "tree-sitter-syntax")]
fn resolve_root_language(request: CoreRootLanguageResolutionRequest) -> CoreResolvedLanguage {
    let candidates = [
        request
            .vim_filetype
            .as_deref()
            .and_then(language_from_filetype)
            .map(|language| (language, CoreLanguageResolutionSource::VimFiletype)),
        request
            .buffer_name
            .as_deref()
            .and_then(language_from_buffer_name)
            .map(|language| (language, CoreLanguageResolutionSource::BufferName)),
        request
            .host_language_hint
            .as_deref()
            .and_then(language_from_hint)
            .map(|language| (language, CoreLanguageResolutionSource::HostHint)),
    ];

    candidates
        .into_iter()
        .flatten()
        .next()
        .map(|(language, source)| {
            resolved_language_from_known(
                request.range,
                CoreLanguageRole::RootDocument,
                language,
                source,
                CoreResolutionConfidence::Exact,
            )
        })
        .unwrap_or_else(|| unsupported_language(request.range, CoreLanguageRole::RootDocument))
}

#[cfg(feature = "tree-sitter-syntax")]
fn resolve_embedded_language(
    request: CoreEmbeddedLanguageResolutionRequest,
) -> CoreResolvedLanguage {
    let Some(normalized) = normalize_markdown_info_string(request.raw_info_string.as_deref())
    else {
        return unsupported_language(request.range, CoreLanguageRole::EmbeddedRegion);
    };

    resolve_markdown_embedded_region_kind(
        request.range,
        &normalized,
        CoreLanguageRole::EmbeddedRegion,
        CoreLanguageResolutionSource::MarkdownInfoString,
    )
    .unwrap_or_else(|| {
        unsupported_language_with_source(
            request.range,
            CoreLanguageRole::EmbeddedRegion,
            CoreLanguageResolutionSource::MarkdownInfoString,
        )
    })
}

#[cfg(feature = "tree-sitter-syntax")]
fn resolved_markdown_link_media(
    range: CoreTextRange,
    kind: CoreEmbeddedBlockKind,
) -> CoreResolvedLanguage {
    CoreResolvedLanguage {
        range,
        role: CoreLanguageRole::EmbeddedRegion,
        status: CoreLanguageResolutionStatus::Unsupported,
        language_id: None,
        package_id: None,
        package_version: None,
        kind,
        confidence: CoreResolutionConfidence::Exact,
        source: CoreLanguageResolutionSource::MarkdownLinkTarget,
    }
}

#[cfg(feature = "tree-sitter-syntax")]
fn resolved_language_from_known(
    range: CoreTextRange,
    role: CoreLanguageRole,
    language: KnownTreeSitterLanguage,
    source: CoreLanguageResolutionSource,
    confidence: CoreResolutionConfidence,
) -> CoreResolvedLanguage {
    let registered_package = registered_package_for_language(language.language_id);
    CoreResolvedLanguage {
        range,
        role,
        status: if registered_package.is_some() {
            CoreLanguageResolutionStatus::Resolved
        } else {
            CoreLanguageResolutionStatus::Unavailable
        },
        language_id: Some(language.language_id.to_string()),
        package_id: Some(language.package_id.to_string()),
        package_version: registered_package.map(|package| package.package_version.to_string()),
        kind: language.kind,
        confidence,
        source,
    }
}

#[cfg(feature = "tree-sitter-syntax")]
fn unsupported_language(range: CoreTextRange, role: CoreLanguageRole) -> CoreResolvedLanguage {
    unsupported_language_with_source(range, role, CoreLanguageResolutionSource::Registry)
}

#[cfg(feature = "tree-sitter-syntax")]
fn unsupported_language_with_source(
    range: CoreTextRange,
    role: CoreLanguageRole,
    source: CoreLanguageResolutionSource,
) -> CoreResolvedLanguage {
    CoreResolvedLanguage {
        range,
        role,
        status: CoreLanguageResolutionStatus::Unsupported,
        language_id: None,
        package_id: None,
        package_version: None,
        kind: CoreEmbeddedBlockKind::Unknown,
        confidence: CoreResolutionConfidence::Unknown,
        source,
    }
}

#[cfg(feature = "tree-sitter-syntax")]
fn registered_package_for_language(language_id: &str) -> Option<BuiltInTreeSitterPackage> {
    tree_sitter_language_packages()
        .iter()
        .copied()
        .find(|package| package.language_id == language_id)
}

#[cfg(feature = "tree-sitter-syntax")]
fn tree_sitter_status_from_resolution(resolved: &CoreResolvedLanguage) -> CoreTreeSitterStatus {
    match resolved.status {
        CoreLanguageResolutionStatus::Resolved => CoreTreeSitterStatus::Prepared,
        CoreLanguageResolutionStatus::Unavailable => CoreTreeSitterStatus::Unavailable,
        CoreLanguageResolutionStatus::Unsupported => CoreTreeSitterStatus::Unsupported,
    }
}

#[cfg(feature = "tree-sitter-syntax")]
fn provenance_for_resolved_language(resolved: &CoreResolvedLanguage) -> CoreTreeSitterProvenance {
    let package = resolved
        .language_id
        .as_deref()
        .and_then(registered_package_for_language);
    CoreTreeSitterProvenance {
        language_id: resolved.language_id.clone().unwrap_or_default(),
        package_id: resolved.package_id.clone().unwrap_or_default(),
        package_version: package
            .map(|package| package.package_version.to_string())
            .or_else(|| resolved.package_version.clone())
            .unwrap_or_default(),
        parser_version: package
            .map(|package| package.parser_version.to_string())
            .unwrap_or_default(),
        query_version: package
            .map(|package| package.query_version.to_string())
            .unwrap_or_default(),
    }
}

#[cfg(feature = "tree-sitter-syntax")]
fn tree_sitter_syntax_cache_key(
    syntax: &CoreTreeSitterRangeSyntax,
    requested_range: CoreTextRange,
) -> TreeSitterSyntaxCacheKey {
    TreeSitterSyntaxCacheKey {
        buffer_id: syntax.buffer_id,
        source_revision: syntax.source_revision,
        range: requested_range,
        language_id: syntax.provenance.language_id.clone(),
        parser_version: syntax.provenance.parser_version.clone(),
        query_version: syntax.provenance.query_version.clone(),
    }
}

#[cfg(feature = "tree-sitter-syntax")]
fn language_from_filetype(filetype: &str) -> Option<KnownTreeSitterLanguage> {
    let normalized = normalize_language_token(filetype)?;
    language_from_hint(&normalized)
}

#[cfg(feature = "tree-sitter-syntax")]
fn language_from_buffer_name(buffer_name: &str) -> Option<KnownTreeSitterLanguage> {
    let lower = buffer_name.to_ascii_lowercase();
    if lower.ends_with(".rs") {
        return known_syntax_language("rust", "tree-sitter-rust");
    }
    if lower.ends_with(".go") {
        return known_syntax_language("go", "tree-sitter-go");
    }
    if lower.ends_with(".tsx") {
        return known_syntax_language("tsx", "tree-sitter-tsx");
    }
    if lower.ends_with(".ts") || lower.ends_with(".mts") || lower.ends_with(".cts") {
        return known_syntax_language("typescript", "tree-sitter-typescript");
    }
    if lower.ends_with(".md")
        || lower.ends_with(".markdown")
        || lower.ends_with(".mkd")
        || lower.ends_with(".mdown")
    {
        return known_syntax_language("markdown", "tree-sitter-markdown");
    }
    None
}

#[cfg(feature = "tree-sitter-syntax")]
fn language_from_hint(hint: &str) -> Option<KnownTreeSitterLanguage> {
    let normalized = normalize_language_token(hint)?;
    match normalized.as_str() {
        "rust" | "rs" => known_syntax_language("rust", "tree-sitter-rust"),
        "go" | "golang" => known_syntax_language("go", "tree-sitter-go"),
        "markdown" | "md" | "mkd" | "mdown" => {
            known_syntax_language("markdown", "tree-sitter-markdown")
        }
        "typescript" | "ts" | "mts" | "cts" => {
            known_syntax_language("typescript", "tree-sitter-typescript")
        }
        "tsx" => known_syntax_language("tsx", "tree-sitter-tsx"),
        _ => None,
    }
}

#[cfg(feature = "tree-sitter-syntax")]
fn known_syntax_language(
    language_id: &'static str,
    package_id: &'static str,
) -> Option<KnownTreeSitterLanguage> {
    Some(KnownTreeSitterLanguage {
        language_id,
        package_id,
        kind: CoreEmbeddedBlockKind::Syntax,
    })
}

#[cfg(feature = "tree-sitter-syntax")]
fn built_in_embedded_region_rules() -> &'static [BuiltInEmbeddedRegionRule] {
    &[
        BuiltInEmbeddedRegionRule {
            aliases: &["rust", "rs"],
            kind: BuiltInEmbeddedRegionKind::Syntax {
                language_id: "rust",
                package_id: "tree-sitter-rust",
            },
        },
        BuiltInEmbeddedRegionRule {
            aliases: &["go", "golang"],
            kind: BuiltInEmbeddedRegionKind::Syntax {
                language_id: "go",
                package_id: "tree-sitter-go",
            },
        },
        BuiltInEmbeddedRegionRule {
            aliases: &["markdown", "md", "mkd", "mdown"],
            kind: BuiltInEmbeddedRegionKind::Syntax {
                language_id: "markdown",
                package_id: "tree-sitter-markdown",
            },
        },
        BuiltInEmbeddedRegionRule {
            aliases: &["typescript", "ts", "mts", "cts"],
            kind: BuiltInEmbeddedRegionKind::Syntax {
                language_id: "typescript",
                package_id: "tree-sitter-typescript",
            },
        },
        BuiltInEmbeddedRegionRule {
            aliases: &["tsx"],
            kind: BuiltInEmbeddedRegionKind::Syntax {
                language_id: "tsx",
                package_id: "tree-sitter-tsx",
            },
        },
        BuiltInEmbeddedRegionRule {
            aliases: &["mermaid"],
            kind: BuiltInEmbeddedRegionKind::Diagram {
                diagram_kind: CoreDiagramKind::Mermaid,
            },
        },
        BuiltInEmbeddedRegionRule {
            aliases: &["svg"],
            kind: BuiltInEmbeddedRegionKind::Media {
                media_kind: CoreMediaKind::Svg,
                flavor: None,
            },
        },
        BuiltInEmbeddedRegionRule {
            aliases: &["png"],
            kind: BuiltInEmbeddedRegionKind::Media {
                media_kind: CoreMediaKind::Png,
                flavor: None,
            },
        },
    ]
}

#[cfg(feature = "tree-sitter-syntax")]
fn resolve_markdown_embedded_region_kind(
    range: CoreTextRange,
    normalized_info_string: &str,
    role: CoreLanguageRole,
    source: CoreLanguageResolutionSource,
) -> Option<CoreResolvedLanguage> {
    let rule = built_in_embedded_region_rules().iter().find(|rule| {
        rule.aliases
            .iter()
            .any(|alias| *alias == normalized_info_string)
    })?;

    let resolved = match &rule.kind {
        BuiltInEmbeddedRegionKind::Syntax {
            language_id,
            package_id,
        } => resolved_language_from_known(
            range,
            role,
            KnownTreeSitterLanguage {
                language_id: *language_id,
                package_id: *package_id,
                kind: CoreEmbeddedBlockKind::Syntax,
            },
            source,
            CoreResolutionConfidence::Exact,
        ),
        BuiltInEmbeddedRegionKind::Diagram { diagram_kind } => CoreResolvedLanguage {
            range,
            role,
            status: CoreLanguageResolutionStatus::Unsupported,
            language_id: None,
            package_id: None,
            package_version: None,
            kind: CoreEmbeddedBlockKind::Diagram {
                diagram_kind: diagram_kind.clone(),
            },
            confidence: CoreResolutionConfidence::Exact,
            source,
        },
        BuiltInEmbeddedRegionKind::Media { media_kind, flavor } => CoreResolvedLanguage {
            range,
            role,
            status: CoreLanguageResolutionStatus::Unsupported,
            language_id: None,
            package_id: None,
            package_version: None,
            kind: CoreEmbeddedBlockKind::Media {
                media_kind: media_kind.clone(),
                flavor: flavor.clone(),
            },
            confidence: CoreResolutionConfidence::Exact,
            source,
        },
    };

    Some(resolved)
}

#[cfg(feature = "tree-sitter-syntax")]
fn normalize_markdown_info_string(info_string: Option<&str>) -> Option<String> {
    let info_string = info_string?.trim();
    let first_token = info_string
        .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == '{' || ch == '}')
        .find(|token| !token.is_empty())?;
    let normalized = normalize_language_token(first_token)?;
    Some(canonical_markdown_info_string(&normalized).unwrap_or(normalized))
}

#[cfg(feature = "tree-sitter-syntax")]
fn canonical_markdown_info_string(normalized_info_string: &str) -> Option<String> {
    let rule = built_in_embedded_region_rules().iter().find(|rule| {
        rule.aliases
            .iter()
            .any(|alias| *alias == normalized_info_string)
    })?;
    match &rule.kind {
        BuiltInEmbeddedRegionKind::Syntax { language_id, .. } => Some((*language_id).to_string()),
        BuiltInEmbeddedRegionKind::Diagram { .. } | BuiltInEmbeddedRegionKind::Media { .. } => {
            rule.aliases.first().map(|alias| (*alias).to_string())
        }
    }
}

#[cfg(feature = "tree-sitter-syntax")]
fn normalize_language_token(value: &str) -> Option<String> {
    let normalized = value.trim().trim_start_matches('.').to_ascii_lowercase();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

#[cfg(feature = "tree-sitter-syntax")]
fn tree_sitter_package_language_and_query(language_id: &str) -> Option<(Language, &'static str)> {
    let package = registered_package_for_language(language_id)?;
    Some(((package.language)(), (package.highlight_query)()))
}

#[cfg(feature = "tree-sitter-syntax")]
fn text_range_from_bytes(line_starts: &[usize], start: usize, end: usize) -> CoreTextRange {
    CoreTextRange {
        start: position_for_byte(line_starts, start),
        end: position_for_byte(line_starts, end),
    }
}

#[cfg(feature = "tree-sitter-syntax")]
fn capture_mapping(capture_name: &str) -> CaptureMapping {
    let mut parts = capture_name.split('.');
    let base = parts.next().unwrap_or_default();
    let mut modifiers = Vec::new();
    let (category, priority) = match base {
        "attribute" => (CoreSyntaxCategory::Attribute, 70),
        "comment" => (CoreSyntaxCategory::Comment, 90),
        "constant" => (CoreSyntaxCategory::Constant, 70),
        "constructor" => (CoreSyntaxCategory::Constructor, 70),
        "function" => (CoreSyntaxCategory::Function, 70),
        "keyword" => (CoreSyntaxCategory::Keyword, 75),
        "label" => (CoreSyntaxCategory::Label, 65),
        "markup" | "text" => (CoreSyntaxCategory::Markup, 45),
        "module" | "namespace" => (CoreSyntaxCategory::Module, 65),
        "number" => (CoreSyntaxCategory::Number, 70),
        "operator" => (CoreSyntaxCategory::Operator, 65),
        "property" | "field" => (CoreSyntaxCategory::Property, 65),
        "punctuation" => (CoreSyntaxCategory::Punctuation, 55),
        "string" | "escape" => (CoreSyntaxCategory::String, 80),
        "tag" => (CoreSyntaxCategory::Tag, 65),
        "type" => (CoreSyntaxCategory::Type, 70),
        "variable" => (CoreSyntaxCategory::Variable, 60),
        "none" => (CoreSyntaxCategory::Text, 1),
        _ => (CoreSyntaxCategory::Unknown, 10),
    };

    for part in capture_name.split('.').skip(1) {
        match part {
            "async" => modifiers.push(CoreSyntaxModifier::Async),
            "declaration" => modifiers.push(CoreSyntaxModifier::Declaration),
            "definition" => modifiers.push(CoreSyntaxModifier::Definition),
            "deprecated" => modifiers.push(CoreSyntaxModifier::Deprecated),
            "documentation" => modifiers.push(CoreSyntaxModifier::Documentation),
            "mutable" => modifiers.push(CoreSyntaxModifier::Mutable),
            "readonly" => modifiers.push(CoreSyntaxModifier::Readonly),
            "static" => modifiers.push(CoreSyntaxModifier::Static),
            _ => {}
        }
    }
    if capture_name == "text.title" || capture_name == "markup.heading" {
        modifiers.push(CoreSyntaxModifier::Definition);
    }
    if capture_name == "comment.documentation" {
        modifiers.push(CoreSyntaxModifier::Documentation);
    }
    modifiers.sort_by_key(|modifier| format!("{modifier:?}"));
    modifiers.dedup();
    CaptureMapping {
        category,
        modifiers,
        priority,
    }
}

#[cfg(feature = "tree-sitter-syntax")]
fn line_start_offsets(text: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            offsets.push(index + 1);
        }
    }
    offsets
}

#[cfg(feature = "tree-sitter-syntax")]
fn line_end_offset(text: &str, line_starts: &[usize], row: usize) -> usize {
    let Some(&line_start) = line_starts.get(row) else {
        return text.len();
    };
    line_starts
        .get(row + 1)
        .copied()
        .map(|next_start| next_start.saturating_sub(1))
        .unwrap_or(text.len())
        .max(line_start)
}

#[cfg(feature = "tree-sitter-syntax")]
fn byte_for_position(text: &str, line_starts: &[usize], position: CoreTextPosition) -> usize {
    let Some(&line_start) = line_starts.get(position.row) else {
        return text.len();
    };
    let line_end = line_end_offset(text, line_starts, position.row);
    line_start.saturating_add(position.col).min(line_end)
}

#[cfg(feature = "tree-sitter-syntax")]
fn position_for_byte(line_starts: &[usize], byte: usize) -> CoreTextPosition {
    let row = match line_starts.binary_search(&byte) {
        Ok(row) => row,
        Err(0) => 0,
        Err(next) => next - 1,
    };
    CoreTextPosition {
        row,
        col: byte.saturating_sub(line_starts[row]),
    }
}

#[cfg(feature = "tree-sitter-syntax")]
fn bytes_for_text_range(text: &str, range: CoreTextRange) -> TextRangeBytes {
    let line_starts = line_start_offsets(text);
    let start = byte_for_position(text, &line_starts, range.start);
    let end = byte_for_position(text, &line_starts, range.end).max(start);
    TextRangeBytes { start, end }
}

#[cfg(feature = "tree-sitter-syntax")]
fn normalize_tree_sitter_captures(
    text: &str,
    requested_range: CoreTextRange,
    raw_captures: Vec<RawTreeSitterCapture>,
) -> Vec<CoreTreeSitterChunk> {
    let requested_bytes = bytes_for_text_range(text, requested_range);
    let line_starts = line_start_offsets(text);
    let mut boundaries = vec![requested_bytes.start, requested_bytes.end];

    for capture in &raw_captures {
        let start = capture.start_byte.max(requested_bytes.start);
        let end = capture.end_byte.min(requested_bytes.end);
        if start < end {
            boundaries.push(start);
            boundaries.push(end);
        }
    }

    boundaries.sort_unstable();
    boundaries.dedup();

    let mut chunks: Vec<CoreTreeSitterChunk> = Vec::new();
    for window in boundaries.windows(2) {
        let start = window[0];
        let end = window[1];
        if start >= end {
            continue;
        }

        let Some(winner) = raw_captures
            .iter()
            .filter(|capture| capture.start_byte <= start && capture.end_byte >= end)
            .max_by_key(|capture| (capture.priority, capture.query_order))
        else {
            continue;
        };

        if matches!(winner.category, CoreSyntaxCategory::Text) && winner.capture_name == "none" {
            continue;
        }

        let range = CoreTextRange {
            start: position_for_byte(&line_starts, start),
            end: position_for_byte(&line_starts, end),
        };
        if let Some(previous) = chunks.last_mut() {
            if previous.range.end == range.start
                && previous.capture_name == winner.capture_name
                && previous.category == winner.category
                && previous.modifiers == winner.modifiers
            {
                previous.range.end = range.end;
                continue;
            }
        }

        chunks.push(CoreTreeSitterChunk {
            range,
            capture_name: winner.capture_name.clone(),
            category: winner.category,
            modifiers: winner.modifiers.clone(),
        });
    }

    chunks
}

#[cfg(feature = "tree-sitter-syntax")]
fn parse_tree_sitter_syntax(
    text: &str,
    range: CoreTextRange,
    resolved: &CoreResolvedLanguage,
) -> (
    CoreTreeSitterStatus,
    bool,
    Vec<CoreTextRange>,
    Vec<CoreTextRange>,
    CoreTreeSitterBudgetStatus,
    Vec<CoreTreeSitterChunk>,
    Vec<CoreEmbeddedRegion>,
) {
    if !matches!(resolved.status, CoreLanguageResolutionStatus::Resolved) {
        return (
            tree_sitter_status_from_resolution(resolved),
            false,
            Vec::new(),
            Vec::new(),
            CoreTreeSitterBudgetStatus::WithinBudget,
            Vec::new(),
            Vec::new(),
        );
    }
    let Some(language_id) = resolved.language_id.as_deref() else {
        return (
            CoreTreeSitterStatus::Unsupported,
            false,
            Vec::new(),
            Vec::new(),
            CoreTreeSitterBudgetStatus::WithinBudget,
            Vec::new(),
            Vec::new(),
        );
    };
    let Some((language, query_source)) = tree_sitter_package_language_and_query(language_id) else {
        return (
            CoreTreeSitterStatus::Unavailable,
            false,
            Vec::new(),
            Vec::new(),
            CoreTreeSitterBudgetStatus::WithinBudget,
            Vec::new(),
            Vec::new(),
        );
    };

    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        return (
            CoreTreeSitterStatus::Unavailable,
            false,
            Vec::new(),
            Vec::new(),
            CoreTreeSitterBudgetStatus::WithinBudget,
            Vec::new(),
            Vec::new(),
        );
    }
    let Some(tree) = parser.parse(text, None) else {
        return (
            CoreTreeSitterStatus::Partial,
            true,
            Vec::new(),
            Vec::new(),
            CoreTreeSitterBudgetStatus::WithinBudget,
            Vec::new(),
            Vec::new(),
        );
    };
    let has_error = tree.root_node().has_error();
    let line_starts = line_start_offsets(text);
    let requested_bytes = bytes_for_text_range(text, range);
    let covered_ranges = vec![range];
    let error_ranges =
        collect_tree_sitter_error_ranges(&line_starts, requested_bytes, tree.root_node());
    let embedded_regions = if language_id == "markdown" {
        collect_markdown_embedded_regions(text, range, tree.root_node())
    } else {
        Vec::new()
    };
    let Ok(query) = Query::new(&language, query_source) else {
        return (
            CoreTreeSitterStatus::Unavailable,
            has_error,
            covered_ranges,
            error_ranges,
            CoreTreeSitterBudgetStatus::WithinBudget,
            Vec::new(),
            embedded_regions,
        );
    };

    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(requested_bytes.start..requested_bytes.end);
    let capture_names = query.capture_names();
    let mut captures = cursor.captures(&query, tree.root_node(), text.as_bytes());
    let mut raw_captures = Vec::new();
    while let Some((query_match, capture_index)) = captures.next() {
        let capture = query_match.captures[*capture_index];
        let node = capture.node;
        let start_byte = node.start_byte().max(requested_bytes.start);
        let end_byte = node.end_byte().min(requested_bytes.end);
        if start_byte >= end_byte {
            continue;
        }
        let capture_name = capture_names
            .get(capture.index as usize)
            .copied()
            .unwrap_or("unknown");
        let mapping = capture_mapping(capture_name);
        raw_captures.push(RawTreeSitterCapture {
            start_byte,
            end_byte,
            capture_name: capture_name.to_string(),
            category: mapping.category,
            modifiers: mapping.modifiers,
            priority: mapping.priority,
            query_order: query_match.pattern_index,
        });
    }
    drop(captures);

    let mut status = CoreTreeSitterStatus::Prepared;
    if cursor.did_exceed_match_limit() {
        status = CoreTreeSitterStatus::Partial;
    }
    let chunks = normalize_tree_sitter_captures(text, range, raw_captures);
    let budget_status = if matches!(status, CoreTreeSitterStatus::Partial) {
        CoreTreeSitterBudgetStatus::MatchLimitExceeded
    } else {
        CoreTreeSitterBudgetStatus::WithinBudget
    };
    let chunks = if language_id == "markdown" {
        inject_markdown_embedded_syntax_chunks(text, chunks, &embedded_regions)
    } else {
        chunks
    };
    (
        status,
        has_error,
        covered_ranges,
        error_ranges,
        budget_status,
        chunks,
        embedded_regions,
    )
}

#[cfg(feature = "tree-sitter-syntax")]
fn collect_tree_sitter_error_ranges(
    line_starts: &[usize],
    requested_bytes: TextRangeBytes,
    node: Node<'_>,
) -> Vec<CoreTextRange> {
    let mut ranges = Vec::new();
    collect_tree_sitter_error_ranges_from_node(line_starts, requested_bytes, node, &mut ranges);
    ranges.sort_by_key(|range| (range.start, range.end));
    ranges.dedup();
    ranges
}

#[cfg(feature = "tree-sitter-syntax")]
fn collect_tree_sitter_error_ranges_from_node(
    line_starts: &[usize],
    requested_bytes: TextRangeBytes,
    node: Node<'_>,
    ranges: &mut Vec<CoreTextRange>,
) {
    if node.end_byte() <= requested_bytes.start || node.start_byte() >= requested_bytes.end {
        return;
    }
    if node.is_error() || node.is_missing() {
        let start = node.start_byte().max(requested_bytes.start);
        let end = node.end_byte().min(requested_bytes.end);
        if start < end {
            ranges.push(text_range_from_bytes(line_starts, start, end));
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_tree_sitter_error_ranges_from_node(line_starts, requested_bytes, child, ranges);
    }
}

#[cfg(feature = "tree-sitter-syntax")]
fn inject_markdown_embedded_syntax_chunks(
    text: &str,
    mut chunks: Vec<CoreTreeSitterChunk>,
    embedded_regions: &[CoreEmbeddedRegion],
) -> Vec<CoreTreeSitterChunk> {
    for region in embedded_regions {
        let Some(resolved) = region.resolved_language.as_ref() else {
            continue;
        };
        let Some(language_id) = resolved.language_id.as_deref() else {
            continue;
        };
        if !matches!(region.normalized_kind, CoreEmbeddedBlockKind::Syntax)
            || !matches!(resolved.status, CoreLanguageResolutionStatus::Resolved)
            || tree_sitter_package_language_and_query(language_id).is_none()
        {
            continue;
        }

        let child_text_bytes = bytes_for_text_range(text, region.content_range);
        let child_text = &text[child_text_bytes.start..child_text_bytes.end];
        let child_line_starts = line_start_offsets(child_text);
        let child_range = CoreTextRange {
            start: CoreTextPosition { row: 0, col: 0 },
            end: position_for_byte(&child_line_starts, child_text.len()),
        };
        let (status, _, _, _, _, child_chunks, _) =
            parse_tree_sitter_syntax(child_text, child_range, resolved);
        if matches!(
            status,
            CoreTreeSitterStatus::Prepared | CoreTreeSitterStatus::Partial
        ) {
            chunks.retain(|chunk| {
                chunk.range.end <= region.content_range.start
                    || chunk.range.start >= region.content_range.end
            });
            chunks.extend(
                child_chunks
                    .into_iter()
                    .map(|chunk| offset_tree_sitter_chunk(chunk, region.content_range.start))
                    .filter(|chunk| {
                        chunk.range.start >= region.content_range.start
                            && chunk.range.end <= region.content_range.end
                    }),
            );
        }
    }
    chunks.sort_by_key(|chunk| (chunk.range.start, chunk.range.end));
    chunks
}

#[cfg(feature = "tree-sitter-syntax")]
fn offset_tree_sitter_chunk(
    chunk: CoreTreeSitterChunk,
    base: CoreTextPosition,
) -> CoreTreeSitterChunk {
    CoreTreeSitterChunk {
        range: CoreTextRange {
            start: offset_text_position(chunk.range.start, base),
            end: offset_text_position(chunk.range.end, base),
        },
        capture_name: chunk.capture_name,
        category: chunk.category,
        modifiers: chunk.modifiers,
    }
}

#[cfg(feature = "tree-sitter-syntax")]
fn offset_text_position(position: CoreTextPosition, base: CoreTextPosition) -> CoreTextPosition {
    CoreTextPosition {
        row: base.row + position.row,
        col: if position.row == 0 {
            base.col + position.col
        } else {
            position.col
        },
    }
}

#[cfg(feature = "tree-sitter-syntax")]
fn clip_tree_sitter_syntax_to_range(
    syntax: &CoreTreeSitterRangeSyntax,
    requested_range: CoreTextRange,
) -> CoreTreeSitterRangeSyntax {
    let mut clipped = syntax.clone();
    clipped.chunks = syntax
        .chunks
        .iter()
        .filter_map(|chunk| intersect_tree_sitter_chunk(chunk, requested_range))
        .collect();
    clipped.covered_ranges = syntax
        .covered_ranges
        .iter()
        .filter_map(|range| intersect_text_range(*range, requested_range))
        .collect();
    clipped.error_ranges = syntax
        .error_ranges
        .iter()
        .filter_map(|range| intersect_text_range(*range, requested_range))
        .collect();
    clipped.embedded_regions = syntax
        .embedded_regions
        .iter()
        .filter(|region| {
            region.range.start < requested_range.end && region.range.end > requested_range.start
        })
        .cloned()
        .collect();
    clipped
}

#[cfg(feature = "tree-sitter-syntax")]
fn intersect_text_range(
    range: CoreTextRange,
    requested_range: CoreTextRange,
) -> Option<CoreTextRange> {
    let start = range.start.max(requested_range.start);
    let end = range.end.min(requested_range.end);
    if start >= end {
        return None;
    }
    Some(CoreTextRange { start, end })
}

#[cfg(feature = "tree-sitter-syntax")]
fn intersect_tree_sitter_chunk(
    chunk: &CoreTreeSitterChunk,
    range: CoreTextRange,
) -> Option<CoreTreeSitterChunk> {
    let range = intersect_text_range(chunk.range, range)?;
    Some(CoreTreeSitterChunk {
        range,
        capture_name: chunk.capture_name.clone(),
        category: chunk.category,
        modifiers: chunk.modifiers.clone(),
    })
}

#[cfg(feature = "tree-sitter-syntax")]
fn collect_markdown_embedded_regions(
    text: &str,
    requested_range: CoreTextRange,
    root_node: Node<'_>,
) -> Vec<CoreEmbeddedRegion> {
    let requested_bytes = bytes_for_text_range(text, requested_range);
    let line_starts = line_start_offsets(text);
    let mut embedded_regions = Vec::new();
    collect_markdown_embedded_regions_from_node(
        text,
        &line_starts,
        requested_bytes,
        root_node,
        &mut embedded_regions,
    );
    embedded_regions
}

#[cfg(feature = "tree-sitter-syntax")]
fn collect_markdown_embedded_regions_from_node(
    text: &str,
    line_starts: &[usize],
    requested_bytes: TextRangeBytes,
    node: Node<'_>,
    embedded_regions: &mut Vec<CoreEmbeddedRegion>,
) {
    if node.kind() == "fenced_code_block" {
        if let Some(region) =
            markdown_embedded_region_for_fenced_code_block(text, line_starts, requested_bytes, node)
        {
            embedded_regions.push(region);
        }
    }
    if node.kind() == "inline" {
        collect_markdown_linked_media_regions(
            text,
            line_starts,
            requested_bytes,
            node,
            embedded_regions,
        );
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_markdown_embedded_regions_from_node(
            text,
            line_starts,
            requested_bytes,
            child,
            embedded_regions,
        );
    }
}

#[cfg(feature = "tree-sitter-syntax")]
fn collect_markdown_linked_media_regions(
    text: &str,
    line_starts: &[usize],
    requested_bytes: TextRangeBytes,
    inline_node: Node<'_>,
    embedded_regions: &mut Vec<CoreEmbeddedRegion>,
) {
    if inline_node.start_byte() >= requested_bytes.end
        || inline_node.end_byte() <= requested_bytes.start
    {
        return;
    }

    #[cfg(feature = "tree-sitter-markdown")]
    {
        let Some(inline_tree) = parse_markdown_inline_tree(text, inline_node) else {
            return;
        };
        collect_markdown_linked_media_regions_from_inline_node(
            text,
            line_starts,
            requested_bytes,
            inline_tree.root_node(),
            embedded_regions,
        );
    }
}

#[cfg(all(feature = "tree-sitter-syntax", feature = "tree-sitter-markdown"))]
fn parse_markdown_inline_tree(text: &str, inline_node: Node<'_>) -> Option<tree_sitter::Tree> {
    let mut parser = Parser::new();
    parser
        .set_language(&markdown_inline_tree_sitter_language())
        .ok()?;
    parser.set_included_ranges(&[inline_node.range()]).ok()?;
    parser.parse(text, None)
}

#[cfg(all(feature = "tree-sitter-syntax", feature = "tree-sitter-markdown"))]
fn collect_markdown_linked_media_regions_from_inline_node(
    text: &str,
    line_starts: &[usize],
    requested_bytes: TextRangeBytes,
    node: Node<'_>,
    embedded_regions: &mut Vec<CoreEmbeddedRegion>,
) {
    if matches!(node.kind(), "image" | "inline_link") {
        if let Some(region) =
            markdown_embedded_region_for_linked_media(text, line_starts, requested_bytes, node)
        {
            embedded_regions.push(region);
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_markdown_linked_media_regions_from_inline_node(
            text,
            line_starts,
            requested_bytes,
            child,
            embedded_regions,
        );
    }
}

#[cfg(all(feature = "tree-sitter-syntax", feature = "tree-sitter-markdown"))]
fn markdown_embedded_region_for_linked_media(
    text: &str,
    line_starts: &[usize],
    requested_bytes: TextRangeBytes,
    node: Node<'_>,
) -> Option<CoreEmbeddedRegion> {
    let mut destination = None;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "link_destination" {
            destination = Some(child);
            break;
        }
    }

    let destination = destination?;
    let raw_target = text[destination.start_byte()..destination.end_byte()]
        .trim()
        .to_string();
    let (normalized_info_string, normalized_kind) =
        markdown_link_media_kind_from_target(&raw_target)?;

    if node.start_byte() >= requested_bytes.end || node.end_byte() <= requested_bytes.start {
        return None;
    }

    let range = text_range_from_bytes(line_starts, node.start_byte(), node.end_byte());
    let content_range = text_range_from_bytes(
        line_starts,
        destination.start_byte(),
        destination.end_byte(),
    );
    let resolved_language = Some(resolved_markdown_link_media(range, normalized_kind.clone()));

    Some(CoreEmbeddedRegion {
        range,
        content_range,
        source: CoreEmbeddedRegionSource::MarkdownLink,
        raw_info_string: Some(raw_target),
        normalized_info_string: Some(normalized_info_string),
        normalized_kind,
        resolved_language,
    })
}

#[cfg(feature = "tree-sitter-syntax")]
fn markdown_link_media_kind_from_target(target: &str) -> Option<(String, CoreEmbeddedBlockKind)> {
    let media_target = normalized_markdown_link_media_target(target)?;
    if media_target.ends_with(".drawio.svg") {
        return Some((
            "svg".to_string(),
            CoreEmbeddedBlockKind::Media {
                media_kind: CoreMediaKind::Svg,
                flavor: Some(CoreMediaFlavor::DrawioSvg),
            },
        ));
    }
    if media_target.ends_with(".svg") {
        return Some((
            "svg".to_string(),
            CoreEmbeddedBlockKind::Media {
                media_kind: CoreMediaKind::Svg,
                flavor: None,
            },
        ));
    }
    if media_target.ends_with(".png") {
        return Some((
            "png".to_string(),
            CoreEmbeddedBlockKind::Media {
                media_kind: CoreMediaKind::Png,
                flavor: None,
            },
        ));
    }
    None
}

#[cfg(feature = "tree-sitter-syntax")]
fn normalized_markdown_link_media_target(target: &str) -> Option<String> {
    let target = target.trim();
    if target.is_empty() {
        return None;
    }
    let target = target
        .strip_prefix('<')
        .and_then(|stripped| stripped.strip_suffix('>'))
        .unwrap_or(target);
    let media_path = target
        .split(['?', '#'])
        .next()
        .unwrap_or(target)
        .trim()
        .to_ascii_lowercase();
    if media_path.is_empty() {
        None
    } else {
        Some(media_path)
    }
}

#[cfg(feature = "tree-sitter-syntax")]
fn markdown_embedded_region_for_fenced_code_block(
    text: &str,
    line_starts: &[usize],
    requested_bytes: TextRangeBytes,
    node: Node<'_>,
) -> Option<CoreEmbeddedRegion> {
    if node.start_byte() < requested_bytes.start || node.end_byte() > requested_bytes.end {
        return None;
    }

    let range = text_range_from_bytes(line_starts, node.start_byte(), node.end_byte());
    let mut info_string = None;
    let mut content_range = None;

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "info_string" => {
                info_string = Some(text[child.start_byte()..child.end_byte()].to_string());
            }
            "code_fence_content" => {
                let content_end = markdown_fenced_content_end_excluding_closing_fence(
                    text,
                    line_starts,
                    node,
                    child,
                );
                content_range = Some(text_range_from_bytes(
                    line_starts,
                    child.start_byte(),
                    content_end,
                ));
            }
            _ => {}
        }
    }

    let normalized_info_string = normalize_markdown_info_string(info_string.as_deref());
    let resolved_language = Some(resolve_embedded_language(
        CoreEmbeddedLanguageResolutionRequest {
            range,
            raw_info_string: info_string.clone(),
        },
    ));
    let normalized_kind = resolved_language
        .as_ref()
        .map(|resolved| resolved.kind.clone())
        .unwrap_or(CoreEmbeddedBlockKind::Unknown);

    Some(CoreEmbeddedRegion {
        range,
        content_range: content_range.unwrap_or(CoreTextRange {
            start: range.end,
            end: range.end,
        }),
        source: CoreEmbeddedRegionSource::MarkdownFence,
        raw_info_string: info_string,
        normalized_info_string,
        normalized_kind,
        resolved_language,
    })
}

#[cfg(feature = "tree-sitter-syntax")]
fn markdown_fenced_content_end_excluding_closing_fence(
    text: &str,
    line_starts: &[usize],
    fenced_block: Node<'_>,
    content_node: Node<'_>,
) -> usize {
    let content_start = content_node.start_byte();
    let mut content_end = content_node.end_byte();
    if content_start >= content_end {
        return content_end;
    }

    let Some((marker, marker_len)) =
        markdown_fence_opening_marker(text, line_starts, fenced_block.start_byte())
    else {
        return content_end;
    };

    let last_content_byte = content_end.saturating_sub(1);
    let closing_line_start = line_starts
        .partition_point(|line_start| *line_start <= last_content_byte)
        .checked_sub(1)
        .and_then(|index| line_starts.get(index).copied())
        .unwrap_or(content_start);
    if closing_line_start < content_start {
        return content_end;
    }

    let candidate = &text[closing_line_start..content_end];
    if markdown_line_is_closing_fence(candidate, marker, marker_len) {
        content_end = closing_line_start;
    }
    content_end
}

#[cfg(feature = "tree-sitter-syntax")]
fn markdown_fence_opening_marker(
    text: &str,
    line_starts: &[usize],
    opening_byte: usize,
) -> Option<(u8, usize)> {
    let opening_row = position_for_byte(line_starts, opening_byte).row;
    let opening_line_end = line_end_offset(text, line_starts, opening_row);
    let opening_line = text[opening_byte..opening_line_end].trim_start();
    let marker = opening_line.as_bytes().first().copied()?;
    if !matches!(marker, b'`' | b'~') {
        return None;
    }
    let marker_len = opening_line
        .as_bytes()
        .iter()
        .take_while(|byte| **byte == marker)
        .count();
    if marker_len >= 3 {
        Some((marker, marker_len))
    } else {
        None
    }
}

#[cfg(feature = "tree-sitter-syntax")]
fn markdown_line_is_closing_fence(line: &str, marker: u8, marker_len: usize) -> bool {
    let trimmed = line.trim();
    let bytes = trimmed.as_bytes();
    let fence_len = bytes.iter().take_while(|byte| **byte == marker).count();
    fence_len >= marker_len
        && bytes
            .get(fence_len..)
            .is_some_and(|suffix| suffix.iter().all(|byte| byte.is_ascii_whitespace()))
}

/// メッセージイベントのペイロード
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreMessageEvent {
    pub severity: CoreMessageSeverity,
    pub category: CoreMessageCategory,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorePagerPromptKind {
    More,
    HitReturn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreEvent {
    Message(CoreMessageEvent),
    PagerPrompt(CorePagerPromptKind),
    Bell,
    Redraw { full: bool, clear_before_draw: bool },
    BufferAdded { buf_id: i32 },
    WindowCreated { win_id: i32 },
    LayoutChanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreCommandTransaction {
    pub outcome: CoreCommandOutcome,
    pub snapshot: CoreSnapshot,
    pub events: Vec<CoreEvent>,
    pub host_actions: Vec<CoreHostAction>,
}

/// 補完候補1件分の情報
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorePumItem {
    pub word: String,
    pub abbr: String,
    pub menu: String,
    pub kind: String,
    pub info: String,
}

/// ポップアップメニュー（補完候補メニュー）全体の情報
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorePumInfo {
    pub row: i32,
    pub col: i32,
    pub width: i32,
    pub height: i32,
    /// 現在選択されている候補のインデックス。未選択時は None。
    pub selected_index: Option<usize>,
    pub items: Vec<CorePumItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreMatchType {
    Regular,
    IncSearch,
    CurSearch,
}

impl From<bindings::vim_core_match_type_t> for CoreMatchType {
    fn from(t: bindings::vim_core_match_type_t) -> Self {
        match t {
            bindings::vim_core_match_type_t_VIM_CORE_MATCH_REGULAR => CoreMatchType::Regular,
            bindings::vim_core_match_type_t_VIM_CORE_MATCH_INCSEARCH => CoreMatchType::IncSearch,
            bindings::vim_core_match_type_t_VIM_CORE_MATCH_CURSEARCH => CoreMatchType::CurSearch,
            _ => CoreMatchType::Regular,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreMatchRange {
    pub start_row: usize,
    pub start_col: usize,
    pub end_row: usize,
    pub end_col: usize,
    pub match_type: CoreMatchType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchCountResult {
    Calculated(usize),
    MaxReached(usize),
    TimedOut,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreCursorMatchInfo {
    pub is_on_match: bool,
    pub current_match_index: usize,
    pub total_matches: MatchCountResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreSearchDirection {
    Forward,
    Backward,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreSearchHighlightMode {
    Disabled,
    HlSearch,
    IncSearch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreSearchCapabilityContract {
    pub live_state_query_available: bool,
    pub inactive_window_query_available: bool,
    pub visible_rows_only: bool,
    pub start_col_inclusive: bool,
    pub end_col_exclusive: bool,
    pub byte_columns: bool,
    pub data_only_payload: bool,
    pub host_owned_presentation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// ポップアップメニュー情報。補完メニューが表示中の場合に Some。
    pub pum: Option<CorePumInfo>,
}

impl CoreSnapshot {
    pub fn active_window(&self) -> Option<&CoreWindowInfo> {
        self.windows.iter().find(|window| window.is_active)
    }

    pub fn active_window_id(&self) -> Option<i32> {
        self.active_window().map(|window| window.id)
    }

    pub fn window(&self, window_id: i32) -> Option<&CoreWindowInfo> {
        self.windows.iter().find(|window| window.id == window_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreLightSnapshot {
    pub revision: u64,
    pub dirty: bool,
    pub mode: CoreMode,
    pub pending_input: CorePendingInput,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub pending_host_actions: usize,
    pub buffers: Vec<CoreBufferInfo>,
    pub windows: Vec<CoreWindowInfo>,
    /// ポップアップメニュー情報。補完メニューが表示中の場合に Some。
    pub pum: Option<CorePumInfo>,
}

impl CoreLightSnapshot {
    pub fn active_window(&self) -> Option<&CoreWindowInfo> {
        self.windows.iter().find(|window| window.is_active)
    }

    pub fn active_window_id(&self) -> Option<i32> {
        self.active_window().map(|window| window.id)
    }

    pub fn window(&self, window_id: i32) -> Option<&CoreWindowInfo> {
        self.windows.iter().find(|window| window.id == window_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreBufferLineRange {
    pub buffer_id: i32,
    pub source_revision: CoreBufferRevision,
    pub start_row: usize,
    pub line_count: usize,
    pub total_line_count: usize,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreVisualSelection {
    pub mode: CoreMode,
    pub start_row: usize,
    pub start_col: usize,
    pub end_row: usize,
    pub end_col: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreCommandError {
    InvalidInput,
    OperationFailed { reason_code: u32 },
    UnknownStatus { status: u32, reason_code: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreSearchQueryError {
    NoActiveWindow,
    InvalidViewport { start_row: i32, end_row: i32 },
    WindowNotFound { window_id: i32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreSessionError {
    SessionAlreadyActive,
    InitializationFailed { reason_code: &'static str },
    CommandFailed(CoreCommandError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreInputResponseError {
    NoPendingInput,
    CorrelationMismatch { expected: u64, actual: u64 },
    Command(CoreCommandError),
    EvalFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CoreSessionOptions {
    /// Runtime mode contract for this session. `Embedded` is the primary mode.
    pub runtime_mode: CoreRuntimeMode,
    /// `None` のとき debug log は無効。指定時のみファイルへ追記する。
    pub debug_log_path: Option<PathBuf>,
}

pub struct VimCoreSession {
    state: NonNull<bindings::vim_bridge_state_t>,
    runtime_mode: CoreRuntimeMode,
    document_coordinator: RefCell<DocumentCoordinator>,
    pending_input_state: RefCell<CorePendingInput>,
    page_edge_line_scroll: RefCell<Option<PageEdgeLineScroll>>,
    active_input_request: RefCell<Option<CoreActiveInputRequest>>,
    completed_input_eval_result: RefCell<Option<String>>,
    pending_host_actions: RefCell<VecDeque<CoreHostAction>>,
    pending_events: RefCell<VecDeque<CoreEvent>>,
    #[cfg(feature = "tree-sitter-syntax")]
    next_tree_sitter_request_id: RefCell<u64>,
    #[cfg(feature = "tree-sitter-syntax")]
    tree_sitter_snapshots: RefCell<TreeSitterSnapshotStore>,
    #[cfg(feature = "tree-sitter-syntax")]
    completed_tree_sitter_preparations: RefCell<VecDeque<CoreTreeSitterPreparationResult>>,
    #[cfg(feature = "tree-sitter-syntax")]
    committed_tree_sitter_syntax:
        RefCell<BTreeMap<TreeSitterSyntaxCacheKey, CoreTreeSitterRangeSyntax>>,
    not_send_sync: PhantomData<Rc<()>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PageEdgeLineScroll {
    UpAfterLastPage,
    DownAfterFirstPage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CoreActiveInputRequest {
    prompt: String,
    input_kind: CoreInputRequestKind,
    correlation_id: u64,
    pending_eval_expr: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParsedExIntent {
    Edit { locator: String },
    Write { path: String, force: bool },
    Update { path: String, force: bool },
    SaveAndClose { force: bool, path: String },
    SaveIfDirtyAndClose { path: String },
    Quit { force: bool },
    Suspend,
}

impl VimCoreSession {
    #[cfg(feature = "tree-sitter-syntax")]
    pub fn tree_sitter_language_packages() -> Vec<CoreTreeSitterLanguagePackage> {
        tree_sitter_language_packages()
            .iter()
            .map(|package| CoreTreeSitterLanguagePackage {
                language_id: package.language_id.to_string(),
                package_id: package.package_id.to_string(),
                package_version: package.package_version.to_string(),
                parser_version: package.parser_version.to_string(),
                query_version: package.query_version.to_string(),
            })
            .collect()
    }

    #[cfg(feature = "tree-sitter-syntax")]
    pub fn resolve_tree_sitter_root_language(
        request: CoreRootLanguageResolutionRequest,
    ) -> CoreResolvedLanguage {
        resolve_root_language(request)
    }

    #[cfg(feature = "tree-sitter-syntax")]
    pub fn resolve_tree_sitter_embedded_language(
        request: CoreEmbeddedLanguageResolutionRequest,
    ) -> CoreResolvedLanguage {
        resolve_embedded_language(request)
    }

    #[cfg(feature = "tree-sitter-syntax")]
    pub fn request_tree_sitter_syntax_preparation(
        &mut self,
        mut request: CoreTreeSitterPreparationRequest,
    ) -> Result<CoreTreeSitterPreparation, CoreCommandError> {
        let request_id = {
            let mut next = self.next_tree_sitter_request_id.borrow_mut();
            let request_id = CoreTreeSitterRequestId { value: *next };
            *next = next.saturating_add(1);
            request_id
        };

        let snapshot = self.snapshot();
        let Some(buffer) = snapshot
            .buffers
            .iter()
            .find(|buffer| buffer.id == request.buffer_id)
        else {
            return Err(CoreCommandError::InvalidInput);
        };
        let source_revision = request.source_revision.unwrap_or(buffer.source_revision);
        if request.buffer_name.is_none() && !buffer.name.is_empty() {
            request.buffer_name = Some(buffer.name.clone());
        }

        let snapshot_status = self.pin_tree_sitter_text_snapshot(
            request.buffer_id,
            source_revision,
            buffer.source_revision,
            &request.snapshot_policy,
        );
        let status = snapshot_status.unwrap_or_else(|| {
            let range = request.range;
            let resolved = resolve_root_language(CoreRootLanguageResolutionRequest {
                range,
                vim_filetype: request.vim_filetype.clone(),
                buffer_name: request.buffer_name.clone(),
                host_language_hint: request.host_language_hint.clone(),
            });
            tree_sitter_status_from_resolution(&resolved)
        });

        let syntax = self.tree_sitter_syntax_result_for_request(
            request.buffer_id,
            source_revision,
            request.range,
            status.clone(),
            &request,
        );

        if !matches!(status, CoreTreeSitterStatus::Stale) {
            self.committed_tree_sitter_syntax.borrow_mut().insert(
                tree_sitter_syntax_cache_key(&syntax, request.range),
                syntax.clone(),
            );
        }
        self.completed_tree_sitter_preparations
            .borrow_mut()
            .push_back(CoreTreeSitterPreparationResult { request_id, syntax });

        if !matches!(
            status,
            CoreTreeSitterStatus::Stale
                | CoreTreeSitterStatus::TooLarge
                | CoreTreeSitterStatus::BudgetExceeded
        ) {
            let mut store = self.tree_sitter_snapshots.borrow_mut();
            store.unpin(request.buffer_id, source_revision);
            let _ = store.evict_unpinned(&request.snapshot_policy);
        }

        Ok(CoreTreeSitterPreparation {
            request_id,
            buffer_id: request.buffer_id,
            source_revision,
            status,
        })
    }

    #[cfg(feature = "tree-sitter-syntax")]
    pub fn poll_tree_sitter_preparation(&mut self) -> Option<CoreTreeSitterPreparationResult> {
        self.completed_tree_sitter_preparations
            .borrow_mut()
            .pop_front()
    }

    #[cfg(feature = "tree-sitter-syntax")]
    pub fn query_tree_sitter_syntax_range(
        &self,
        buffer_id: i32,
        source_revision: CoreBufferRevision,
        range: CoreTextRange,
    ) -> Option<CoreTreeSitterRangeSyntax> {
        let cache = self.committed_tree_sitter_syntax.borrow();
        if let Some((_, exact)) = cache.iter().find(|(key, _)| {
            key.buffer_id == buffer_id
                && key.source_revision == source_revision
                && key.range == range
        }) {
            return Some(exact.clone());
        }
        cache
            .iter()
            .filter(|(key, _)| {
                key.buffer_id == buffer_id
                    && key.source_revision == source_revision
                    && key.range.start <= range.start
                    && key.range.end >= range.end
            })
            .min_by_key(|(key, _)| (key.range.start, key.range.end))
            .map(|(_, syntax)| clip_tree_sitter_syntax_to_range(syntax, range))
    }

    #[cfg(feature = "tree-sitter-syntax")]
    pub fn tree_sitter_snapshot_store_stats(&self) -> CoreTreeSitterSnapshotStoreStats {
        self.tree_sitter_snapshots.borrow().stats()
    }

    pub fn new(initial_text: &str) -> Result<Self, CoreSessionError> {
        Self::new_with_options(initial_text, CoreSessionOptions::default())
    }

    pub fn new_with_options(
        initial_text: &str,
        options: CoreSessionOptions,
    ) -> Result<Self, CoreSessionError> {
        if !matches!(options.runtime_mode, CoreRuntimeMode::Embedded) {
            return Err(CoreSessionError::InitializationFailed {
                reason_code: "unsupported_runtime_mode",
            });
        }
        if ACTIVE_SESSION
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(CoreSessionError::SessionAlreadyActive);
        }

        let log_config = debug_log::DebugLogConfig {
            path: options.debug_log_path,
        };
        if debug_log::configure(&log_config).is_err() {
            ACTIVE_SESSION.store(false, Ordering::Release);
            return Err(CoreSessionError::InitializationFailed {
                reason_code: "debug_log_init_failed",
            });
        }
        let native_log_path = log_config
            .path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned());
        match native_log_path.as_deref() {
            Some(path) => unsafe {
                bindings::vim_bridge_set_debug_log_path(path.as_ptr().cast(), path.len())
            },
            None => unsafe { bindings::vim_bridge_set_debug_log_path(std::ptr::null(), 0) },
        }

        let state_ptr = unsafe {
            bindings::vim_bridge_state_new(initial_text.as_ptr().cast(), initial_text.len())
        };

        let Some(state) = NonNull::new(state_ptr) else {
            ACTIVE_SESSION.store(false, Ordering::Release);
            return Err(CoreSessionError::InitializationFailed {
                reason_code: "state_new_returned_null",
            });
        };

        let mut session = Self {
            state,
            runtime_mode: options.runtime_mode,
            document_coordinator: RefCell::new(DocumentCoordinator::new()),
            pending_input_state: RefCell::new(CorePendingInput::none()),
            page_edge_line_scroll: RefCell::new(None),
            active_input_request: RefCell::new(None),
            completed_input_eval_result: RefCell::new(None),
            pending_host_actions: RefCell::new(VecDeque::new()),
            pending_events: RefCell::new(VecDeque::new()),
            #[cfg(feature = "tree-sitter-syntax")]
            next_tree_sitter_request_id: RefCell::new(1),
            #[cfg(feature = "tree-sitter-syntax")]
            tree_sitter_snapshots: RefCell::new(TreeSitterSnapshotStore::default()),
            #[cfg(feature = "tree-sitter-syntax")]
            completed_tree_sitter_preparations: RefCell::new(VecDeque::new()),
            #[cfg(feature = "tree-sitter-syntax")]
            committed_tree_sitter_syntax: RefCell::new(BTreeMap::new()),
            not_send_sync: PhantomData,
        };

        session.maybe_apply_xdg_runtimepath();

        // /* println debug removed */
        Ok(session)
    }

    fn maybe_apply_xdg_runtimepath(&mut self) {
        let Some(xdg_config_home) = std::env::var_os("XDG_CONFIG_HOME") else {
            return;
        };

        let xdg_vim_dir = PathBuf::from(xdg_config_home).join("vim");
        if !xdg_vim_dir.join("vimrc").is_file() {
            return;
        }

        let xdg_vim_dir = xdg_vim_dir.to_string_lossy().into_owned();
        let xdg_literal = format!("'{}'", xdg_vim_dir.replace('\'', "''"));
        let command =
            format!("let &rtp = {xdg_literal} . ',' . &rtp | let &pp = {xdg_literal} . ',' . &pp");
        let _ = self.execute_ex_command(&command);
    }

    #[cfg(feature = "tree-sitter-syntax")]
    fn pin_tree_sitter_text_snapshot(
        &self,
        buffer_id: i32,
        source_revision: CoreBufferRevision,
        current_source_revision: CoreBufferRevision,
        policy: &CoreTreeSitterSnapshotPolicy,
    ) -> Option<CoreTreeSitterStatus> {
        if source_revision != current_source_revision {
            if self
                .tree_sitter_snapshots
                .borrow_mut()
                .pin_existing(buffer_id, source_revision)
            {
                return None;
            }
            return Some(CoreTreeSitterStatus::Stale);
        }

        let Some(text) = self.buffer_text(buffer_id) else {
            return Some(CoreTreeSitterStatus::Unsupported);
        };
        self.tree_sitter_snapshots
            .borrow_mut()
            .pin_or_insert(buffer_id, source_revision, text, policy)
            .err()
    }

    #[cfg(feature = "tree-sitter-syntax")]
    fn tree_sitter_syntax_result_for_request(
        &self,
        buffer_id: i32,
        source_revision: CoreBufferRevision,
        range: CoreTextRange,
        status: CoreTreeSitterStatus,
        request: &CoreTreeSitterPreparationRequest,
    ) -> CoreTreeSitterRangeSyntax {
        let resolved = resolve_root_language(CoreRootLanguageResolutionRequest {
            range,
            vim_filetype: request.vim_filetype.clone(),
            buffer_name: request.buffer_name.clone(),
            host_language_hint: request.host_language_hint.clone(),
        });
        let mut status = status;
        let mut has_error = false;
        let mut covered_ranges = Vec::new();
        let mut error_ranges = Vec::new();
        let mut budget_status = match status {
            CoreTreeSitterStatus::TooLarge => CoreTreeSitterBudgetStatus::SnapshotTooLarge,
            CoreTreeSitterStatus::BudgetExceeded => {
                CoreTreeSitterBudgetStatus::GlobalBudgetExceeded
            }
            _ => CoreTreeSitterBudgetStatus::WithinBudget,
        };
        let mut chunks = Vec::new();
        let mut embedded_regions = Vec::new();
        if matches!(status, CoreTreeSitterStatus::Prepared) {
            if let Some(text) = self
                .tree_sitter_snapshots
                .borrow()
                .text(buffer_id, source_revision)
            {
                let (
                    parse_status,
                    parse_has_error,
                    parse_covered_ranges,
                    parse_error_ranges,
                    parse_budget_status,
                    parse_chunks,
                    parse_embedded_regions,
                ) = parse_tree_sitter_syntax(text, range, &resolved);
                status = parse_status;
                has_error = parse_has_error;
                covered_ranges = parse_covered_ranges;
                error_ranges = parse_error_ranges;
                budget_status = parse_budget_status;
                chunks = parse_chunks;
                embedded_regions = parse_embedded_regions;
            } else {
                status = CoreTreeSitterStatus::Stale;
            }
        }
        CoreTreeSitterRangeSyntax {
            buffer_id,
            source_revision,
            provenance: provenance_for_resolved_language(&resolved),
            status,
            has_error,
            covered_ranges,
            error_ranges,
            budget_status,
            chunks,
            embedded_regions,
        }
    }

    pub fn snapshot(&self) -> CoreSnapshot {
        let started_at = Instant::now();
        let snapshot = unsafe { bindings::vim_bridge_snapshot(self.state.as_ptr()) };
        let ffi_ms = started_at.elapsed().as_millis();
        let convert_started_at = Instant::now();
        let mut snapshot = convert_snapshot(snapshot);
        let convert_ms = convert_started_at.elapsed().as_millis();
        self.project_buffer_metadata(&mut snapshot.buffers);

        snapshot.pending_host_actions += self.pending_host_actions.borrow().len();
        snapshot.pending_input = self.pending_input();

        debug_log!(
            "[PERF][vim_core_rs] snapshot text_len={} buffers={} windows={} ffi_ms={} convert_ms={} total_ms={}",
            snapshot.text.len(),
            snapshot.buffers.len(),
            snapshot.windows.len(),
            ffi_ms,
            convert_ms,
            started_at.elapsed().as_millis()
        );

        snapshot
    }

    pub fn light_snapshot(&self) -> CoreLightSnapshot {
        let started_at = Instant::now();
        let snapshot = unsafe { bindings::vim_bridge_light_snapshot(self.state.as_ptr()) };
        let ffi_ms = started_at.elapsed().as_millis();
        let convert_started_at = Instant::now();
        let mut snapshot = convert_light_snapshot(snapshot);
        let convert_ms = convert_started_at.elapsed().as_millis();

        self.project_buffer_metadata(&mut snapshot.buffers);
        snapshot.pending_host_actions += self.pending_host_actions.borrow().len();
        snapshot.pending_input = self.pending_input();

        debug_log!(
            "[PERF][vim_core_rs] light_snapshot buffers={} windows={} ffi_ms={} convert_ms={} total_ms={}",
            snapshot.buffers.len(),
            snapshot.windows.len(),
            ffi_ms,
            convert_ms,
            started_at.elapsed().as_millis()
        );

        snapshot
    }

    fn project_buffer_metadata(&self, buffers: &mut [CoreBufferInfo]) {
        let buffer_seeds = buffers
            .iter()
            .map(|buffer| (buffer.id, buffer.name.clone()))
            .collect::<Vec<_>>();

        let mut coordinator = self.document_coordinator.borrow_mut();
        coordinator.sync_buffers(&buffer_seeds);
        for buffer in buffers {
            if let Some(binding) = coordinator.binding(buffer.id) {
                buffer.source_kind = binding.source_kind;
                buffer.document_id = binding.document_id.clone();
                buffer.pending_vfs_operation = binding.pending_operation;
                buffer.deferred_close = binding.deferred_close;
                buffer.last_vfs_error = binding.last_vfs_error.clone();
            }
        }
    }

    pub fn mode(&self) -> CoreMode {
        self.light_snapshot().mode
    }

    pub fn runtime_mode(&self) -> CoreRuntimeMode {
        self.runtime_mode
    }

    pub fn pending_input(&self) -> CorePendingInput {
        self.pending_input_state.borrow().clone()
    }

    fn ctrl_b_workaround_command(&self) -> Option<String> {
        let snapshot = self.snapshot();
        let page_height = snapshot
            .windows
            .iter()
            .find(|window| window.is_active)
            .or_else(|| snapshot.windows.first())
            .map(|window| window.height.max(1))
            .unwrap_or(1);
        if page_height == 0 {
            return None;
        }

        Some("k".repeat(page_height))
    }

    pub fn mark(&self, mark_name: char) -> Option<CoreMarkPosition> {
        let mark_name_c = mark_name as std::os::raw::c_char;
        let mut mark = unsafe { std::mem::zeroed::<bindings::vim_core_mark_position_t>() };
        let is_set =
            unsafe { bindings::vim_bridge_get_mark(self.state.as_ptr(), mark_name_c, &mut mark) };
        if !is_set || !mark.is_set {
            return None;
        }

        Some(convert_mark_position(mark))
    }

    pub fn current_visual_selection(&mut self) -> Option<CoreVisualSelection> {
        let snapshot = self.snapshot();
        if !core_mode_is_visual(snapshot.mode) {
            return None;
        }

        let current = (snapshot.cursor_row, snapshot.cursor_col);
        self.execute_normal_command("o").ok()?;
        let swapped = self.snapshot();
        let anchor = (swapped.cursor_row, swapped.cursor_col);
        self.execute_normal_command("o").ok()?;

        let ((start_row, start_col), (end_row, end_col)) =
            normalize_visual_selection_bounds(anchor, current);
        Some(CoreVisualSelection {
            mode: snapshot.mode,
            start_row,
            start_col,
            end_row,
            end_col,
        })
    }

    pub fn set_mark(
        &mut self,
        mark_name: char,
        buf_id: i32,
        row: usize,
        col: usize,
    ) -> Result<(), CoreCommandError> {
        let mark_name_c = mark_name as std::os::raw::c_char;
        let status = unsafe {
            bindings::vim_bridge_set_mark(self.state.as_ptr(), mark_name_c, buf_id, row, col)
        };
        convert_status(status)
    }

    pub fn jumplist(&self) -> CoreJumpList {
        let jumplist = unsafe { bindings::vim_bridge_get_jumplist(self.state.as_ptr()) };
        convert_jumplist(jumplist)
    }

    pub fn inject_vfd_data(&mut self, vfd: i32, data: &[u8]) -> Result<(), CoreCommandError> {
        let mut mgr = crate::vfd::get_manager();
        if mgr.inject_data(vfd, data) {
            Ok(())
        } else {
            Err(CoreCommandError::InvalidInput)
        }
    }

    pub fn notify_job_status(
        &mut self,
        job_id: i32,
        status: JobStatus,
        exit_code: i32,
    ) -> Result<(), CoreCommandError> {
        let mut mgr = crate::vfd::get_manager();
        if mgr.update_job_status(job_id, status as i32, exit_code) {
            Ok(())
        } else {
            Err(CoreCommandError::InvalidInput)
        }
    }

    pub fn submit_vfs_response(
        &mut self,
        response: CoreVfsResponse,
    ) -> Result<CoreCommandOutcome, CoreCommandError> {
        let request_id = response.request_id();
        let mut coordinator = self.document_coordinator.borrow_mut();
        let Some(entry) = coordinator.request_entry(request_id) else {
            debug_log!(
                "[DEBUG] submit_vfs_response: unknown request_id={} response={:?}",
                request_id,
                response
            );
            return Err(CoreCommandError::InvalidInput);
        };

        debug_log!(
            "[DEBUG] submit_vfs_response: request_id={} buf_id={} operation={:?} response={:?}",
            request_id,
            entry.target_buf_id,
            entry.operation_kind,
            response
        );

        match response.clone() {
            CoreVfsResponse::Resolved { document_id, .. } => {
                let apply_outcome = coordinator.apply_response(response);
                if !matches!(apply_outcome, vfs::CoreResponseApplyOutcome::Applied) {
                    return Err(CoreCommandError::OperationFailed { reason_code: 0 });
                }
                let request = coordinator.issue_load(entry.target_buf_id, document_id);
                drop(coordinator);
                self.pending_host_actions
                    .borrow_mut()
                    .push_back(CoreHostAction::VfsRequest(request));
                Ok(CoreCommandOutcome::HostActionQueued)
            }
            CoreVfsResponse::ResolvedLocalFallback { locator, .. } => {
                let apply_outcome = coordinator.apply_response(response);
                if !matches!(apply_outcome, vfs::CoreResponseApplyOutcome::Applied) {
                    return Err(CoreCommandError::OperationFailed { reason_code: 0 });
                }
                drop(coordinator);
                let command = format!(":edit {}", locator);
                let outcome = self.execute_native_ex_command_for_outcome(&command)?;
                let snapshot = self.snapshot();
                let revision = snapshot.revision;
                let active_buffer = snapshot
                    .buffers
                    .iter()
                    .find(|buffer| buffer.is_active)
                    .ok_or(CoreCommandError::OperationFailed { reason_code: 0 })?;
                self.document_coordinator.borrow_mut().bind_local_buffer(
                    active_buffer.id,
                    Some(locator),
                    active_buffer.name.clone(),
                    revision,
                );
                Ok(outcome)
            }
            CoreVfsResponse::Loaded {
                document_id, text, ..
            } => {
                let apply_outcome = coordinator.apply_response(response.clone());
                if !matches!(apply_outcome, vfs::CoreResponseApplyOutcome::Applied) {
                    return Err(CoreCommandError::OperationFailed { reason_code: 0 });
                }
                let display_name = coordinator
                    .binding(entry.target_buf_id)
                    .map(|binding| binding.display_name.clone())
                    .unwrap_or_else(|| document_id.clone());
                drop(coordinator);

                self.apply_loaded_buffer(entry.target_buf_id, &display_name, &text)?;

                let snapshot = self.snapshot();
                self.document_coordinator
                    .borrow_mut()
                    .commit_loaded_revision(entry.target_buf_id, snapshot.revision);

                Ok(CoreCommandOutcome::BufferChanged {
                    revision: snapshot.revision,
                })
            }
            CoreVfsResponse::Saved { document_id, .. } => {
                let target_buf_id = entry.target_buf_id;
                let apply_outcome = coordinator.apply_response(response.clone());

                match apply_outcome {
                    vfs::CoreResponseApplyOutcome::Applied => {
                        debug_log!(
                            "[DEBUG] submit_vfs_response: save success applied buf_id={} document_id={}",
                            target_buf_id,
                            document_id
                        );
                        // dirty フラグをクリア
                        drop(coordinator);
                        let status = unsafe {
                            bindings::vim_bridge_set_buffer_dirty(
                                self.state.as_ptr(),
                                target_buf_id,
                                false,
                            )
                        };
                        convert_status(status)?;

                        // deferred close をチェック
                        let deferred = self
                            .document_coordinator
                            .borrow()
                            .deferred_close(target_buf_id);
                        if let Some(close_kind) = deferred {
                            debug_log!(
                                "[DEBUG] submit_vfs_response: deferred close triggered buf_id={} kind={:?}",
                                target_buf_id,
                                close_kind
                            );
                            self.document_coordinator
                                .borrow_mut()
                                .clear_deferred_close(target_buf_id, "save applied");
                            let snapshot = self.snapshot();
                            self.pending_host_actions.borrow_mut().push_back(
                                CoreHostAction::Quit {
                                    force: false,
                                    issued_after_revision: snapshot.revision,
                                },
                            );
                        }

                        Ok(CoreCommandOutcome::NoChange)
                    }
                    vfs::CoreResponseApplyOutcome::StaleRejected => {
                        debug_log!(
                            "[DEBUG] submit_vfs_response: save stale rejected buf_id={} document_id={}",
                            target_buf_id,
                            document_id
                        );
                        // revision mismatch の場合、deferred close もクリアして拒否
                        let deferred = coordinator.deferred_close(target_buf_id);
                        if deferred.is_some() {
                            coordinator
                                .clear_deferred_close(target_buf_id, "save rejected as stale");
                            debug_log!(
                                "[DEBUG] submit_vfs_response: deferred close cleared due to stale save buf_id={}",
                                target_buf_id
                            );
                        }
                        drop(coordinator);
                        Ok(CoreCommandOutcome::NoChange)
                    }
                    _ => {
                        drop(coordinator);
                        Ok(CoreCommandOutcome::NoChange)
                    }
                }
            }
            CoreVfsResponse::Failed { .. }
            | CoreVfsResponse::Cancelled { .. }
            | CoreVfsResponse::TimedOut { .. } => {
                let target_buf_id = entry.target_buf_id;
                let apply_outcome = coordinator.apply_response(response);

                // save failure, cancel, timeout の場合は deferred close をクリア
                let deferred = coordinator.deferred_close(target_buf_id);
                if deferred.is_some() {
                    coordinator.clear_deferred_close(target_buf_id, "save failed or interrupted");
                    debug_log!(
                        "[DEBUG] submit_vfs_response: deferred close cleared due to save failure/cancel/timeout buf_id={}",
                        target_buf_id
                    );
                }
                drop(coordinator);

                if matches!(apply_outcome, vfs::CoreResponseApplyOutcome::UnknownRequest) {
                    return Err(CoreCommandError::InvalidInput);
                }
                Ok(CoreCommandOutcome::NoChange)
            }
            _ => {
                let apply_outcome = coordinator.apply_response(response);
                if matches!(apply_outcome, vfs::CoreResponseApplyOutcome::UnknownRequest) {
                    return Err(CoreCommandError::InvalidInput);
                }
                Ok(CoreCommandOutcome::NoChange)
            }
        }
    }

    pub fn submit_input_response(
        &mut self,
        response: CoreInputResponse,
    ) -> Result<CoreCommandTransaction, CoreInputResponseError> {
        let actual = response.correlation_id();
        let Some(active) = self.active_input_request.borrow().clone() else {
            debug_log!(
                "[DEBUG] submit_input_response: no active request response={:?}",
                response
            );
            return Err(CoreInputResponseError::NoPendingInput);
        };

        if active.correlation_id != actual {
            debug_log!(
                "[DEBUG] submit_input_response: correlation mismatch expected={} actual={} prompt={:?} input_kind={:?}",
                active.correlation_id,
                actual,
                active.prompt,
                active.input_kind
            );
            return Err(CoreInputResponseError::CorrelationMismatch {
                expected: active.correlation_id,
                actual,
            });
        }

        match &response {
            CoreInputResponse::Submitted { value, .. } => {
                debug_log!(
                    "[DEBUG] submit_input_response: accepted submit correlation_id={} value_len={} prompt={:?} input_kind={:?} pending_eval={}",
                    actual,
                    value.len(),
                    active.prompt,
                    active.input_kind,
                    active.pending_eval_expr.is_some()
                );
            }
            CoreInputResponse::Cancelled { .. } => {
                debug_log!(
                    "[DEBUG] submit_input_response: accepted cancel correlation_id={} prompt={:?} input_kind={:?} pending_eval={}",
                    actual,
                    active.prompt,
                    active.input_kind,
                    active.pending_eval_expr.is_some()
                );
            }
        }

        self.active_input_request.borrow_mut().take();
        if let Some(expr) = active.pending_eval_expr {
            self.submit_native_input_response(&response);
            let value = self.eval_string_after_input_response(&expr)?;
            debug_log!(
                "[DEBUG] submit_input_response: completed eval continuation correlation_id={} input_kind={:?} result_len={}",
                actual,
                active.input_kind,
                value.len()
            );
            *self.completed_input_eval_result.borrow_mut() = Some(value);
        }
        Ok(CoreCommandTransaction {
            outcome: CoreCommandOutcome::NoChange,
            snapshot: self.snapshot(),
            events: Vec::new(),
            host_actions: Vec::new(),
        })
    }

    /// Returns the completed `eval_string()` result produced after an input response.
    ///
    /// `eval_string()` returns `None` when an embedded Vimscript prompt needs
    /// host input. After the matching `submit_input_response()` call resumes
    /// that evaluation, the completed string result can be taken here.
    pub fn take_completed_input_eval_result(&mut self) -> Option<String> {
        self.completed_input_eval_result.borrow_mut().take()
    }

    fn execute_native_ex_command_for_outcome(
        &mut self,
        command: &str,
    ) -> Result<CoreCommandOutcome, CoreCommandError> {
        let (outcome, _) = self.invoke_native_ex_command(command)?;
        self.drain_native_host_actions();
        Ok(normalize_outcome_after_host_action_drain(
            outcome,
            &self.pending_host_actions.borrow(),
        ))
    }

    fn apply_intent(
        &mut self,
        intent: ParsedExIntent,
    ) -> Result<CoreCommandOutcome, CoreCommandError> {
        match intent {
            ParsedExIntent::Edit { locator } => {
                let target_buf_id = self
                    .snapshot()
                    .buffers
                    .iter()
                    .find(|buffer| buffer.is_active)
                    .map(|buffer| buffer.id)
                    .ok_or(CoreCommandError::OperationFailed { reason_code: 0 })?;
                let request = self
                    .document_coordinator
                    .borrow_mut()
                    .issue_resolve(target_buf_id, locator);
                self.pending_host_actions
                    .borrow_mut()
                    .push_back(CoreHostAction::VfsRequest(request));
                Ok(CoreCommandOutcome::HostActionQueued)
            }
            ParsedExIntent::Write { path, force } => self.apply_write_intent(path, force, None),
            ParsedExIntent::Update { path, force } => {
                let snapshot = self.snapshot();
                let active_buf = snapshot
                    .buffers
                    .iter()
                    .find(|b| b.is_active)
                    .ok_or(CoreCommandError::OperationFailed { reason_code: 0 })?;
                let buf_id = active_buf.id;

                let is_vfs = self.document_coordinator.borrow().is_vfs_buffer(buf_id);

                // VFS buffer の場合: :update は dirty な場合のみ save
                if is_vfs && !snapshot.dirty {
                    debug_log!(
                        "[DEBUG] apply_intent: :update on clean VFS buffer buf_id={}, skipping save",
                        buf_id
                    );
                    return Ok(CoreCommandOutcome::NoChange);
                }

                self.apply_write_intent(path, force, None)
            }
            ParsedExIntent::SaveAndClose { force, path } => {
                let snapshot = self.snapshot();
                let active_buf = snapshot
                    .buffers
                    .iter()
                    .find(|b| b.is_active)
                    .ok_or(CoreCommandError::OperationFailed { reason_code: 0 })?;
                let buf_id = active_buf.id;

                let is_vfs = self.document_coordinator.borrow().is_vfs_buffer(buf_id);
                if is_vfs {
                    debug_log!(
                        "[DEBUG] apply_intent: :wq on VFS buffer buf_id={}, initiating save-and-close",
                        buf_id
                    );
                    self.document_coordinator
                        .borrow_mut()
                        .set_deferred_close(buf_id, CoreDeferredClose::SaveAndClose);
                    self.apply_write_intent(path, force, None)
                } else {
                    // local buffer: :wq は Write → Quit の順でキューする（ホストが save-before-quit を調整）
                    let revision = snapshot.revision;
                    debug_log!(
                        "[DEBUG] apply_intent: :wq on local buffer buf_id={}, queuing Write then Quit (revision={})",
                        buf_id,
                        revision
                    );
                    let mut actions = self.pending_host_actions.borrow_mut();
                    actions.push_back(CoreHostAction::Write {
                        path: String::new(),
                        force,
                        issued_after_revision: revision,
                    });
                    actions.push_back(CoreHostAction::Quit {
                        force,
                        issued_after_revision: revision,
                    });
                    Ok(CoreCommandOutcome::HostActionQueued)
                }
            }
            ParsedExIntent::SaveIfDirtyAndClose { path } => {
                let snapshot = self.snapshot();
                let active_buf = snapshot
                    .buffers
                    .iter()
                    .find(|b| b.is_active)
                    .ok_or(CoreCommandError::OperationFailed { reason_code: 0 })?;
                let buf_id = active_buf.id;

                let is_vfs = self.document_coordinator.borrow().is_vfs_buffer(buf_id);
                if is_vfs && snapshot.dirty {
                    debug_log!(
                        "[DEBUG] apply_intent: :xit on dirty VFS buffer buf_id={}, initiating save-if-dirty-and-close",
                        buf_id
                    );
                    self.document_coordinator
                        .borrow_mut()
                        .set_deferred_close(buf_id, CoreDeferredClose::SaveIfDirtyAndClose);
                    self.apply_write_intent(path, false, None)
                } else if is_vfs {
                    // clean VFS buffer -> そのまま close
                    let revision = snapshot.revision;
                    self.pending_host_actions
                        .borrow_mut()
                        .push_back(CoreHostAction::Quit {
                            force: false,
                            issued_after_revision: revision,
                        });
                    Ok(CoreCommandOutcome::HostActionQueued)
                } else {
                    // local buffer: :xit は dirty の場合のみ Write をキューし、Quit は常にキューする
                    let revision = snapshot.revision;
                    let is_dirty = snapshot.dirty;
                    debug_log!(
                        "[DEBUG] apply_intent: :xit on local buffer buf_id={}, dirty={}, queuing {}Quit (revision={})",
                        buf_id,
                        is_dirty,
                        if is_dirty { "Write then " } else { "" },
                        revision
                    );
                    let mut actions = self.pending_host_actions.borrow_mut();
                    if is_dirty {
                        actions.push_back(CoreHostAction::Write {
                            path: String::new(),
                            force: false,
                            issued_after_revision: revision,
                        });
                    }
                    actions.push_back(CoreHostAction::Quit {
                        force: false,
                        issued_after_revision: revision,
                    });
                    Ok(CoreCommandOutcome::HostActionQueued)
                }
            }
            ParsedExIntent::Quit { force } => {
                let snapshot = self.snapshot();
                let active_buf = snapshot
                    .buffers
                    .iter()
                    .find(|b| b.is_active)
                    .ok_or(CoreCommandError::OperationFailed { reason_code: 0 })?;
                let buf_id = active_buf.id;

                // :quit! は常に許可
                if force {
                    let revision = snapshot.revision;
                    self.pending_host_actions
                        .borrow_mut()
                        .push_back(CoreHostAction::Quit {
                            force: true,
                            issued_after_revision: revision,
                        });
                    return Ok(CoreCommandOutcome::HostActionQueued);
                }

                // pending save がある VFS buffer では :quit を拒否
                let coordinator = self.document_coordinator.borrow();
                if coordinator.has_pending_save(buf_id) {
                    debug_log!(
                        "[DEBUG] apply_intent: :quit rejected on VFS buffer buf_id={} with pending save",
                        buf_id
                    );
                    drop(coordinator);
                    self.document_coordinator
                        .borrow_mut()
                        .log_quit_denied(buf_id, "pending save blocks quit");
                    return Err(CoreCommandError::OperationFailed { reason_code: 1 });
                }
                drop(coordinator);

                let revision = snapshot.revision;
                self.pending_host_actions
                    .borrow_mut()
                    .push_back(CoreHostAction::Quit {
                        force: false,
                        issued_after_revision: revision,
                    });
                Ok(CoreCommandOutcome::HostActionQueued)
            }
            ParsedExIntent::Suspend => {
                debug_log!("[DEBUG] apply_intent: suspend requested through host action");
                self.pending_host_actions
                    .borrow_mut()
                    .push_back(CoreHostAction::Suspend);
                Ok(CoreCommandOutcome::HostActionQueued)
            }
        }
    }

    /// VFS buffer の場合は host save flow に接続し、local buffer は既存フローを維持する。
    fn apply_write_intent(
        &mut self,
        path: String,
        force: bool,
        _deferred_close: Option<CoreDeferredClose>,
    ) -> Result<CoreCommandOutcome, CoreCommandError> {
        let snapshot = self.snapshot();
        let active_buf = snapshot
            .buffers
            .iter()
            .find(|b| b.is_active)
            .ok_or(CoreCommandError::OperationFailed { reason_code: 0 })?;
        let buf_id = active_buf.id;

        let is_vfs = self.document_coordinator.borrow().is_vfs_buffer(buf_id);

        if is_vfs {
            let coordinator = self.document_coordinator.borrow();
            let (document_id, _current_rev) = coordinator
                .buffer_text_snapshot(buf_id)
                .ok_or(CoreCommandError::OperationFailed { reason_code: 0 })?;
            drop(coordinator);

            // バッファテキストを取得
            let text = self.buffer_text(buf_id).unwrap_or_default();

            let target_locator = if path.is_empty() { None } else { Some(path) };

            // revision を更新してから save 発行
            self.document_coordinator
                .borrow_mut()
                .note_buffer_revision(buf_id, snapshot.revision);

            let request = self.document_coordinator.borrow_mut().issue_save(
                buf_id,
                document_id,
                target_locator,
                text,
                force,
            );

            debug_log!(
                "[DEBUG] apply_write_intent: VFS save queued buf_id={} request={:?}",
                buf_id,
                request
            );

            self.pending_host_actions
                .borrow_mut()
                .push_back(CoreHostAction::VfsRequest(request));
            Ok(CoreCommandOutcome::HostActionQueued)
        } else {
            // local buffer: 既存フロー
            let revision = snapshot.revision;
            debug_log!(
                "[DEBUG] apply_write_intent: local write buf_id={} path={} force={} revision={}",
                buf_id,
                path,
                force,
                revision
            );
            self.pending_host_actions
                .borrow_mut()
                .push_back(CoreHostAction::Write {
                    path,
                    force,
                    issued_after_revision: revision,
                });
            Ok(CoreCommandOutcome::HostActionQueued)
        }
    }

    pub fn take_pending_host_action(&mut self) -> Option<CoreHostAction> {
        self.drain_native_host_actions();
        self.pending_host_actions.borrow_mut().pop_front()
    }

    pub fn take_pending_event(&mut self) -> Option<CoreEvent> {
        self.drain_native_events();
        self.pending_events.borrow_mut().pop_front()
    }

    pub fn execute_normal_command(
        &mut self,
        command: &str,
    ) -> Result<CoreCommandTransaction, CoreCommandError> {
        let previous_pending = self.pending_input();
        if command == "\x02" && !previous_pending.is_pending() {
            let synthesized = self.ctrl_b_workaround_command();
            debug_log!(
                "[DEBUG] execute_normal_command: ctrl-b workaround -> {:?}",
                synthesized
            );
            if let Some(synthesized) = synthesized {
                let (outcome, snapshot) = self.invoke_native_normal_command(&synthesized)?;
                self.remember_first_page_line_scroll_after_ctrl_b(&snapshot);
                let native_pending = self.read_native_pending_argument();
                let mut transaction = self.collect_transaction(outcome, snapshot);
                let pending_input = derive_direct_pending_input(
                    &synthesized,
                    transaction.snapshot.mode,
                    native_pending,
                );
                self.store_pending_input(pending_input.clone());
                transaction.snapshot.pending_input = pending_input;
                return Ok(transaction);
            }
        }
        let (outcome, snapshot) =
            self.invoke_normal_command_with_ctrl_f_last_page_adjust(command)?;
        let native_pending = self.read_native_pending_argument();
        let mut transaction = self.collect_transaction(outcome, snapshot);
        let pending_input = if command.chars().count() == 1 {
            derive_sequential_pending_input(
                &previous_pending,
                command,
                transaction.snapshot.mode,
                native_pending,
            )
        } else {
            derive_direct_pending_input(command, transaction.snapshot.mode, native_pending)
        };
        self.store_pending_input(pending_input.clone());
        transaction.snapshot.pending_input = pending_input;
        Ok(transaction)
    }

    pub fn dispatch_key(&mut self, key: &str) -> Result<CoreCommandTransaction, CoreCommandError> {
        if key.is_empty() {
            return Err(CoreCommandError::InvalidInput);
        }

        let mode_at_dispatch = self.mode();
        let previous_pending = self.pending_input();
        if key == "\x1a" && !previous_pending.is_pending() {
            debug_log!(
                "[DEBUG] dispatch_key: Ctrl-Z suspend requested in mode={:?}",
                mode_at_dispatch
            );
            self.pending_host_actions
                .borrow_mut()
                .push_back(CoreHostAction::Suspend);
            self.store_pending_input(CorePendingInput::none());
            return Ok(
                self.collect_transaction(CoreCommandOutcome::HostActionQueued, self.snapshot())
            );
        }
        let interprets_sequential_input =
            previous_pending.is_pending() || mode_uses_normal_sequence_grammar(mode_at_dispatch);
        if key == "\x02" && !previous_pending.is_pending() {
            let synthesized = self.ctrl_b_workaround_command().unwrap_or_default();
            debug_log!(
                "[DEBUG] dispatch_key: ctrl-b workaround -> synthesizing {:?} (page_height={})",
                synthesized,
                synthesized.len()
            );
            let transaction = self.execute_normal_command(&synthesized)?;
            self.remember_first_page_line_scroll_after_ctrl_b(&transaction.snapshot);
            return Ok(transaction);
        }
        debug_log!(
            "[DEBUG] dispatch_key:start key={:?} mode_at_dispatch={:?} previous_pending={:?} interprets_sequential_input={}",
            key,
            mode_at_dispatch,
            previous_pending,
            interprets_sequential_input
        );

        if !interprets_sequential_input {
            let (outcome, snapshot) =
                self.invoke_normal_command_with_ctrl_f_last_page_adjust(key)?;
            let native_pending = self.read_native_pending_argument();
            let mut transaction = self.collect_transaction(outcome, snapshot);
            let pending_input =
                derive_direct_pending_input(key, transaction.snapshot.mode, native_pending);

            debug_log!(
                "[DEBUG] dispatch_key:finish key={:?} outcome={:?} mode={:?} native_pending={:?} next_pending={:?} command_completed={} remains_pending={}",
                key,
                transaction.outcome,
                transaction.snapshot.mode,
                native_pending,
                pending_input,
                !pending_input.is_pending(),
                pending_input.is_pending()
            );

            self.store_pending_input(pending_input.clone());
            transaction.snapshot.pending_input = pending_input;
            return Ok(transaction);
        }

        let command = format!("{}{}", previous_pending.pending_keys, key);
        let next_pending = pending_for_dispatch_sequence(&command);
        debug_log!(
            "[DEBUG] dispatch_key:predicted key={:?} command={:?} previous_pending={:?} next_pending={:?}",
            key,
            command,
            previous_pending,
            next_pending
        );

        if next_pending.is_pending() {
            self.store_pending_input(next_pending.clone());
            let mut snapshot = self.snapshot();
            snapshot.pending_input = next_pending.clone();
            debug_log!(
                "[DEBUG] dispatch_key:finish key={:?} outcome={:?} mode={:?} native_pending={:?} next_pending={:?} command_completed=false remains_pending=true",
                key,
                CoreCommandOutcome::NoChange,
                snapshot.mode,
                None::<CorePendingArgumentKind>,
                next_pending
            );
            return Ok(CoreCommandTransaction {
                outcome: CoreCommandOutcome::NoChange,
                snapshot,
                events: Vec::new(),
                host_actions: Vec::new(),
            });
        }

        let (outcome, snapshot) =
            self.invoke_normal_command_with_ctrl_f_last_page_adjust(&command)?;
        let native_pending = self.read_native_pending_argument();
        let mut transaction = self.collect_transaction(outcome, snapshot);
        let pending_input =
            derive_direct_pending_input(&command, transaction.snapshot.mode, native_pending);

        debug_log!(
            "[DEBUG] dispatch_key:finish key={:?} outcome={:?} mode={:?} native_pending={:?} next_pending={:?} command_completed={} remains_pending={}",
            command,
            transaction.outcome,
            transaction.snapshot.mode,
            native_pending,
            pending_input,
            !pending_input.is_pending(),
            pending_input.is_pending()
        );

        self.store_pending_input(pending_input.clone());
        transaction.snapshot.pending_input = pending_input;
        Ok(transaction)
    }

    pub fn execute_ex_command(
        &mut self,
        command: &str,
    ) -> Result<CoreCommandTransaction, CoreCommandError> {
        let trimmed = command.trim();
        let stripped = trimmed.strip_prefix(':').unwrap_or(trimmed).trim();

        // 複合コマンド（パイプ含む）の場合: インターセプト対象より前のサブコマンドをネイティブ実行
        if stripped.contains('|') {
            let segments = split_compound_ex(stripped);
            let mut intercept_idx = None;
            for (i, seg) in segments.iter().enumerate() {
                let seg_trimmed = seg.trim();
                if !seg_trimmed.is_empty() && parse_single_ex_intent(seg_trimmed).is_some() {
                    intercept_idx = Some(i);
                    break;
                }
            }

            if let Some(idx) = intercept_idx {
                // インターセプト対象より前のサブコマンドをネイティブ実行
                for seg in &segments[..idx] {
                    let seg_trimmed = seg.trim();
                    if !seg_trimmed.is_empty() {
                        debug_log!(
                            "[DEBUG] execute_ex_command: executing non-intercepted sub-command natively: '{}'",
                            seg_trimmed
                        );
                        let native_cmd = format!(":{}", seg_trimmed);
                        let _ = self.invoke_native_ex_command(&native_cmd);
                    }
                }
                // インターセプト対象のサブコマンドを処理
                let intent = parse_single_ex_intent(segments[idx].trim()).unwrap();
                let intent = rewrite_vfs_trailing_quit_intent(self, intent, &segments[idx + 1..]);
                let should_chain_trailing_quit =
                    should_chain_trailing_quit_for_local_write(self, &intent, &segments[idx + 1..]);
                let outcome = self.apply_intent(intent)?;
                if should_chain_trailing_quit
                    && let Some(trailing_quit) =
                        find_first_trailing_quit_intent(&segments[idx + 1..])
                {
                    let _ = self.apply_intent(trailing_quit)?;
                }
                return Ok(self.collect_transaction(outcome, self.snapshot()));
            }
        }

        // 単一コマンドまたはインターセプト対象なしの複合コマンド
        if let Some(intent) = parse_ex_intent(command) {
            let outcome = self.apply_intent(intent)?;
            return Ok(self.collect_transaction(outcome, self.snapshot()));
        }

        let (outcome, snapshot) = self.invoke_native_ex_command(command)?;
        Ok(self.collect_transaction(outcome, snapshot))
    }

    pub fn get_undo_tree(&self, buf_id: i32) -> Result<CoreUndoTree, CoreCommandError> {
        let mut tree = unsafe { std::mem::zeroed() };
        let result =
            unsafe { bindings::vim_bridge_get_undo_tree(self.state.as_ptr(), buf_id, &mut tree) };
        if result != 0 {
            return Err(CoreCommandError::OperationFailed {
                reason_code: result as u32,
            });
        }
        Ok(convert_undo_tree(tree))
    }

    pub fn undo_jump(&mut self, buf_id: i32, seq: i32) -> Result<(), CoreCommandError> {
        let result = unsafe {
            bindings::vim_bridge_undo_jump(
                self.state.as_ptr(),
                buf_id,
                seq as ::std::os::raw::c_long,
            )
        };
        if result != 0 {
            return Err(CoreCommandError::OperationFailed {
                reason_code: result as u32,
            });
        }
        Ok(())
    }

    pub fn backend_identity(&self) -> CoreBackendIdentity {
        let identity = unsafe { bindings::vim_bridge_backend_identity(self.state.as_ptr()) };
        match identity {
            value
                if value
                    == bindings::vim_runtime_backend_identity_VIM_CORE_BACKEND_IDENTITY_UPSTREAM_RUNTIME =>
            {
                CoreBackendIdentity::UpstreamRuntime
            }
            value if value == bindings::vim_runtime_backend_identity_VIM_CORE_BACKEND_IDENTITY_BRIDGE_STUB => {
                CoreBackendIdentity::BridgeStub
            }
            _ => CoreBackendIdentity::BridgeStub,
        }
    }

    pub fn get_syntax_name(&self, syn_id: i32) -> Option<String> {
        let name_ptr = unsafe { bindings::vim_bridge_get_syntax_name(self.state.as_ptr(), syn_id) };
        if name_ptr.is_null() {
            return None;
        }
        let c_str = unsafe { std::ffi::CStr::from_ptr(name_ptr) };
        let s = c_str.to_string_lossy().into_owned();
        if s.is_empty() { None } else { Some(s) }
    }

    pub fn get_line_syntax(
        &self,
        win_id: i32,
        lnum: i64,
    ) -> Result<Vec<CoreSyntaxChunk>, CoreCommandError> {
        let mut out_ids = vec![0i32; 1024]; // allocate enough space for a line
        let cols = unsafe {
            bindings::vim_bridge_get_line_syntax(
                self.state.as_ptr(),
                win_id,
                lnum as std::os::raw::c_long,
                out_ids.as_mut_ptr(),
                out_ids.len() as std::os::raw::c_int,
            )
        };

        if cols < 0 {
            return Err(CoreCommandError::OperationFailed {
                reason_code: cols as u32,
            });
        }

        let mut chunks = Vec::new();
        if cols == 0 {
            return Ok(chunks);
        }

        let mut current_id = out_ids[0];
        let mut start_col = 0;

        for (i, syn_id) in out_ids
            .iter()
            .copied()
            .enumerate()
            .take(cols as usize)
            .skip(1)
        {
            if syn_id != current_id {
                chunks.push(CoreSyntaxChunk {
                    start_col,
                    end_col: i,
                    syn_id: current_id,
                    name: self.get_syntax_name(current_id),
                });
                current_id = syn_id;
                start_col = i;
            }
        }

        chunks.push(CoreSyntaxChunk {
            start_col,
            end_col: cols as usize,
            syn_id: current_id,
            name: self.get_syntax_name(current_id),
        });

        Ok(chunks)
    }

    /// Vimscript式を評価し、結果を文字列として返す
    pub fn eval_string(&mut self, expr: &str) -> Option<String> {
        self.completed_input_eval_result.borrow_mut().take();
        let expr_c = CString::new(expr).ok()?;
        let ptr = unsafe { bindings::vim_bridge_eval_string(self.state.as_ptr(), expr_c.as_ptr()) };
        self.drain_native_host_actions_with_input_source(Some(expr));
        self.drain_native_events();
        if ptr.is_null() {
            /* println debug removed */
            return None;
        }
        let len = unsafe { std::ffi::CStr::from_ptr(ptr).to_bytes().len() };
        let s = string_from_parts(ptr, len);
        unsafe { bindings::vim_bridge_free_string(ptr) };
        /* println debug removed */
        Some(s)
    }

    fn eval_string_after_input_response(
        &mut self,
        expr: &str,
    ) -> Result<String, CoreInputResponseError> {
        let expr_c = CString::new(expr).map_err(|_| CoreInputResponseError::EvalFailed)?;
        let ptr = unsafe { bindings::vim_bridge_eval_string(self.state.as_ptr(), expr_c.as_ptr()) };
        self.drain_native_host_actions_with_input_source(Some(expr));
        self.drain_native_events();
        if ptr.is_null() {
            return Err(CoreInputResponseError::EvalFailed);
        }
        let len = unsafe { std::ffi::CStr::from_ptr(ptr).to_bytes().len() };
        let s = string_from_parts(ptr, len);
        unsafe { bindings::vim_bridge_free_string(ptr) };
        Ok(s)
    }

    fn submit_native_input_response(&mut self, response: &CoreInputResponse) {
        match response {
            CoreInputResponse::Submitted { value, .. } => unsafe {
                bindings::vim_bridge_submit_input_response(
                    self.state.as_ptr(),
                    value.as_ptr().cast(),
                    value.len(),
                    false,
                );
            },
            CoreInputResponse::Cancelled { .. } => unsafe {
                bindings::vim_bridge_submit_input_response(
                    self.state.as_ptr(),
                    std::ptr::null(),
                    0,
                    true,
                );
            },
        }
    }

    fn invoke_native_normal_command(
        &mut self,
        command: &str,
    ) -> Result<(CoreCommandOutcome, CoreSnapshot), CoreCommandError> {
        let result = unsafe {
            bindings::vim_bridge_execute_normal_command(
                self.state.as_ptr(),
                command.as_ptr().cast(),
                command.len(),
            )
        };
        convert_command_result_with_snapshot(result)
    }

    fn invoke_page_edge_line_scroll_if_needed(
        &mut self,
        command: &str,
    ) -> Result<Option<(CoreCommandOutcome, CoreSnapshot)>, CoreCommandError> {
        let pending = self.page_edge_line_scroll.borrow().clone();
        let Some(pending) = pending else {
            return Ok(None);
        };

        let matches_pending = match (&pending, command) {
            (PageEdgeLineScroll::UpAfterLastPage, "k") => true,
            (PageEdgeLineScroll::DownAfterFirstPage, "j") => true,
            _ => {
                debug_log!(
                    "[DEBUG] page-edge line scroll cleared by non-matching command: pending={:?} command={:?}",
                    pending,
                    command
                );
                self.page_edge_line_scroll.replace(None);
                return Ok(None);
            }
        };
        if !matches_pending {
            return Ok(None);
        }

        let before = self.snapshot();
        let before_window = before
            .windows
            .iter()
            .find(|window| window.is_active)
            .or_else(|| before.windows.first())
            .cloned();
        let target_topline = before_window.as_ref().map(|window| match pending {
            PageEdgeLineScroll::UpAfterLastPage => window.topline.saturating_sub(1).max(1),
            PageEdgeLineScroll::DownAfterFirstPage => window.topline.saturating_add(1),
        });
        debug_log!(
            "[DEBUG] page-edge line scroll start: pending={:?} command={:?} target_topline={:?} before_window={:?} before_cursor={}",
            pending,
            command,
            target_topline,
            before_window,
            before.cursor_row
        );

        let (mut outcome, mut snapshot) = self.invoke_native_normal_command(command)?;
        if let Some(target_topline) = target_topline {
            let mut correction_attempts = 0usize;
            loop {
                let Some(window) = snapshot
                    .windows
                    .iter()
                    .find(|window| window.is_active)
                    .or_else(|| snapshot.windows.first())
                else {
                    break;
                };
                if window.topline == target_topline {
                    break;
                }
                if correction_attempts >= 8 {
                    debug_log!(
                        "[DEBUG] page-edge line scroll correction stopped after attempt limit: pending={:?} command={:?} current_topline={} target_topline={}",
                        pending,
                        command,
                        window.topline,
                        target_topline
                    );
                    break;
                }

                let correction = if window.topline > target_topline {
                    "\x19"
                } else {
                    "\x05"
                };
                let before_correction_topline = window.topline;
                debug_log!(
                    "[DEBUG] page-edge line scroll correcting topline: pending={:?} command={:?} correction={:?} current_topline={} target_topline={}",
                    pending,
                    command,
                    correction,
                    window.topline,
                    target_topline
                );
                (outcome, snapshot) = self.invoke_native_normal_command(correction)?;
                correction_attempts = correction_attempts.saturating_add(1);
                let corrected_topline = snapshot
                    .windows
                    .iter()
                    .find(|window| window.is_active)
                    .or_else(|| snapshot.windows.first())
                    .map(|window| window.topline);
                if corrected_topline == Some(before_correction_topline) {
                    debug_log!(
                        "[DEBUG] page-edge line scroll correction made no progress: pending={:?} command={:?} correction={:?} topline={} target_topline={}",
                        pending,
                        command,
                        correction,
                        before_correction_topline,
                        target_topline
                    );
                    break;
                }
            }
        }
        let after_window = snapshot
            .windows
            .iter()
            .find(|window| window.is_active)
            .or_else(|| snapshot.windows.first())
            .cloned();
        debug_log!(
            "[DEBUG] page-edge line scroll finish: pending={:?} command={:?} target_topline={:?} after_window={:?} after_cursor={}",
            pending,
            command,
            target_topline,
            after_window,
            snapshot.cursor_row
        );

        self.page_edge_line_scroll.replace(Some(pending));
        Ok(Some((outcome, snapshot)))
    }

    fn invoke_normal_command_with_ctrl_f_last_page_adjust(
        &mut self,
        command: &str,
    ) -> Result<(CoreCommandOutcome, CoreSnapshot), CoreCommandError> {
        if let Some(result) = self.invoke_page_edge_line_scroll_if_needed(command)? {
            return Ok(result);
        }

        let (outcome, snapshot) = self.invoke_native_normal_command(command)?;
        if command != "\x06" {
            self.page_edge_line_scroll.replace(None);
            return Ok((outcome, snapshot));
        }

        let Some(window) = snapshot
            .windows
            .iter()
            .find(|window| window.is_active)
            .or_else(|| snapshot.windows.first())
        else {
            return Ok((outcome, snapshot));
        };
        let total_lines = self.snapshot().text.lines().count().max(1);
        let cursor_at_topline = window.cursor_row.saturating_add(1) == window.topline;
        let reached_last_page = window.botline >= total_lines;
        if !reached_last_page || !cursor_at_topline {
            debug_log!(
                "[DEBUG] ctrl-f last-page adjust skipped: topline={} botline={} cursor_row={} total_lines={} reached_last_page={} cursor_at_topline={}",
                window.topline,
                window.botline,
                window.cursor_row,
                total_lines,
                reached_last_page,
                cursor_at_topline
            );
            self.page_edge_line_scroll.replace(None);
            return Ok((outcome, snapshot));
        }

        debug_log!(
            "[DEBUG] ctrl-f last-page adjust: moving cursor to visible bottom with L after native Ctrl-F: topline={} botline={} cursor_row={} total_lines={}",
            window.topline,
            window.botline,
            window.cursor_row,
            total_lines
        );
        let adjusted = self.invoke_native_normal_command("L")?;
        self.page_edge_line_scroll
            .replace(Some(PageEdgeLineScroll::UpAfterLastPage));
        Ok(adjusted)
    }

    fn remember_first_page_line_scroll_after_ctrl_b(&self, snapshot: &CoreSnapshot) {
        let Some(window) = snapshot
            .windows
            .iter()
            .find(|window| window.is_active)
            .or_else(|| snapshot.windows.first())
        else {
            self.page_edge_line_scroll.replace(None);
            return;
        };
        let cursor_at_topline = window.cursor_row.saturating_add(1) == window.topline;
        if window.topline == 1 && cursor_at_topline {
            debug_log!(
                "[DEBUG] ctrl-b first-page adjust: remembering page-edge line scroll down: topline={} botline={} cursor_row={}",
                window.topline,
                window.botline,
                window.cursor_row
            );
            self.page_edge_line_scroll
                .replace(Some(PageEdgeLineScroll::DownAfterFirstPage));
        } else {
            debug_log!(
                "[DEBUG] ctrl-b first-page adjust skipped: topline={} botline={} cursor_row={} cursor_at_topline={}",
                window.topline,
                window.botline,
                window.cursor_row,
                cursor_at_topline
            );
            self.page_edge_line_scroll.replace(None);
        }
    }

    fn invoke_native_ex_command(
        &mut self,
        command: &str,
    ) -> Result<(CoreCommandOutcome, CoreSnapshot), CoreCommandError> {
        let result = unsafe {
            bindings::vim_bridge_execute_ex_command(
                self.state.as_ptr(),
                command.as_ptr().cast(),
                command.len(),
            )
        };
        convert_command_result_with_snapshot(result)
    }

    fn collect_transaction(
        &mut self,
        outcome: CoreCommandOutcome,
        mut snapshot: CoreSnapshot,
    ) -> CoreCommandTransaction {
        self.drain_native_host_actions();
        let events = self.drain_native_events_to_vec();

        let drained_host_actions: Vec<CoreHostAction> =
            self.pending_host_actions.borrow_mut().drain(..).collect();
        self.pending_events
            .borrow_mut()
            .extend(events.iter().cloned());
        let host_actions = drained_host_actions;
        self.record_active_input_request_from_host_actions(&host_actions, None);
        let outcome = normalize_transaction_outcome(outcome, &host_actions);
        snapshot.pending_host_actions = host_actions.len();
        snapshot.pending_input = self.pending_input();

        CoreCommandTransaction {
            outcome,
            snapshot,
            events,
            host_actions,
        }
    }

    fn record_active_input_request_from_host_actions(
        &self,
        host_actions: &[CoreHostAction],
        pending_eval_expr: Option<&str>,
    ) {
        for action in host_actions {
            if let CoreHostAction::RequestInput {
                prompt,
                input_kind,
                correlation_id,
            } = action
            {
                debug_log!(
                    "[DEBUG] active_input_request: recorded correlation_id={} prompt={:?} input_kind={:?} pending_eval={}",
                    correlation_id,
                    prompt,
                    input_kind,
                    pending_eval_expr.is_some()
                );
                *self.active_input_request.borrow_mut() = Some(CoreActiveInputRequest {
                    prompt: prompt.clone(),
                    input_kind: *input_kind,
                    correlation_id: *correlation_id,
                    pending_eval_expr: pending_eval_expr.map(ToOwned::to_owned),
                });
            }
        }
    }

    fn read_native_pending_argument(&self) -> Option<CorePendingArgumentKind> {
        let pending_input = unsafe { bindings::vim_bridge_get_pending_input(self.state.as_ptr()) };
        convert_native_pending_argument(pending_input)
    }

    fn store_pending_input(&self, pending_input: CorePendingInput) {
        debug_log!("[DEBUG] pending_input_transition: {:?}", pending_input);
        *self.pending_input_state.borrow_mut() = pending_input;
    }

    pub fn register(&self, regname: char) -> Option<String> {
        let regname_c = regname as std::os::raw::c_char;
        let result = unsafe { bindings::vim_bridge_get_register(self.state.as_ptr(), regname_c) };
        if result.payload_ptr.is_null() {
            return None;
        }

        let s = string_from_parts(result.payload_ptr, result.payload_len);

        unsafe { bindings::vim_bridge_free_string(result.payload_ptr.cast_mut()) };
        Some(s)
    }

    pub fn set_screen_size(&mut self, rows: i32, cols: i32) {
        unsafe {
            bindings::vim_bridge_set_screen_size(self.state.as_ptr(), rows, cols);
        }
    }

    pub fn buffers(&self) -> Vec<CoreBufferInfo> {
        self.snapshot().buffers
    }

    pub fn windows(&self) -> Vec<CoreWindowInfo> {
        self.snapshot().windows
    }

    pub fn active_window_id(&self) -> Option<i32> {
        self.light_snapshot().active_window_id()
    }

    pub fn buffer_binding(&self, buf_id: i32) -> Option<CoreBufferBinding> {
        let _ = self.snapshot();
        self.document_coordinator.borrow().binding(buf_id).cloned()
    }

    pub fn vfs_request_ledger(&self) -> Vec<CoreRequestEntry> {
        self.document_coordinator.borrow().ledger_entries()
    }

    pub fn vfs_transaction_log(&self) -> Vec<VfsLogEntry> {
        self.document_coordinator
            .borrow()
            .transaction_log()
            .to_vec()
    }

    pub fn switch_to_buffer(&mut self, buf_id: i32) -> Result<(), CoreCommandError> {
        let status = unsafe { bindings::vim_bridge_switch_to_buffer(self.state.as_ptr(), buf_id) };
        convert_status(status)
    }

    pub fn switch_to_window(&mut self, win_id: i32) -> Result<(), CoreCommandError> {
        let status = unsafe { bindings::vim_bridge_switch_to_window(self.state.as_ptr(), win_id) };
        convert_status(status)
    }

    pub fn buffer_text(&self, buf_id: i32) -> Option<String> {
        let ptr = unsafe { bindings::vim_bridge_get_buffer_text(self.state.as_ptr(), buf_id) };
        if ptr.is_null() {
            return None;
        }
        let len = unsafe { std::ffi::CStr::from_ptr(ptr).to_bytes().len() };
        let s = string_from_parts(ptr, len);
        unsafe { bindings::vim_bridge_free_string(ptr) };
        Some(s)
    }

    pub fn buffer_line_range(
        &self,
        buf_id: i32,
        start_row: usize,
        line_count: usize,
    ) -> Option<CoreBufferLineRange> {
        let range = unsafe {
            bindings::vim_bridge_get_buffer_line_range(
                self.state.as_ptr(),
                buf_id,
                start_row,
                line_count,
            )
        };
        if !range.found {
            return None;
        }

        let lines = if range.lines.is_null() || range.line_count == 0 {
            Vec::new()
        } else {
            let slice = unsafe { slice::from_raw_parts(range.lines, range.line_count) };
            slice
                .iter()
                .map(|line| string_from_parts(line.text_ptr, line.text_len))
                .collect::<Vec<_>>()
        };

        let result = CoreBufferLineRange {
            buffer_id: range.buffer_id,
            source_revision: CoreBufferRevision {
                value: range.source_revision,
            },
            start_row: range.start_row,
            line_count: range.line_count,
            total_line_count: range.total_line_count,
            lines,
        };

        unsafe { bindings::vim_bridge_free_buffer_line_range(range) };
        Some(result)
    }

    /// VFS load/save の結果を target buffer に反映する runtime apply contract。
    /// vim_bridge_commit_buffer_update を使用して text, name, dirty を一括で反映する。
    fn apply_loaded_buffer(
        &mut self,
        buf_id: i32,
        display_name: &str,
        text: &str,
    ) -> Result<(), CoreCommandError> {
        debug_log!(
            "[DEBUG] apply_loaded_buffer: buf_id={} display_name={} text_len={}",
            buf_id,
            display_name,
            text.len()
        );
        let commit = bindings::vim_core_buffer_commit_t {
            target_buf_id: buf_id,
            replace_text: true,
            text_ptr: text.as_ptr().cast(),
            text_len: text.len(),
            display_name_ptr: display_name.as_ptr().cast(),
            display_name_len: display_name.len(),
            clear_dirty: true,
        };
        let status =
            unsafe { bindings::vim_bridge_commit_buffer_update(self.state.as_ptr(), &commit) };
        convert_status(status)
    }

    fn drain_native_host_actions(&mut self) {
        self.drain_native_host_actions_with_input_source(None);
    }

    fn drain_native_host_actions_with_input_source(&mut self, pending_eval_expr: Option<&str>) {
        let mut drained_actions = Vec::new();
        loop {
            let action =
                unsafe { bindings::vim_bridge_take_pending_host_action(self.state.as_ptr()) };
            let Some(action) = convert_host_action(action) else {
                break;
            };
            if should_expose_host_action_in_queue_api(&action) {
                drained_actions.push(action.clone());
                self.pending_host_actions.borrow_mut().push_back(action);
            }
        }
        self.record_active_input_request_from_host_actions(&drained_actions, pending_eval_expr);

        let pending_job_writes = {
            let mut mgr = crate::vfd::get_manager();
            let mut writes = Vec::new();
            while let Some(write) = mgr.take_pending_job_write() {
                writes.push(write);
            }
            writes
        };

        for write in pending_job_writes {
            self.pending_host_actions
                .borrow_mut()
                .push_back(CoreHostAction::JobWrite {
                    vfd: write.vfd,
                    data: write.data,
                });
        }
    }

    fn drain_native_events(&mut self) {
        let events = self.drain_native_events_to_vec();
        self.pending_events.borrow_mut().extend(events);
    }

    fn drain_native_events_to_vec(&mut self) -> Vec<CoreEvent> {
        let mut events = Vec::new();
        loop {
            let event = unsafe { bindings::vim_bridge_take_pending_event(self.state.as_ptr()) };
            let Some(event) = convert_event(event) else {
                break;
            };
            events.push(event);
        }
        events
    }

    pub fn set_register(&mut self, regname: char, text: &str) {
        let regname_c = regname as std::os::raw::c_char;
        unsafe {
            bindings::vim_bridge_set_register(
                self.state.as_ptr(),
                regname_c,
                text.as_ptr().cast(),
                text.len(),
            )
        }
    }

    pub fn get_option_number(
        &self,
        name: &str,
        scope: CoreOptionScope,
    ) -> Result<i64, CoreOptionError> {
        match self.get_option_value(name, scope, CoreOptionType::Number)? {
            ConvertedOptionValue::Number(value) => Ok(value),
            other => Err(CoreOptionError::InternalError {
                name: name.to_string(),
                detail: format!(
                    "number option getter received unexpected converted value: {:?}",
                    other
                ),
            }),
        }
    }

    pub fn get_option_bool(
        &self,
        name: &str,
        scope: CoreOptionScope,
    ) -> Result<bool, CoreOptionError> {
        match self.get_option_value(name, scope, CoreOptionType::Bool)? {
            ConvertedOptionValue::Bool(value) => Ok(value),
            other => Err(CoreOptionError::InternalError {
                name: name.to_string(),
                detail: format!(
                    "bool option getter received unexpected converted value: {:?}",
                    other
                ),
            }),
        }
    }

    pub fn get_option_string(
        &self,
        name: &str,
        scope: CoreOptionScope,
    ) -> Result<String, CoreOptionError> {
        match self.get_option_value(name, scope, CoreOptionType::String)? {
            ConvertedOptionValue::String(value) => Ok(value),
            other => Err(CoreOptionError::InternalError {
                name: name.to_string(),
                detail: format!(
                    "string option getter received unexpected converted value: {:?}",
                    other
                ),
            }),
        }
    }

    pub fn set_option_number(
        &mut self,
        name: &str,
        value: i64,
        scope: CoreOptionScope,
    ) -> Result<(), CoreOptionError> {
        debug_log!(
            "[DEBUG] VimCoreSession::set_option_number: name={} value={} scope={:?}",
            name,
            value,
            scope
        );
        let name_c = option_name_to_cstring(name)?;
        let result = unsafe {
            bindings::vim_bridge_set_option_number(
                self.state.as_ptr(),
                name_c.as_ptr(),
                value,
                convert_option_scope(scope),
            )
        };

        convert_option_set_result(name, result)
    }

    pub fn set_option_bool(
        &mut self,
        name: &str,
        value: bool,
        scope: CoreOptionScope,
    ) -> Result<(), CoreOptionError> {
        debug_log!(
            "[DEBUG] VimCoreSession::set_option_bool: name={} value={} scope={:?}",
            name,
            value,
            scope
        );
        self.set_option_number(name, i64::from(value), scope)
    }

    pub fn set_option_string(
        &mut self,
        name: &str,
        value: &str,
        scope: CoreOptionScope,
    ) -> Result<(), CoreOptionError> {
        debug_log!(
            "[DEBUG] VimCoreSession::set_option_string: name={} value={} scope={:?}",
            name,
            value,
            scope
        );
        let name_c = option_name_to_cstring(name)?;
        let value_c = option_value_to_cstring(name, value)?;
        let result = unsafe {
            bindings::vim_bridge_set_option_string(
                self.state.as_ptr(),
                name_c.as_ptr(),
                value_c.as_ptr(),
                convert_option_scope(scope),
            )
        };

        convert_option_set_result(name, result)
    }

    fn get_option_value(
        &self,
        name: &str,
        scope: CoreOptionScope,
        expected: CoreOptionType,
    ) -> Result<ConvertedOptionValue, CoreOptionError> {
        debug_log!(
            "[DEBUG] VimCoreSession::get_option_value: name={} scope={:?} expected={:?}",
            name,
            scope,
            expected
        );
        let name_c = option_name_to_cstring(name)?;
        let result = unsafe {
            bindings::vim_bridge_get_option(
                self.state.as_ptr(),
                name_c.as_ptr(),
                convert_option_scope(scope),
            )
        };

        convert_option_get_result(name, scope, expected, result)
    }

    pub fn get_search_pattern(&self) -> Option<String> {
        unsafe {
            let ptr = bindings::vim_bridge_get_search_pattern();
            if ptr.is_null() {
                None
            } else {
                let c_str = std::ffi::CStr::from_ptr(ptr);
                let s = c_str.to_string_lossy().into_owned();
                if s.is_empty() { None } else { Some(s) }
            }
        }
    }

    pub fn is_hlsearch_active(&self) -> bool {
        unsafe { bindings::vim_bridge_is_hlsearch_active() != 0 }
    }

    pub fn get_search_direction(&self) -> CoreSearchDirection {
        let dir = unsafe { bindings::vim_bridge_get_search_direction() };
        if dir == 1 {
            CoreSearchDirection::Forward
        } else {
            CoreSearchDirection::Backward
        }
    }

    pub fn get_search_highlights(
        &self,
        window_id: i32,
        start_row: i32,
        end_row: i32,
    ) -> Vec<CoreMatchRange> {
        unsafe {
            let list = bindings::vim_bridge_get_search_highlights(window_id, start_row, end_row);
            let mut result = Vec::new();
            if !list.ranges.is_null() && list.count > 0 {
                let slice = std::slice::from_raw_parts(list.ranges, list.count as usize);
                for range in slice {
                    result.push(CoreMatchRange {
                        start_row: range.start_row as usize,
                        start_col: range.start_col as usize,
                        end_row: range.end_row as usize,
                        end_col: range.end_col as usize,
                        match_type: CoreMatchType::from(range.match_type),
                    });
                }
            }
            bindings::vim_bridge_free_match_list(list);
            result
        }
    }

    pub fn get_cursor_match_info(
        &self,
        window_id: i32,
        row: i32,
        col: i32,
        max_count: i32,
        timeout_ms: i32,
    ) -> CoreCursorMatchInfo {
        unsafe {
            let info = bindings::vim_bridge_get_cursor_match_info(
                window_id, row, col, max_count, timeout_ms,
            );
            let total_matches = match info.status {
                bindings::vim_core_match_count_status_t_VIM_CORE_MATCH_COUNT_MAX_REACHED => {
                    MatchCountResult::MaxReached(info.total_matches as usize)
                }
                bindings::vim_core_match_count_status_t_VIM_CORE_MATCH_COUNT_TIMED_OUT => {
                    MatchCountResult::TimedOut
                }
                _ => MatchCountResult::Calculated(info.total_matches as usize),
            };

            CoreCursorMatchInfo {
                is_on_match: info.is_on_match != 0,
                current_match_index: info.current_match_index as usize,
                total_matches,
            }
        }
    }

    pub fn is_incsearch_active(&self) -> bool {
        unsafe { bindings::vim_bridge_is_incsearch_active() != 0 }
    }

    pub fn get_incsearch_pattern(&self) -> Option<String> {
        unsafe {
            let ptr = bindings::vim_bridge_get_incsearch_pattern();
            if ptr.is_null() {
                None
            } else {
                let c_str = std::ffi::CStr::from_ptr(ptr);
                let s = c_str.to_string_lossy().into_owned();
                if s.is_empty() { None } else { Some(s) }
            }
        }
    }

    pub fn get_search_input_pattern(&self) -> Option<String> {
        unsafe {
            let ptr = bindings::vim_bridge_get_search_input_pattern();
            if ptr.is_null() {
                None
            } else {
                let c_str = std::ffi::CStr::from_ptr(ptr);
                let s = c_str.to_string_lossy().into_owned();
                if s.is_empty() { None } else { Some(s) }
            }
        }
    }

    pub fn query_visible_search_state(
        &mut self,
        start_row: i32,
        end_row: i32,
    ) -> Result<CoreVisibleSearchState, CoreSearchQueryError> {
        validate_search_viewport(start_row, end_row)?;
        let snapshot = self.snapshot();
        let window_id = resolve_active_search_window_id(&snapshot)?;
        self.query_visible_search_state_for_window(window_id, start_row, end_row)
    }

    pub fn query_visible_search_state_for_window(
        &mut self,
        window_id: i32,
        start_row: i32,
        end_row: i32,
    ) -> Result<CoreVisibleSearchState, CoreSearchQueryError> {
        validate_search_viewport(start_row, end_row)?;

        let snapshot = self.snapshot();
        if !snapshot.windows.iter().any(|window| window.id == window_id) {
            return Err(CoreSearchQueryError::WindowNotFound { window_id });
        }

        let input_pattern = self.current_search_input_pattern();
        let incsearch_active = self.is_incsearch_active();
        let hlsearch_enabled = self
            .get_option_bool("hlsearch", CoreOptionScope::Default)
            .unwrap_or(false);
        let hlsearch_suspended = hlsearch_enabled && !self.is_hlsearch_active();
        let pattern = if incsearch_active {
            self.get_incsearch_pattern()
        } else {
            self.get_search_pattern()
        };
        let mode = if incsearch_active {
            CoreSearchHighlightMode::IncSearch
        } else if self.is_hlsearch_active() {
            CoreSearchHighlightMode::HlSearch
        } else {
            CoreSearchHighlightMode::Disabled
        };
        let ranges = if matches!(mode, CoreSearchHighlightMode::Disabled) {
            Vec::new()
        } else {
            self.get_search_highlights(window_id, start_row, end_row)
        };

        Ok(CoreVisibleSearchState {
            window_id,
            start_row: start_row as usize,
            end_row: end_row as usize,
            mode,
            pattern,
            input_pattern,
            hlsearch_enabled,
            hlsearch_suspended,
            incsearch_active,
            ranges,
        })
    }

    pub fn search_capability_contract() -> CoreSearchCapabilityContract {
        CoreSearchCapabilityContract {
            live_state_query_available: true,
            inactive_window_query_available: true,
            visible_rows_only: true,
            start_col_inclusive: true,
            end_col_exclusive: true,
            byte_columns: true,
            data_only_payload: true,
            host_owned_presentation: true,
        }
    }

    fn current_search_input_pattern(&self) -> Option<String> {
        self.get_search_input_pattern()
    }
}

impl Drop for VimCoreSession {
    fn drop(&mut self) {
        unsafe {
            bindings::vim_bridge_state_free(self.state.as_ptr());
        }
        crate::vfd::get_manager().clear_all();
        ACTIVE_SESSION.store(false, Ordering::Release);
    }
}

fn validate_search_viewport(start_row: i32, end_row: i32) -> Result<(), CoreSearchQueryError> {
    if start_row < 1 || end_row < 1 || start_row > end_row {
        return Err(CoreSearchQueryError::InvalidViewport { start_row, end_row });
    }
    Ok(())
}

fn active_window_id_from_snapshot(snapshot: &CoreSnapshot) -> Option<i32> {
    snapshot
        .windows
        .iter()
        .find(|window| window.is_active)
        .map(|window| window.id)
}

fn resolve_active_search_window_id(snapshot: &CoreSnapshot) -> Result<i32, CoreSearchQueryError> {
    active_window_id_from_snapshot(snapshot).ok_or(CoreSearchQueryError::NoActiveWindow)
}

fn option_name_to_cstring(name: &str) -> Result<CString, CoreOptionError> {
    CString::new(name).map_err(|_| CoreOptionError::InternalError {
        name: name.to_string(),
        detail: "option name contains interior NUL byte".to_string(),
    })
}

fn option_value_to_cstring(name: &str, value: &str) -> Result<CString, CoreOptionError> {
    CString::new(value).map_err(|_| CoreOptionError::InternalError {
        name: name.to_string(),
        detail: "option value contains interior NUL byte".to_string(),
    })
}

fn core_mode_is_visual(mode: CoreMode) -> bool {
    matches!(
        mode,
        CoreMode::Visual | CoreMode::VisualLine | CoreMode::VisualBlock
    )
}

fn normalize_visual_selection_bounds(
    first: (usize, usize),
    second: (usize, usize),
) -> ((usize, usize), (usize, usize)) {
    if first <= second {
        (first, second)
    } else {
        (second, first)
    }
}

fn should_expose_host_action_in_queue_api(action: &CoreHostAction) -> bool {
    !matches!(action, CoreHostAction::Redraw { .. } | CoreHostAction::Bell)
}

fn normalize_outcome_after_host_action_drain(
    outcome: CoreCommandOutcome,
    pending_host_actions: &VecDeque<CoreHostAction>,
) -> CoreCommandOutcome {
    if matches!(outcome, CoreCommandOutcome::HostActionQueued) && pending_host_actions.is_empty() {
        CoreCommandOutcome::NoChange
    } else {
        outcome
    }
}

fn normalize_transaction_outcome(
    outcome: CoreCommandOutcome,
    host_actions: &[CoreHostAction],
) -> CoreCommandOutcome {
    if matches!(outcome, CoreCommandOutcome::HostActionQueued) && host_actions.is_empty() {
        CoreCommandOutcome::NoChange
    } else {
        outcome
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConvertedOptionValue {
    Bool(bool),
    Number(i64),
    String(String),
}

fn string_from_parts(ptr: *const ::std::os::raw::c_char, len: usize) -> String {
    if ptr.is_null() || len == 0 {
        return String::new();
    }

    let bytes = unsafe { slice::from_raw_parts(ptr.cast::<u8>(), len) };
    str::from_utf8(bytes)
        .expect("vim bridge returned non-utf8 text")
        .to_owned()
}

#[cfg(test)]
mod search_query_tests {
    use super::*;

    #[test]
    fn resolve_active_search_window_id_requires_active_window() {
        let snapshot = CoreSnapshot {
            text: String::new(),
            revision: 0,
            dirty: false,
            mode: CoreMode::Normal,
            pending_input: CorePendingInput::none(),
            cursor_row: 0,
            cursor_col: 0,
            pending_host_actions: 0,
            buffers: Vec::new(),
            windows: Vec::new(),
            pum: None,
        };

        assert_eq!(
            resolve_active_search_window_id(&snapshot),
            Err(CoreSearchQueryError::NoActiveWindow)
        );
    }
}
