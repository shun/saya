# ADR 0002: Define compatibility boundaries between upstream Vim,
# `vim-core-rs`, and the host

This ADR records the repository-level decision for how responsibilities are
split between upstream Vim, `vim-core-rs`, and the embedding host. It exists
to remove ambiguity about what this repository must preserve from Vim, what
it must adapt into host-facing contracts, and what sits outside the product
boundary.

- Status: Accepted
- Date: March 28, 2026

## Context

`vim-core-rs` vendors upstream Vim, but it does not aim to expose or
reproduce the whole standalone Vim product.

The repository already documents the `vim-core-rs` product boundary in
`docs/SCOPE.md` and behavior contracts in `docs/api-contracts.md`. Those
documents explain what `vim-core-rs` owns, but they do not explicitly define
the role of upstream Vim as a dependency and compatibility source.

That gap makes test-scope decisions harder than they need to be. Without an
explicit boundary, it is easy to make two opposite mistakes:

- Treat every upstream Vim test as mandatory compatibility coverage.
- Treat host-integration behavior as "just upstream Vim" and leave it
  under-specified.

The repository needs a durable rule for deciding:

- which upstream Vim behaviors must remain compatible,
- which upstream Vim behaviors must be adapted through `vim-core-rs`
  contracts, and
- which upstream Vim features are intentionally outside the scope of this
  crate.

## Decision

The repository will treat upstream Vim as the compatibility source for modal
editing semantics and runtime behaviors that belong inside the documented
`vim-core-rs` scope, but not as the product definition for the whole
embedded system.

Responsibility is split into three layers:

1. Upstream Vim owns core editing semantics and runtime behaviors that the
   embedded engine reuses.
2. `vim-core-rs` owns the embedding boundary, typed Rust APIs, state
   extraction, and host-facing adaptation of selected Vim behaviors.
3. The host owns rendering, persistence, process execution, asynchronous
   orchestration, and other application-level behavior outside the core.

This means compatibility is selective by design. The repository must preserve
the parts of Vim that define the embedded editing core, but it must not widen
its obligations to every upstream subsystem.

## Role of upstream Vim

Upstream Vim is the behavioral source for editing semantics that
`vim-core-rs` embeds.

In this repository, upstream Vim primarily provides:

- modal editing behavior,
- Ex and Normal command semantics,
- registers, marks, jumplist, undo, and search behavior,
- buffer and window state transitions,
- runtime-derived state that `vim-core-rs` explicitly extracts, such as
  syntax chunks and pop-up menu state, and
- the baseline script tests that validate those behaviors.

Upstream Vim does not define the full system boundary for this repository.
Standalone Vim features that depend on terminal ownership, GUI ownership,
plugin hosting, external interpreter integrations, or application-level
process control do not automatically become `vim-core-rs` obligations.

## Role of `vim-core-rs`

`vim-core-rs` owns the contract between embedded Vim and the host
application.

Its responsibilities are:

- embed upstream Vim as a library component,
- expose a typed Rust API over selected Vim capabilities,
- translate file-like command flows into host-facing VFS or write actions,
- translate Vim job requests into host-facing process actions and VFD
  bridging,
- expose coherent snapshots and state-extraction APIs for host rendering, and
- preserve repository-declared scope boundaries even when upstream Vim offers
  broader features.

`vim-core-rs` is therefore not a thin pass-through. It is the policy layer
that decides which Vim behaviors are directly preserved, which are adapted,
and which are intentionally not modeled as part of the crate contract.

## Role of the host

The host owns all concerns that the repository scope assigns outside the
embedded core.

These responsibilities include:

- rendering and user-visible presentation,
- persistence and document storage,
- process spawning and asynchronous orchestration,
- terminal and PTY ownership,
- overlay composition, diagnostics presentation, and semantic parsing, and
- application-level plugin or extension policy.

When `vim-core-rs` emits a host action, the host is the system component that
must complete the effect. `vim-core-rs` does not become responsible for that
effect merely because Vim initiated it.

## Compatibility classes

This repository will classify Vim-related behavior into four categories.

### Preserve directly

The repository must preserve upstream Vim behavior directly when it falls
inside the embedded editing-core scope.

This includes behavior such as:

- insert, delete, replace, and motion semantics,
- mode transitions,
- Ex and Normal command behavior that does not cross the host boundary,
- search, substitute, register, mark, jumplist, and undo behavior, and
- buffer and window semantics that the public API exposes.

These behaviors are appropriate targets for upstream script-suite coverage and
for repository contract tests that assert Vim-compatible results.

### Preserve through adaptation

The repository must preserve the intent of upstream Vim behavior, but it may
adapt the execution model into host-facing contracts when the crate boundary
requires it.

This includes behavior such as:

- `:edit`, `:write`, `:update`, `:wq`, and `:xit` flows,
- VFS request and response sequencing,
- job lifecycle handling and VFD bridging,
- message and redraw observability,
- runtime discovery and runtime-path behavior, and
- extraction APIs for syntax, search highlights, and pop-up menu state.

These behaviors are not validated only by "did upstream Vim do this
internally?" They are validated by whether `vim-core-rs` preserves the
correct external contract for the host.

### Out of scope

The repository will not treat a Vim feature as a `vim-core-rs` obligation
when the feature lives outside the documented crate boundary.

This includes behavior such as:

- terminal emulator parity and libvterm behavior,
- GUI behavior,
- socket server and clientserver integration,
- NetBeans integration,
- full plugin-hosting compatibility,
- external language interpreter integrations, and
- full-screen rendering fidelity tests for standalone Vim UI behavior.

Upstream tests in these areas may still be useful references, but they are
not mandatory compatibility obligations for this crate.

### Temporarily excluded

Some behavior may conceptually fit the scope but remain excluded for explicit
repository reasons such as encoding limits, build limits, or unsupported test
infrastructure.

These exclusions must be recorded explicitly, for example through a skip list
or a limitation note. They are not the same as "out of scope." They are
scope-adjacent gaps with an explicit justification.

## Testing consequences

This decision changes how the repository interprets upstream Vim tests.

The generated upstream test runner is the default compatibility harness for
vendored `src/testdir/test_*.vim` cases that fit the embedded scope.

Contract tests in `tests/` remain the source of truth for repository-specific
adaptation layers, especially:

- VFS,
- host-action sequencing,
- job and VFD bridging,
- event delivery,
- snapshot extraction, and
- repository-declared invariants that upstream Vim does not model as a Rust
  embedding contract.

The repository will not use "upstream has a test" as the only rule for test
selection. Test selection must follow this ADR's responsibility split.

## Consequences

This decision has the following positive effects:

- It gives the repository an explicit definition of what compatibility means.
- It prevents scope creep from standalone Vim into the embedding layer.
- It justifies why some behavior is tested through upstream scripts while
  other behavior is tested through repository contracts.
- It makes test-skipping decisions easier to review.
- It gives future ADRs and scope updates a stable vocabulary.

This decision also has explicit costs:

- Engineers must classify behavior before adding or importing tests.
- Some test discussions now require a scope argument, not only a technical
  implementation argument.
- The repository must keep skip lists and limitation notes honest, because
  temporary exclusions are now distinct from non-goals.

## Rejected alternatives

The repository rejects the following alternatives:

- Treat upstream Vim as the complete product specification.
  This conflicts with the documented host-owned architecture.
- Treat `vim-core-rs` as only a thin FFI wrapper over Vim.
  This ignores the crate's explicit policy and adaptation responsibilities.
- Treat every missing upstream test as a coverage bug.
  This erases the difference between scope boundaries and temporary gaps.
- Treat all host-facing behavior as outside compatibility discussion.
  This would leave the most important embedding contracts under-specified.

## Status note

This ADR is accepted. Existing repository documents already describe large
parts of this boundary. This ADR makes the role split explicit so test-scope
and compatibility decisions can reference one stable source.
