# vim-core-rs README summary

This file captures the repository-level guidance from `README.md` in a
standalone-friendly form.

## Repository purpose

`vim-core-rs` is a Rust-facing host integration layer around one embedded
upstream Vim runtime. It is not a generic editor toolkit. It exists so a host
application can use Vim's modal editing engine, buffer state, selected
rendering data, host-mediated virtual document I/O, and host-managed job
bridging.

## Hard invariants

- Only one live `VimCoreSession` may exist per process.
- `VimCoreSession` is stateful, not `Send`, and not `Sync`.
- `take_pending_host_action()` is part of normal control flow, not optional
  diagnostics.
- VFS requests are explicit and host-owned. The host must answer them with
  `submit_vfs_response()`.
- Job execution is host-owned. The host must react to `JobStart`, then feed
  bytes back with `inject_vfd_data()` and lifecycle updates with
  `notify_job_status()`.
- Contract tests are the source of truth when prose and intuition disagree.

## Mental model

Reason about the crate as four coupled state machines.

- Session machine
  Owns lifetime, embedded runtime pointer, and the single-session lock
- Command machine
  Executes Normal or Ex input and returns one coarse outcome
- VFS machine
  Tracks buffer-document bindings, request sequencing, request ledger status,
  and deferred close behavior
- VFD machine
  Tracks virtual file descriptors and host-managed job status

Do not model the crate as a library that directly edits files. That framing
creates wrong assumptions about persistence, ownership, and concurrency.

## Host responsibilities

- Drain `take_pending_host_action()` until it returns `None`.
- Drain `take_pending_event()` until it returns `None` when you consume
  events outside transaction results.
- Handle `CoreHostAction::Write` and `CoreHostAction::Quit`.
- Handle every `CoreHostAction::VfsRequest` and send one matching
  `CoreVfsResponse`.
- Spawn real jobs when `CoreHostAction::JobStart` appears.
- Feed stdout and stderr bytes back through `inject_vfd_data()`.
- Report terminal job status through `notify_job_status()`.
- Set UI size with `set_screen_size()` when geometry matters.
- Drain `take_pending_event()` when you need structured message or layout
  events outside the transaction result.

## Dangerous assumptions to avoid

- Do not assume file writes happen automatically inside the crate.
- Do not assume `CoreCommandOutcome::HostActionQueued` means a side effect has
  already completed.
- Do not assume every Ex command reaches native Vim unchanged. A file-like
  subset is intercepted and rerouted through Rust policy.
- Do not assume VFS request IDs are reusable or unordered. They are monotonic
  session-local identities.
- Do not assume every `CoreVfsResponse::Saved` applies. Save responses can be
  rejected as stale when revisions advance.
- Do not assume the embedded runtime owns process execution. It only requests
  host action.

## Build and release facts

- Repository development should usually run with `VIM_CORE_FROM_SOURCE=1`.
- A bare `cargo test` follows the default consumer path and expects a released
  prebuilt artifact for the current crate version.
- Read the repository `README.md` before changing packaging, prebuilt artifact
  resolution, or release workflow behavior.

## Source-of-truth hierarchy

When artifacts disagree, prefer this order.

1. Contract tests in `tests/`
2. Rust implementation in `src/`
3. Native bridge implementation in `native/`
4. Repository documentation
5. README-style summaries
