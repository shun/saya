# Testing

The repository relies heavily on unit and integration tests, with a strong
preference for headless verification. This page explains how the current test
suite is organized and what caveats you must know before you run it.

The most important caveat is the single-session constraint inherited from the
editing core integration.

ADR 0001 defines the testing boundary that matters most here: `saya` tests the
host application layer around `vim-core-rs`, while `vim-core-rs` remains the
source of truth for detailed editing semantics and in-scope upstream Vim
compatibility.

## Test categories

The current test suite is organized around these groups.

- Unit tests inside `src/*.rs`
- Integration tests under `tests/`
- API surface guard tests
- Runtime callback dispatch tests
- Projection and terminal lifecycle tests

The integration tests are especially valuable because they verify startup,
save or quit flows, projection, terminal behavior, and TypeScript capability
surfaces without needing a live interactive session.

## Testing boundary

Use this repository to prove application behavior that sits above the editing
core.

- Test CLI startup, launch preparation, and session lifecycle here.
- Test save or quit policy, message projection, viewport behavior, terminal
  lifecycle, input routing, and event-loop orchestration here.
- Test startup TypeScript evaluation and runtime callback integration here.
- Prefer headless end-to-end coverage over duplicated core-detail checks.
- Add saya tests only when they prove host-application value.
- Keep detailed core compatibility work in `vim-core-rs`, including most
  fine-grained editing semantics and upstream Vim compatibility cases.
- Do not add detailed editing-semantics validation here. Keep that validation
  in `vim-core-rs`.

`saya` may keep smoke tests for representative editing flows when they prove
host integration, but it must not grow into a second exhaustive compatibility
suite for core editor behavior.

- Do not add exhaustive register-behavior coverage here. Keep detailed
  register semantics in `vim-core-rs`.
- Do not add exhaustive mark and jumplist coverage here. Keep detailed mark
  and jumplist semantics in `vim-core-rs`.
- Do not add exhaustive undo-tree coverage here. Keep detailed undo-tree
  semantics in `vim-core-rs`.
- Do not add exhaustive search, syntax, or pop-up menu extraction coverage
  here. Keep detailed search, syntax, and pop-up menu extraction semantics in
  `vim-core-rs`.
- Do not add detailed VFS protocol or job protocol contract suites here when
  `vim-core-rs` already owns them.

## Useful commands

Use these commands from the repository root.

List all tests with this command.

```bash
cargo test -- --list
```

Run the suite serially with this command.

```bash
gtimeout 120 cargo test -- --test-threads=1
```

## Why serial execution matters

`saya` uses a single-session guard because `vim-core-rs` permits only one live
session per process. When tests run in parallel, some session-oriented tests
can fail with `SessionAlreadyActive`.

Serial execution does not fix every failing test, but it removes the most
obvious concurrency-related noise.

## Current known issue

As of March 23, 2026, the full serial test suite still has one known failing
unit test.

- `bootstrap::tests::extracts_initial_tab_size_from_config_file`

At the same time, the TypeScript startup and runtime integration tests pass,
which means the public capability surfaces are in better shape than the single
failing bootstrap assertion suggests.

## High-value test files

If you want to understand the repository through tests, start with these files.

- `tests/integration_startup.rs`
- `tests/integration_save_quit.rs`
- `tests/integration_terminal.rs`
- `tests/integration_typescript_runtime_config_api.rs`
- `tests/integration_typescript_runtime_command.rs`
- `tests/integration_typescript_runtime_typed_payload.rs`
- `tests/saya_surface_guard.rs`
- `tests/public_surface_guard.rs`

## Next steps

If you want to compare the test suite with the implementation state, read
[Status](status.md). If you need the formal scope boundary behind this testing
strategy, read
[ADR 0001](adr/0001-define-saya-scope-against-vim-core-rs.md).
