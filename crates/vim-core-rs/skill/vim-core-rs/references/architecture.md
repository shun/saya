# vim-core-rs architecture and repository boundaries

This reference explains how the repository is split, what the crate owns, what
the host owns, and which files matter when you change behavior.

If you are inside the original repository, pair this file with `docs/SCOPE.md`,
`docs/api-index.md`, and `docs/internal-api-reference.md`. Do not assume those
files exist when this skill is copied elsewhere.

## Core architecture

`vim-core-rs` wraps an embedded upstream Vim runtime through a Rust-facing FFI
layer.

- `src/lib.rs`
  Public API surface, session lifecycle, state extraction, command dispatch,
  option accessors, search and syntax queries, message routing, and host-action
  queuing.
- `src/vfs.rs`
  Virtual file system coordination, request ledger, transaction log, deferred
  close handling, and buffer-document bindings.
- `src/vfd.rs`
  Virtual file descriptor management for job I/O bridging.
- `native/`
  C bridge layer between Rust and the vendored Vim runtime.
- `build.rs` and `build_*.rs`
  Bindgen, native compilation, audit generation, and traceability proofs.

## Scope boundaries

Read `docs/SCOPE.md` before widening the design. The repository explicitly
positions `vim-core-rs` as a modal text-editing engine, not a complete editor
platform.

In scope:

- Modal editing and mode transitions.
- Buffer mutation and undo tree extraction.
- Snapshot extraction for buffers, windows, cursor, and completion state.
- Search highlight, syntax chunk, and other Vim-derived rendering inputs.
- Host-mediated file and job integration through VFS and VFD.

Out of scope:

- Deep Vimscript or Lua embedding as a first-class product surface.
- Modern async orchestration beyond the host-action bridge.
- Rich semantic highlighting that belongs to host-side parsers such as
  Tree-sitter.
- Terminal emulator ownership inside the core.
- Host-managed overlays such as virtual text.

Do not add code that fights these boundaries unless the user explicitly wants
to redefine the project scope.

## Single-session design

The crate permits only one live `VimCoreSession` per process. This affects both
product code and tests.

- Do not design APIs that require multiple concurrent sessions.
- Serialize tests that create sessions.
- If a failure smells like random flakiness, check for leaked sessions first.
- If you create helper abstractions around the session, make the ownership
  obvious so `Drop` reliably releases the global lock.

## Host integration boundary

The core owns modal editing state. The host owns environment integration.

Host responsibilities:

- Drain `take_pending_host_action()`.
- Drain `take_pending_event()` when you need structured event data outside the
  transaction result.
- Handle `Write`, `Quit`, `Redraw`, and `RequestInput`.
- Resolve and serve VFS requests, then call `submit_vfs_response()`.
- Spawn and manage external jobs after `JobStart`.
- Feed job output with `inject_vfd_data()` and completion with
  `notify_job_status()`.
- Provide UI sizing through `set_screen_size()`.

If behavior looks incomplete, first ask whether the missing piece belongs in
the host instead of the crate.

## Build and vendor maintenance

The repository vendors upstream Vim and treats build traceability as part of the
contract.

Key files:

- `scripts/vendor-sync.sh`
  Synchronize allowlisted upstream sources and refresh vendor patches.
- `vim-source-allowlist.txt`
  Declare which upstream files are copied into the build.
- `vim-source-build-manifest.txt`
  Declare which C sources are compiled.
- `upstream-metadata.json`
  Track upstream version and commit provenance.
- `build_compile_plan.rs`, `build_link_audit.rs`, `build_allowlist.rs`,
  `build_test_runner.rs`
  Generate audit artifacts and enforce repository integrity.

When changing native or vendor behavior, inspect
`tests/quality_gate_contract.rs` to understand which reports must continue to
exist.
