# vim-core-rs bundled internal API reference

This file describes the non-public APIs that implement `vim-core-rs`. Treat it
as the implementation map for code review, refactoring, and repository
maintenance.

## Internal structure overview

- `src/lib.rs`
  Owns the public facade, Ex intent parsing, host-action draining, option
  conversion, snapshot conversion, and FFI result translation.
- `src/vfs.rs`
  Owns virtual document coordination, request ledger state, deferred close, and
  VFS transaction logging.
- `src/vfd.rs`
  Owns job I/O emulation through virtual file descriptors and exported Rust
  symbols that Vim calls through the bridge.

## `src/lib.rs`

### Internal enums

- `ParsedExIntent`
  `Edit { locator }`, `Write { path, force }`, `Update { path, force }`,
  `SaveAndClose { force }`, `SaveIfDirtyAndClose`, `Quit { force }`

### Internal `VimCoreSession` helpers

- `invoke_native_normal_command(&mut self, command: &str)
  -> Result<(CoreCommandOutcome, CoreSnapshot), CoreCommandError>`
- `invoke_native_ex_command(&mut self, command: &str)
  -> Result<(CoreCommandOutcome, CoreSnapshot), CoreCommandError>`
- `apply_intent(&mut self, intent: ParsedExIntent)
  -> Result<CoreCommandOutcome, CoreCommandError>`
- `apply_write_intent(&mut self, path: String, force: bool,
  _deferred_close: Option<CoreDeferredClose>)
  -> Result<CoreCommandOutcome, CoreCommandError>`
- `apply_loaded_buffer(&mut self, buf_id: i32, display_name: &str, text: &str)
  -> Result<(), CoreCommandError>`
- `drain_native_host_actions(&mut self)`
- `drain_native_events(&mut self)`
- `get_option_value(&self, name: &str, scope: CoreOptionScope,
  expected: CoreOptionType) -> Result<ConvertedOptionValue, CoreOptionError>`

These helpers are the policy boundary for intercepted Ex commands, message
polling, and host-action queue behavior.

### Conversion and parser helpers

- `convert_command_result`
- `option_name_to_cstring`
- `option_value_to_cstring`
- `convert_status`
- `convert_snapshot`
- `convert_pum_info`
- `c_str_to_string`
- `convert_buffer_list`
- `convert_window_list`
  Converts the FFI window array into `Vec<CoreWindowInfo>`, including
  geometry, viewport state, active-window state, and per-window cursor state.
- `convert_mode`
- `convert_pending_input`
- `convert_mark_position`
- `convert_jumplist`
- `convert_jumplist_entries`
- `convert_undo_tree`
- `parse_ex_intent`
- `convert_host_action`
- `convert_input_kind`
- `convert_option_scope`
- `convert_option_type`
- `convert_option_get_result`
- `convert_option_set_result`
- `validate_search_viewport`
- `resolve_active_search_window_id`
- `is_error_message`
- `string_from_parts`

### Internal-only types

- `bindings`
  Private bindgen module containing raw bridge and runtime symbols
- `ConvertedOptionValue`
  Internal staging enum used by typed option getters

## `src/vfs.rs`

### Internal structs and enums

- `CoreResponseApplyOutcome`
  `Applied`, `StaleRejected`, `ProtocolMismatchRejected`, `UnknownRequest`
- `BufferState`
  Holds `binding` and `current_revision`
- `DocumentCoordinator`
  Owns `next_request_id`, `next_issued_order`, `bindings`, `requests`,
  `transaction_log`

### `BufferState` helper

- `local(buf_id: i32, display_name: String) -> Self`

### `DocumentCoordinator` methods

- `new() -> Self`
- `transaction_log(&self) -> &[VfsLogEntry]`
- `emit_log(&mut self, entry: VfsLogEntry)`
- `log_binding_event(&mut self, buf_id: i32, event: VfsLogEvent,
  detail: Option<String>)`
- `sync_buffers(&mut self, buffers: &[(i32, String)])`
- `binding(&self, buf_id: i32) -> Option<&CoreBufferBinding>`
- `request_entry(&self, request_id: u64) -> Option<CoreRequestEntry>`
- `ledger_entries(&self) -> Vec<CoreRequestEntry>`
- `bind_virtual_document(&mut self, buf_id: i32, locator: Option<String>,
  document_id: String, display_name: String, committed_revision: u64)`
- `bind_local_buffer(&mut self, buf_id: i32, locator: Option<String>,
  display_name: String, committed_revision: u64)`
- `commit_loaded_revision(&mut self, buf_id: i32, revision: u64)`
- `note_buffer_revision(&mut self, buf_id: i32, revision: u64)`
- `issue_resolve(&mut self, target_buf_id: i32, locator: String)
  -> CoreVfsRequest`
- `issue_exists(&mut self, locator: String) -> CoreVfsRequest`
- `issue_load(&mut self, target_buf_id: i32, document_id: String)
  -> CoreVfsRequest`
- `issue_save(&mut self, target_buf_id: i32, document_id: String,
  target_locator: Option<String>, text: String, force: bool)
  -> CoreVfsRequest`
- `apply_response(&mut self, response: CoreVfsResponse)
  -> CoreResponseApplyOutcome`
- `allocate_request_identity(&mut self) -> (u64, u64)`
- `is_vfs_buffer(&self, buf_id: i32) -> bool`
- `has_pending_save(&self, buf_id: i32) -> bool`
- `deferred_close(&self, buf_id: i32) -> Option<CoreDeferredClose>`
- `set_deferred_close(&mut self, buf_id: i32, close: CoreDeferredClose)`
- `clear_deferred_close(&mut self, buf_id: i32, reason: &str)`
- `log_quit_denied(&mut self, buf_id: i32, reason: &str)`
- `buffer_text_snapshot(&self, buf_id: i32) -> Option<(String, u64)>`
- `clear_pending_if_matches(&mut self, buf_id: i32, request_id: u64)`
- `record_buffer_error(&mut self, buf_id: i32, error: CoreVfsError)`

### `CoreVfsResponse` internal helpers

- `request_id(&self) -> u64`
- `operation_kind(&self) -> CoreVfsOperationKind`

## `src/vfd.rs`

### Internal data types

- `pollfd { fd, events, revents }`
- `POLLIN`
- `POLLOUT`
- `VfdState { read_queue, is_closed }`
- `JobState { vfd_in, vfd_out, vfd_err, is_closed, status, exit_code, reaped }`
- `VfdManager { vfds, jobs, next_vfd }`

### `VfdManager` methods

- `new() -> Self`
- `register_job(&mut self, job_id: i32, vfd_in: i32, vfd_out: i32, vfd_err: i32)`
- `ensure_vfd(&mut self, fd: i32)`
- `update_job_status(&mut self, job_id: i32, status: i32, exit_code: i32)
  -> bool`
- `inject_data(&mut self, fd: c_int, data: &[u8]) -> bool`
- `read_data(&mut self, fd: c_int, buf: &mut [u8]) -> isize`
- `close_fd(&mut self, fd: c_int) -> c_int`
- `poll_fds(&self, fds: &mut [pollfd]) -> c_int`
- `clear_all(&mut self)`

### Global accessor and exported C symbols

- `get_manager() -> MutexGuard<'static, VfdManager>`
- `vim_core_vfd_read(fd: c_int, buf: *mut c_void, count: usize) -> isize`
- `vim_core_vfd_write(_fd: c_int, _buf: *const c_void, count: usize) -> isize`
- `vim_core_vfd_close(fd: c_int) -> c_int`
- `vim_core_vfd_poll(fds: *mut pollfd, nfds: c_ulong, _timeout: c_int)
  -> c_int`
- `vim_core_job_get_status(job_id: c_int, exit_code_out: *mut c_int) -> c_int`
- `vim_core_job_clear(job_id: c_int)`

## Internal invariants

- `ParsedExIntent` exists only for Ex commands that must be intercepted for
  host-driven file semantics.
- `DocumentCoordinator` owns VFS request IDs.
- `DocumentCoordinator` stores revisions, not text. Callers must fetch text
  from the session before issuing a save.
- `VfdManager` is process-global state. Dropping a `VimCoreSession` clears it.
- `drain_native_events` preserves native event ordering as the Rust layer
  moves them into the transaction/result queue.
- `validate_search_viewport` rejects rows below 1, rows below `end_row`, and
  inverted ranges before any native query runs.
- `resolve_active_search_window_id` is only used for the active-window search
  convenience method. The window-targeted search API still requires an
  explicit `window_id`.

## Reading guidance

- Read [api-contracts.md](api-contracts.md) when you need sequencing on top of
  these helpers.
- Read [public-api-reference.md](public-api-reference.md) when you need the
  externally callable surface.
