# vim-core-rs known limitations

This file lists current limitations and intentionally incomplete behavior in
`vim-core-rs`. Read it before changing code or relying on an API. The goal is
to prevent both humans and agents from mistaking exposed symbols for fully
realized features.

## Current implementation gaps

The incremental-search and visible-search APIs now return live Vim state. The
main caveat is narrower: search columns are byte offsets, not display-cell
widths or Unicode scalar indexes.

- `CoreMatchRange.start_col`
  Inclusive byte offset into the line.
- `CoreMatchRange.end_col`
  Exclusive byte offset into the line.

If a host renders by grapheme cluster or display cell, it must convert from
the byte contract explicitly before drawing.

Some message-producing paths are still incomplete. In current tests, basic
`echo*` and prompt-oriented commands do not always surface as `CoreEvent`
values or terminal output, so hosts should not assume every Vim message path is
fully bridged yet.

## Job bridge limits

- The crate does not spawn OS processes itself.
- `CoreHostAction::JobStart` is a host request, not process execution.
- `vim_core_vfd_write()` currently ignores bytes written from Vim and simply
  reports success.
- Job working directory propagation is not fully implemented. A
  `CoreJobStartRequest.cwd` may be `None` even when Vimscript job options imply
  a directory.
- When a job reaches a terminal status, the associated VFDs are closed so Vim
  observes EOF. Late injected data is rejected.

## Local file-command semantics

- For local buffers, `:write` and `:update` enqueue `CoreHostAction::Write`.
- For VFS-backed buffers, `:write` and `:update` enqueue
  `CoreHostAction::VfsRequest(CoreVfsRequest::Save)`.
- For local buffers, `:wq` and `:xit` currently enqueue `CoreHostAction::Quit`
  instead of automatically sequencing `Write` followed by `Quit`.

If a host wants local save-and-quit semantics, the host must coordinate them
after observing the queued action.

## Build and feature limits

- The vendored upstream Vim build disables native terminal support.
- The build disables native socket server support.
- The build disables native channel support in the vendored configure step.
- Some upstream Vim tests are intentionally skipped because they require those
  disabled features.

Do not infer missing coverage from those skips. They reflect declared feature
boundaries.

## Architecture limits

- Only one `VimCoreSession` may exist per process.
- The crate is not a multi-session or multi-tenant engine.
- The crate does not own semantic parsing, virtual text, terminal emulation,
  plugin hosting, or generalized editor UI.
- The VFS ledger is session-local and not persisted across sessions.
- The VFD manager is global process state and is cleared when a session drops.

## Interpretation rules

- If a type or method exists, first check whether contract tests exercise it.
- If a method is present but contract tests do not cover the behavior you need,
  verify the native bridge and add a focused test before widening the contract.
- If a behavior depends on host action, the crate is only one half of the
  feature.

## Next steps

Read [api-contracts.md](api-contracts.md) for sequencing rules, then read
[public-api-reference.md](public-api-reference.md) for the callable surface.
