# Testing

The repository relies heavily on unit and integration tests, with a strong
preference for headless verification. This page explains how the current test
suite is organized and what caveats you must know before you run it.

The most important caveat is the single-session constraint inherited from the
editing core integration.

## Test categories

The current test suite is organized around these groups.

- Unit tests inside `src/*.rs`
- Integration tests under `tests/`
- API surface guard tests
- Runtime callback dispatch tests
- Projection and terminal lifecycle tests

The integration tests are especially valuable because they verify startup,
editing, save or quit behavior, and the TypeScript capability surfaces without
needing a live interactive session.

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
- `tests/integration_editing.rs`
- `tests/integration_save_quit.rs`
- `tests/integration_typescript_config_api.rs`
- `tests/saya_surface_guard.rs`
- `tests/public_surface_guard.rs`

## Next steps

If you want to compare the test suite with the implementation state, read
[Status](status.md).
