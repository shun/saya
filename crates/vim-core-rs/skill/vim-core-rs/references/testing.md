# vim-core-rs testing map

Use this file to choose the smallest test suite that proves a change.

If you are inside the original repository, read `docs/api-contracts.md`
alongside this map for the higher-level sequencing rules behind the tests.
Do not rely on that file being present when this skill is used standalone.

## Baseline rules

- For repository development, prefer `VIM_CORE_FROM_SOURCE=1 cargo test`.
- Do not treat a bare `cargo test` as the repository default unless you are
  intentionally validating the prebuilt-consumer path.
- Run `cargo test --test upstream_vim_generated` when the change could alter
  embedded Vim behavior relative to upstream Vim, especially after touching
  `vendor/upstream/vim`, `vendor/vim_src`, `native/`, `build.rs`, or command
  execution paths with Vimscript-visible effects.

## Session and public API contracts

- `tests/public_api_contract.rs`
  Session creation and drop behavior, single-session rejection, option getter
  and setter behavior, backend identity, basic editing outcomes,
  `CoreSessionOptions.debug_log_path` file logging, and typed API contracts.
- `tests/integration_contract.rs`
  Ex-command side effects, host-action emission, mark and jumplist consistency,
  mode and pending-input state, and normal-command integration behavior.

Use these first for most changes in `src/lib.rs`.

## VFS and file-oriented flows

- `tests/vfs_contract.rs`
  Resolve, load, save, local fallback, request ledger state, transaction log
  events, deferred close, stale save handling, and VFS-backed quit behavior.
- `tests/quit_contract.rs`
  Quit-related behaviors that are not purely VFS specific.
- `tests/ex_command_contract.rs`
  Ex parsing and user-visible command behavior.

Use these when touching `src/vfs.rs`, Ex intent parsing, or host-action
sequencing around file operations.

## Jobs and VFD bridging

- `tests/job_contract.rs`
  Job host-action basics.
- `tests/job_api_contract.rs`
  End-to-end host interaction through `JobStart`, `inject_vfd_data()`, and
  `notify_job_status()`, including invalid-input behavior after cleanup.

Use these when changing job control, VFD management, or the public job API.

## Editing state, navigation, and registers

- `tests/mode_transition_contract.rs`
  Mode transitions across editing commands.
- `tests/pending_input_contract.rs`
  Pending input states such as char, mark, and register waits.
- `tests/sequential_dispatch_contract.rs`
  Single-key `dispatch_key()` behavior, UI-style sequential input,
  pending-prefix transitions, counted motion sequences, and parity with direct
  `execute_normal_command()` where applicable.
- `tests/mark_contract.rs`
  Mark set and retrieval behavior.
- `tests/jumplist_contract.rs`
  Jump history extraction.
- `tests/register_contract.rs`
  Register round-trips and editing semantics.
- `tests/undo_contract.rs`
  Undo tree extraction and undo navigation.

Use these when changing modal state, motion, history, or register behavior.

## Rendering-facing extraction APIs

- `tests/search_highlight_contract.rs`
  Search pattern state, direction, highlight ranges, and cursor match metadata.
- `tests/incsearch_contract.rs`
  Visible-search-state queries, `incsearch` live preview, and viewport query
  error handling.
- `tests/search_highlight_c_contract.rs`
  Lower-level search highlight integration.
- `tests/syntax_contract.rs`
  Syntax chunk extraction.
- `tests/pum_contract.rs`
  Completion popup extraction and selected-index behavior.
- `tests/scroll_viewport_contract.rs`
  Window viewport and scrolling state.
- `tests/multi_buffer_window_contract.rs`
  Buffer and window extraction, split semantics, per-window cursor state,
  `Ctrl-w` navigation and resize contracts, and inactive-window search queries.
- `tests/multi_buffer_window_integration.rs`
  Multi-buffer workflow coverage across more realistic flows.

Use these when changing data consumed by a renderer or UI shell.

## Messages, eval, and regressions

- `tests/message_log_contract.rs`
  `eval_string()`, pending-event delivery, and structured message routing with
  `CoreMessageSeverity` and `CoreMessageCategory`.
- `tests/repro_e182.rs`
  Regression coverage for a specific Vim error path.
- `tests/repro_prefix_conflict.rs`
  Regression coverage for key-prefix or parsing conflicts.

Use these when working near message polling, Vim expression evaluation, or
known regressions.

## Build and repository integrity

- `tests/build_modules_contract.rs`
  Build helper integration.
- `tests/quality_gate_contract.rs`
  Audit reports and upstream build fingerprint traceability.
- `tests/purification_test.rs`
  Repository hygiene and isolation checks.
- `tests/upstream_vim_generated.rs`
  Generated upstream-facing contract coverage.

Use these when touching `build.rs`, audit helpers, vendor sync, or generated
native integration surfaces.

## Practical selection rule

Pick one narrow suite first, then expand only if the change crosses boundaries.

- Rust public API change:
  `public_api_contract`
- Ex or host-action behavior:
  `integration_contract`
- UI-style single-key dispatch or pending prefixes:
  `sequential_dispatch_contract`
- Message delivery or Vim expression evaluation:
  `message_log_contract`
- VFS:
  `vfs_contract`
- Jobs:
  `job_api_contract`
- Search or syntax:
  `search_highlight_contract`, `incsearch_contract`, or `syntax_contract`
- Completion UI extraction:
  `pum_contract`
- Window layout, split, viewport, or `Ctrl-w` semantics:
  `multi_buffer_window_contract`
- Native build or vendor pipeline:
  `quality_gate_contract`
