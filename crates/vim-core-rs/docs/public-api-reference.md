# Public API reference

This document describes the full crate-public API of `vim-core-rs`. It is
written so that an LLM or host developer can understand the callable surface,
the data model, and the behavioral intent without reopening the source code.

## Public surface summary

The public surface is intentionally concentrated in one place.

- The crate root exposes `VimCoreSession` as the main stateful facade.
- The rendering-state family is an additive grouping over existing
  `VimCoreSession` accessors, not a new runtime owner and not a replacement
  surface. It is a Vim-owned read-only extraction boundary, has no new family descriptor, and this feature does not add a new family descriptor or facade.
- The crate root also exposes plain data types that describe snapshots,
  transactions, events, host actions, VFS contracts, options, undo trees,
  and rendering data.
- The `ffi` module exposes a small FFI-facing contract for POD structs and
  VFS-related constants.
- `src/vfs.rs` contributes public types through crate-root re-exports. The
  `vfs` module itself is not public.

## Public module: `ffi`

The `ffi` module exists for narrow interop. It is not the preferred host API
for normal Rust callers, but it is part of the stable crate surface.

### Re-exported POD structs

- `vim_core_buffer_commit_t`: The FFI struct used when the Rust side applies a
  loaded or saved buffer payload back into the embedded Vim runtime.
- `vim_core_buffer_info_t`: The raw FFI buffer metadata struct used by bridge
  code and some contract tests.

### Exported constants

These constants mirror the C enum values that describe VFS operations and
buffer source kinds.

- `VIM_CORE_VFS_OPERATION_NONE`
- `VIM_CORE_VFS_OPERATION_RESOLVE`
- `VIM_CORE_VFS_OPERATION_EXISTS`
- `VIM_CORE_VFS_OPERATION_LOAD`
- `VIM_CORE_VFS_OPERATION_SAVE`
- `VIM_CORE_BUFFER_SOURCE_LOCAL`
- `VIM_CORE_BUFFER_SOURCE_VFS`

## Session type: `VimCoreSession`

`VimCoreSession` is the main public object. It owns one embedded Vim runtime,
tracks host-facing queues, and coordinates VFS and VFD bridges.

### Ownership and concurrency model

You must understand these constraints before using any method.

- The process may hold only one live `VimCoreSession` at a time.
- `VimCoreSession` is intentionally neither `Send` nor `Sync`.
- Dropping the session releases the global single-session lock and clears VFD
  state.
- The session is stateful. Method results depend on prior commands, prior host
  actions, and prior VFS responses.

### Lifecycle and snapshot methods

These methods create the session and extract high-level state.

- `new(initial_text: &str) -> Result<Self, CoreSessionError>`
  Creates a new embedded Vim session seeded with `initial_text`. It fails with
  `CoreSessionError::SessionAlreadyActive` if another session is still alive.
- `new_with_options(initial_text: &str, options: CoreSessionOptions)
  -> Result<Self, CoreSessionError>`
  Creates a session with explicit runtime and debug-log options. The current
  implementation supports `CoreRuntimeMode::Embedded` and rejects
  `Standalone`.
- `snapshot(&self) -> CoreSnapshot`
  Returns a coherent state capture. It includes full active-buffer text,
  revision, dirty state, mode, pending input, active-window cursor position,
  pending host-action count, buffer metadata, window metadata, and pop-up menu
  state. Use it for compatibility paths that need full text.
- `light_snapshot(&self) -> CoreLightSnapshot`
  Returns the snapshot metadata without materializing active-buffer text. Use
  it for normal render and refresh paths that only need mode, cursor, buffer,
  window, pending input, pending host-action, and pop-up menu state.
- `mode(&self) -> CoreMode`
  Returns the current mode. It is equivalent to `snapshot().mode` but cheaper
  to consume conceptually.
- `runtime_mode(&self) -> CoreRuntimeMode`
  Returns the active runtime-mode contract for the session.
- `pending_input(&self) -> CorePendingInput`
  Returns whether Vim is waiting for another keystroke category, such as a
  register name or a mark target.

### Navigation and cursor-adjacent methods

These methods let you inspect or update navigation state.

- `mark(&self, mark_name: char) -> Option<CoreMarkPosition>`
  Returns the mark location if the mark is set.
- `set_mark(&mut self, mark_name: char, buf_id: i32, row: usize, col: usize)
  -> Result<(), CoreCommandError>`
  Sets a mark programmatically.
- `jumplist(&self) -> CoreJumpList`
  Returns the current jumplist and the current index within it.
- `switch_to_buffer(&mut self, buf_id: i32) -> Result<(), CoreCommandError>`
  Changes the active buffer.
- `switch_to_window(&mut self, win_id: i32) -> Result<(), CoreCommandError>`
  Changes the active window.
- `buffer_text(&self, buf_id: i32) -> Option<String>`
  Returns the full text content of one buffer.
- `buffer_line_range(&self, buf_id: i32, start_row: usize, line_count: usize)
  -> Option<CoreBufferLineRange>`
  Returns only the requested zero-based row range for one buffer, plus
  `total_line_count` and the buffer-local `source_revision`.

### Command execution methods

These methods mutate editor state or inspect Vimscript-level behavior.

- `execute_normal_command(&mut self, command: &str)
  -> Result<CoreCommandTransaction, CoreCommandError>`
  Executes a Normal-mode key sequence and returns the transaction result that
  includes the final snapshot, emitted events, and emitted host actions.
- `dispatch_key(&mut self, key: &str)
  -> Result<CoreCommandTransaction, CoreCommandError>`
  Dispatches one typed key event through Vim's current mode. Normal, Visual,
  and Operator-pending modes report pending command prefixes through
  `pending_input`. Insert mode treats printable keys as literal input, leaves
  `pending_input` empty, and treats `"\x08"` as Backspace for one-key host
  input.
- `execute_ex_command(&mut self, command: &str)
  -> Result<CoreCommandTransaction, CoreCommandError>`
  Executes an Ex command. The crate intercepts file-oriented commands such as
  `:edit`, `:write`, `:update`, `:wq`, `:xit`, and `:quit` so it can route
  file operations through VFS or host actions instead of blindly delegating to
  native Vim file I/O.
- `eval_string(&mut self, expr: &str) -> Option<String>`
  Evaluates a Vimscript expression and returns the result as a string if the
  bridge returns one.

### Host integration methods

These methods connect the session to the application that embeds it.

- `take_pending_host_action(&mut self) -> Option<CoreHostAction>`
  Drains newly emitted native host actions into the Rust queue, then pops one
  action in FIFO order. Call this repeatedly until it returns `None`.
- `take_pending_event(&mut self) -> Option<CoreEvent>`
  Drains newly emitted native events into the Rust queue, then pops one event
  in FIFO order. Call this repeatedly until it returns `None` when you consume
  events outside a transaction result.
- `set_screen_size(&mut self, rows: i32, cols: i32)`
  Updates the runtime with the host UI dimensions.
- `submit_vfs_response(&mut self, response: CoreVfsResponse)
  -> Result<CoreCommandOutcome, CoreCommandError>`
  Applies one host-produced VFS response. A `Resolved` response automatically
  queues a `Load` request. A successful `Saved` response may resume a deferred
  quit. An unknown request ID is rejected as `CoreCommandError::InvalidInput`.

### Buffer and window inspection methods

These methods extract structural information about the editor state.

- `buffers(&self) -> Vec<CoreBufferInfo>`
  Returns the buffer list from the latest snapshot.
- `windows(&self) -> Vec<CoreWindowInfo>`
  Returns the window list from the latest snapshot, including geometry,
  viewport state, active-window state, and per-window cursor state.
- `active_window_id(&self) -> Option<i32>`
  Returns the canonical `window_id` for the active window. Use this when you
  need active-pane resolution without scanning the window list in host code.
  When you already hold a `CoreSnapshot`, prefer `snapshot.active_window_id()`
  so the identity lookup stays within one coherent read.
  Treat `None` as an explicit no-active-window case, not as a signal to fall
  back to a synthetic host default.
- `buffer_binding(&self, buf_id: i32) -> Option<CoreBufferBinding>`
  Returns VFS binding metadata for one buffer after synchronizing bindings with
  the current snapshot.
- `vfs_request_ledger(&self) -> Vec<CoreRequestEntry>`
  Returns the full request ledger, including completed, failed, stale, and
  timed-out requests.
- `vfs_transaction_log(&self) -> Vec<VfsLogEntry>`
  Returns the chronological VFS transaction log used for diagnostics.

### Register and option methods

These methods expose register state and typed option accessors.

- `register(&self, regname: char) -> Option<String>`
  Returns the full contents of a register, including multiline text and
  embedded newlines.
- `set_register(&mut self, regname: char, text: &str)`
  Replaces the register contents.
- `get_option_number(&self, name: &str, scope: CoreOptionScope)
  -> Result<i64, CoreOptionError>`
  Returns a numeric option after validating the expected type.
- `get_option_bool(&self, name: &str, scope: CoreOptionScope)
  -> Result<bool, CoreOptionError>`
  Returns a boolean option after validating the expected type.
- `get_option_string(&self, name: &str, scope: CoreOptionScope)
  -> Result<String, CoreOptionError>`
  Returns a string option after validating the expected type.
- `set_option_number(&mut self, name: &str, value: i64,
  scope: CoreOptionScope) -> Result<(), CoreOptionError>`
  Writes a numeric option.
- `set_option_bool(&mut self, name: &str, value: bool,
  scope: CoreOptionScope) -> Result<(), CoreOptionError>`
  Writes a boolean option. Internally, the crate routes this through the
  numeric setter with `0` and `1`.
- `set_option_string(&mut self, name: &str, value: &str,
  scope: CoreOptionScope) -> Result<(), CoreOptionError>`
  Writes a string option.

### Search, syntax, and rendering methods

These methods expose rendering-relevant state that the host can draw directly.

- `get_search_pattern(&self) -> Option<String>`
  Returns the current search pattern if one exists.
- `is_hlsearch_active(&self) -> bool`
  Returns whether persistent search highlighting is active.
- `get_search_direction(&self) -> CoreSearchDirection`
  Returns the current search direction.
- `get_search_highlights(&self, window_id: i32, start_row: i32,
  end_row: i32) -> Vec<CoreMatchRange>`
  Returns search matches for a row range in a specific window.
- `get_cursor_match_info(&self, window_id: i32, row: i32, col: i32,
  max_count: i32, timeout_ms: i32) -> CoreCursorMatchInfo`
  Returns cursor-relative match count metadata, including timeout and
  max-count saturation signals.
- `is_incsearch_active(&self) -> bool`
  Returns whether incremental search is active.
- `get_incsearch_pattern(&self) -> Option<String>`
  Returns the incremental search pattern if one exists.
- `get_search_input_pattern(&self) -> Option<String>`
  Returns the current `/` or `?` command-line pattern even when `incsearch`
  is disabled.
- `query_visible_search_state(&mut self, start_row: i32, end_row: i32)
  -> Result<CoreVisibleSearchState, CoreSearchQueryError>`
  Returns the active window's search state for a 1-based inclusive viewport.
- `query_visible_search_state_for_window(&mut self, window_id: i32,
  start_row: i32, end_row: i32)
  -> Result<CoreVisibleSearchState, CoreSearchQueryError>`
  Returns the specified window's search state for a 1-based inclusive
  viewport, including inactive window queries. This is a live viewport query
  and keeps the inactive-window query wording explicit.
- `search_capability_contract() -> CoreSearchCapabilityContract`
  Returns the stable Search family contract for live search extraction. This
  is a data-only extraction contract.
- `get_syntax_name(&self, syn_id: i32) -> Option<String>`
  Maps a syntax ID to its human-readable syntax group name.
- `get_line_syntax(&self, win_id: i32, lnum: i64)
  -> Result<Vec<CoreSyntaxChunk>, CoreCommandError>`
  Returns syntax chunks for one line in one window. This keeps Syntax as
  line-scoped extraction.

Search ranges now follow one fixed contract: `start_col` is inclusive and
`end_col` is exclusive, both are byte columns, and results are limited to the
requested visible rows. Hosts can call
`query_visible_search_state_for_window()` for inactive window viewports
without promoting a new rendering owner. `CoreVisibleSearchState` also
separates
`hlsearch_enabled`, `hlsearch_suspended`, `incsearch_active`, `pattern`,
`input_pattern`, and per-range `CoreMatchType` values so hosts can render
persistent matches, incremental preview matches, and the current match
without reimplementing Vim search semantics.

This is the current Search family extraction boundary for `VimCoreSession`.
It keeps inactive window queries, byte columns, and `incsearch` preview state
inside the Search family contract while leaving popup ownership as
host-owned presentation. `textprop` is the deferred placeholder in the
`Annotations` family, and the crate does not expose a public popupwin
extractor or a public textprop extractor.
In plain terms, textprop is the deferred placeholder until a narrower
annotation contract exists.

`search_capability_contract()` is the structured summary of that boundary.
Its fields report live-state availability, inactive-window query support,
visible-row scoping, inclusive/exclusive byte-column semantics, the data-only
payload guarantee, and the host-owned presentation boundary.

In plain terms, start_col is inclusive and end_col is exclusive.
The Search family member keeps &mut self because it issues a live viewport
query against session state and preserves inactive-window query behavior.
The Syntax family member keeps &self because it is read-only line-scoped
extraction over existing state.

The rendering-state family keeps `popupwin` outside the family because
popupwin is host-owned presentation. `popupwin` is the exclusion, and popup
placement, popup composition, popup borders, and overlay layout stay out of
scope for this crate.
pum stays separate from popupwin exclusion because it is completion payload
extraction, not popup-window presentation.
The crate does not expose a public popupwin extractor.
The crate does not expose a public textprop extractor.
The crate does not expose highlight definition tables or resolved highlight
attribute tables as a public extraction surface.

### Undo and backend methods

These methods expose runtime metadata that is not part of normal text
execution.

- `get_undo_tree(&self, buf_id: i32)
  -> Result<CoreUndoTree, CoreCommandError>`
  Returns the undo tree for one buffer.
- `undo_jump(&mut self, buf_id: i32, seq: i32)
  -> Result<(), CoreCommandError>`
  Moves the buffer to a specific undo node sequence number.
- `backend_identity(&self) -> CoreBackendIdentity`
  Returns whether the embedded runtime is the real upstream runtime or a
  bridge stub.

### Job and VFD bridge methods

These methods support host-managed jobs and virtual file descriptors.

- `inject_vfd_data(&mut self, vfd: i32, data: &[u8])
  -> Result<(), CoreCommandError>`
  Pushes process output bytes into one VFD queue.
- `notify_job_status(&mut self, job_id: i32, status: JobStatus,
  exit_code: i32) -> Result<(), CoreCommandError>`
  Updates the status of a host-managed job. When the status is terminal, the
  crate closes the associated VFDs to signal EOF to Vim.

## Public value types

The public value types are plain data carriers. They exist so hosts can reason
about state without touching raw FFI.

### Mode and pending-input enums

These enums describe the editor input state.

- `CoreMode`: `Normal`, `Insert`, `Visual`, `VisualLine`, `VisualBlock`,
  `Replace`, `Select`, `SelectLine`, `SelectBlock`, `CommandLine`,
  `OperatorPending`
- `CorePendingInput`: `None`, `Char`, `Replace`, `MarkSet`, `MarkJump`,
  `Register`
- `CoreRuntimeMode`: `Embedded`, `Standalone`

### Transaction, event, and host-action types

These types describe the observable result surface for embedded execution.

- `CoreCommandTransaction { outcome, snapshot, events, host_actions }`
- `CoreEvent`: `Message(CoreMessageEvent)`,
  `PagerPrompt(CorePagerPromptKind)`, `Bell`,
  `Redraw { full, clear_before_draw }`, `BufferAdded { buf_id }`,
  `WindowCreated { win_id }`, `LayoutChanged`
- `CoreMessageSeverity`: `Info`, `Warning`, `Error`
- `CoreMessageCategory`: `UserVisible`, `CommandFeedback`
- `CorePagerPromptKind`: `More`, `HitReturn`
- `CoreHostAction`: `VfsRequest`, `Write`, `Quit`, `Redraw`,
  `RequestInput`, `Bell`, `JobStart`, `JobWrite`, `JobStop`

### Session configuration types

These types configure session startup behavior.

- `CoreSessionOptions { runtime_mode, debug_log_path }`

### Position and navigation structs

These structs describe marks and jump locations.

- `CoreMarkPosition { buf_id, row, col }`
- `CoreJumpListEntry { buf_id, row, col }`
- `CoreJumpList { current_index, entries }`

### Command and session result enums

These enums define the common result vocabulary of public methods.

- `CoreCommandOutcome`
  - `NoChange`
  - `BufferChanged { revision }`
  - `CursorChanged { row, col }`
  - `ModeChanged { mode }`
  - `HostActionQueued`
- `CoreCommandError`
  - `InvalidInput`
  - `OperationFailed { reason_code }`
  - `UnknownStatus { status, reason_code }`
- `CoreSessionError`
  - `SessionAlreadyActive`
  - `InitializationFailed { reason_code }`
  - `CommandFailed(CoreCommandError)`

### Host-action types

These types define the host-driven side effects that the core cannot execute
on its own.

- `CoreInputRequestKind`: `CommandLine`, `Confirmation`, `Secret`
- `CoreJobStartRequest { job_id, argv, cwd, vfd_in, vfd_out, vfd_err }`
- `JobStatus`: `Running`, `Finished`, `Failed`
- `CoreHostAction`
  - `VfsRequest(CoreVfsRequest)`
  - `Write { path, force, issued_after_revision }`
  - `Quit { force, issued_after_revision }`
  - `Redraw { full, clear_before_draw }`
  - `RequestInput { prompt, input_kind, correlation_id }`
  - `Bell`
  - `JobStart(CoreJobStartRequest)`
  - `JobWrite { vfd, data }`
  - `JobStop { job_id }`

### Buffer, window, and snapshot structs

These structs expose the current editor layout.

- `CoreBufferInfo`
  - `id`
  - `name`
  - `source_revision`
  - `dirty`
  - `is_active`
  - `source_kind`
  - `document_id`
  - `pending_vfs_operation`
  - `deferred_close`
  - `last_vfs_error`
- `CoreWindowInfo`
  - `id`
  - `buf_id`
  - `row`
  - `col`
  - `width`
  - `height`
  - `topline`
  - `botline`
  - `leftcol`
  - `skipcol`
  - `cursor_row`
  - `cursor_col`
  - `is_active`
- `CoreSnapshot`
  - `text`
  - `revision`
  - `dirty`
  - `mode`
  - `pending_input`
  - `cursor_row`
  - `cursor_col`
  - `pending_host_actions`
  - `buffers`
  - `windows`
  - `pum`
  - helper methods: `active_window()`, `active_window_id()`,
    `window(window_id)`

`CoreSnapshot.windows` is the full per-window contract. Use
`CoreWindowInfo.id` as the stable identity, `is_active` or
`active_window_id()` for focus, and the viewport fields (`topline`, `botline`,
`leftcol`, and `skipcol`) for window-local rendering state.
Treat `None` from `active_window_id()` and `window(window_id)` as explicit
contract results. Do not replace them with host-side fallbacks such as "first
window" or a hard-coded ID.
When you already hold a snapshot, use `window(window_id)` as the standard
lookup path for per-window projection instead of rescanning or inferring by
geometry.

### Undo, syntax, and completion structs

These structs feed editor UI and history views.

- `CoreUndoNode { seq, time, save_nr, prev_seq, next_seq, alt_next_seq,
  alt_prev_seq, is_newhead, is_curhead }`
- `CoreUndoTree { nodes, synced, seq_last, save_last, seq_cur, time_cur,
  save_cur }`
- `CoreBufferRevision { value }`
- `CoreSyntaxChunk { start_col, end_col, syn_id, name }`
- `CorePumItem { word, abbr, menu, kind, info }`
- `CorePumInfo { row, col, width, height, selected_index, items }`

### Tree-sitter syntax structs

The `tree-sitter-syntax` Cargo feature exposes the stable opt-in
Tree-sitter extraction surface. The feature is default-off, and the
`tree-sitter-go`, `tree-sitter-markdown`, `tree-sitter-rust`, and
`tree-sitter-typescript` package features enable parser and query packages for
those languages. `tree-sitter-typescript` registers both TypeScript and TSX
packages. Enabled package features are the only packages registered by the
built-in registry.
The older `experimental-tree-sitter` feature remains as a compatibility alias
for `tree-sitter-syntax`. Host applications must prefer
`tree-sitter-syntax` for new integrations and migrate existing preview
integrations to that feature name. No alias removal release is scheduled; if
the alias is removed in a later release, that removal must be documented as a
breaking change.

These types are feature-gated and separate from `CoreSyntaxChunk`. They model
Tree-sitter provenance, explicit preparation status, byte ranges, capture
names, normalized categories and modifiers, and embedded regions. Prepared
Markdown and Rust package results use crate-owned capture mapping and return
non-overlapping public chunks. Markdown fenced blocks are detected as
data-only embedded regions and carry raw and normalized info strings. Markdown
linked SVG and PNG targets are also detected as data-only media regions. Linked
`*.drawio.svg` targets are classified as SVG media with `DrawioSvg` flavor.
Markdown fenced syntax injection is registry-driven and bounded to the fenced
content range for enabled parser and query packages, including Go,
TypeScript, and TSX. It reports coverage, error ranges, and budget state on
the parent result.

- `CoreTextPosition { row, col }`
- `CoreTextRange { start, end }`
- `CoreTreeSitterLanguagePackage
  { language_id, package_id, package_version, parser_version, query_version }`
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
- `CoreSyntaxCategory`: normalized public highlight categories such as
  `Keyword`, `String`, `Function`, `Type`, `Variable`, `Comment`, and
  `Unknown`
- `CoreSyntaxModifier`: normalized public modifiers such as `Declaration`,
  `Definition`, `Readonly`, `Static`, `Deprecated`, `Async`, and `Mutable`
- `CoreEmbeddedRegion
  { range, content_range, source, raw_info_string, normalized_info_string,
  normalized_kind, resolved_language }`
- `CoreEmbeddedBlockKind`: `Syntax`, `Diagram`, `Media`, `Unknown`
- `CoreDiagramKind`, `CoreMediaKind`, and `CoreMediaFlavor`
- `CoreLanguageResolutionSource`: `Registry`, `VimFiletype`, `BufferName`,
  `HostHint`, `MarkdownInfoString`, `MarkdownLinkTarget`
- `CoreLanguageResolutionStatus`: `Resolved`, `Unavailable`, `Unsupported`
- `CoreResolvedLanguage
  { range, role, status, language_id, package_id, package_version, kind,
  confidence, source }`
- `CoreRootLanguageResolutionRequest
  { range, vim_filetype, buffer_name, host_language_hint }`
- `CoreEmbeddedLanguageResolutionRequest { range, raw_info_string }`

Tree-sitter output must not be routed through `CoreSyntaxChunk`. It does not
carry Vim `syn_id` values, Vim highlight attributes, or conceal display
substitutions. Hosts compare `CoreBufferInfo.source_revision` with
`CoreTreeSitterRangeSyntax.source_revision` before using delayed or cached
syntax data.

`VimCoreSession::tree_sitter_language_packages()` returns the feature-enabled
built-in package registry with package, parser, and query versions.
`resolve_tree_sitter_root_language()` resolves a root document language from
Vim `filetype`, buffer name, and an optional host hint.
`resolve_tree_sitter_embedded_language()` resolves Markdown info strings for
embedded regions. Known languages without an enabled package return
`Unavailable`; unknown or non-syntax embedded kinds return `Unsupported`.
Use this flow from host renderers:

1. Read the active `CoreBufferInfo` and visible range from `snapshot()`.
2. Call `request_tree_sitter_syntax_preparation()` with the buffer ID,
   `source_revision`, visible range, Vim `filetype`, buffer name, optional
   host language hint, and snapshot policy.
3. Drain `poll_tree_sitter_preparation()` until the matching request result is
   returned.
4. Render only results whose `status` is `Prepared` and whose
   `syntax.source_revision` equals the current `buffer.source_revision`.
5. Use `query_tree_sitter_syntax_range()` on later draw passes to read and clip
   committed cache entries for the visible range.

A minimal host integration looks like this:

```rust
let snapshot = session.snapshot();
let buffer = snapshot
    .buffers
    .iter()
    .find(|buffer| buffer.is_active)
    .expect("active buffer");
let window = snapshot
    .windows
    .iter()
    .find(|window| window.is_active)
    .expect("active window");

let visible_range = CoreTextRange {
    start: CoreTextPosition {
        row: window.topline.saturating_sub(1) as usize,
        col: 0,
    },
    end: CoreTextPosition {
        row: window.botline as usize,
        col: 0,
    },
};
let vim_filetype = session.get_option_string("filetype", CoreOptionScope::Local).ok();

let preparation = session.request_tree_sitter_syntax_preparation(
    CoreTreeSitterPreparationRequest {
        buffer_id: buffer.id,
        source_revision: Some(buffer.source_revision),
        range: visible_range,
        vim_filetype,
        buffer_name: (!buffer.name.is_empty()).then(|| buffer.name.clone()),
        host_language_hint: None,
        snapshot_policy: CoreTreeSitterSnapshotPolicy::default(),
    },
)?;

while let Some(completed) = session.poll_tree_sitter_preparation() {
    if completed.request_id == preparation.request_id {
        let syntax = completed.syntax;
        if syntax.status == CoreTreeSitterStatus::Prepared
            && syntax.source_revision == buffer.source_revision
        {
            for chunk in &syntax.chunks {
                render_highlight(chunk.range, chunk.category, &chunk.modifiers);
            }
        }
        break;
    }
}

if let Some(syntax) = session.query_tree_sitter_syntax_range(
    buffer.id,
    buffer.source_revision,
    visible_range,
) {
    if syntax.status == CoreTreeSitterStatus::Prepared
        && syntax.source_revision == buffer.source_revision
    {
        for chunk in &syntax.chunks {
            render_highlight(chunk.range, chunk.category, &chunk.modifiers);
        }
    }
}
```

The host render cache key must include the buffer-local source revision. Use a
shape such as `(buffer_id, source_revision, visible_range)`. Do not use
`CoreSnapshot.revision` as a substitute for `CoreBufferInfo.source_revision`;
the snapshot revision is session-wide, while Tree-sitter freshness is
buffer-local.

`request_tree_sitter_syntax_preparation()` creates or reuses an immutable text
snapshot keyed by `(buffer_id, source_revision)`, pins it for the synchronous
preparation, parses enabled Markdown, Rust, TypeScript, or TSX packages when
available, commits normalized non-overlapping chunks with an explicit status,
and queues a pollable completion. `poll_tree_sitter_preparation()` drains
completed results. `query_tree_sitter_syntax_range()` reads the committed
cache only, clips cached results to the requested visible range, and doesn't
parse in the draw path.

Hosts must interpret non-`Prepared` statuses as diagnostic states rather than
fresh highlight:

- `Unavailable` means the language is known but its package feature isn't
  enabled in this build.
- `Unsupported` means `vim-core-rs` doesn't support the requested language or
  embedded block kind as Tree-sitter syntax.
- `Stale` means the requested source revision is no longer current and no
  matching prepared snapshot can be reused.
- `TooLarge` means the snapshot exceeds `max_snapshot_bytes`.
- `BudgetExceeded` means the snapshot can't fit within `global_byte_budget`.
- `TimedOut` is reserved for preparation implementations that can time out.
- `Partial` means parsing completed with partial coverage, such as
  `budget_status == MatchLimitExceeded`. Conservative host MVPs can render
  only `Prepared` results. Hosts that support degraded rendering may render
  `Partial` chunks clipped to `covered_ranges` and surface the budget status
  as a diagnostic.
- Parser errors keep `status` as `Prepared`, set `has_error`, and populate
  `error_ranges`. A conservative host MVP may skip rendering when `has_error`
  is true or `error_ranges` is non-empty. The recommended richer behavior is
  to render valid chunks outside `error_ranges` and surface parser errors as a
  diagnostic.

For basic styling, hosts must use `CoreTreeSitterChunk.category` and
`CoreTreeSitterChunk.modifiers`. `capture_name` remains provenance and
advanced-styling metadata and isn't required for basic highlight rendering.

Hosts such as `saya` must route editor integration through this crate:
`saya/editor_core -> vim-core-rs -> Vim native/FFI`. They must not keep a
parallel `saya/editor_core -> vim_ffi` path for Tree-sitter syntax migration.
During migration, avoid linking both a direct `vim_ffi` dependency and
`vim-core-rs` into the same `sy` process, because the embedded Vim native
runtime has process-global state.

Snapshot retention keeps only unpinned snapshots, preserves pinned in-flight
snapshots, and applies both latest-N-per-buffer and global byte-budget
eviction. Oversized snapshots return `TooLarge`, and snapshots that can't fit
within the configured budget return `BudgetExceeded`.

### Search and message structs

These types capture message and search metadata.

- `CoreMessageSeverity`: `Info`, `Warning`, `Error`
- `CoreMessageCategory`: `UserVisible`, `CommandFeedback`
- `CoreMessageEvent { severity, category, content }`
- `CoreMatchType`: `Regular`, `IncSearch`, `CurSearch`
- `CoreMatchRange { start_row, start_col, end_row, end_col, match_type }`
- `MatchCountResult`: `Calculated(usize)`, `MaxReached(usize)`, `TimedOut`
- `CoreCursorMatchInfo { is_on_match, current_match_index, total_matches }`
- `CoreSearchDirection`: `Forward`, `Backward`
- `CoreSearchHighlightMode`: `Disabled`, `HlSearch`, `IncSearch`
- `CoreVisibleSearchState`
  `{ window_id, start_row, end_row, mode, pattern, input_pattern,
  hlsearch_enabled, hlsearch_suspended, incsearch_active, ranges }`
- `CoreSearchCapabilityContract`
  `{ live_state_query_available, inactive_window_query_available,
  visible_rows_only, start_col_inclusive, end_col_exclusive, byte_columns,
  data_only_payload, host_owned_presentation }`
- `CoreSearchQueryError`
  `NoActiveWindow`, `InvalidViewport { start_row, end_row }`,
  `WindowNotFound { window_id }`

### Option types

These types define the typed option system.

- `CoreOptionScope`: `Default`, `Global`, `Local`
- `CoreOptionType`: `Bool`, `Number`, `String`
- `CoreOptionError`
  - `UnknownOption { name }`
  - `TypeMismatch { name, expected, actual }`
  - `SetFailed { name, reason }`
  - `ScopeNotSupported { name, scope }`
  - `InternalError { name, detail }`

## Public VFS types

The VFS types are public because the host must handle VFS requests and return
VFS responses.

### Buffer-source and request-state enums

These enums tell the host what kind of buffer and operation it is dealing
with.

- `CoreBufferSourceKind`: `Local`, `Virtual`
- `CoreVfsOperationKind`: `Resolve`, `Exists`, `Load`, `Save`
- `CoreDeferredClose`: `Quit`, `SaveAndClose`, `SaveIfDirtyAndClose`
- `CoreVfsErrorKind`: `ResolveFailed`, `ExistsFailed`, `LoadFailed`,
  `SaveFailed`, `NotFound`, `InvalidResponse`, `HostUnavailable`,
  `Cancelled`, `TimedOut`, `RevisionMismatch`

### VFS structs

These structs represent buffer bindings, request state, and transaction logs.

- `CoreVfsError { kind, message }`
- `CorePendingVfsOperation { request_id, kind, issued_order }`
- `CoreBufferBinding { buf_id, source_kind, locator, document_id,
  display_name, committed_revision, pending_operation, deferred_close,
  last_saved_revision, last_vfs_error }`
- `CoreRequestEntry { request_id, operation_kind, target_buf_id, document_id,
  locator, base_revision, status, issued_order }`
- `VfsLogEntry { event, operation_kind, request_id, buf_id, document_id,
  locator, base_revision, current_revision, detail }`

### VFS request and response enums

These enums form the host-facing VFS protocol.

- `CoreRequestStatus`
  - `Pending`
  - `Succeeded`
  - `Failed(CoreVfsError)`
  - `Cancelled`
  - `TimedOut`
  - `Stale { reason }`
  - `ProtocolMismatch { expected, actual }`
- `CoreVfsRequest`
  - `Resolve { request_id, target_buf_id, locator }`
  - `Exists { request_id, locator }`
  - `Load { request_id, target_buf_id, document_id }`
  - `Save { request_id, target_buf_id, document_id, target_locator, text,
    base_revision, force }`
- `CoreVfsResponse`
  - `Resolved { request_id, document_id, display_name }`
  - `ResolvedLocalFallback { request_id, locator }`
  - `ResolvedMissing { request_id, locator }`
  - `ExistsResult { request_id, exists }`
  - `Loaded { request_id, document_id, text }`
  - `Saved { request_id, document_id }`
  - `Failed { request_id, error }`
  - `Cancelled { request_id }`
  - `TimedOut { request_id }`

## Operational notes

This section captures the public behaviors that are easy to miss when you only
read signatures.

- `take_pending_host_action` and `take_pending_event` are the public queue-drain
  methods for asynchronous host work and out-of-band events. If you do not
  call them, queued host work and queued events remain buffered.
- VFS save requests are revision-aware. A `Saved` response can be rejected as
  stale if the buffer revision has advanced.
- `snapshot().pending_host_actions` includes both already buffered Rust-side
  actions and actions that native host draining would add later in the turn.

## Next steps

Read `api-contracts.md` when you need the state machine behind these APIs. Read
`internal-api-reference.md` when you need to understand the non-public helpers
that implement this surface.
