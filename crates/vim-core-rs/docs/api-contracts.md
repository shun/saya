# API contracts

This document captures the behavioral contracts of `vim-core-rs`. Read this
page when raw signatures are not enough, which is most of the time for this
crate because the central API is a state machine, not a stateless function
catalog.

## Contract model

You can understand the crate as a composition of four contracts.

- Session contract: one embedded Vim runtime per process
- Command contract: commands mutate editor state and may enqueue host work
- VFS contract: the host owns document storage and answers explicit requests
- VFD contract: the host owns process execution and feeds bytes back into Vim

## Rendering State Family contract

The rendering state family is the vocabulary we use to keep the current
read-only extraction boundary stable. It names the current members, the deferred placeholder, and the exclusion boundary without introducing a new runtime facade. It is an additive explanation layer over the existing
`VimCoreSession` surface, not a new runtime owner and not a replacement
surface. The authoritative source for this boundary is the set of docs,
tests, and classification metadata named in `docs/SCOPE.md`. The family is a
Vim-owned read-only extraction boundary.

- `Search` and `Syntax` are the current members.
- `Annotations` is the deferred placeholder for text-property extraction.
- `popupwin` is the exclusion and stays outside the family as host-owned
  presentation.
- popup placement, popup composition, popup borders, and overlay layout stay
  outside the family as host-owned presentation policy.
- This feature does not add a new family descriptor or facade. If a future
  family-level descriptor is introduced, it must remain an additive stateless
  summary over `VimCoreSession`.

## Register readback contract

The public register readback promise is documented in
`docs/public-api-reference.md`, but `tests/register_contract.rs` is the source
of truth for observable behavior.

- `VimCoreSession::register()` returns the full register contents, including
  multiline text and trailing newlines.
- The contract tests define the observable semantics for unnamed, black-hole,
  and multiline register readback.

## Session contract

The session contract defines object lifetime and process-level exclusivity.

- A process may hold only one live `VimCoreSession` at a time.
- `VimCoreSession::new` acquires a global lock. Failure to acquire it produces
  `CoreSessionError::SessionAlreadyActive`.
- Dropping the session releases the global lock and clears the VFD manager.
- The public API assumes mutation happens through one owner. The type is not
  `Send` and not `Sync`.

## Snapshot contract

The snapshot contract explains what the full and lightweight snapshot APIs
mean. Use `light_snapshot()` for normal rendering and refresh paths that need
state but not full buffer text.

- `snapshot()` returns a coherent point-in-time read of the embedded runtime.
  It materializes the active buffer text for compatibility.
- `light_snapshot()` returns the same metadata family without materializing
  active buffer text.
- `buffer_line_range(buffer_id, start_row, line_count)` returns only the
  requested zero-based row range for one buffer, plus `total_line_count` and
  the buffer-local `source_revision`.
- The method overlays VFS binding metadata from `DocumentCoordinator` onto the
  raw buffer list before returning.
- `snapshot().pending_host_actions` includes the length of the Rust-side queue
  in addition to whatever the runtime already reported.
- `buffers()` and `windows()` are convenience wrappers over `snapshot()` for
  compatibility. Hosts that only need render metadata can call
  `light_snapshot()` directly.
- `CoreBufferInfo.source_revision` is the crate-owned, buffer-local source
  revision for freshness checks. It is distinct from the session-wide
  `CoreSnapshot.revision`.
- `CoreSnapshot.cursor_row` and `cursor_col` describe the active window only.
- `CoreWindowInfo` is the per-window source of truth for geometry, viewport
  state, active-window state, and window-local cursor state.
- Exactly one entry in `CoreSnapshot.windows` is active when the snapshot
  contains windows.
- `CoreWindowInfo.id` is the only canonical window identity across the host
  boundary. Hosts must key projection and follow-up queries by `window_id`
  instead of inferring pane identity from row, column, buffer, or ordering.
- `CoreSnapshot.active_window()`, `active_window_id()`, and `window(window_id)`
  are the read-only helpers for resolving the active window and looking up a
  window by canonical ID without host-side guesswork.
- `active_window_id()` is the preferred convenience API when the host needs
  only the active window identity. If the host already holds a snapshot, use
  `snapshot.active_window_id()` and `snapshot.window(window_id)` from that same
  snapshot instead of re-scanning or mixing reads from another snapshot.
- `None` from the active-window helpers and `None` from `window(window_id)` are
  intentional contract states. Hosts must treat them as "no active window" or
  "unknown window" and must not substitute fallback values such as the first
  window or a hard-coded `window_id`.
- When host code needs the active window ID, prefer `active_window_id()` as
  the stable read path. `CoreSnapshot.active_window()` is the equivalent helper
  when the host already holds a snapshot and needs the active window record.
- Hosts must treat `None`, `WindowNotFound`, and `InvalidViewport` as explicit
  contract outcomes. Do not fall back to the first window, `window_id = 1`, or
  any other host-side heuristic.

## Command contract

The command contract explains how mutations report outcomes.

- `execute_normal_command` and `execute_ex_command` return one
  `CoreCommandTransaction` that contains the outcome, final snapshot, emitted
  events, and emitted host actions.
- `dispatch_key` accepts one typed key event at a time. In Insert mode,
  printable keys are literal text input and do not contribute to Normal-mode
  pending command prefixes. A standalone Ctrl-H byte (`"\x08"`) is treated as
  Insert-mode Backspace and deletes the previous inserted character when Vim's
  Backspace rules permit it.
- `CoreCommandOutcome::HostActionQueued` means the host must drain pending
  actions. It does not mean the side effect already completed.
- `execute_ex_command` first parses command text into a `ParsedExIntent` for
  file-like commands. Other Ex commands go straight to native execution.

## Ex intent routing contract

The Ex command parser is part of the public behavior even though the parser
type is private.

- The contract tests in `tests/vfs_contract.rs` define the accepted and
  excluded compound forms for this routing boundary.
- `:edit` becomes a `CoreVfsRequest::Resolve` against the active buffer.
- `:write` and `:update` become either `CoreHostAction::Write` for local
  buffers or `CoreVfsRequest::Save` for virtual buffers.
- On local buffers, a compound Ex command with an intercepted `:write` or
  `:update` followed immediately by `:quit` also queues `Quit` so the host can
  keep save-before-quit ordering.
- On VFS-backed buffers, `:write [path] | quit-family`, `:write! [path] |
  quit-family`, and dirty `:update [path] | quit-family` enter the same
  deferred close flows as `:wq` and `:xit`.
- `:update! [path] | quit-family`, range-prefixed forms, and generalized
  pipeline semantics beyond the first `quit-family` trailing segment stay
  outside this contract.
- `:update` on a clean VFS buffer becomes `CoreCommandOutcome::NoChange`.
- `:wq` on a VFS buffer sets `CoreDeferredClose::SaveAndClose`, issues a save,
  and only later resumes quit after save success.
- `:xit` on a dirty VFS buffer sets
  `CoreDeferredClose::SaveIfDirtyAndClose`, issues a save, and resumes quit
  only after save success.
- `:quit` on a VFS buffer with a pending save is rejected unless forced.
- `:quit!` always queues a forced `CoreHostAction::Quit`.

## Host-action queue contract

The host-action queue is the bridge between the embedded runtime and the host
application.

- The queue is FIFO.
- The queue can contain actions emitted directly by Rust policy code and
  actions drained from the native runtime.
- The host must repeatedly call `take_pending_host_action()` until it returns
  `None`.
- `Write`, `Quit`, `Redraw`, `RequestInput`, `JobStart`, `JobWrite`,
  `JobStop`, and `VfsRequest` are requests to the host. The crate does not
  complete them by itself.
- `RequestInput` is the bridge used by the dedicated `:input` Ex command.
  Vimscript `input()`, `inputsecret()`, `confirm()`, and `:confirm` are
  intentionally unsupported in embedded mode and surface as user-visible
  `CoreEvent::Message` values instead of host actions.

## Event delivery contract

The event contract explains how embedded-mode observability works.

- Native code enqueues `CoreEvent` values directly at the source of the Vim
  side effect.
- `take_pending_event()` drains the native event queue into a Rust FIFO and
  returns one event at a time.
- `execute_normal_command()` and `execute_ex_command()` drain both the
  event queue and the host-action queue before returning the transaction.
- Message delivery does not depend on `execute('messages')`, `v:errmsg`, or a
  registered callback.
- `CoreMessageEvent` carries structured metadata. Hosts must distinguish
  user-visible notifications from command feedback through
  `CoreMessageCategory`, not by parsing message text.
- `CoreSnapshot` is state-only. Reading a snapshot does not drain pending
  events.
- UI-like notifications such as bell, redraw, buffer creation, window
  creation, and layout changes are modeled as `CoreEvent`, not duplicated
  host actions in v2 transactions.

## VFS request contract

The VFS contract exists because the core does not own storage.

- The core emits explicit VFS requests through `CoreHostAction::VfsRequest`.
- The host must answer with `submit_vfs_response`.
- Request IDs are monotonic and unique within one session.
- `CoreRequestEntry.issued_order` is monotonic and tracks causal order across
  requests.
- The VFS ledger records every request until the session ends. It is not only
  a queue of pending work.

## VFS operation flow contract

The common VFS flows have fixed sequencing rules.

### Resolve and load

Resolve and load happen in two stages.

1. The core issues `CoreVfsRequest::Resolve`.
2. The host replies with one of:
   - `Resolved`, which transitions into an automatic `Load` request
   - `ResolvedLocalFallback`, which switches the buffer to local ownership
   - `ResolvedMissing`, which records a `NotFound` error
3. If the host replies with `Resolved`, the core issues
   `CoreVfsRequest::Load`.
4. When the host replies with `Loaded`, the core applies the buffer contents
   into Vim and updates binding metadata.

### Save

Save is revision-aware and intentionally conservative.

1. The core snapshots the active buffer revision and text.
2. The core issues `CoreVfsRequest::Save { base_revision, text, ... }`.
3. The host persists the payload and replies with one of:
   - `Saved`
   - `Failed`
   - `Cancelled`
   - `TimedOut`
4. On `Saved`, the core accepts the response only when the buffer still has
   the same `document_id` and `current_revision == base_revision`.
5. If the revision has advanced, the response becomes
   `CoreRequestStatus::Stale`, the buffer remains dirty, and the transaction
   log records `VfsLogEvent::StaleRejected`.

## VFS validation contract

The VFS coordinator validates more than request IDs.

- A response for an unknown request ID is rejected as
  `CoreResponseApplyOutcome::UnknownRequest` and logged as
  `VfsLogEvent::UnknownRequestRejected`.
- A response whose logical operation does not match the ledger entry becomes
  `CoreRequestStatus::ProtocolMismatch`, records
  `CoreVfsErrorKind::InvalidResponse`, and logs
  `VfsLogEvent::ProtocolMismatchRejected`.
- A `Saved` response with a mismatched `document_id` is treated as a protocol
  error.
- A `Saved` response without a known base revision is also treated as a
  protocol error.

## Deferred close contract

Deferred close exists to support `:wq` and `:xit` on VFS-backed buffers.

- The coordinator stores deferred close state on the active buffer binding.
- The VFS-backed compound forms documented in the Ex intent routing contract
  reuse these same deferred close states instead of introducing another close
  model.
- `SaveAndClose` means a quit must happen after save completion regardless of
  prior dirty state.
- `SaveIfDirtyAndClose` means a quit must happen after save completion only for
  the save path that was triggered by dirty state.
- The transaction log records `QuitDeferred`, `QuitResumed`, and `QuitDenied`
  events so the host can explain why closing did or did not happen.

## Option contract

The option system is typed and scope-aware.

- Getter methods require the expected type up front and return
  `CoreOptionError::TypeMismatch` when the runtime type differs.
- Unknown options return `CoreOptionError::UnknownOption`.
- Unsupported scope combinations return
  `CoreOptionError::ScopeNotSupported`.
- String getters and setters validate embedded NUL bytes through `CString`
  conversion before crossing the FFI boundary.

## Search and syntax contract

The search and syntax methods are read-only rendering helpers. They expose
Vim-owned state for host rendering, but they do not move rendering policy into
the crate.

For the current Search family boundary, this means `incsearch`, inactive
window queries, and byte-column search ranges are contract data, while popup
ownership stays host-owned presentation.

- Search highlight methods return plain ranges. They do not own rendering.
- `query_visible_search_state()` and
  `query_visible_search_state_for_window()` are the stable pane-local search
  contract. They bundle mode, pattern, visible ranges, and `hlsearch` /
  `incsearch` state without moving rendering policy into the core.
- `search_capability_contract()` is the structured Search family summary for
  this boundary. It reports live-state availability, inactive-window support,
  visible-row scoping, byte-column semantics, the data-only payload contract,
  and the host-owned presentation boundary.
- `query_visible_search_state_for_window(window_id, ...)` accepts inactive
  windows as long as the `window_id` exists in the current snapshot.
- Search columns are byte offsets. Hosts that render by grapheme or display
  cell must convert explicitly.
- `get_cursor_match_info` can signal `TimedOut` or `MaxReached` instead of a
  concrete full count.
- `get_line_syntax()` groups consecutive columns with the same syntax ID into
  `CoreSyntaxChunk` values, and `get_syntax_name()` resolves the group name.
- This is the public syntax extraction boundary. It does not expose
  highlight definition tables, including `:highlight` definition tables, or
  resolved highlight attribute tables yet.
- Tree-sitter extraction, when enabled, must use a separate public surface and
  must not route output through `CoreSyntaxChunk`.
- The stable opt-in Tree-sitter package registry is enabled with
  `tree-sitter-syntax`. The older `experimental-tree-sitter` feature remains
  as a compatibility alias. Hosts must prefer `tree-sitter-syntax` for new
  integrations and migrate preview integrations to that feature name. No alias
  removal release is scheduled. The registry registers only packages enabled
  by per-language Cargo features. Root language resolution uses Vim `filetype`,
  buffer name, and an optional host hint as inputs. Embedded language
  resolution uses Markdown info strings as inputs. Missing enabled-package
  support is reported as `Unavailable`, and unknown language or non-syntax
  inputs are reported as `Unsupported`.
- Tree-sitter preparation uses a request, poll, and query model. Request
  methods may prepare synchronously in the current implementation, poll methods
  drain completed results, and range queries read committed cache state only.
  Hosts must render only `Prepared` results whose `source_revision` matches
  the current buffer revision. `Stale`, `Unavailable`, `Unsupported`,
  `TooLarge`, `BudgetExceeded`, `TimedOut`, and `Partial` are explicit
  diagnostic states rather than fresh highlight.
- Conservative hosts render only `Prepared` results. Hosts that support
  degraded rendering may render `Partial` chunks clipped to `covered_ranges`
  when `budget_status == MatchLimitExceeded`. For parser errors, conservative
  hosts may skip rendering when `has_error` is true or `error_ranges` is
  non-empty, while richer hosts may render valid chunks outside
  `error_ranges`.
- Markdown fenced blocks are detected as data-only embedded regions. The
  returned region records preserve raw and normalized info strings, and they
  are classified through the registry-backed embedded-region resolver rather
  than host-side ad hoc parsing. Markdown fenced syntax injection is
  registry-driven and bounded to the fenced content range for enabled parser
  and query packages, including Go, TypeScript, and TSX. It remains separate
  from diagram and media regions.
- Markdown linked SVG and PNG targets are detected as data-only media embedded
  regions. The raw link target is preserved on the region record, linked
  `*.drawio.svg` targets are classified as SVG media with `DrawioSvg` flavor,
  and rendering or decoding remains host-owned.
- Enabled Markdown, Rust, TypeScript, and TSX Tree-sitter packages own
  parser/query metadata, capture-to-category/modifier mapping, explicit capture
  priorities, and overlap normalization. The standard public chunks are
  non-overlapping, and visible range queries clip committed cache results
  without parsing.
- Host styling must treat `CoreTreeSitterChunk.category` and `modifiers` as
  the primary public styling contract. `capture_name` remains provenance,
  debugging, and advanced-styling metadata.
- Tree-sitter preparation snapshots are immutable and keyed by
  `(buffer_id, source_revision)`. Queued or running requests pin snapshots.
  Completed snapshots are retained with latest-N-per-buffer and global
  byte-budget eviction for unpinned snapshots only. Oversized snapshots return
  `TooLarge`, and requests that can't fit within the configured budget return
  `BudgetExceeded`.
- Host render caches must use buffer-local syntax freshness keys such as
  `(buffer_id, source_revision, visible_range)`. Hosts must not substitute the
  session-wide `CoreSnapshot.revision` for `CoreBufferInfo.source_revision`.
- Hosts such as `saya` must route editor integration through
  `saya/editor_core -> vim-core-rs -> Vim native/FFI`, not through a parallel
  direct `saya/editor_core -> vim_ffi` dependency. During migration, avoid
  linking both direct `vim_ffi` and `vim-core-rs` into the same `sy` process
  because the embedded Vim native runtime has process-global state.
- `textprop` is deferred as future annotation-state extraction. The crate does
  not expose it as a public contract yet.
- popup placement, popup composition, popup borders, and overlay layout stay
  outside this contract even when hosts render search ranges, syntax chunks,
  or future annotations.
- `get_syntax_name()` may return `None` when Vim does not provide a non-empty
  group name for the ID.

## VFD and job contract

The VFD contract explains what the host must do for jobs.

- `CoreHostAction::JobStart` means Vim requested a process. The host must spawn
  it and retain the requested job and VFD IDs.
- `CoreHostAction::JobWrite { vfd, data }` means Vim wrote bytes to a job
  channel. The host must forward those bytes to the real process input that
  owns `vfd`.
- The host must feed stdout and stderr bytes back through
  `inject_vfd_data(vfd, bytes)`.
- The host must report lifecycle transitions through
  `notify_job_status(job_id, status, exit_code)`.
- Terminal job statuses close the three associated VFDs so that Vim sees EOF.
- `vim_core_job_get_status` reports an ended job exactly once as `1`, then as
  `2` on subsequent reads after the job has been reaped.

## Diagnostics contract

The crate exposes enough state for host debugging without requiring direct
inspection of internal fields.

- `vfs_request_ledger()` is the source of truth for request status.
- `vfs_transaction_log()` is the source of truth for chronological VFS events.
- `buffer_binding()` is the source of truth for current per-buffer VFS state.
- `backend_identity()` tells you whether you are running against the real Vim
  runtime or a stubbed backend.

## Testing contract

Repository tests define large parts of the intended behavior.

- `tests/public_api_contract.rs` covers stable public surface expectations.
- `tests/vfs_contract.rs` covers VFS sequencing and error handling.
- `tests/job_api_contract.rs` and `tests/job_contract.rs` cover job bridging.
- `tests/mode_transition_contract.rs`, `tests/undo_contract.rs`,
  `tests/search_highlight_contract.rs`, and related contract tests lock in
  important user-visible behavior beyond type signatures.

## Next steps

Read `public-api-reference.md` for the symbol catalog and
`internal-api-reference.md` for the helper inventory behind these contracts.
