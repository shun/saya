# vim-core-rs bundled API contracts

This file captures the behavioral contracts of `vim-core-rs`. Read it when raw
signatures are not enough, which is most of the time for this crate because the
central API is a state machine rather than a stateless function catalog.

## Contract model

You can understand the crate as a composition of four contracts.

- Session contract
  One embedded Vim runtime per process
- Command contract
  Commands mutate editor state and may enqueue host work
- VFS contract
  The host owns document storage and answers explicit requests
- VFD contract
  The host owns process execution and feeds bytes back into Vim

## Session contract

- A process may hold only one live `VimCoreSession` at a time.
- `VimCoreSession::new` acquires a global lock. Failure produces
  `CoreSessionError::SessionAlreadyActive`.
- Dropping the session releases the global lock and clears the VFD manager.
- The public API assumes one owner. The type is not `Send` and not `Sync`.

## Snapshot contract

- `snapshot()` returns a coherent point-in-time read of the embedded runtime.
- The method overlays VFS binding metadata from `DocumentCoordinator` onto the
  raw buffer list before returning.
- `snapshot().pending_host_actions` includes the Rust-side queue length in
  addition to whatever the runtime already reported.
- `buffers()` and `windows()` are convenience wrappers over `snapshot()`.
- `CoreSnapshot.cursor_row` and `cursor_col` describe the active window only.
- `CoreWindowInfo` is the per-window source of truth for geometry, viewport
  state, active-window state, and window-local cursor state.

## Command contract

- `execute_normal_command` and `execute_ex_command` return one
  `CoreCommandTransaction`, not a full diff.
- `CoreCommandOutcome::HostActionQueued` means the host must drain pending
  actions. It does not mean the side effect already completed.
- The returned transaction includes the final snapshot, emitted events, and
  emitted host actions.
- `execute_normal_command` and `execute_ex_command` surface structured message
  events. Hosts should filter by `CoreMessageSeverity` and
  `CoreMessageCategory`, not by parsing content.
- `execute_ex_command` parses file-like Ex commands into `ParsedExIntent`.
  Non-intercepted Ex commands go straight to native execution.

## Ex intent routing contract

- `:edit` becomes a `CoreVfsRequest::Resolve` against the active buffer.
- `:write` and `:update` become either `CoreHostAction::Write` for local
  buffers or `CoreVfsRequest::Save` for virtual buffers.
- `:update` on a clean VFS buffer becomes `CoreCommandOutcome::NoChange`.
- `:wq` on a VFS buffer sets `CoreDeferredClose::SaveAndClose`, issues a save,
  then resumes quit after save success.
- `:xit` on a dirty VFS buffer sets
  `CoreDeferredClose::SaveIfDirtyAndClose`, issues a save, then resumes quit
  after save success.
- `:quit` on a VFS buffer with a pending save is rejected unless forced.
- `:quit!` always queues a forced `CoreHostAction::Quit`.

## Host-action queue contract

- The queue is FIFO.
- The queue can contain actions emitted directly by Rust policy code and
  actions drained from the native runtime.
- The host must repeatedly call `take_pending_host_action()` until it returns
  `None`.
- `Write`, `Quit`, `Redraw`, `RequestInput`, `JobStart`, `JobStop`, and
  `VfsRequest` are requests to the host. The crate does not complete them.

## Message delivery contract

- `take_pending_event()` drains the native event queue into a Rust FIFO and
  returns one event at a time.
- `execute_normal_command()` and `execute_ex_command()` drain both the event
  queue and the host-action queue before returning the transaction.
- Message delivery does not depend on `execute('messages')`, `v:errmsg`, or a
  registered callback.
- `CoreMessageEvent` carries structured metadata. Hosts must distinguish
  user-visible notifications from command feedback through
  `CoreMessageCategory`, not by parsing message text.
- `CoreSnapshot` is state-only. Reading a snapshot does not drain pending
  events.
- UI-like notifications such as bell, redraw, buffer creation, window
  creation, and layout changes are modeled as `CoreEvent`, not duplicated host
  actions in v2 transactions.

## VFS request contract

- The core emits explicit VFS requests through `CoreHostAction::VfsRequest`.
- The host must answer with `submit_vfs_response`.
- Request IDs are monotonic and unique within one session.
- `CoreRequestEntry.issued_order` is monotonic and tracks causal order across
  requests.
- The VFS ledger records every request until session end. It is not just a
  queue of pending work.

## VFS operation flow contract

### Resolve and load

1. The core issues `CoreVfsRequest::Resolve`.
2. The host replies with one of:
   - `Resolved`, which transitions into an automatic `Load`
   - `ResolvedLocalFallback`, which switches the buffer to local ownership
   - `ResolvedMissing`, which records a `NotFound` error
3. If the reply is `Resolved`, the core issues `CoreVfsRequest::Load`.
4. When the host replies with `Loaded`, the core applies the text into Vim and
   updates binding metadata.

### Save

1. The core snapshots the active buffer revision and text.
2. The core issues `CoreVfsRequest::Save { base_revision, text, ... }`.
3. The host persists the payload and replies with one of:
   `Saved`, `Failed`, `Cancelled`, `TimedOut`.
4. On `Saved`, the core accepts the response only when the buffer still has the
   same `document_id` and `current_revision == base_revision`.
5. If the revision advanced, the response becomes `CoreRequestStatus::Stale`,
   the buffer remains dirty, and the transaction log records
   `VfsLogEvent::StaleRejected`.

## VFS validation contract

- A response for an unknown request ID is rejected as
  `CoreResponseApplyOutcome::UnknownRequest` and logged as
  `VfsLogEvent::UnknownRequestRejected`.
- A response whose logical operation does not match the ledger entry becomes
  `CoreRequestStatus::ProtocolMismatch`, records
  `CoreVfsErrorKind::InvalidResponse`, and logs
  `VfsLogEvent::ProtocolMismatchRejected`.
- A `Saved` response with a mismatched `document_id` is a protocol error.
- A `Saved` response without a known base revision is also a protocol error.

## Deferred close contract

- Deferred close exists to support `:wq` and `:xit` on VFS-backed buffers.
- `SaveAndClose` means quit after save completion regardless of prior dirty
  state.
- `SaveIfDirtyAndClose` means quit after save completion only for the
  dirty-triggered save path.
- The transaction log records `QuitDeferred`, `QuitResumed`, and `QuitDenied`
  so hosts can explain why closing did or did not happen.

## Option contract

- Typed getters require the expected type up front and return
  `CoreOptionError::TypeMismatch` when the runtime type differs.
- Unknown options return `CoreOptionError::UnknownOption`.
- Unsupported scope combinations return `CoreOptionError::ScopeNotSupported`.
- String getters and setters validate embedded NUL bytes through `CString`
  conversion before crossing the FFI boundary.

## Search and syntax contract

- Search highlight methods return plain ranges. They do not own rendering.
- `query_visible_search_state()` and
  `query_visible_search_state_for_window()` are the stable pane-local search
  contract. They bundle mode, pattern, visible ranges, and `hlsearch` /
  `incsearch` state without moving rendering policy into the core.
- Search columns are byte offsets. Hosts that render by grapheme or display
  cell must convert explicitly.
- `get_cursor_match_info` can signal `TimedOut` or `MaxReached` instead of a
  concrete full count.
- `CoreBufferInfo.source_revision` is the crate-owned, buffer-local source
  revision for freshness checks. It is distinct from the session-wide
  `CoreSnapshot.revision`.
- Syntax extraction groups consecutive columns with the same syntax ID into one
  `CoreSyntaxChunk`.
- Tree-sitter extraction, when enabled, must use a separate public surface and
  must not route output through `CoreSyntaxChunk`.
- Enabled Go, Markdown, Rust, TypeScript, and TSX Tree-sitter packages own
  parser/query metadata, capture mapping, explicit priorities, overlap
  normalization, and visible range reads from committed cache.
- Markdown fenced blocks are extracted as data-only embedded regions. The
  region records preserve raw and normalized info strings, and classification
  stays registry-backed. Markdown fenced syntax injection is registry-driven
  and bounded to the fenced content range for enabled parser and query
  packages.
- Markdown linked SVG and PNG targets are extracted as data-only media
  embedded regions. The raw link target is preserved, linked `*.drawio.svg`
  targets carry SVG media with `DrawioSvg` flavor, and rendering or decoding
  remains host-owned.
- `get_syntax_name` may return `None` when Vim does not provide a non-empty
  group name.

## VFD and job contract

- `CoreHostAction::JobStart` means Vim requested a process. The host must spawn
  it and retain job and VFD IDs.
- The host must feed stdout and stderr bytes back through
  `inject_vfd_data(vfd, bytes)`.
- The host must report lifecycle transitions through
  `notify_job_status(job_id, status, exit_code)`.
- Terminal job statuses close the three associated VFDs so Vim sees EOF.
- `vim_core_job_get_status` reports an ended job exactly once as `1`, then as
  `2` on subsequent reads after reaping.

## Diagnostics contract

- `vfs_request_ledger()` is the source of truth for request status.
- `vfs_transaction_log()` is the source of truth for chronological VFS events.
- `buffer_binding()` is the source of truth for current per-buffer VFS state.
- `backend_identity()` tells you whether the runtime is real upstream Vim or a
  stub.

## Testing contract

- `tests/public_api_contract.rs` covers stable public surface expectations.
- `tests/vfs_contract.rs` covers VFS sequencing and error handling.
- `tests/job_api_contract.rs` and `tests/job_contract.rs` cover job bridging.
- `tests/mode_transition_contract.rs`, `tests/undo_contract.rs`,
  `tests/search_highlight_contract.rs`, and related suites lock in important
  user-visible behavior beyond type signatures.

## Reading guidance

- Read [public-api-reference.md](public-api-reference.md) for the symbol
  catalog.
- Read [internal-api-reference.md](internal-api-reference.md) for the helper
  inventory behind these contracts.
