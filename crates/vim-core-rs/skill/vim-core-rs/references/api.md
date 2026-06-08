# vim-core-rs API reference

This document covers the public `VimCoreSession` API and how to choose the
right method for a task.

If you are inside the original repository, you can supplement this file with
`docs/public-api-reference.md` for exhaustive symbol coverage and
`docs/api-contracts.md` for cross-cutting behavior contracts. Use this file as
the shorter, task-oriented companion that still works when the skill is copied
standalone.

## Session lifecycle

- `VimCoreSession::new(initial_text: &str) -> Result<Self, CoreSessionError>`
  Create a new core session seeded with text. Fail with
  `CoreSessionError::SessionAlreadyActive` if another session is still alive.
- `VimCoreSession::new_with_options(initial_text: &str, options: CoreSessionOptions)
  -> Result<Self, CoreSessionError>`
  Create a session with explicit initialization options. Set
  `CoreSessionOptions.debug_log_path` to append Rust-side debug logs to a file;
  if it is `None`, debug logs continue to go to stderr.
- `Drop`
  Release the native state and the global single-session lock.

Use the constructor carefully in tests and helper utilities because the crate
does not permit concurrent sessions in one process.

## Commands and evaluation

- `execute_normal_command(&mut self, command: &str)
  -> Result<CoreCommandTransaction, CoreCommandError>`
  Inject a Normal-mode key sequence. Use this for modal editing semantics,
  motions, operators, insert entry, and command sequences such as `dd`, `i`,
  `ZZ`, or `<C-n>`. Inspect the returned transaction when you care about
  emitted events or host actions.
- `execute_ex_command(&mut self, command: &str)
  -> Result<CoreCommandTransaction, CoreCommandError>`
  Execute an Ex command. This path also translates certain commands into
  host-facing intents such as `:edit`, `:write`, `:quit`, and redraw or input
  requests. The returned transaction includes emitted events and host actions.
- `eval_string(&mut self, expr: &str) -> Option<String>`
  Evaluate a Vim expression and return its stringified value.

Check `CoreCommandOutcome` instead of assuming that a command changed text.

## State inspection

- `snapshot(&self) -> CoreSnapshot`
  Capture a coherent snapshot of editor state, including text, revision,
  buffers, windows, pending host actions, mode, and optional completion popup.
- `mode(&self) -> CoreMode`
  Return the current mode.
- `runtime_mode(&self) -> CoreRuntimeMode`
  Return the runtime-mode contract for the active session.
- `pending_input(&self) -> CorePendingInput`
  Return whether Vim is waiting for a char, mark, register, replace target, or
  no additional input.
- `buffers(&self) -> Vec<CoreBufferInfo>`
  Return visible buffer metadata.
- `windows(&self) -> Vec<CoreWindowInfo>`
  Return window layout metadata, including geometry, viewport, active-window
  state, and per-window cursor state.
- `buffer_text(&self, buf_id: i32) -> Option<String>`
  Return the current full text of a specific buffer.

Use `snapshot()` when multiple fields must be internally consistent.

## Navigation, marks, registers, and windows

- `mark(&self, mark_name: char) -> Option<CoreMarkPosition>`
- `set_mark(&mut self, mark_name: char, buf_id: i32, row: usize, col: usize)
  -> Result<(), CoreCommandError>`
- `jumplist(&self) -> CoreJumpList`
- `switch_to_buffer(&mut self, buf_id: i32) -> Result<(), CoreCommandError>`
- `switch_to_window(&mut self, win_id: i32) -> Result<(), CoreCommandError>`
- `register(&self, regname: char) -> Option<String>`
- `set_register(&mut self, regname: char, text: &str)`

Use these APIs for host-driven inspection or setup in tests rather than
simulating every action through Vimscript.

## Options

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

The API is typed and scope-aware. It can report unknown options, type mismatch,
unsupported local scope, and Vim validation failures.

## Host-action queue, events, and VFS inspection

- `take_pending_host_action(&mut self) -> Option<CoreHostAction>`
  Drain the next host-facing action from the queue.
- `take_pending_event(&mut self) -> Option<CoreEvent>`
  Drain the next queued event from the Rust-side FIFO after synchronizing with
  the native event queue.
- `submit_vfs_response(&mut self, response: CoreVfsResponse)
  -> Result<CoreCommandOutcome, CoreCommandError>`
  Feed the result of a host-side VFS operation back into the core.
- `buffer_binding(&self, buf_id: i32) -> Option<CoreBufferBinding>`
  Inspect how a buffer is bound to local or VFS-backed storage.
- `vfs_request_ledger(&self) -> Vec<CoreRequestEntry>`
  Inspect request lifecycle bookkeeping.
- `vfs_transaction_log(&self) -> Vec<VfsLogEntry>`
  Inspect higher-level VFS transaction events.

Use the ledger and log when debugging deferred close, stale saves, or mixed
local and VFS flows.

## Jobs and VFD integration

- `inject_vfd_data(&mut self, vfd: i32, data: &[u8])
  -> Result<(), CoreCommandError>`
  Feed stdout or stderr bytes into a virtual file descriptor previously exposed
  by `CoreHostAction::JobStart`.
- `notify_job_status(&mut self, job_id: i32, status: JobStatus, exit_code: i32)
  -> Result<(), CoreCommandError>`
  Tell the core that a host-managed job finished or failed.

Use these only after the host has handled a `JobStart` action.

## Undo, search, syntax, completion, and UI-facing extraction

- `get_undo_tree(&self, buf_id: i32) -> Result<CoreUndoTree, CoreCommandError>`
- `undo_jump(&mut self, buf_id: i32, seq: i32) -> Result<(), CoreCommandError>`
- `get_search_pattern(&self) -> Option<String>`
- `get_search_direction(&self) -> CoreSearchDirection`
- `is_hlsearch_active(&self) -> bool`
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
- `set_screen_size(&mut self, rows: i32, cols: i32)`

These methods exist so the host can render or inspect Vim-derived state without
screen scraping. Search columns use byte offsets, and visible-search queries
are the preferred pane-local contract because they already combine `hlsearch`,
`:nohlsearch`, `incsearch`, the current pattern, and per-range
`CoreMatchType`.

## Messaging and diagnostics

- `take_pending_event(&mut self) -> Option<CoreEvent>`
  Drain queued events after command execution when you need out-of-band
  structured messages, pager prompts, redraws, or layout notifications.
- `backend_identity(&self) -> CoreBackendIdentity`
  Report whether the session runs against the real upstream runtime or the
  bridge stub.
