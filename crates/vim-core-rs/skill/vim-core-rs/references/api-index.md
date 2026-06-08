# vim-core-rs bundled API index

This file is the standalone entry point for the bundled API documentation in
this skill. Use it when the skill is copied outside the repository and you
still need the full mental model of the crate.

## What each bundled document covers

Read the bundled pages in this order when you need complete coverage.

- `readme-summary.md`
  Documents repository purpose, invariants, host obligations, and dangerous
  assumptions.
- `architecture.md`
  Documents design boundaries and non-goals.
- `known-limitations.md`
  Documents current implementation gaps and intentionally incomplete behavior.
- `public-api-reference.md`
  Documents the crate-public surface that a host application can call.
- `internal-api-reference.md`
  Documents non-public Rust APIs, coordination layers, and internal C ABI
  shims that matter when maintaining the repository.
- `api-contracts.md`
  Documents sequencing, invariants, and host obligations that matter more than
  raw signatures.

## Fast symbol map

### Public root symbols

- Module: `ffi`
- Session type: `VimCoreSession`
- Enums: `CoreMode`, `CorePendingInput`, `CoreCommandOutcome`,
  `CoreInputRequestKind`, `CoreBackendIdentity`, `CoreOptionScope`,
  `CoreOptionType`, `CoreOptionError`, `JobStatus`, `CoreHostAction`,
  `CoreMessageSeverity`, `CoreMessageCategory`, `CoreMatchType`,
  `MatchCountResult`,
  `CoreSearchDirection`, `CoreCommandError`, `CoreSessionError`
- Structs: `CoreMarkPosition`, `CoreJumpListEntry`, `CoreJumpList`,
  `CoreJobStartRequest`, `CoreBufferRevision`, `CoreBufferInfo`,
  `CoreWindowInfo`, `CoreUndoNode`, `CoreUndoTree`, `CoreSyntaxChunk`,
  `CoreMessageEvent`,
  `CoreCommandTransaction`, `CorePumItem`, `CorePumInfo`, `CoreMatchRange`,
  `CoreCursorMatchInfo`, `CoreSessionOptions`, `CoreSnapshot`
- Experimental Tree-sitter symbols, only with `experimental-tree-sitter`:
  `CoreTextPosition`, `CoreTextRange`, `CoreTreeSitterProvenance`,
  `CoreTreeSitterStatus`, `CoreTreeSitterBudgetStatus`,
  `CoreTreeSitterRangeSyntax`, `CoreTreeSitterChunk`,
  `CoreTreeSitterRequestId`, `CoreTreeSitterPreparationRequest`,
  `CoreTreeSitterPreparation`, `CoreTreeSitterPreparationResult`,
  `CoreTreeSitterSnapshotPolicy`, `CoreTreeSitterSnapshotStoreEntry`,
  `CoreTreeSitterSnapshotStoreStats`, `CoreSyntaxCategory`,
  `CoreSyntaxModifier`, `CoreResolvedLanguage`, and `CoreEmbeddedRegion`
- Re-exported VFS items: `CoreBufferBinding`, `CoreBufferSourceKind`,
  `CoreDeferredClose`, `CorePendingVfsOperation`, `CoreRequestEntry`,
  `CoreRequestStatus`, `CoreVfsError`, `CoreVfsErrorKind`,
  `CoreVfsOperationKind`, `CoreVfsRequest`, `CoreVfsResponse`,
  `VfsLogEntry`, `VfsLogEvent`

### Public `VimCoreSession` methods

- Lifecycle and snapshots: `new`, `new_with_options`, `snapshot`, `mode`,
  `pending_input`
- Navigation and state writes: `mark`, `set_mark`, `jumplist`,
  `switch_to_buffer`, `switch_to_window`, `buffer_text`
- Command execution: `execute_normal_command`, `execute_ex_command`,
  `eval_string`
- Host integration: `take_pending_host_action`, `take_pending_event`,
  `set_screen_size`, `submit_vfs_response`
- Buffer and window inspection: `buffers`, `windows`, `buffer_binding`,
  `vfs_request_ledger`, `vfs_transaction_log`
- Registers and options: `register`, `set_register`, `get_option_number`,
  `get_option_bool`, `get_option_string`, `set_option_number`,
  `set_option_bool`, `set_option_string`
- Search and syntax: `get_search_pattern`, `is_hlsearch_active`,
  `get_search_direction`, `get_search_highlights`,
  `get_cursor_match_info`, `is_incsearch_active`,
  `get_incsearch_pattern`, `get_search_input_pattern`,
  `query_visible_search_state`, `query_visible_search_state_for_window`,
  `search_capability_contract`, `get_syntax_name`, `get_line_syntax`
- Undo and backend metadata: `get_undo_tree`, `undo_jump`,
  `backend_identity`
- Job and VFD bridge helpers: `inject_vfd_data`, `notify_job_status`

### Experimental Tree-sitter surface

The `experimental-tree-sitter` feature is default-off. It exposes a separate
Tree-sitter syntax surface and package features for Markdown, Rust,
TypeScript, and TSX parser/query packages.
Tree-sitter output stays separate from `get_line_syntax()` and
`CoreSyntaxChunk`; it uses source revisions, package/query provenance,
explicit statuses, byte ranges, capture names, normalized categories and
modifiers, coverage ranges, error ranges, budget state, and embedded region
records. Markdown fenced blocks are detected as embedded regions with raw and
normalized info strings. Markdown linked SVG and PNG targets are detected as
data-only media regions, and linked `*.drawio.svg` targets are SVG media with
`DrawioSvg` flavor.
Phase 4 adds request, poll, and cache-query preparation methods plus an
immutable text snapshot store with in-flight pinning, latest-N-per-buffer
retention, a global byte budget, and explicit `TooLarge` or `BudgetExceeded`
statuses.
Phase 5 adds synchronous Markdown and Rust parsing, crate-owned capture
mapping with explicit priorities, normalized non-overlapping chunks, and
visible range reads from committed cache only.
Phase 6 adds data-only Markdown fenced-block embedded region records and does
not perform child syntax injection.
Phase 7 adds data-only Markdown linked SVG/PNG media region records and keeps
all rendering and decoding host-owned.
Phase 8 adds TypeScript and TSX packages, result coverage and budget fields,
and bounded Markdown fenced syntax injection. Later package additions, such as
Go, use the same registry-driven embedded extraction path.

### Internal-only areas

- `src/lib.rs`
  `ParsedExIntent`, `invoke_native_normal_command`,
  `invoke_native_ex_command`, `apply_intent`,
  `apply_write_intent`, `apply_loaded_buffer`, `drain_native_host_actions`,
  `drain_native_events`, `get_option_value`, and conversion helpers
- `src/vfs.rs`
  `DocumentCoordinator`, `BufferState`, `CoreResponseApplyOutcome`, and
  response helper methods
- `src/vfd.rs`
  `pollfd`, `POLLIN`, `POLLOUT`, `VfdState`, `JobState`, `VfdManager`,
  `get_manager`, and `vim_core_*` exported shims

## How to choose the next document

- If you need callable surface area as a crate user, read
  [public-api-reference.md](public-api-reference.md).
- If you need implementation boundaries as a maintainer, read
  [internal-api-reference.md](internal-api-reference.md).
- If you need sequencing or invariants, read
  [api-contracts.md](api-contracts.md).
- If you need current caveats before trusting an exposed API, read
  [known-limitations.md](known-limitations.md).
