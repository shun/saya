# Runtime API

This page documents the public TypeScript runtime surface that `saya` exposes
while executing runtime callbacks. This API is separate from the startup API and
focuses on typed state reads plus explicit command execution.

If you need startup-time registration APIs, read the startup API page instead of
this one.

## Availability

The runtime API is available only inside runtime callback execution. The
repository currently validates this behavior primarily through headless tests.

The runtime surface is narrower than the startup surface by design.

For completion-specific setup and extension points, read the
[Completion API](completion-api.md).

## Namespace

The runtime surface lives under the global `saya` object and exposes these
top-level areas.

- `saya.commands`
- `saya.buffer`
- `saya.window`
- `saya.editor`
- `saya.workspace`
- `saya.fs`
- `saya.lsif`
- `saya.input`
- `saya.selector`
- `saya.completion`
- `saya.process`
- `saya.panel`
- `saya.filer`
- `saya.plugins`

## Commands

The commands surface lets runtime code execute named commands through the host
capability bridge.

### `saya.commands.execute(name)`

Use this method to execute a named command.

```ts
await saya.commands.execute("write");
```

Commands may resolve to a registered runtime callback or to a host-side command
implementation, depending on the current registry and host bridge.

## Workspace

The workspace surface exposes narrow project-root detection for runtime
callbacks that need a workspace boundary without broad filesystem access.

### `saya.workspace.findRoot(path, markers)`

Use this method to find the nearest ancestor of `path` that contains one of the
named marker files or directories.

```ts
const root = await saya.workspace.findRoot(buffer.path, [
  "go.mod",
  "Cargo.toml",
  ".git",
]);
```

The method returns a path string when a marker matches and `null` when no marker
is found. The LSP preview plugin uses this API to resolve per-server workspace
roots from marker lists such as `go.mod`, `Cargo.toml`, and `.git`.

## LSP and LSIF

The LSP surface exposes a narrow managed session capability for preview live
language server integration. The bundled LSP plugin uses it after selecting a
server and constructing protocol payloads. The host owns server startup, stdio
transport, JSON-RPC framing, lifecycle cleanup, and validation. TypeScript owns
server selection, feature policy, response interpretation, and UI routing.

The LSIF surface remains a separate typed host bridge for preview static index
lookup.

> **Note:** This is a preview feature currently under active development. See
> [LSP preview](lsp-preview.md) for setup examples, the feature support matrix,
> managed-session details, LSIF limitations, and verification commands.

### `saya.lsp.connect(options)`

Use this method to open a host-managed LSP session. Most users call it
indirectly through `setupSayaLspClient()` from the bundled LSP plugin.

```ts
const client = await saya.lsp.connect({
  server: {
    name: "gopls",
    command: "gopls",
    args: ["serve"],
    rootMarkers: ["go.mod", ".git"],
  },
  initializeParams: {
    processId: null,
    rootUri: "file:///workspace",
    capabilities: {},
  },
});

const hover = await client.request("textDocument/hover", {
  textDocument: { uri: "file:///workspace/main.go" },
  position: { line: 0, character: 0 },
});

for (const notification of client.takeNotifications()) {
  console.log(notification);
}

await client.notify("exit", null);
await client.close();
```

The returned client exposes `request(method, params)`,
`notify(method, params)`, `takeNotifications()`, and `close()`. The host keeps
the child process and transport private to the session. Use this capability for
LSP integration instead of building LSP on top of `saya.process.spawn()`.

### `saya.lsif.request(payload)`

Use this method to send a validated LSIF lookup request to the host. Most users
call it indirectly through `setupSayaLspClient()` from the bundled LSP plugin.

```ts
const response = await saya.lsif.request({
  source: "lsif",
  lspVersion: "3.17",
  method: "textDocument/hover",
  clientName: "saya-lsif",
  rootUri: "file:///workspace",
  languageId: "go",
  positionEncoding: "utf-16",
  dumpPath: ".cache/index.lsif",
  textDocument: { uri: "file:///workspace/main.go" },
  position: { line: 0, character: 0 },
  params: {},
  buffer: await saya.buffer.current(),
  editor: await saya.editor.current(),
});
```

The method returns `{ source, method, result }` when the host completes the LSIF
lookup. Invalid payloads and host failures surface as command errors with
user-safe messages.

## Buffer

The buffer surface lets you read the current buffer path or snapshot. Use the
path-only API when a command only needs to resolve a filesystem location.

### `saya.buffer.current()`

Use this method to retrieve a typed buffer snapshot.

```ts
const buffer = await saya.buffer.current();
console.log(buffer.id, buffer.path, buffer.lineCount, buffer.currentLine);
```

The returned snapshot currently includes:

- `id`
- `path`
- `lineCount`
- `cursorRow`
- `cursorCol`
- `currentLine`
- `text`

`cursorRow`, `cursorCol`, `currentLine`, and `text` are read-only snapshot
fields for plugins that need display context. Dired-style commands use
`saya.filer.currentEntry()` when they need the filesystem entry associated with
the cursor row.

### `saya.buffer.selection()`

Use this method to retrieve the current visual selection snapshot when the
editor is in a visual mode.

```ts
const selection = await saya.buffer.selection();
if (selection) {
  console.log(selection.mode, selection.text);
}
```

The method returns `null` when no visual selection is active. Active selection
snapshots include `mode`, `startLine`, `startColumn`, `endLine`, `endColumn`,
and `text`.

### `saya.buffer.currentPath()`

Use this method to retrieve only the current buffer path without fetching the
buffer text.

```ts
const path = await saya.buffer.currentPath();
```

This is the preferred API for path-based commands such as opening the parent
directory.

## Window

The window surface lets you read the current window snapshot and manage floating
windows through typed host-mediated requests. Runtime code never receives raw
renderer access.

### `saya.window.current()`

Use this method to retrieve a typed window snapshot.

```ts
const windowState = await saya.window.current();
console.log(windowState.id);
```

### `saya.window.openFloat(options)`

Use this method to open a floating surface. The host owns placement, focus,
lifecycle, rendering composition, and terminal process state.

```ts
const float = await saya.window.openFloat({
  content: { kind: "lines", lines: ["Type information", "from a plugin"] },
  relativeTo: { kind: "cursor" },
  width: 60,
  height: 12,
  row: 1,
  col: 2,
  focusable: true,
  border: "single",
  zIndex: "hover",
  lifecycle: "closeOnCursorMove",
  group: "plugin:hover",
});
```

The method returns a read-only float snapshot with `id`, `kind`, `focused`,
`focusable`, `width`, `height`, `row`, `col`, `border`, `zIndex`, `lifecycle`,
and `replacementGroup`.

Supported content kinds are:

- `lines`: Static read-only lines.
- `buffer`: A buffer-backed view using an existing host window, or a buffer ID
  that is already visible in an existing host window.
- `terminal`: A PTY-backed terminal surface with an explicit command array.

For buffer floats, `windowId` binds the float to that existing host window. If
you pass only `bufferId`, the host selects an existing window that already
displays that buffer. The host rejects buffer IDs that are not backed by a
current core window because hidden core-window creation is not part of the
runtime API yet.

For terminal floats, pass the command as an array. The host owns the PTY, parses
terminal output, routes focused key input to the terminal session, and applies
the close policy when the float closes.

```ts
const terminal = await saya.window.openFloat({
  content: {
    kind: "terminal",
    command: ["sh", "-lc", "git status"],
    closeBehavior: "killOnClose",
  },
  width: 90,
  height: 20,
  focusable: true,
  border: "single",
  zIndex: "user",
});
```

### `saya.window.focus(id)`

Use this method to focus a focusable float.

```ts
const focused = await saya.window.focus(float.id);
```

The method returns `true` when the host focused the float.

### `saya.window.close(id)`

Use this method to close a float.

```ts
const closed = await saya.window.close(float.id);
```

The method returns `true` when the host closed the float. Closing a terminal
float applies the terminal close policy chosen when the float was opened.

### `saya.window.floats()`

Use this method to retrieve read-only snapshots for the currently open floats.

```ts
const floats = await saya.window.floats();
for (const float of floats) {
  console.log(float.id, float.kind, float.focused);
}
```

## Filesystem

The filesystem surface is intentionally narrow. It exists for read-only
directory listing helpers that don't need the full filer operation surface.

### `saya.fs.readDir(path, options)`

Use this method to read a directory listing with the same listing options used
by `saya.filer.list()`.

```ts
const entries = await saya.fs.readDir(".", {
  showHidden: false,
  sortBy: "kind",
  filter: "rs",
});
```

The method returns filer-style entries with `name`, `path`, `kind`,
`displayText`, and optional metadata such as `size` and `modifiedTimeMs`.

## Input

The input surface lets runtime callbacks ask the host for a focused prompt
without exposing raw terminal input.

### `saya.input.prompt(options)`

Use this method to show a prompt and wait for a string response.

```ts
const name = await saya.input.prompt({
  title: "New file",
  placeholder: "notes.md",
});
```

The method returns the entered text, or `null` when the prompt is cancelled.

## Selector

The selector surface opens and controls host-managed selection workflows for
static items or `rg`-backed searches.

### `saya.selector.open(options)`

Use this method to open a selector.

```ts
const selector = await saya.selector.open({
  source: {
    kind: "static",
    items: [
      { id: "a", value: "alpha", kind: "item", detail: null },
      { id: "b", value: "beta", kind: "item", detail: null },
    ],
  },
  matcher: "substringAnd",
});
```

The selector surface also exposes `update(id, options)`, `current(id)`,
`control(id, options)`, `cancel(id)`, and `dispose(id)` for query updates,
snapshot reads, cursor control, cancellation, and cleanup.

## Completion

The completion surface lets runtime callbacks open and close the typed
completion menu. Source orchestration lives in the bundled completion plugin.

### `saya.completion.show(request)`

Use this method to show completion candidates for a request-scoped replace
range.

```ts
await saya.completion.show({
  sessionId: "buffer-1",
  requestId: 1,
  replaceRange: {
    start: { line: 0, character: 0 },
    end: { line: 0, character: 3 },
  },
  candidates: [{ label: "alpha", insertText: "alpha" }],
  selectedIndex: 0,
});
```

Use `saya.completion.close()` to close the active completion menu.

## Process

The process surface is a preview capability used by bundled runtime plugins
that need host-mediated process I/O. It is not the stable foundation for LSP;
use `saya.lsp.connect()` for language server sessions.

### `saya.process.spawn(spec)`

Use this method to spawn a process through the host.

```ts
const handle = await saya.process.spawn({
  command: "rg",
  args: ["--line-number", "needle", "."],
  stdout: "piped",
  stderr: "piped",
});
```

The returned handle exposes `stdin`, `stdout`, `stderr`, `kill()`, and `wait()`.
Treat this surface as preview-only while the plugin and permission model is
still settling.

## Plugins

The plugins surface is used by the plugin manager to load lazy plugins after a
declared trigger fires.

### `saya.plugins.loadLazy(request)`

Use this method to load a lazy plugin module selected by the startup plugin
declarations.

```ts
await saya.plugins.loadLazy({
  kind: "command",
  name: "GitStatus",
  plugin: "git-tools",
  module: "index.ts",
  exportName: "setup",
});
```

## Panels

The panel surface lets runtime callbacks open persistent side panels. Panels are
different from transient floating windows: they are intended for longer running
tool surfaces such as terminal-backed assistants, status views, and plugin work
areas.

> **Note:** This is a preview feature currently under active development.

### Panel command behavior

The bundled agent panel setup registers user-facing panel commands during
startup. Use these commands from normal Ex command-line input:

```vim
:panel.toggle
:panel.close
:panel.focus
:panel.unfocus
```

`panel.toggle` opens or closes the configured panel without stealing editor
focus. This lets you run another editor command immediately after opening the
panel. `panel.focus` selects the panel as the active persistent display area.
Only terminal-backed panels use that focus as terminal input mode. From a
focused terminal panel, press `:` to enter the editor command line, press `/` to
enter editor search, or press `Ctrl-w` to return focus to the editor while
leaving the panel open. Runtime code can also call `panel.unfocus`. Use
`panel.close` to close the panel from the editor command line.

The older `agent.toggle`, `agent.focus`, `agent.close`, and `agent.detach`
aliases are not registered. Agent-specific commands remain available only for
sending editor context, such as `agent.sendCurrentLine` and
`agent.sendSelectedRange`.

### `saya.panel.open(options)`

Use this method to open or replace a persistent panel. The host owns placement,
rendering, terminal process state, focus, and lifecycle.

```ts
const panel = await saya.panel.open({
  id: "review-panel",
  position: "right",
  size: "35%",
  content: {
    kind: "terminal",
    command: ["codex"],
    closeBehavior: "detach",
  },
  focus: false,
});
```

The method returns a read-only panel snapshot with `id`, `numericId`,
`position`, `size`, `kind`, and `focused`.

Supported content kinds are:

- `lines`: Static read-only lines.
- `terminal`: A PTY-backed terminal panel with an explicit command array.
- `view`: Structured plugin-controlled UI nodes rendered by the host TUI.

For terminal panels, pass the command as an array. The host owns the PTY, parses
terminal output, routes key input to the terminal only when the panel is
focused, and applies the close policy when the panel closes.

For view panels, pass a declarative `nodes` array. The host owns layout,
rendering, focus routing, and lifecycle. Plugins don't receive raw renderer or
terminal drawing access.

```ts
await saya.panel.open({
  id: "dashboard",
  position: "right",
  size: "35%",
  content: {
    kind: "view",
    nodes: [
      { type: "heading", text: "Weather" },
      { type: "text", text: "16C" },
      { type: "badge", label: "rain" },
      { type: "progress", label: "build", value: 50 },
      { type: "divider" },
      { type: "button", label: "Refresh" },
      { type: "image", src: "/tmp/moon.png", alt: "Moon phase" },
    ],
  },
  focus: true,
});
```

The initial view node subset is `text`, `heading`, `divider`, `image`, `badge`,
`progress`, and `button`. Image nodes render as text fallback in the TUI until
terminal image rendering is introduced.

### `saya.panel.focus(id)`

Use this method to move input focus to an existing panel.

```ts
const focused = await saya.panel.focus(panel.id);
```

The method returns `true` when the host focused the panel. A focused terminal
panel receives typed keys. A focused `lines` or `view` panel is selected, but it
doesn't enter terminal input mode. The `:` and `/` keys are reserved for
returning from a focused terminal panel to the editor command line and editor
search.

### `saya.panel.unfocus()`

Use this method to clear panel focus while keeping the panel open.

```ts
const unfocused = await saya.panel.unfocus();
```

The method returns `true` when a panel had focus. Terminal panel users can also
press `Ctrl-w` to return focus to the editor without closing the panel.

### `saya.panel.close(id)`

Use this method to close a panel.

```ts
const closed = await saya.panel.close(panel.id);
```

The method returns `true` when the host closed the panel. Closing a terminal
panel applies the close policy chosen when the panel was opened.

### `saya.panel.list()`

Use this method to retrieve read-only snapshots for the currently open panels.

```ts
const panels = await saya.panel.list();
for (const panel of panels) {
  console.log(panel.id, panel.position, panel.focused);
}
```

### `saya.panel.send(id, text)`

Use this method to send text to a terminal-backed panel.

```ts
await saya.panel.send(panel.id, "Review the current file\n");
```

The method returns `true` when the panel exists and accepts terminal input.
`lines` and `view` panels don't accept sent text.

## Editor

The editor surface lets you inspect editor-level state.

### `saya.editor.current()`

Use this method to retrieve a typed editor snapshot.

```ts
const editor = await saya.editor.current();
console.log(editor.mode);
```

### `saya.editor.mode()`

Use this method when you only need the current mode.

```ts
const mode = await saya.editor.mode();
console.log(mode);
```

## Filer

The filer surface lets TypeScript plugins read a directory listing and inspect
the active directory buffer without opening a broad filesystem API. Use it for
directory-editor plugins that need file entries while keeping writes and
arbitrary filesystem access out of the runtime surface.

> **Note:** Dired and filer operations are preview APIs. They are covered by
> public surface guards, but command names, option shapes, and operation reports
> can still change before the dired API is stabilized. See
> [Dired API v1](dired-api-v1.md) for the versioned local dired contract,
> migration notes, and plugin author anti-patterns.

Directory listings are not saved as regular files. Writable directory buffers
use a save-time preview flow instead: plain `:write` prepares an operation
preview and opens a confirmation prompt. Press `y` or Enter to apply only the
latest matching preview through host-mediated filer operations. Press `n` or Esc
to cancel without changing the filesystem. The preview message includes the
preview ID and the high-risk operation count. `:write!` remains a legacy
explicit confirmation path; missing, stale, or invalid previews don't mutate the
filesystem.

Confirmed writable-buffer operations run as host-side transactions. The host
checks for path conflicts before execution, uses temporary paths to avoid rename
collisions, runs deletes after create and rename operations, and refreshes
directory metadata from the filesystem after success or failure. If a
transaction partially succeeds, the diagnostic message includes structured
counts for successful steps, failed steps, rollback results, and manual recovery
requirements.

### `saya.filer.list(path, options)`

Use this method to read a sorted directory listing. The default order groups
directories before other entries, matching `eza --group-directories-first`.
Within the directory group and the non-directory group, entries are sorted by
display text.

```ts
const entries = await saya.filer.list(".", {
  showHidden: false,
  sortBy: "size",
  filter: "notes",
});
console.log(entries.map((entry) => `${entry.kind}:${entry.name}`));
```

The optional `options` object supports:

- `showHidden`, as `true` to include dotfiles and `false` to hide them. The
  default is `true` for compatibility with earlier `saya.filer.list(path)`
  behavior.
- `sortBy`, as `"name"`, `"kind"`, `"modifiedTime"`, or `"size"`. The default is
  `"kind"`, which groups directories before other entries.
- `filter`, as a case-insensitive substring matched against `name` and
  `displayText`. Empty strings and omitted values keep the listing unfiltered.

Each entry contains:

- `name`
- `path`
- `kind`, as `"directory"`, `"file"`, `"symlink"`, or `"other"`
- `displayText`, with `/` for directories, `@` for symlinks, and `?` for other
  filesystem entries
- `size`, when host metadata is available
- `modifiedTimeMs`, when host metadata is available

### `saya.filer.currentEntry()`

Use this method from a dired-style command to read the entry associated with the
current cursor row in the active directory buffer.

```ts
const entry = await saya.filer.currentEntry();
if (entry) {
  await saya.commands.execute(`edit ${entry.path}`);
}
```

The API returns `null` when the active buffer is not a directory buffer or the
cursor row has no entry. The returned entry comes from host-side directory
buffer metadata, not from parsing the rendered listing text. This keeps commands
stable when the listing display changes.

Each current entry contains:

- `id`
- `name`
- `path`
- `kind`, as `"directory"`, `"file"`, `"symlink"`, or `"other"`
- `rootPath`
- `displayText`

### `saya.filer.createFile(path)`

Use this method to create one new empty file through the host application. The
operation fails when the target already exists.

```ts
await saya.filer.createFile(`${entry.rootPath}/notes.md`);
await saya.commands.execute(`edit ${entry.rootPath}`);
```

### `saya.filer.createDirectory(path)`

Use this method to create one directory through the host application. The
operation fails when the target already exists.

```ts
await saya.filer.createDirectory(`${entry.rootPath}/src`);
await saya.commands.execute(`edit ${entry.rootPath}`);
```

### `saya.filer.copy(from, to)`

Use this method to copy one regular file through the host application. The
operation fails when the source path doesn't exist, the destination path
collides, or the source is a directory. Directory copy is intentionally not
recursive in the preview API.

```ts
await saya.filer.copy(entry.path, `${entry.rootPath}/copy.md`);
await saya.commands.execute(`edit ${entry.rootPath}`);
```

### `saya.filer.move(from, to)`

Use this method to move one file or directory through the host application. The
operation uses the host rename path and fails when the source path doesn't exist
or the destination path collides.

```ts
await saya.filer.move(entry.path, `${entry.rootPath}/moved.md`);
await saya.commands.execute(`edit ${entry.rootPath}`);
```

### `saya.filer.rename(from, to)`

Use this method to rename one file or directory through the host application.
The operation fails when the source path doesn't exist or the destination path
collides.

```ts
await saya.filer.rename(entry.path, `${entry.rootPath}/renamed.md`);
await saya.commands.execute(`edit ${entry.rootPath}`);
```

### `saya.filer.delete(path, options)`

Use this method to delete one file or one empty directory through the host
application. You must pass `{ confirm: true }`; the operation fails without an
explicit confirmation flag. Recursive deletion is disabled, and
`{ trash: true }` fails with an unsupported-backend error until a platform trash
policy is configured.

```ts
await saya.filer.delete(entry.path, { confirm: true });
await saya.commands.execute(`edit ${entry.rootPath}`);
```

### `saya.filer.mark(path)`

Use this method to mark one entry in the active directory buffer. The path must
match an entry from the active directory buffer metadata.

```ts
const entry = await saya.filer.currentEntry();
if (entry) {
  await saya.filer.mark(entry.path);
}
```

### `saya.filer.unmark(path)`

Use this method to remove one entry from the active directory mark set. The path
must match an entry from the active directory buffer metadata.

```ts
const entry = await saya.filer.currentEntry();
if (entry) {
  await saya.filer.unmark(entry.path);
}
```

### `saya.filer.clearMarks()`

Use this method to remove every mark from the active directory buffer.

```ts
await saya.filer.clearMarks();
```

### `saya.filer.bulkDeletePreview()`

Use this method to preview the currently marked entries before a bulk delete.
The report contains the entries and a `previewId` derived from the current mark
set.

```ts
const preview = await saya.filer.bulkDeletePreview();
console.log(preview.entries.map((entry) => entry.path));
```

### `saya.filer.bulkDelete(options)`

Use this method to delete the currently marked entries after previewing them.
You must pass `{ confirm: true, previewId }` with the latest preview ID. The
operation fails if the preview ID is missing, stale, or not confirmed.

```ts
const preview = await saya.filer.bulkDeletePreview();
await saya.filer.bulkDelete({
  confirm: true,
  previewId: preview.previewId,
});
```

Each successful operation returns a report with:

- `operation`, as `"createFile"`, `"createDirectory"`, `"rename"`, `"delete"`,
  `"copy"`, `"move"`, `"mark"`, `"unmark"`, `"clearMarks"`,
  `"bulkDeletePreview"`, or `"bulkDelete"`
- `path`
- `targetPath`, for rename operations
- `entries`, for mark and bulk operations
- `previewId`, for bulk delete preview and confirmed bulk delete

Filer operation failures surface a structured error payload in the runtime error
message. The payload includes the operation, path, optional target path, error
kind, and host error message. Error kinds include `"alreadyExists"`,
`"confirmationRequired"`, `"notFound"`, and `"permissionDenied"`.

Save-time directory transaction failures also include structured counts in the
host error message so UI code and logs can distinguish successful steps, failed
steps, rollback results, and manual recovery requirements.

## What the runtime API does not expose

The runtime surface deliberately excludes startup registration and high-risk
capabilities.

- `saya.options.*`
- `saya.keymap.set(...)`
- `saya.commands.register(...)`
- `saya.events.on(...)`
- Broad filesystem access
- Network access
- Vim-compatibility string DSLs

## Next steps

If you want to understand how these APIs are produced and isolated, read
[TypeScript runtime design](../design/typescript-runtime.md).
