# Known limitations

This page lists current limitations and intentionally incomplete behavior in
`vim-core-rs`. Read it before changing code or relying on an API. The goal is
to prevent LLMs and humans from mistaking exposed symbols for fully realized
features.

## Current implementation gaps

The incremental search getters and viewport query API now return live Vim
state. The remaining caveat is narrower: hosts should treat the column values
as byte offsets, not display-cell widths or Unicode scalar indexes.

- `CoreMatchRange.start_col`
  Inclusive byte offset into the line.
- `CoreMatchRange.end_col`
  Exclusive byte offset into the line.

If your UI renders by grapheme cluster or display cell, convert from the byte
contract explicitly before drawing.

`echo`, `echon`, `echomsg`, `echoerr`, and `echoconsole` output reaches the
host as `CoreEvent::Message`. The `more-prompt` and `hit-return` prompts are
reported as `CoreEvent::PagerPrompt` without blocking. The dedicated `:input`
Ex command is bridged to `CoreHostAction::RequestInput`, but Vimscript
`input()`, `inputsecret()`, `confirm()`, and `:confirm` are intentionally not
supported in embedded mode. They emit a user-visible info message and return
the cancel sentinel without blocking or leaking terminal prompts.

## Job bridge limits

The job bridge is real, but intentionally narrow.

- The crate does not spawn OS processes itself.
- `CoreHostAction::JobStart` is a host request, not process execution.
- Bytes written from Vim to a job channel surface as
  `CoreHostAction::JobWrite { vfd, data }`. The host must consume that action
  and forward the bytes to the real process.
- When a job reaches a terminal status, the associated VFDs are closed so Vim
  observes EOF. Late injected data is rejected.

## Local file-command semantics

Local buffers and VFS-backed buffers do not follow the same save path.

- For local buffers, `:write` and `:update` enqueue `CoreHostAction::Write`.
- For VFS-backed buffers, `:write` and `:update` enqueue
  `CoreHostAction::VfsRequest(CoreVfsRequest::Save)`.
- For local buffers, `:wq` enqueues `[Write, Quit]` in that order so the host
  can coordinate save-before-quit.
- For local buffers, `:xit` / `:x` enqueues `[Write, Quit]` only when the
  buffer is dirty; on a clean buffer it enqueues `Quit` alone.
- Compound Ex commands (pipe-separated, e.g. `:set number | write! file`)
  are now split and the write/quit sub-command is intercepted by the bridge.
  Non-intercepted sub-commands before the intercepted one are executed
  natively.
- For local buffers, `:write {path} | quit` and `:update {path} | quit` keep
  the same host coordination and enqueue `[Write, Quit]`.
- For VFS-backed buffers, `:write [path] | quit-family`, `:write! [path] |
  quit-family`, and dirty `:update [path] | quit-family` now enter the same
  deferred close flows as `:wq` and `:xit`.
- For VFS-backed buffers, `:update! [path] | quit-family`, range-prefixed
  forms, and generalized pipeline semantics beyond the first quit-family
  trailing segment remain outside this intercept path.
- For VFS-backed buffers, clean `:update [path] | quit-family` still skips an
  unnecessary save and follows the existing no-op contract.

## Build and feature limits

The embedded upstream Vim is compiled with explicit feature reductions.

- Native terminal support is disabled.
- Native socket server support is disabled.
- Native channel support is disabled in the vendored configure step.
- Some upstream Vim tests are intentionally skipped because they require
  disabled features such as terminal UI or channel support.

Do not infer "missing tests" from those skips. They are repository-declared
feature boundaries.

If a future upstream case is marked as `temporarily_excluded`, treat that as
a test-infrastructure or encoding gap, not as a scope decision. Policy
exclusions belong in the upstream classification manifest as `out_of_scope`
or `preserve_through_adaptation`, not in the skiplist. The current manifest
has no `temporarily_excluded` cases.

## Architecture limits

These are intentional design constraints, not temporary bugs.

- Only one `VimCoreSession` may exist per process.
- The crate is not a multi-session or multi-tenant engine.
- The crate does not own semantic parsing, virtual text, terminal emulation,
  plugin hosting, or generalized editor UI.
- The VFS ledger is session-local and is not persisted across sessions.
- The VFD manager is global process state and is cleared when a session drops.

## Documentation interpretation rules

Use these rules when an API looks broader than the current behavior.

- If a type or method exists, first check whether contract tests exercise it.
- If a method is present but the native bridge returns placeholder data, treat
  it as exposed-but-incomplete.
- If a behavior depends on host action, the crate is only one half of the
  feature.

## Next steps

Read `api-contracts.md` for sequencing rules. Read `public-api-reference.md`
for the callable surface, then compare it with this page before assuming an
API is production-complete.
