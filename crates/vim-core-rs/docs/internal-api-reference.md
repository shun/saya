# Internal API reference

This document describes the non-public APIs that implement `vim-core-rs`.
Treat this page as the implementation map for code review, refactoring, and
LLM reasoning. None of these symbols are crate-public unless this page says
otherwise.

## Internal structure overview

The internal implementation is split into three layers.

- `src/lib.rs` owns the public facade, intent parsing, host-action draining,
  option conversion, and FFI result translation.
- `src/vfs.rs` owns the virtual document coordination layer, request ledger,
  deferred close state, and VFS transaction log.
- `src/vfd.rs` owns job I/O emulation through virtual file descriptors and the
  exported C shims that Vim calls.

## Internal API in `src/lib.rs`

This section documents the non-public symbols that sit next to
`VimCoreSession`.

### Internal enums

These enums are not exposed publicly, but they decide how file-like Ex
commands behave.

- `ParsedExIntent`
  - `Edit { locator }`
  - `Write { path, force }`
  - `Update { path, force }`
  - `SaveAndClose { force }`
  - `SaveIfDirtyAndClose`
  - `Quit { force }`

### Internal session methods

These methods are private helpers on `VimCoreSession`.

- `invoke_native_ex_command(&mut self, command: &str)
  -> Result<(CoreCommandOutcome, CoreSnapshot), CoreCommandError>`
  Sends an Ex command directly to the bridge without intent interception, then
  returns the outcome together with a snapshot.
- `invoke_native_normal_command(&mut self, command: &str)
  -> Result<(CoreCommandOutcome, CoreSnapshot), CoreCommandError>`
  Sends a Normal command directly to the bridge without extra interception.
- `apply_intent(&mut self, intent: ParsedExIntent)
  -> Result<CoreCommandOutcome, CoreCommandError>`
  Routes intercepted Ex commands into host-driven flows. It is the main policy
  point for `:edit`, `:write`, `:update`, `:wq`, `:xit`, and `:quit`.
- `apply_write_intent(&mut self, path: String, force: bool,
  _deferred_close: Option<CoreDeferredClose>)
  -> Result<CoreCommandOutcome, CoreCommandError>`
  Converts a write-like intent into either `CoreHostAction::Write` for local
  buffers or `CoreHostAction::VfsRequest(CoreVfsRequest::Save)` for virtual
  buffers.
- `apply_loaded_buffer(&mut self, buf_id: i32, display_name: &str, text: &str)
  -> Result<(), CoreCommandError>`
  Applies one loaded VFS payload into the embedded runtime as a single FFI
  buffer commit. It replaces the text, updates the display name, and clears
  dirty state.
- `drain_native_host_actions(&mut self)`
  Pulls native bridge host actions into the Rust-side FIFO queue until the
  bridge reports no more actions.
- `drain_native_events(&mut self)`
  Pulls native bridge events into the Rust-side FIFO queue until the bridge
  reports no more events.
- `get_option_value(&self, name: &str, scope: CoreOptionScope,
  expected: CoreOptionType) -> Result<ConvertedOptionValue, CoreOptionError>`
  Performs the typed option read before the public typed accessors unpack the
  internal enum.

### Free helper functions in `src/lib.rs`

These helpers convert raw bridge output into safe Rust values.

- `convert_command_result`
  Maps the bridge command result enum into `CoreCommandOutcome` or
  `CoreCommandError`.
- `option_name_to_cstring`
  Validates and converts an option name into a `CString`, returning
  `CoreOptionError` on invalid embedded NUL bytes.
- `option_value_to_cstring`
  Validates and converts an option string value into a `CString`.
- `convert_status`
  Converts raw bridge status values into `CoreCommandError`.
- `convert_snapshot`
  Converts the raw snapshot struct into `CoreSnapshot`.
- `convert_pum_info`
  Converts pop-up menu FFI data into `CorePumInfo`.
- `c_str_to_string`
  Copies a null-terminated C string into a Rust `String`.
- `convert_buffer_list`
  Converts the FFI buffer array into `Vec<CoreBufferInfo>`.
- `convert_window_list`
  Converts the FFI window array into `Vec<CoreWindowInfo>`, including
  geometry, viewport state, active-window state, and per-window cursor state.
- `convert_mode`
  Maps the raw Vim mode enum into `CoreMode`.
- `convert_pending_input`
  Maps the raw pending-input enum into `CorePendingInput`.
- `convert_mark_position`
  Converts the raw mark struct into `CoreMarkPosition`.
- `convert_jumplist`
  Converts the raw jumplist struct into `CoreJumpList`.
- `convert_jumplist_entries`
  Converts the raw jumplist entry array into `Vec<CoreJumpListEntry>`.
- `convert_undo_tree`
  Converts the raw undo tree into `CoreUndoTree`.
- `parse_ex_intent`
  Parses user-provided Ex command text into a structured `ParsedExIntent` when
  the command belongs to the host-driven file command subset.
- `convert_host_action`
  Converts raw bridge host-action payloads into `CoreHostAction`.
- `convert_input_kind`
  Maps raw input request kinds into `CoreInputRequestKind`.
- `convert_option_scope`
  Maps `CoreOptionScope` into the FFI scope enum.
- `convert_option_type`
  Converts the raw option type tag into `CoreOptionType`.
- `convert_option_get_result`
  Converts the raw option getter result into a typed internal value or a
  `CoreOptionError`.
- `convert_option_set_result`
  Converts the raw option setter result into `()` or `CoreOptionError`.
- `validate_search_viewport`
  Validates the 1-based inclusive row range used by visible-search queries.
- `resolve_active_search_window_id`
  Finds the active window ID in a snapshot for `query_visible_search_state()`.
- `is_error_message`
  Heuristically classifies one Vim message line as an error.
- `string_from_parts`
  Copies a string from a pointer and an explicit byte length.

### Internal-only types not exported publicly

These types are implementation details used by internal helpers.

- `bindings`
  The bindgen-generated private module containing raw bridge and runtime
  symbols.
- `ConvertedOptionValue`
  The internal enum used to stage typed option getter results before the
  public getters downcast them.

## Search helper behavior

- `drain_native_events` preserves native event ordering as the Rust layer
  moves them into the transaction/result queue.
- `validate_search_viewport` rejects rows below 1, rows below `end_row`, and
  inverted ranges before any native query runs.
- `resolve_active_search_window_id` is only used for the active-window search
  convenience method. The window-targeted search API still requires an
  explicit `window_id`.

## Internal API in `src/vfs.rs`

This section documents the crate-visible VFS coordination layer. Public VFS
payload types are already covered in `public-api-reference.md`. This page
focuses on the implementation-only coordination API.

### Internal structs and enums

These types only exist to run the VFS state machine.

- `CoreResponseApplyOutcome`
  - `Applied`
  - `StaleRejected`
  - `ProtocolMismatchRejected`
  - `UnknownRequest`
- `BufferState`
  - Fields: `binding`, `current_revision`
  - Purpose: Keeps mutable per-buffer VFS coordination state next to the
    public-facing `CoreBufferBinding`.
- `DocumentCoordinator`
  - Fields: `next_request_id`, `next_issued_order`, `bindings`, `requests`,
    `transaction_log`
  - Purpose: Owns the full virtual-document ledger and applies host responses.

### `BufferState` methods

`BufferState` has one constructor-like helper.

- `local(buf_id: i32, display_name: String) -> Self`
  Creates an initial local-buffer binding with empty VFS state.

### `DocumentCoordinator` methods

These methods implement the VFS control plane.

- `new() -> Self`
  Creates an empty coordinator.
- `transaction_log(&self) -> &[VfsLogEntry]`
  Returns the accumulated transaction log by slice.
- `emit_log(&mut self, entry: VfsLogEntry)`
  Appends a log entry and prints a debug trace.
- `log_binding_event(&mut self, buf_id: i32, event: VfsLogEvent,
  detail: Option<String>)`
  Builds a log entry from the current binding state of one buffer.
- `sync_buffers(&mut self, buffers: &[(i32, String)])`
  Reconciles coordinator bindings against the current runtime buffer list,
  removing stale bindings and refreshing display names.
- `binding(&self, buf_id: i32) -> Option<&CoreBufferBinding>`
  Returns one live binding.
- `request_entry(&self, request_id: u64) -> Option<CoreRequestEntry>`
  Returns a cloned ledger entry for one request.
- `ledger_entries(&self) -> Vec<CoreRequestEntry>`
  Returns the full cloned request ledger.
- `bind_virtual_document(&mut self, buf_id: i32, locator: Option<String>,
  document_id: String, display_name: String, committed_revision: u64)`
  Marks a buffer as VFS-backed and seeds document metadata.
- `bind_local_buffer(&mut self, buf_id: i32, locator: Option<String>,
  display_name: String, committed_revision: u64)`
  Marks a buffer as local and clears VFS-only state.
- `commit_loaded_revision(&mut self, buf_id: i32, revision: u64)`
  Records the committed revision after a successful load.
- `note_buffer_revision(&mut self, buf_id: i32, revision: u64)`
  Updates the coordinator's idea of the current revision before a save.
- `issue_resolve(&mut self, target_buf_id: i32, locator: String)
  -> CoreVfsRequest`
  Allocates a resolve request, records ledger state, sets pending operation,
  and returns `CoreVfsRequest::Resolve`.
- `issue_exists(&mut self, locator: String) -> CoreVfsRequest`
  Allocates an existence check request for a path-like locator.
- `issue_load(&mut self, target_buf_id: i32, document_id: String)
  -> CoreVfsRequest`
  Allocates a load request for an already resolved document.
- `issue_save(&mut self, target_buf_id: i32, document_id: String,
  target_locator: Option<String>, text: String, force: bool)
  -> CoreVfsRequest`
  Allocates a revision-aware save request and records the base revision in the
  ledger.
- `apply_response(&mut self, response: CoreVfsResponse)
  -> CoreResponseApplyOutcome`
  Applies one host response to the request ledger and binding state. This is
  the core validation function for stale responses, protocol mismatches,
  cancellations, timeouts, and successful transitions.
- `allocate_request_identity(&mut self) -> (u64, u64)`
  Allocates a monotonic request ID and monotonic issued-order value.
- `is_vfs_buffer(&self, buf_id: i32) -> bool`
  Returns whether the buffer is currently virtual.
- `has_pending_save(&self, buf_id: i32) -> bool`
  Returns whether the buffer has a save request in flight.
- `deferred_close(&self, buf_id: i32) -> Option<CoreDeferredClose>`
  Returns the current deferred close mode, if one exists.
- `set_deferred_close(&mut self, buf_id: i32, close: CoreDeferredClose)`
  Records a deferred close intent and writes a `QuitDeferred` log event.
- `clear_deferred_close(&mut self, buf_id: i32, reason: &str)`
  Clears deferred close state and writes a `QuitResumed` log event.
- `log_quit_denied(&mut self, buf_id: i32, reason: &str)`
  Writes a `QuitDenied` log event without otherwise mutating state.
- `buffer_text_snapshot(&self, buf_id: i32) -> Option<(String, u64)>`
  Returns the current `document_id` and revision. It does not cache text.
- `clear_pending_if_matches(&mut self, buf_id: i32, request_id: u64)`
  Clears `pending_operation` when the currently pending request matches.
- `record_buffer_error(&mut self, buf_id: i32, error: CoreVfsError)`
  Stores the last VFS error on one buffer binding.

### `CoreVfsResponse` internal helper methods

These methods are crate-visible convenience helpers for response processing.

- `request_id(&self) -> u64`
  Returns the response request ID regardless of variant.
- `operation_kind(&self) -> CoreVfsOperationKind`
  Infers the logical operation kind for one response. For `Failed`, the method
  infers the operation from `CoreVfsErrorKind`.

## Internal API in `src/vfd.rs`

This section documents the internal job-bridge implementation. The module is
private, but it intentionally contains `pub` items because the embedded C
runtime accesses them through Rust symbol export boundaries.

### Internal data types

These types model the virtual file descriptor subsystem.

- `pollfd { fd, events, revents }`
  Rust representation of the C `pollfd` struct used by the Vim bridge.
- `POLLIN`
  Read-ready bit for `pollfd.events` and `pollfd.revents`.
- `POLLOUT`
  Write-ready bit for `pollfd.events` and `pollfd.revents`.
- `VfdState { read_queue, is_closed }`
  Per-VFD state. `read_queue` stores bytes that the host injected.
- `JobState { vfd_in, vfd_out, vfd_err, is_closed, status, exit_code, reaped }`
  Per-job state. `reaped` prevents the same terminal job status from being
  reported repeatedly.
- `PendingJobWrite { vfd, data }`
  Captures one pending Vim-to-host job write until the session drains it into a
  host action.
- `VfdManager { vfds, jobs, pending_job_writes, next_vfd }`
  Global manager for all VFD and job state in the process.

### `VfdManager` methods

These methods are the complete internal VFD management API.

- `new() -> Self`
  Creates an empty manager with the next synthetic VFD starting at `512`.
- `register_job(&mut self, job_id: i32, vfd_in: i32, vfd_out: i32,
  vfd_err: i32)`
  Records a job and ensures that the three referenced VFDs exist.
- `ensure_vfd(&mut self, fd: i32)`
  Lazily creates a VFD entry when `fd >= 0` and the entry does not already
  exist.
- `update_job_status(&mut self, job_id: i32, status: i32, exit_code: i32)
  -> bool`
  Stores job status. If the status is terminal, it marks the job as closed and
  closes the associated VFDs to deliver EOF.
- `is_job_input_fd(&self, fd: c_int) -> bool`
  Reports whether one open job currently owns `fd` as its stdin VFD.
- `enqueue_job_write(&mut self, fd: c_int, data: Vec<u8>) -> bool`
  Queues one Vim-to-host write when `fd` is an open job stdin VFD.
- `take_pending_job_write(&mut self) -> Option<PendingJobWrite>`
  Pops one queued Vim-to-host job write for host-action draining.
- `inject_data(&mut self, fd: c_int, data: &[u8]) -> bool`
  Appends bytes to one VFD's read queue if the VFD exists and is still open.
- `read_data(&mut self, fd: c_int, buf: &mut [u8]) -> isize`
  Reads from a VFD queue into the caller-provided buffer.
  It returns:
  - `0` for EOF
  - `-2` when the queue is empty but the VFD is still open
  - `-1` when the VFD does not exist
  - A positive byte count on success
- `close_fd(&mut self, fd: c_int) -> c_int`
  Marks a VFD as closed and returns `0`, or returns `-1` if the VFD does not
  exist.
- `poll_fds(&self, fds: &mut [pollfd]) -> c_int`
  Updates `revents` for each requested VFD and returns the number of ready
  descriptors.
- `clear_all(&mut self)`
  Clears all VFD and job state and resets the synthetic VFD counter.

### Global accessor and exported C symbols

These functions form the internal C ABI that the embedded runtime uses.

- `get_manager() -> MutexGuard<'static, VfdManager>`
  Returns the process-global locked manager.
- `vim_core_vfd_read(fd: c_int, buf: *mut c_void, count: usize) -> isize`
  C-callable wrapper over `VfdManager::read_data`.
- `vim_core_vfd_write(_fd: c_int, _buf: *const c_void, count: usize) -> isize`
  Currently ignores the payload and reports success by returning `count`.
- `vim_core_vfd_close(fd: c_int) -> c_int`
  C-callable wrapper over `VfdManager::close_fd`.
- `vim_core_vfd_poll(fds: *mut pollfd, nfds: c_ulong, _timeout: c_int)
  -> c_int`
  C-callable wrapper over `VfdManager::poll_fds`.
- `vim_core_job_get_status(job_id: c_int, exit_code_out: *mut c_int) -> c_int`
  Returns `1` once when a job has newly ended, `2` after it has already been
  reaped, `0` while it is still running, and `-1` when the job is unknown.
- `vim_core_job_clear(job_id: c_int)`
  Removes one job from the manager after Vim no longer needs it.

## Internal invariants

These invariants are part of the internal API even though they are not
represented by Rust signatures alone.

- `ParsedExIntent` exists only for Ex commands that the crate must intercept
  for host-driven file semantics.
- `DocumentCoordinator` owns request IDs. No other component allocates VFS
  request IDs.
- `DocumentCoordinator` stores revisions, not text. Callers must fetch buffer
  text from the session before issuing a save.
- `VfdManager` is global process state. A dropped `VimCoreSession` clears it to
  avoid leakage across tests or subsequent sessions.
- `drain_native_events` preserves native event ordering as the Rust layer moves
  events into the session-local FIFO queue.

## Next steps

Read `api-contracts.md` when you need the behavioral sequencing that sits on
top of these helpers. Read `public-api-reference.md` when you need the
externally callable surface.
