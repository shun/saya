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

ADR 0002 defines the test architecture inside that boundary. `saya` now
organizes test work around three headless-friendly layers: local unit and
narrow integration tests, functional harness tests, and PTY-backed end-to-end
tests.

## Test categories

The current test suite is moving toward these groups.

- Layer 1 unit and narrow integration tests inside `src/*.rs`
- Layer 2 functional harness tests under `tests/`
- Layer 2 API surface and runtime integration tests
- Layer 3 PTY-backed end-to-end tests

Layer 2 tests are especially valuable because they verify startup, save or
quit flows, projection, terminal behavior, and TypeScript capability surfaces
without needing a live interactive session. Layer 3 then validates the real
process and terminal boundary with a smaller representative suite.

## Testing boundary

Use this repository to prove application behavior that sits above the editing
core.

- Test CLI startup, launch preparation, and session lifecycle here.
- Test save or quit policy, message projection, viewport behavior, terminal
  lifecycle, input routing, and event-loop orchestration here.
- Test startup TypeScript evaluation and runtime callback integration here.
- Prefer headless end-to-end coverage over duplicated core-detail checks.
- Add saya tests only when they prove host-application value.
- Default new host-behavior tests to Layer 2 unless the behavior is clearly
  local logic or clearly requires the real terminal process boundary.
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
- Do not add exhaustive search, syntax, highlight, conceal, or pop-up menu
  extraction coverage here. Keep detailed search, syntax, highlight, conceal,
  and pop-up menu extraction semantics in `vim-core-rs`.
- Do not add detailed VFS protocol or job protocol contract suites here when
  `vim-core-rs` already owns them.

Syntax, highlight, and conceal tests in `saya` must stay at the presentation
boundary. They can prove that public data from `vim-core-rs`, such as
`CoreSyntaxChunk` values, is collected for visible rows, projected into display
cells, layered with search or visual overlays, and rendered without mutating
buffer text. They must not prove Vim-compatible extraction semantics,
`:highlight` attribute resolution, or `matchadd()` conceal behavior.

Markdown WYSIWYG tests are different from Vim syntax and conceal extraction
tests. The Markdown document map is host-side presentation metadata used by
`saya` to decide rich display, raw block expansion, and display-space
projection. These tests belong here only while they verify the CLI presentation
path and preserve the raw buffer owned by `vim-core-rs`.

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

Run the notification prompt contract suite with its pinned acceptance command.

```bash
gtimeout 120 cargo test notification_prompt -- --test-threads=1
```

Run the public surface guard suite with its pinned acceptance command.

```bash
gtimeout 120 cargo test public_surface_guard -- --test-threads=1
```

Run the structural refresh and redraw acceptance suites with this pinned
headless command.

```bash
gtimeout 30s cargo test --test structural_refresh_contract && gtimeout 30s cargo test --test core_outcome_contract && gtimeout 30s cargo test --test tui_render_coordinator && gtimeout 30s cargo test --test integration_terminal && gtimeout 30s cargo test --test public_surface_guard
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
[ADR 0001](adr/0001-define-saya-scope-against-vim-core-rs.md). If you need the
test-layer decision, read
[ADR 0002](adr/0002-adopt-three-layer-headless-test-architecture.md). If you
need the target harness design, read
[Test architecture design](design/test-architecture.md).
