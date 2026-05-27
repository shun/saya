# Requirements

This page captures the long-lived requirements that the repository currently
implements or explicitly targets. It replaces temporary planning notes and
serves as the durable product-level contract for contributors.

The requirements are grouped into the CLI editor MVP and the TypeScript-first
configuration and runtime model.

ADR 0001 defines the architecture boundary behind these requirements. The
requirements on this page describe the host application layer that `saya`
owns around `vim-core-rs`, not a second specification for the embedded editing
core itself.

## CLI editor requirements

These requirements define the minimum editor behavior that the CLI application
must keep stable.

### Startup and file I/O

The editor must let you start with an existing file or a new empty buffer, and
it must handle file persistence through explicit host-side save logic around
the core host-action boundary.

- The CLI must open a provided file path and present the file contents for
  editing.
- The CLI must start with a new buffer when you do not provide a target path.
- The CLI must return a fatal startup error when a target path cannot be read.
- The CLI must write the current buffer contents to the target path on save.
- The CLI must preserve unsaved edits when a save attempt fails.

### Basic editing experience

The application must present a narrow but real Vim-derived editing flow by
embedding `vim-core-rs` rather than approximating editor behavior through
generic text-area logic.

- The application must surface at least Normal mode and Insert mode through the
  embedded core.
- The application must reflect cursor movement from normal-mode commands in the
  projected UI state.
- The application must reflect text insertion at the current position in Insert
  mode.
- The application must reflect delete operations through the core editing
  model.
- The application must track and display dirty state after modifications.

### Session control

The application must remain safe to use from a terminal session and must make
save or quit state visible.

- The UI must display the current mode and file identity.
- The UI must process save and quit requests without blocking the session loop.
- The editor must warn about unsaved changes before a normal quit.
- The editor must support force quit when the caller explicitly requests it.

### Scope boundary

These requirements intentionally stop at the host application boundary.

- `saya` must not re-specify or re-implement detailed Vim editing semantics
  that already belong to `vim-core-rs`.
- `saya` must treat `vim-core-rs` as the source of truth for embedded editing
  behavior and detailed compatibility coverage.
- `saya` must focus its own requirements on startup, orchestration, rendering,
  host I/O, and TypeScript-facing behavior.

## TypeScript configuration requirements

These requirements define the startup capability model that replaces Vim script
as the primary configuration path.

### Startup capability surface

The startup configuration file must run on a TypeScript-capable runtime and
must expose a narrow, explicit `saya` namespace.

- The editor must evaluate `init.ts` through `deno_core`.
- The startup surface must expose `saya.options.*`.
- The startup surface must expose `saya.keymap.set(...)`.
- The startup surface must expose `saya.commands.register(...)`.
- The startup surface must expose `saya.events.on(...)`.
- The startup surface must expose command references through
  `saya.commands.execute(...)`.

### Startup normalization and isolation

The startup evaluation result must become deterministic Rust-side data before
session startup continues.

- The editor must convert startup results into a normalized registry.
- The editor must preserve registration order for options, keymaps, commands,
  and events.
- The editor must keep startup capability evaluation separate from the main UI
  loop.
- The editor must fall back to safe defaults when startup config loading or
  evaluation fails.

## TypeScript runtime requirements

These requirements define the runtime callback surface that exists after
startup.

### Runtime capability surface

The runtime surface must remain smaller than the startup surface and must
prioritize typed, read-only state access plus explicit command execution.

- The runtime surface must expose `saya.commands.execute(name)`.
- The runtime surface must expose `saya.buffer.current()`.
- The runtime surface must expose `saya.buffer.currentPath()`.
- The runtime surface must expose `saya.window.current()`.
- The runtime surface must expose `saya.editor.current()`.
- The runtime surface must expose `saya.editor.mode()`.
- The runtime surface must expose `saya.filer.list(path, options)` for
  read-only directory listing plugins, including narrow sort, hidden-file, and
  substring filter options.
- The runtime surface must expose `saya.filer.currentEntry()` so dired-style
  commands can read the current entry from host-side directory buffer metadata.
- The runtime surface must expose host-mediated filer operations for creating
  one file, creating one directory, copying one regular file, moving or
  renaming one path, and deleting one explicitly confirmed path.
- The runtime buffer snapshot must include the current cursor row and rendered
  current line for non-mutating directory navigation commands.
- Directory listing buffers must not be saved as regular files. Writable
  directory buffers must convert rendered listing edits into a save-time
  preview, require a matching confirmation before apply, and execute confirmed
  mutations through explicit host-mediated filer operations.

### Runtime boundaries

The runtime and startup phases must remain separate so the repository does not
blur registration logic and live callback execution.

- Startup-only registration APIs must not leak into runtime callbacks.
- Runtime-only state access APIs must not leak into startup evaluation.
- Public APIs must stay under the `saya` namespace.
- Public APIs must not center compatibility-oriented string DSLs.
- Public APIs must not include broad filesystem or network access in the MVP.
- Filer plugins must use the dedicated read-only filer surface instead of a
  general filesystem namespace.

## Next steps

Use the following pages to see how these requirements map to implementation.

1. Read [Architecture](architecture.md).
2. Read [ADR 0001](adr/0001-define-saya-scope-against-vim-core-rs.md).
3. Read [Boot flow design](design/boot-flow.md).
4. Read [TypeScript runtime design](design/typescript-runtime.md).
