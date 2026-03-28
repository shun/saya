# ADR 0001: Define `saya` scope against `vim-core-rs`

## Status

Accepted

## Context

`saya` is built on top of `vim-core-rs`, but the repository historically mixed
two kinds of expectations.

- Some expectations treated `saya` as the place that owns Vim-derived editing
  semantics.
- Other expectations treated `saya` as the host application layer around an
  embedded editing core.

That ambiguity makes it harder to decide where a feature belongs, where a test
belongs, and when a behavior should be considered duplicate coverage.

`vim-core-rs` now defines a narrower contract for the embedded core.

- It owns modal editing behavior, Ex and Normal command execution, buffer and
  window state extraction, selected rendering-adjacent state, host-mediated
  VFS flows, and host-mediated job bridging.
- It classifies vendored upstream Vim tests into cases that must be preserved
  directly, cases that must be preserved through host-boundary adaptation, and
  cases that are intentionally out of scope.
- It treats repository contract tests as the source of truth for adapted
  host-boundary behavior.

`saya` therefore needs an explicit architecture decision that states what this
repository owns and what it must not try to own.

## Decision

`saya` is the host application layer around `vim-core-rs`, not a second
editing core.

This repository owns these concerns.

- CLI startup, argument parsing, and launch preparation.
- Session lifecycle, single-session coordination, and startup failure
  recovery.
- Host-side save and quit policy around core host actions.
- Screen projection, viewport management, terminal lifecycle, input routing,
  event-loop orchestration, and rendering.
- Startup TypeScript evaluation, runtime callback hosting, and the public
  `saya` namespace.
- Application-specific integration between `vim-core-rs` snapshots, host
  actions, TypeScript behavior, and the TUI.

This repository does not own these concerns.

- Re-implementing Vim modal editing semantics that already belong to
  `vim-core-rs`.
- Becoming the primary home for upstream Vim compatibility coverage.
- Re-validating detailed core behavior such as registers, marks, undo trees,
  search extraction, syntax extraction, pop-up menu extraction, VFS protocol
  rules, or job protocol rules beyond host-application integration needs.
- Pulling plugin-hosting, semantic parsing, terminal-emulator ownership, or
  generalized async orchestration into the embedded core boundary.

When a change proposal is ambiguous, use this filter.

- If it is pure editing semantics or embedded Vim state extraction, it belongs
  in `vim-core-rs`.
- If it is application orchestration, rendering, startup flow, TypeScript
  surface design, or host action handling in the CLI app, it belongs in
  `saya`.
- If it crosses the boundary, keep the semantic contract in `vim-core-rs` and
  keep the user-facing integration in `saya`.

## Testing implications

The testing strategy follows the same boundary.

- `vim-core-rs` is the source of truth for detailed editing semantics and for
  upstream Vim compatibility that remains in scope for the embedded core.
- `saya` keeps unit and integration tests for application behavior built on
  top of that core.
- `saya` tests must focus on startup, save or quit flows, viewport and screen
  projection, terminal lifecycle, event-loop behavior, input routing,
  TypeScript startup behavior, runtime callback integration, and binary-level
  orchestration.
- `saya` may keep smoke coverage for representative editing flows when those
  flows prove host integration, but it must not grow into a second exhaustive
  compatibility suite for core editor semantics.

In practice, this means `saya` prefers end-to-end and integration coverage for
the host application path, while `vim-core-rs` remains responsible for the
fine-grained editing and compatibility contracts.

## Consequences

This decision gives the repository a simpler maintenance rule.

- New editing-semantic work should start by asking whether the behavior already
  belongs in `vim-core-rs`.
- New `saya` tests should justify themselves in terms of host application
  value, not core duplication.
- Documentation for `saya` should describe it as an application shell around
  `vim-core-rs`, not as an independent editor engine.
- Future architectural work should strengthen the live integration path
  between input, event-loop coordination, runtime callbacks, host actions, and
  rendering instead of duplicating core semantics.

## Next steps

Reflect this boundary in the surrounding architecture and testing documents as
those pages evolve.
