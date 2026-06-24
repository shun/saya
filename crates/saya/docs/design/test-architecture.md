# Test architecture design

This page explains the target three-layer test architecture for `saya`. It
turns ADR 0002 into an implementation plan for a deterministic functional
harness plus a smaller PTY-backed end-to-end layer.

ADR 0002 defines the decision. This page describes how to realize that
decision in code and how to migrate the current suite without losing coverage.

## Goals

The testing architecture must give you fast feedback, high confidence, and a
clear answer to where a new test belongs.

- Keep detailed editing semantics in `vim-core-rs`.
- Make Layer 2 the default home for new host-application behavior.
- Keep Layer 3 focused on the real process and terminal boundary.
- Avoid flaky timing-based tests.
- Make state and screen assertions reusable across test files.

## Layer definitions

The repository uses these three layers.

### Layer 1: Unit and narrow integration

Layer 1 validates local logic with direct data assertions.

- Examples: `screen_model`, `editor_session`, `event_loop`,
  `terminal_lifecycle`, and parser tests in `src/*.rs`.
- Private helper contracts may live in sibling test files loaded with
  `#[cfg(test)]` and `#[path = "..."] mod tests;`, so production files do not
  accumulate large inline test modules.
- Typical assertion style: compare structs, enums, messages, and small
  normalized outputs.
- Typical failure shape: one module contract regressed.

### Layer 2: Functional harness

Layer 2 drives a realistic host-application path without a PTY.

- Start from launch preparation or terminal startup helpers.
- Drive input, runtime events, resize events, command-line operations, save or
  quit flows, and redraw decisions through one shared harness API.
- Observe both semantic state and projected screen state.
- Keep the harness deterministic by using explicit event drains and redraw
  checkpoints instead of sleeps.

This is the primary layer for new `saya` behavior.

### Layer 3: PTY-backed end-to-end

Layer 3 launches the real executable in a PTY.

- Use the real `sy` process.
- Assert terminal lifecycle behavior that only exists at the process boundary.
- Keep coverage representative and focused.
- Capture useful artifacts when a test fails, such as screen snapshots,
  stderr, exit status, and PTY transcript fragments.

This layer is intentionally smaller than Layer 2.

## Functional harness design

The Layer 2 harness should present one reusable session model to tests. The
harness API does not need to mirror public product APIs. It needs to make
deterministic host-path testing easy.

### Core test object

Create a `TestSession`-style helper that owns the moving pieces currently
assembled manually across integration tests.

- Launch request and startup outcome
- Editor session state
- Optional runtime session owner
- Event-loop coordinator and sender
- Viewport state
- Transient message state
- Command-line state
- Optional recording terminal backend

### Driver operations

The harness should expose small operations that match user-visible flows.

- `start(...)`
- `feed_key(...)`
- `feed_keys(...)`
- `enter_command_line()`
- `type_command(...)`
- `submit_command()`
- `send_resize(...)`
- `dispatch_runtime_event(...)`
- `drain_until_idle()`
- `render_snapshot()`
- `shutdown()`

These operations should drive the same orchestration decisions that `main`
uses today, but in a deterministic test-owned environment.

### Assertion surfaces

The harness should expose multiple assertion surfaces because terminal apps
have more than one important truth.

- Semantic snapshot: mode, dirty state, cursor position, target path, pending
  messages, shutdown reason, and runtime side effects
- Screen snapshot: visible lines, cursor row or column, status line, message
  line, line-number projection, and visual-selection projection
- Event or trace snapshot: redraw requested, host action processed, runtime
  follow-up events dispatched, terminal restore recorded

Tests should prefer the narrowest assertion surface that proves the intended
behavior.

### Determinism rules

Layer 2 must not become a sleep-heavy pseudo-E2E suite.

- Wait on explicit events, queue drains, and redraw conditions.
- Fix terminal dimensions in test setup.
- Keep runtime callbacks on explicit worker boundaries while exposing stable
  completion hooks to the harness.
- Record transient messages and redraw requests through normalized helpers.

## PTY-backed test design

Layer 3 exists to prove the real process boundary.

### Scope

Use PTY tests for these kinds of behaviors.

- Real raw-mode and alternate-screen entry or restore
- Real process shutdown sequencing
- Real stdin or stdout or stderr interaction at the executable boundary
- Real command-line editing and screen refresh through the executable
- Real terminal-control regressions where the byte stream matters, such as
  accidental `ESC[2J` full-screen clears, `ESC[2K` line clears, cursor-style
  rewrites, or flush patterns during command-line typing

Do not use PTY tests for every editing permutation or runtime edge case that
Layer 2 can already prove more deterministically.

### Expected PTY helper shape

Create a PTY runner with a narrow, reusable API.

- `spawn_sy(...)`
- `write_input(...)`
- `read_until(...)`
- `expect_screen(...)`
- `expect_exit(...)`
- `collect_transcript()`

The PTY helper should normalize terminal size, environment variables, working
directory, and timeout handling.

The helper should also expose terminal transcript assertions. Redraw-sensitive
tests need to count terminal-control sequences, not only visible text. For
example, a command-line typing scenario can assert that `:syntax on` emits no
`ESC[2J` full-screen clear while characters are being typed, while still
allowing an initial full draw and the normal redraw after command submission.

## Migration plan

The current suite already contains useful building blocks. Migrate it in this
order.

1. Extract shared Layer 2 harness utilities from the current integration
   tests.
2. Split large inline test modules into sibling test files by responsibility,
   while keeping private helper tests inside the owning module boundary.
3. Move feature-specific tests from binary orchestration modules back to the
   owning feature or presentation module.
4. Move `main`-path command-line, runtime, redraw, and shutdown scenarios onto
   the harness.
5. Replace the custom binary smoke shortcut with real Layer 3 PTY tests.
6. Keep a small number of representative PTY scenarios and push most behavior
   back down to Layer 2.

## Mapping from the current suite

The existing files map into the new structure like this.

- `src/*.rs` tests remain Layer 1.
- Large `mod tests` blocks in `src/*.rs` should be split into sibling test
  files when they cover more than one local contract.
- Most `tests/integration_*.rs` files move toward Layer 2.
- `tests/integration_binary_smoke.rs` is a temporary Layer 3 precursor and
  should be replaced by PTY-backed tests.

## Next steps

After the harness exists, update `docs/testing.md` to describe the concrete
commands and the new test categories in operational detail.
