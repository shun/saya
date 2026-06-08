# vim-core-rs bundled public API reference

This file describes the standalone crate-public API of `vim-core-rs`. Use it
when you need the callable surface and data model without reopening source
files.

## Public surface summary

- The crate root exposes `VimCoreSession` as the main stateful facade.
- The crate root also exposes plain types for snapshots, commands, host
  actions, VFS contracts, options, undo trees, and rendering data.
- The `ffi` module exposes a narrow FFI-facing contract for POD structs and
  VFS-related constants.
- `src/vfs.rs` contributes public types through crate-root re-exports even
  though the module itself is private.

### Public root types

- `CoreCommandTransaction`
- `CoreMessageEvent`
- `CoreMessageSeverity`
- `CoreMessageCategory`

## `ffi` module

Use the `ffi` module only for narrow interop and contract testing.

Re-exported POD structs:

- `vim_core_buffer_commit_t`
- `vim_core_buffer_info_t`

Exported constants:

- `VIM_CORE_VFS_OPERATION_NONE`
- `VIM_CORE_VFS_OPERATION_RESOLVE`
- `VIM_CORE_VFS_OPERATION_EXISTS`
- `VIM_CORE_VFS_OPERATION_LOAD`
- `VIM_CORE_VFS_OPERATION_SAVE`
- `VIM_CORE_BUFFER_SOURCE_LOCAL`
- `VIM_CORE_BUFFER_SOURCE_VFS`

## `VimCoreSession`

`VimCoreSession` owns one embedded Vim runtime, tracks host-facing queues, and
coordinates VFS and VFD bridges.

### Ownership and concurrency

- A process may hold only one live `VimCoreSession` at a time.
- `VimCoreSession` is intentionally neither `Send` nor `Sync`.
- Dropping the session releases the global single-session lock and clears VFD
  state.
- The session is stateful. Results depend on prior commands, prior host
  actions, and prior VFS responses.

### Lifecycle and snapshot methods

- `new(initial_text: &str) -> Result<Self, CoreSessionError>`
- `new_with_options(initial_text: &str, options: CoreSessionOptions)
  -> Result<Self, CoreSessionError>`
- `snapshot(&self) -> CoreSnapshot`
- `mode(&self) -> CoreMode`
- `pending_input(&self) -> CorePendingInput`

`CoreSessionOptions` currently exposes `debug_log_path: Option<PathBuf>`. When
set, Rust-side debug log lines are appended to that file instead of stderr.

### Navigation and state methods

- `mark(&self, mark_name: char) -> Option<CoreMarkPosition>`
- `set_mark(&mut self, mark_name: char, buf_id: i32, row: usize, col: usize)
  -> Result<(), CoreCommandError>`
- `jumplist(&self) -> CoreJumpList`
- `switch_to_buffer(&mut self, buf_id: i32) -> Result<(), CoreCommandError>`
- `switch_to_window(&mut self, win_id: i32) -> Result<(), CoreCommandError>`
- `buffer_text(&self, buf_id: i32) -> Option<String>`

### Command execution methods

- `execute_normal_command(&mut self, command: &str)
  -> Result<CoreCommandTransaction, CoreCommandError>`
- `execute_ex_command(&mut self, command: &str)
  -> Result<CoreCommandTransaction, CoreCommandError>`
- `eval_string(&mut self, expr: &str) -> Option<String>`

Use `execute_normal_command` for modal editing semantics and
`execute_ex_command` for Ex behavior, especially file-like commands that route
through host policy.

### Host integration methods

- `take_pending_host_action(&mut self) -> Option<CoreHostAction>`
- `take_pending_event(&mut self) -> Option<CoreEvent>`
- `set_screen_size(&mut self, rows: i32, cols: i32)`
- `submit_vfs_response(&mut self, response: CoreVfsResponse)
  -> Result<CoreCommandOutcome, CoreCommandError>`

`submit_vfs_response()` applies one host-produced VFS response. A `Resolved`
response automatically queues a `Load` request. A successful `Saved` response
may resume a deferred quit. An unknown request ID is rejected as
`CoreCommandError::InvalidInput`.

### Buffer and window inspection methods

- `buffers(&self) -> Vec<CoreBufferInfo>`
- `windows(&self) -> Vec<CoreWindowInfo>`
- `buffer_binding(&self, buf_id: i32) -> Option<CoreBufferBinding>`
- `vfs_request_ledger(&self) -> Vec<CoreRequestEntry>`
- `vfs_transaction_log(&self) -> Vec<VfsLogEntry>`

### Register and option methods

- `register(&self, regname: char) -> Option<String>`
- `set_register(&mut self, regname: char, text: &str)`
- `get_option_number(&self, name: &str, scope: CoreOptionScope)
  -> Result<i64, CoreOptionError>`
- `get_option_bool(&self, name: &str, scope: CoreOptionScope)
  -> Result<bool, CoreOptionError>`
- `get_option_string(&self, name: &str, scope: CoreOptionScope)
  -> Result<String, CoreOptionError>`
- `set_option_number(&mut self, name: &str, value: i64, scope: CoreOptionScope)
  -> Result<(), CoreOptionError>`
- `set_option_bool(&mut self, name: &str, value: bool, scope: CoreOptionScope)
  -> Result<(), CoreOptionError>`
- `set_option_string(&mut self, name: &str, value: &str, scope: CoreOptionScope)
  -> Result<(), CoreOptionError>`

### Search, syntax, and rendering methods

- `get_search_pattern(&self) -> Option<String>`
- `is_hlsearch_active(&self) -> bool`
- `get_search_direction(&self) -> CoreSearchDirection`
- `get_search_highlights(&self, window_id: i32, start_row: i32, end_row: i32)
  -> Vec<CoreMatchRange>`
- `get_cursor_match_info(&self, window_id: i32, row: i32, col: i32,
  max_count: i32, timeout_ms: i32) -> CoreCursorMatchInfo`
- `is_incsearch_active(&self) -> bool`
- `get_incsearch_pattern(&self) -> Option<String>`
- `get_search_input_pattern(&self) -> Option<String>`
- `query_visible_search_state(&mut self, start_row: i32, end_row: i32)
  -> Result<CoreVisibleSearchState, CoreSearchQueryError>`
- `query_visible_search_state_for_window(&mut self, window_id: i32,
  start_row: i32, end_row: i32)
  -> Result<CoreVisibleSearchState, CoreSearchQueryError>`
- `search_capability_contract() -> CoreSearchCapabilityContract`
- `get_syntax_name(&self, syn_id: i32) -> Option<String>`
- `get_line_syntax(&self, win_id: i32, lnum: i64)
  -> Result<Vec<CoreSyntaxChunk>, CoreCommandError>`

Search ranges follow one fixed contract: `start_col` is inclusive, `end_col`
is exclusive, both are byte columns, and visible-state queries are limited to
the requested 1-based inclusive row range for the target window.

### Undo, backend, job, and VFD methods

- `get_undo_tree(&self, buf_id: i32) -> Result<CoreUndoTree, CoreCommandError>`
- `undo_jump(&mut self, buf_id: i32, seq: i32) -> Result<(), CoreCommandError>`
- `backend_identity(&self) -> CoreBackendIdentity`
- `inject_vfd_data(&mut self, vfd: i32, data: &[u8])
  -> Result<(), CoreCommandError>`
- `notify_job_status(&mut self, job_id: i32, status: JobStatus, exit_code: i32)
  -> Result<(), CoreCommandError>`

## Public value types

### Mode and pending-input enums

- `CoreMode`
  `Normal`, `Insert`, `Visual`, `VisualLine`, `VisualBlock`, `Replace`,
  `Select`, `SelectLine`, `SelectBlock`, `CommandLine`, `OperatorPending`
- `CorePendingInput`
  `None`, `Char`, `Replace`, `MarkSet`, `MarkJump`, `Register`

### Command and session results

- `CoreCommandOutcome`
  `NoChange`, `BufferChanged { revision }`, `CursorChanged { row, col }`,
  `ModeChanged { mode }`, `HostActionQueued`
- `CoreCommandError`
  `InvalidInput`, `OperationFailed { reason_code }`,
  `UnknownStatus { status, reason_code }`
- `CoreSessionError`
  `SessionAlreadyActive`, `InitializationFailed { reason_code }`,
  `CommandFailed(CoreCommandError)`

### Position and navigation

- `CoreMarkPosition { buf_id, row, col }`
- `CoreJumpListEntry { buf_id, row, col }`
- `CoreJumpList { current_index, entries }`

### Host-action and job types

- `CoreInputRequestKind`
  `CommandLine`, `Confirmation`, `Secret`
- `CoreJobStartRequest`
  `{ job_id, argv, cwd, vfd_in, vfd_out, vfd_err }`
- `JobStatus`
  `Running`, `Finished`, `Failed`
- `CoreHostAction`
  `VfsRequest(CoreVfsRequest)`, `Write`, `Quit`, `Redraw`, `RequestInput`,
  `Bell`, `BufAdd`, `WinNew`, `LayoutChanged`, `JobStart`, `JobStop`

### Buffer, window, snapshot, undo, syntax, PUM

- `CoreBufferInfo`
  `{ id, name, source_revision, dirty, is_active, source_kind, document_id,
  pending_vfs_operation, deferred_close, last_vfs_error }`
- `CoreWindowInfo`
  `{ id, buf_id, row, col, width, height, topline, botline, leftcol, skipcol,
  cursor_row, cursor_col, is_active }`
- `CoreSnapshot`
  `{ text, revision, dirty, mode, pending_input, cursor_row, cursor_col,
  pending_host_actions, buffers, windows, pum }`
- `CoreUndoNode`, `CoreUndoTree`
- `CoreBufferRevision { value }`
- `CoreSyntaxChunk { start_col, end_col, syn_id, name }`
- `CorePumItem { word, abbr, menu, kind, info }`
- `CorePumInfo { row, col, width, height, selected_index, items }`

### Experimental Tree-sitter structs

These structs are available only with the default-off
`experimental-tree-sitter` feature. The `tree-sitter-go`,
`tree-sitter-markdown`, `tree-sitter-rust`, and `tree-sitter-typescript`
package features enable parser and query packages for those languages.
Prepared package results use crate-owned capture mapping and return
normalized, non-overlapping chunks.
Markdown fenced blocks are detected as data-only embedded regions and carry
raw and normalized info strings. Markdown linked SVG and PNG targets are
detected as data-only media regions, and linked `*.drawio.svg` targets are SVG
media with `DrawioSvg` flavor. Markdown fenced syntax injection is
registry-driven and bounded to the fenced content range for enabled parser and
query packages.

- `CoreTextPosition { row, col }`
- `CoreTextRange { start, end }`
- `CoreTreeSitterProvenance
  { language_id, package_id, package_version, parser_version, query_version }`
- `CoreTreeSitterStatus`: `Prepared`, `Stale`, `Unavailable`, `Unsupported`,
  `Partial`, `TimedOut`, `BudgetExceeded`, `TooLarge`
- `CoreTreeSitterBudgetStatus`: `WithinBudget`, `SnapshotTooLarge`,
  `GlobalBudgetExceeded`, `MatchLimitExceeded`
- `CoreTreeSitterRangeSyntax
  { buffer_id, source_revision, provenance, status, has_error, chunks,
  covered_ranges, error_ranges, budget_status, embedded_regions }`
- `CoreTreeSitterChunk { range, capture_name, category, modifiers }`
- `CoreTreeSitterRequestId { value }`
- `CoreTreeSitterSnapshotPolicy
  { retain_latest_per_buffer, global_byte_budget, max_snapshot_bytes }`
- `CoreTreeSitterPreparationRequest
  { buffer_id, source_revision, range, vim_filetype, buffer_name,
  host_language_hint, snapshot_policy }`
- `CoreTreeSitterPreparation { request_id, buffer_id, source_revision,
  status }`
- `CoreTreeSitterPreparationResult { request_id, syntax }`
- `CoreTreeSitterSnapshotStoreStats
  { snapshot_count, pinned_snapshot_count, total_unpinned_bytes,
  snapshots }`
- `CoreTreeSitterSnapshotStoreEntry
  { buffer_id, source_revision, byte_len, pin_count }`
- `CoreSyntaxCategory` and `CoreSyntaxModifier`
- `CoreEmbeddedRegion
  { range, content_range, source, raw_info_string, normalized_info_string,
  normalized_kind, resolved_language }`
- `CoreEmbeddedBlockKind`: `Syntax`, `Diagram`, `Media`, `Unknown`
- `CoreResolvedLanguage
  { range, role, language_id, package_id, package_version, kind, confidence,
  source }`

Tree-sitter output is feature-gated and separate from `CoreSyntaxChunk`. It
does not carry Vim `syn_id` values, Vim highlight attributes, or conceal
display substitutions.
`request_tree_sitter_syntax_preparation()` creates or reuses an immutable
snapshot, parses enabled Markdown, Rust, TypeScript, or TSX packages
synchronously, commits normalized chunks and embedded region records, and
queues a result for `poll_tree_sitter_preparation()`.
`query_tree_sitter_syntax_range()` reads committed cache state only and clips
cached results to visible subranges.

### Search and message types

- `CoreMessageSeverity`
  `Info`, `Warning`, `Error`
- `CoreMessageCategory`
  `UserVisible`, `CommandFeedback`
- `CoreMessageEvent { severity, category, content }`
- `CoreMatchType`
  `Regular`, `IncSearch`, `CurSearch`
- `CoreMatchRange { start_row, start_col, end_row, end_col, match_type }`
- `MatchCountResult`
  `Calculated(usize)`, `MaxReached(usize)`, `TimedOut`
- `CoreCursorMatchInfo { is_on_match, current_match_index, total_matches }`
- `CoreSearchDirection`
  `Forward`, `Backward`
- `CoreSearchHighlightMode`
  `Disabled`, `HlSearch`, `IncSearch`
- `CoreVisibleSearchState`
  `{ window_id, start_row, end_row, mode, pattern, input_pattern,
  hlsearch_enabled, hlsearch_suspended, incsearch_active, ranges }`
- `CoreSearchCapabilityContract`
  `{ live_state_query_available, visible_rows_only, start_col_inclusive,
  end_col_exclusive }`
- `CoreSearchQueryError`
  `NoActiveWindow`, `InvalidViewport { start_row, end_row }`,
  `WindowNotFound { window_id }`

### Option types

- `CoreOptionScope`
  `Default`, `Global`, `Local`
- `CoreOptionType`
  `Bool`, `Number`, `String`
- `CoreOptionError`
  `UnknownOption`, `TypeMismatch`, `SetFailed`, `ScopeNotSupported`,
  `InternalError`

## Public VFS types

### VFS enums

- `CoreBufferSourceKind`
  `Local`, `Virtual`
- `CoreVfsOperationKind`
  `Resolve`, `Exists`, `Load`, `Save`
- `CoreDeferredClose`
  `Quit`, `SaveAndClose`, `SaveIfDirtyAndClose`
- `CoreVfsErrorKind`
  `ResolveFailed`, `ExistsFailed`, `LoadFailed`, `SaveFailed`, `NotFound`,
  `InvalidResponse`, `HostUnavailable`, `Cancelled`, `TimedOut`,
  `RevisionMismatch`

### VFS structs

- `CoreVfsError { kind, message }`
- `CorePendingVfsOperation { request_id, kind, issued_order }`
- `CoreBufferBinding { buf_id, source_kind, locator, document_id, display_name,
  committed_revision, pending_operation, deferred_close, last_saved_revision,
  last_vfs_error }`
- `CoreRequestEntry { request_id, operation_kind, target_buf_id, document_id,
  locator, base_revision, status, issued_order }`
- `VfsLogEntry { event, operation_kind, request_id, buf_id, document_id,
  locator, base_revision, current_revision, detail }`

### VFS request and response enums

- `CoreRequestStatus`
  `Pending`, `Succeeded`, `Failed(CoreVfsError)`, `Cancelled`, `TimedOut`,
  `Stale { reason }`, `ProtocolMismatch { expected, actual }`
- `CoreVfsRequest`
  `Resolve { request_id, target_buf_id, locator }`,
  `Exists { request_id, locator }`,
  `Load { request_id, target_buf_id, document_id }`,
  `Save { request_id, target_buf_id, document_id, target_locator, text,
  base_revision, force }`
- `CoreVfsResponse`
  `Resolved { request_id, document_id, display_name }`,
  `ResolvedLocalFallback { request_id, locator }`,
  `ResolvedMissing { request_id, locator }`,
  `Loaded { request_id, document_id, text }`,
  `Saved { request_id, document_id }`,
  `Failed { request_id, error }`,
  `Cancelled { request_id }`,
  `TimedOut { request_id }`

## Practical reading guidance

- Read [api-contracts.md](api-contracts.md) when signatures are not enough.
- Read [internal-api-reference.md](internal-api-reference.md) when you need the
  helper inventory behind these public methods.
