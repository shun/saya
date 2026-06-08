# API documentation index

This directory documents the complete API surface of `vim-core-rs` for
LLM-first consumption. Use these pages when you need to reason about the
crate without reopening `src/lib.rs`, `src/vfs.rs`, or `src/vfd.rs`.

Start with the repository-root `README.md` for purpose and invariants, then
`SCOPE.md` for design boundaries, then `known-limitations.md` for current
gaps. Only after that should you descend into the API reference pages.

## Rendering State Family boundary

`Search` and `Syntax` are the current rendering-state family members that the
crate exposes today. `Annotations` is the deferred placeholder for
text-property extraction, and `popupwin` is the exclusion because it is
host-owned presentation. The family is a Vim-owned read-only extraction boundary,
the authoritative source for it is the docs, tests, and classification metadata
named in `docs/SCOPE.md`, and this feature does not add a new family descriptor
or facade. It is a data-only extraction contract. It does not expose popup
layout/composition or highlight definition and attribute tables. pum stays separate from popupwin exclusion because it is completion payload extraction, not popup-window presentation.

## What each document covers

Read the pages in this order when you need the full mental model.

- The repository-root `README.md` explains repository purpose, hard
  invariants, host obligations, and the main assumptions that are unsafe.
- `SCOPE.md` defines the intended product boundary and non-goals.
- `known-limitations.md` lists current implementation gaps and intentionally
  incomplete behavior.
- `api-contracts.md` documents the behavior contracts that matter more than
  raw signatures, including session ownership, host-action flow, VFS
  sequencing, message delivery, and job bridging.
- `public-api-reference.md` documents every crate-public module, type, enum
  variant family, alias, and method that a host application can call.
- `internal-api-reference.md` documents every non-public Rust API that the
  crate uses to implement the public surface, including private helper
  functions, crate-visible coordination layers, and C ABI shims.

## Symbol map

This section gives you a fast lookup path before you jump into the detailed
references.

### Public root symbols

The crate root exports these user-facing symbols.

- Module: `ffi`
- Session type: `VimCoreSession`
- Enums: `CoreMode`, `CorePendingInput`, `CoreCommandOutcome`,
  `CoreInputRequestKind`, `CoreBackendIdentity`, `CoreRuntimeMode`,
  `CoreOptionScope`, `CoreOptionType`, `CoreOptionError`, `JobStatus`,
  `CoreHostAction`, `CoreMessageSeverity`, `CoreMessageCategory`,
  `CorePagerPromptKind`, `CoreEvent`,
  `CoreMatchType`, `MatchCountResult`, `CoreSearchDirection`,
  `CoreCommandError`, `CoreSessionError`
- Structs: `CoreMarkPosition`, `CoreJumpListEntry`, `CoreJumpList`,
  `CoreJobStartRequest`, `CoreBufferRevision`, `CoreBufferInfo`,
  `CoreWindowInfo`, `CoreUndoNode`, `CoreUndoTree`, `CoreSyntaxChunk`,
  `CoreMessageEvent`,
  `CoreCommandTransaction`, `CoreSessionOptions`, `CorePumItem`, `CorePumInfo`,
  `CoreMatchRange`, `CoreCursorMatchInfo`, `CoreSnapshot`,
  `CoreLightSnapshot`, `CoreBufferLineRange`
- Tree-sitter syntax symbols, available only with `tree-sitter-syntax`
  or the compatibility alias `experimental-tree-sitter`: `CoreTextPosition`,
  `CoreTextRange`,
  `CoreTreeSitterProvenance`, `CoreTreeSitterStatus`,
  `CoreTreeSitterRangeSyntax`, `CoreTreeSitterChunk`,
  `CoreTreeSitterRequestId`, `CoreTreeSitterPreparationRequest`,
  `CoreTreeSitterPreparation`, `CoreTreeSitterPreparationResult`,
  `CoreTreeSitterSnapshotPolicy`, `CoreTreeSitterSnapshotStoreEntry`,
  `CoreTreeSitterSnapshotStoreStats`,
  `CoreSyntaxCategory`, `CoreSyntaxModifier`, `CoreLanguageRole`,
  `CoreLanguageResolutionSource`, `CoreResolutionConfidence`,
  `CoreResolvedLanguage`, `CoreEmbeddedRegionSource`, `CoreDiagramKind`,
  `CoreMediaKind`, `CoreMediaFlavor`, `CoreEmbeddedBlockKind`, and
  `CoreEmbeddedRegion`
- Re-exported VFS enums and structs: `CoreBufferBinding`,
  `CoreBufferSourceKind`, `CoreDeferredClose`, `CorePendingVfsOperation`,
  `CoreRequestEntry`, `CoreRequestStatus`, `CoreVfsError`,
  `CoreVfsErrorKind`, `CoreVfsOperationKind`, `CoreVfsRequest`,
  `CoreVfsResponse`, `VfsLogEntry`, `VfsLogEvent`

### Public `VimCoreSession` methods

The public session methods are grouped by role.

- Lifecycle and snapshots: `new`, `new_with_options`, `snapshot`, `mode`,
  `runtime_mode`, `pending_input`
- Navigation and state writes: `mark`, `set_mark`, `jumplist`,
  `switch_to_buffer`, `switch_to_window`, `buffer_text`
- Command execution: `execute_normal_command`, `dispatch_key`,
  `execute_ex_command`, `eval_string`
- Host integration: `take_pending_host_action`, `take_pending_event`,
  `set_screen_size`, `submit_vfs_response`
- Buffer and window inspection: `buffers`, `windows`, `active_window_id`,
  `buffer_binding`, `vfs_request_ledger`, `vfs_transaction_log`
- Registers and options: `register`, `set_register`, `get_option_number`,
  `get_option_bool`, `get_option_string`, `set_option_number`,
  `set_option_bool`, `set_option_string`
- Search and syntax: `get_search_pattern`, `is_hlsearch_active`,
  `get_search_direction`, `get_search_highlights`,
  `get_cursor_match_info`, `is_incsearch_active`,
  `get_incsearch_pattern`, `get_search_input_pattern`,
  `query_visible_search_state`, `query_visible_search_state_for_window`,
  `search_capability_contract`, `get_syntax_name`, `get_line_syntax`
- These accessors cover the current `Search` and `Syntax` family members.
  The Search family summary includes inactive window queries, byte columns,
  and `incsearch` state as contract data. `textprop` is the deferred
  placeholder in `Annotations`, and popup ownership stays host-owned
  presentation because the crate does not expose a public popupwin extractor
  or highlight-table extractor.
- `search_capability_contract` is the typed summary for this Search family
  boundary. It reports live-state availability, inactive-window support,
  byte-column semantics, data-only payload rules, and host-owned
  presentation.
- Undo and backend metadata: `get_undo_tree`, `undo_jump`,
  `backend_identity`
- Job and VFD bridge helpers: `inject_vfd_data`, `notify_job_status`

### Tree-sitter syntax surface

The `tree-sitter-syntax` feature adds type definitions for a separate stable
opt-in Tree-sitter extraction surface. The feature is default-off, and the
`tree-sitter-go`, `tree-sitter-markdown`, `tree-sitter-rust`, and
`tree-sitter-typescript` package features opt into that surface with optional
parser and query packages. The older `experimental-tree-sitter` feature
remains as a compatibility alias for `tree-sitter-syntax`. Host applications
must use `tree-sitter-syntax` for new integrations and migrate preview
integrations to that feature name. No alias removal release is scheduled.

The Tree-sitter surface is separate from `CoreSyntaxChunk` and
`get_line_syntax()`. It carries crate-owned `source_revision` provenance,
package and query versions, explicit preparation status, byte ranges, capture
names, normalized categories and modifiers, coverage ranges, error ranges,
budget state, and data-only embedded region records. Markdown fenced blocks
are detected as embedded regions with raw and normalized info strings.
Markdown linked SVG and PNG targets are detected as media embedded regions
with the raw target preserved on the region record. Linked `*.drawio.svg`
targets are SVG media with `DrawioSvg` flavor. Markdown fenced syntax
injection is registry-driven and bounded to the fenced content range for
languages with enabled parser and query packages, including Go, TypeScript,
and TSX. It does not expose Vim syntax IDs, Vim highlight attributes, or
conceal substitutions.

`tree_sitter_language_packages()` exposes the feature-enabled built-in package
registry. `resolve_tree_sitter_root_language()` uses Vim `filetype`, buffer
name, and an optional host hint as inputs.
`resolve_tree_sitter_embedded_language()` uses Markdown info strings as
embedded-region inputs. The resolver returns explicit `Resolved`,
`Unavailable`, or `Unsupported` states and keeps final language selection
inside `vim-core-rs`. `request_tree_sitter_syntax_preparation()` also returns
Markdown fenced-block and linked SVG/PNG media embedded region records as
data-only extraction.

`request_tree_sitter_syntax_preparation()` adds the request/response
preparation shape. The implementation creates or reuses immutable text
snapshots keyed by `(buffer_id, source_revision)`, pins queued or running
requests, parses enabled Markdown, Rust, TypeScript, or TSX packages
synchronously, normalizes captures into non-overlapping chunks, and queues
completed results for `poll_tree_sitter_preparation()`.
`query_tree_sitter_syntax_range()` reads committed cache state only and clips
cached results to visible subranges.
`tree_sitter_snapshot_store_stats()` exposes retention diagnostics for tests
and host debugging, including pinned counts and unpinned byte usage.

Host renderers request preparation, poll completion, render only `Prepared`
results whose `source_revision` matches the current buffer, and use later
queries as committed-cache reads. `Unavailable`, `Unsupported`, `Stale`,
`TooLarge`, `BudgetExceeded`, `TimedOut`, and `Partial` are explicit
diagnostic states, not fresh highlight. Normalized `category` and `modifiers`
are the primary styling contract; `capture_name` is provenance and optional
advanced styling metadata.

Conservative hosts render only `Prepared` results with matching
`CoreBufferInfo.source_revision`. Hosts that support degraded rendering may
render `Partial` chunks clipped to `covered_ranges` when
`budget_status == MatchLimitExceeded`, and may render valid parser output
outside `error_ranges` when `has_error` is true. Host render caches must use a
buffer-local key such as `(buffer_id, source_revision, visible_range)` and must
not substitute the session-wide `CoreSnapshot.revision`.

### Internal-only symbols

The internal reference breaks non-public APIs into these areas.

- `src/lib.rs`: `ParsedExIntent`, `invoke_native_normal_command`,
  `invoke_native_ex_command`, `apply_intent`, `apply_write_intent`,
  `apply_loaded_buffer`, `drain_native_host_actions`, `drain_native_events`,
  `get_option_value`, conversion helpers, and parser helpers
- `src/vfs.rs`: `DocumentCoordinator`, `BufferState`,
  `CoreResponseApplyOutcome`, and `CoreVfsResponse` helper methods
- `src/vfd.rs`: `pollfd`, `POLLIN`, `POLLOUT`, `VfdState`, `JobState`,
  `VfdManager`, `get_manager`, and the `vim_core_*` exported C functions

## Reachability rules

This section explains how to interpret "public" versus "internal" in this
crate, because `src/vfd.rs` contains `pub` items inside a private module.

- A symbol counts as public if a downstream crate can name it through the
  crate root.
- A symbol counts as internal if it is only visible inside this crate, even
  when it uses the Rust `pub` keyword in a private module.
- The `ffi` module is public, but it is intentionally narrow. It exists to
  expose POD types and constants that tests and hosts need for FFI contracts.

## Next steps

Use `public-api-reference.md` when you need callable surface area. Use
`internal-api-reference.md` when you need implementation boundaries. Use
`api-contracts.md` when you need sequencing, invariants, or host obligations.
