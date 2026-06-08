# ADR 0003: Keep dired in the host layer

## Status

Accepted.

## Context

`saya` is a host application around `vim-core-rs`. The embedded core owns Vim
editing semantics, while the application layer owns host I/O, terminal
projection, startup configuration, and runtime callback orchestration.

Dired needs directory reads, directory buffer metadata, path-based operations,
confirmation policy, diagnostics, and refresh behavior. Those concerns are
filesystem UI concerns, not text-editing semantics. Putting them in
`vim-core-rs` would make the core responsible for host policy and would make
the boundary between editor semantics and application orchestration harder to
maintain.

The TypeScript plugin surface also needs to stay narrow. External plugins must
be able to reuse dired behavior without receiving broad filesystem or network
access.

## Decision

Dired stays in the host application layer.

The host layer owns these responsibilities:

- Directory buffer metadata, including root path, rendered text, entry IDs,
  entry kind, and mark state.
- Host-mediated filer operations for create, rename, delete, mark, unmark,
  clear-mark, bulk-preview, and confirmed bulk-delete flows.
- Refresh behavior after host operations, including cursor preservation where
  the current metadata can support it.
- Diagnostic logging for listing, operation, error, and refresh paths.
- User-facing policy for destructive operations.

The TypeScript runtime surface exposes only narrow dired and filer capabilities
under `saya.filer`. It does not expose broad filesystem access. The startup
plugin in `plugins/saya-dired.ts` provides preview setup options for command
names, keymaps, root selection, hidden-file policy, sort policy, and confirm
strategy.

## Writable directory buffers

Writable directory buffers use rendered listing text as an editing surface, but
they don't execute filesystem operations directly from text edits. On `:write`,
the host compares the edited text with directory buffer metadata and prepares a
filesystem operation preview. The preview records the operation kind, source
path, target path, target count, risk level, and a preview ID. Plain `:write`
does not mutate the filesystem until the user confirms the prompt.

Plain `:write` opens the save-time confirmation prompt for the latest preview.
Pressing `y` or Enter applies the operation only when the preview ID matches
the pending preview. Pressing `n` or Esc clears the pending preview state
without changing the filesystem. `:write!` remains a legacy explicit
confirmation path and does not bypass safety checks. Missing, stale, or invalid
previews fail before any filesystem operation runs. The preview prompt is
represented as host state so the confirm and cancel paths can be tested
headlessly.

The plan builder treats rendered suffixes as display decorations. A directory
entry can render as `src/`, but the target name remains `src`. New lines ending
in `/` plan directory creation, and new lines without a suffix plan file
creation. `@` and `?` are reserved display decorations for symlink and other
entries, so new lines using those suffixes are validation errors.

The validation step rejects empty lines, duplicate names, parent-directory
escapes, path separators inside names, and unsupported display decorations.
Pure row reordering does not produce filesystem operations.

## Operation transactions

Confirmed writable-buffer operations run through a host-side transaction
executor. The executor validates conflicts before it mutates the filesystem,
then orders operations so directory creation happens before child creation,
rename chains use temporary names to avoid destination collisions, file
creation happens after renames, and deletes run last.

The conflict check rejects plans when a create target already exists, a rename
or delete source is missing, a rename target collides with a path that isn't
also moving in the same plan, or an unsupported special-file entry would be
renamed or deleted. Permission failures can still happen at execution time, so
they remain part of the operation report and diagnostic log.

Rollback is intentionally conservative. Rename steps are automatically rolled
back when a later step fails and the rollback path is still available. Create
and delete steps are reported instead of automatically reversed, because
reversing them can destroy user data or recreate incomplete state. When a
transaction partially succeeds, the user-facing message and diagnostic log
include structured counts for successful steps, failed steps, rollback results,
and operations that require manual recovery. After a failure, the host refreshes
directory metadata from the current filesystem state instead of keeping a stale
success view.

## Destructive operations

Destructive operations must be explicit host-mediated operations. A plugin must
not delete by mutating rendered listing text.

Single-path delete requires `{ confirm: true }`. Bulk delete requires a fresh
preview report and a matching `previewId` with `{ confirm: true }`. Recursive
delete remains disabled, and trash requests fail with an unsupported-backend
error until a platform policy exists. Copy supports regular files only; move
uses the host rename path for files and directories.

Copy, move, and delete operations log their duration at the host boundary so
large operations can be identified from diagnostics. Directory copy is rejected
instead of starting a recursive operation, so there is no partial directory-copy
state to cancel. Directory move follows the host rename semantics; if it fails,
the structured filer error reports the source, target, and error kind.

The default dired keymap exposes bulk delete as a preview feature. It prepares
the preview but does not bind a default command that immediately deletes marked
entries.

The mark keymap avoids `u` so directory mark operations don't shadow
normal-mode undo while a writable directory listing is being edited. The
defaults use `m` to mark, `M` to unmark, and `gM` to clear marks.

## Consequences

This keeps filesystem policy out of `vim-core-rs` and lets `saya` test dired
headlessly at the host boundary. It also keeps TypeScript plugins reusable
without making the runtime a general-purpose filesystem API.

The cost is that directory buffer metadata, refresh, and operation policy live
as application-layer contracts. Those contracts need public surface guards and
integration tests before the API is treated as stable.

Dired remains a preview feature. The public setup and runtime APIs are intended
for reuse, but the project can still adjust names and option shapes before API
stabilization.
