# ADR 0002: Adopt a three-layer headless test architecture

## Status

Accepted

## Context

`saya` already has many headless tests, but the current suite mixes several
testing styles without a single architecture that explains why each style
exists.

- Unit tests in `src/*.rs` validate isolated logic.
- Integration tests under `tests/` often assemble internal components
  directly.
- Binary smoke tests exercise a real executable path, but today they bypass
  parts of the live main-loop wiring.

That structure gives useful coverage, but it does not yet define a stable
testing ladder like the one used by more mature editor projects such as
Neovim.

The repository therefore needs an explicit test architecture decision that
answers these questions.

- Which tests are the default home for new application behavior?
- Which tests must stay deterministic and state-aware without a PTY?
- Which tests must exercise the real terminal lifecycle and process boundary?
- How do these test layers reinforce ADR 0001 instead of duplicating
  `vim-core-rs` editing contracts?

## Decision

`saya` adopts a three-layer headless test architecture.

### Layer 1: Unit and narrow integration tests

Layer 1 validates local logic with minimal wiring.

- Keep unit tests in `src/*.rs`.
- Keep narrow integration tests when they validate one module boundary or one
  small application responsibility.
- Prefer direct assertions on Rust data structures and normalized outcomes.
- Do not use this layer to prove the full application path.

### Layer 2: Functional harness tests

Layer 2 becomes the primary home for new host-application behavior.

- Introduce a shared functional harness that drives `saya` through
  deterministic, headless application flows without a PTY.
- Test launch, input routing, event-loop coordination, projection, command
  line, save or quit policy, startup/runtime callback wiring, redraw
  behavior, and shutdown sequencing here.
- Standardize a reusable test session API instead of assembling internal
  pieces ad hoc in each test file.
- Assert both semantic state and projected screen state where that improves
  confidence.

This layer must remain headless, deterministic, and fast enough to serve as
the main integration safety net in CI.

### Layer 3: PTY-backed end-to-end tests

Layer 3 validates the real process and terminal boundary.

- Launch the real `sy` executable under a PTY.
- Exercise the real main loop, terminal mode transitions, and process
  shutdown path.
- Keep this layer focused on representative end-to-end scenarios, not an
  exhaustive matrix.
- Use this layer to catch regressions that only appear when the application
  runs as an actual terminal program.

PTY use is a tool, not the architecture. The architecture is the layered test
strategy around deterministic responsibilities.

## Testing boundary implications

ADR 0001 still defines what belongs in `saya` at all.

- Detailed editing semantics remain in `vim-core-rs`.
- `saya` uses Layer 2 and Layer 3 to prove host-application behavior built on
  top of those semantics.
- Representative editing-path smoke coverage is allowed when it proves host
  integration, but exhaustive core compatibility remains out of scope.

In practice, this means `saya` must grow a stronger functional harness before
it grows many more PTY cases.

## Consequences

This decision changes how new test work is planned.

- New host behavior should default to Layer 2 unless it is truly local logic
  or truly terminal-boundary behavior.
- Existing integration tests may stay in place initially, but they should move
  toward a shared harness over time.
- Binary smoke tests should evolve into real PTY-backed Layer 3 tests instead
  of remaining a custom shortcut path.
- The repository should prefer event-driven waits, semantic snapshots, and
  reusable assertions over sleep-based or ANSI-string-only checks.
- CI should eventually treat Layer 2 as the main correctness gate and Layer 3
  as the real-terminal confidence layer.

## Next steps

Document the functional harness and PTY-backed end-to-end design in the design
pages, then migrate the existing test inventory toward this structure.
