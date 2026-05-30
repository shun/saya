# Testing todo

This page tracks the test work that follows ADR 0001. Use it to keep `saya`
focused on host-application behavior around `vim-core-rs` instead of growing a
second compatibility suite for embedded editing semantics.

## Principles

- [x] Keep detailed editing-semantics validation in `vim-core-rs`.
- [x] Add `saya` tests only when they prove host-application value.
- [x] Prefer headless end-to-end coverage over duplicated core-detail checks.
- [x] Revisit this list whenever the live runtime path gains new integration
      points.

## Recommended execution order

Work through this page from top to bottom. The order intentionally starts with
host-layer tests that ADR 0001 clearly assigns to `saya`, then trims existing
core-duplication, then cleans up naming and ownership boundaries.

## 1. Add test cases

These test cases strengthen `saya` as a host application layer. Finish these
before restructuring existing suites.

### 1.1 Startup and session orchestration

- [x] Add a headless test that proves startup warnings are projected into the
      first visible message line.
- [x] Add a headless test that proves startup failure does not leave terminal
      lifecycle state partially initialized.
- [x] Add a test that proves CLI launch state, session guard, and bootstrap
      cleanup remain consistent across repeated start-fail-start cycles.

### 1.2 Save and quit policy

- [x] Add a test that proves `CoreHostAction::Write` updates transient UI
      messages correctly on success.
- [x] Add a test that proves `CoreHostAction::Write` updates transient UI
      messages correctly on failure.
- [x] Add a test that proves local `:wq` host coordination in `saya` follows
      the intended save-then-quit policy instead of assuming core completion.
- [x] Add a test that proves force-quit messaging and normal-quit messaging
      remain distinct in the application layer.

### 1.3 Event loop, input, and rendering integration

- [x] Add a headless integration test for
      `input_loop -> event_loop -> core_bridge -> screen_model`.
- [x] Add a renderer-focused test that proves projected message text, status
      text, and cursor placement stay consistent after one integrated update
      cycle.
- [x] Add a headless integration test for resize handling from terminal event
      through viewport or projection refresh.
- [x] Add a headless integration test that proves redraw events are coalesced
      without dropping non-redraw events.

### 1.4 TypeScript startup and runtime integration

- [x] Add a test that proves startup-registered commands become callable from
      the live runtime path after application boot.
- [x] Add a test that proves runtime callback failures surface as application
      messages without corrupting session state.
- [x] Add a test that proves runtime callback completion can trigger redraw or
      projection refresh in the host path.
- [x] Add a test that proves startup and runtime capability boundaries still
      hold after full application wiring, not only in isolated runtime tests.

### 1.5 Binary-level smoke coverage

- [x] Add a binary-level headless smoke test for opening a file, editing once,
      and quitting cleanly.
- [x] Add a binary-level headless smoke test for opening with `-u init.ts` and
      confirming startup configuration affects the projected UI.
- [x] Add a binary-level headless smoke test for stdin startup and save-path
      restrictions.

## 2. Restructure existing tests

These items keep the current suite aligned with the scope boundary after the
high-priority host tests above exist.

### 2.1 First, classify `tests/integration_editing.rs`

- [x] Review `tests/integration_editing.rs` and mark each case as either
      `host-integration` or `core-duplication`.
- [x] Keep only the representative editing-flow smoke tests in
      `tests/integration_editing.rs`.
- [x] Move detailed cursor-motion expectations out of the critical-path `saya`
      suite when they only restate `vim-core-rs` guarantees.
- [x] Move detailed delete-operation expectations out of the critical-path
      `saya` suite when they only restate `vim-core-rs` guarantees.
- [x] Move visual-selection detail checks out of the critical-path `saya`
      suite when they do not prove projection or rendering integration.

### 2.2 Then, lock in the high-value `saya` suites

- [x] Keep `tests/integration_startup.rs` as the main startup and session
      orchestration suite.
- [x] Keep `tests/integration_save_quit.rs` as the main host save or quit
      policy suite.
- [x] Keep `tests/integration_terminal.rs` as the terminal and presentation
      integration suite.
- [x] Keep `tests/integration_typescript_runtime_config_api.rs`,
      `tests/integration_typescript_runtime_command.rs`, and
      `tests/integration_typescript_runtime_typed_payload.rs` as the TypeScript
      host integration suites.
- [x] Keep `tests/saya_surface_guard.rs` and
      `tests/public_surface_guard.rs` as public-surface boundary suites.

### 2.3 Finally, group files by repository responsibility

- [x] Reorganize startup-related tests under one clear startup-oriented naming
      convention.
- [x] Reorganize terminal, viewport, and rendering tests under one clear
      presentation-oriented naming convention.
- [x] Reorganize TypeScript startup and runtime tests under one clear runtime
      integration naming convention.
- [x] Add a short file-level comment to each major integration test file that
      states the host-layer responsibility it proves.

## 3. Explicitly out of scope for `saya`

These items should not grow as first-class `saya` test work unless the host
integration itself is under test.

- [x] Do not add exhaustive register-behavior coverage here.
- [x] Do not add exhaustive mark and jumplist coverage here.
- [x] Do not add exhaustive undo-tree coverage here.
- [x] Do not add exhaustive search, syntax, or pop-up menu extraction coverage
      here.
- [x] Do not add detailed VFS protocol or job protocol contract suites here
      when `vim-core-rs` already owns them.

## 4. Next review

- [x] Review this list after the main TUI loop gains fuller
      `SayaLiveRuntime` integration.
- [x] Review this list after any large test migration between `saya` and
      `vim-core-rs`.

### Review log: March 28, 2026

This historical repository review re-inventoried the `saya` test suite against
ADR 0001 and the then-current `vim-core-rs` contract suites after a large
migration of detailed editing semantics into `vim-core-rs`. It is retained as
context, not as a current complete inventory of every test file.

#### `saya` tests that remain clearly host/application integration

- `tests/integration_startup.rs`
- `tests/integration_startup_boot_flow.rs`
- `tests/integration_startup_tab_size.rs`
- `tests/integration_save_quit.rs`
- `tests/integration_terminal.rs`
- `tests/integration_presentation_line_numbers.rs`
- `tests/integration_typescript_runtime_config_api.rs`
- `tests/integration_typescript_runtime_command.rs`
- `tests/integration_typescript_runtime_typed_payload.rs`
- `tests/integration_binary_smoke.rs`
- `tests/integration_config.rs`
- `tests/callback_registry_seed.rs`
- `tests/startup_runtime_scaffold.rs`
- `tests/public_api_declaration.rs`
- `tests/public_surface_guard.rs`
- `tests/saya_surface_guard.rs`

These files still prove startup preparation, terminal lifecycle, screen
projection, save or quit policy, runtime callback wiring, public API gating,
and binary-level orchestration. Those responsibilities stay in `saya`.

#### `saya` tests that remain representative host/application smoke

- `tests/integration_editing.rs`

This file still has legitimate host smoke value because it exercises input
routing, bridge dispatch, dirty-state projection, and viewport coordination
through the embedded core. The detailed editing-semantics assertions that used
to create duplication risk now live in `vim-core-rs`.

Current case-by-case classification inside `tests/integration_editing.rs`:

- Keep `mode_transition_flow_through_input_router_to_screen_model` as
  host/application smoke because it proves `input_router -> core_bridge ->
  screen_model` handoff, but avoid growing the `CoreMode` assertions into a
  mode-semantics matrix.
- Keep `viewport_auto_scroll_keeps_cursor_visible_during_vertical_motion` in
  `saya` because viewport visibility and projected cursor placement remain
  presentation concerns.
- Keep `visual_inner_word_selection_is_projected_for_rendering` as render
  handoff smoke only. `saya` now checks only that a core-owned selection can be
  handed off and projected.
- Keep `text_input_reflected_in_screen_model_lines` as representative
  host-side smoke because it only proves that projected lines follow one edit
  path.
- Keep `tab_size_setting_changes_screen_projection_for_tabs` in `saya`
  because tab expansion and cursor display cells are presentation concerns.
- Keep `dirty_state_follows_editing_in_screen_model` and
  `dirty_state_set_after_delete_operation` in `saya` because the application
  layer owns dirty projection even when edits originate in the core.
- Keep `full_editing_flow_mode_move_insert_delete` as end-to-end host smoke
  only. `saya` now checks that routed keys lead to projected updates and dirty
  state, not the exact insert, cursor, or delete semantics.
- Keep viewport visibility, tab-size projection, and dirty-state projection in
  `saya` because those remain presentation or application concerns even when
  they are driven by core snapshots.

This migration moved the detailed ownership boundary to the following
`vim-core-rs` suites:

- `vim-core-rs/tests/visual_selection_contract.rs`
- `vim-core-rs/tests/mode_transition_contract.rs`
- `vim-core-rs/tests/normal_deletion_contract.rs`

Keep future `saya` changes in this file at the smoke-test level unless a new
host/application integration responsibility appears.
