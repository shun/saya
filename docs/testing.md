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

## State-based acceptance

Behavioral tests must assert the final state that the user or caller depends
on. A log line, command dispatch, callback invocation, or UI event is useful
evidence that a path was exercised, but it is not enough when the behavior is
supposed to change application state.

Use read-back assertions as the default acceptance shape:

- If an operation edits a buffer, assert `buffer_text()` or the equivalent
  readonly buffer snapshot after the operation.
- If an operation saves or writes a file, read the file back and assert its
  contents or metadata.
- If an operation changes runtime-visible state, query the public runtime or
  host surface again and assert the returned state.
- If an operation opens, closes, selects, or reorders UI, assert the projected
  model, floating window state, selected row, or visible item list. If the UI
  action also changes buffer/file state, assert that state too.
- If an operation is expected to emit diagnostics or logs, assert the relevant
  log line only as a path or observability check. Do not use logs as the only
  success criterion for a state-changing behavior.

When adding a smoke test, make it fail for the common false positive: the
command ran and logged something, but the buffer, file, or runtime state stayed
unchanged.

## Runtime API side effects

Tests that exercise TypeScript runtime APIs must prove whether the API is
readonly or mutating. A fake host that turns every API into a harmless stub can
hide the exact failure that users see in the editor.

Use explicit side-effect boundaries in tests:

- If a source, filter, sorter, or query helper only reads editor, filesystem, or
  workspace state, call a readonly API and make the fake host fail when a
  mutating API is used.
- If an API opens, edits, projects, or refreshes a buffer, name that behavior in
  the API or test and assert the changed buffer/window/session state.
- If a readonly feature lists the filesystem, assert that the active buffer text
  or saved file contents remain the expected user buffer after the operation.
- Do not mock a mutating API as a readonly API in unit tests. Either expose a
  readonly surface for the feature, or add a Layer 2 or binary smoke that proves
  the mutating behavior does not occur.

For bundled completion, path completion must use `saya.fs.readDir()` rather
than `saya.filer.list()`. `saya.filer.list()` projects a directory listing into
the active buffer as part of the dired/filer workflow, so using it from a
completion source corrupts the edited buffer.

## TypeScript startup callback boundary

Startup-registered commands and events cross a source serialization boundary.
The startup runtime records callbacks with `Function.prototype.toString()` and
the live runtime evaluates that callback source later. A startup smoke test
that only proves config evaluation and keymap registration is not enough for
commands that users invoke after boot.

When you change bundled plugins or startup APIs that register commands or
events, add a test that proves the callback works after this boundary. The test
must run the registered callback source without the original startup closure.
Use the narrowest layer that proves the behavior:

- For bundled TypeScript plugin logic, add a Deno test that registers the
  command, rebuilds the callback with `new Function("return (" +
  callback.toString() + ");")()`, and executes it with only the public
  `saya.*` runtime surface.
- For host integration behavior, add a Layer 2 Rust test that starts from
  startup config, builds the live runtime from `CallbackRegistrySeed`, invokes
  the registered command or event, and observes the host side effect or message
  line.
- For real executable coverage, use Layer 3 only when the process or terminal
  boundary matters.

These tests must fail if the callback depends on closed-over startup variables,
module-local helper values that are not included in the callback source, or APIs
that exist only in the startup runtime. This is the guard against silent
"registered successfully, fails when pressed" regressions.

## Completion UX acceptance

Completion tests must prove the user-visible editing workflow, not only the
source or request shape. For changes under bundled completion, typed completion
host handling, input routing, PUM/floating windows, or completion confirm
application, a sufficient verification path must include these observations.

- More than one candidate is available when the behavior depends on ordering or
  selection.
- The rendered menu identifies the selected row, and a navigation key such as
  Down changes that selected row.
- Enter confirms the selected candidate, not just the first candidate.
- The resulting buffer text or saved file contents are asserted after confirm.
  Logs may be used to prove the intermediate menu state, but logs are not a
  substitute for a buffer/file contents assertion.
- When a startup keymap or bundled plugin is involved, include a binary smoke or
  Layer 2 live-runtime test that crosses the startup registry boundary.

The canonical regression shape is a file containing:

```text
ty
type
typed
```

The completion smoke should open completion at `ty`, verify both `type` and
`typed` are present, move selection to `typed`, confirm, and assert the final
contents:

```text
typed
type
typed
```

Use the focused test when touching this path:

```bash
gtimeout 120 cargo test --test integration_binary_smoke bundled_completion_binary_smoke_can_select_second_candidate
```

Path completion has an additional guard because it reads filesystem state. The
smoke must prove that `./` completion keeps the original file buffer and only
replaces the typed path prefix:

```bash
gtimeout 120 cargo test --test integration_binary_smoke bundled_path_completion_does_not_replace_buffer_with_directory_listing
```

For release verification, rebuild and run the same smoke through
`target/release/sy` or another explicit release binary path. Do not assume a
debug binary proves the release binary the user is running.

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
