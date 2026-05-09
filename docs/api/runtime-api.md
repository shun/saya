# Runtime API

This page documents the public TypeScript runtime surface that `saya` exposes
while executing runtime callbacks. This API is separate from the startup API
and focuses on typed state reads plus explicit command execution.

If you need startup-time registration APIs, read the startup API page instead
of this one.

## Availability

The runtime API is available only inside runtime callback execution. The
repository currently validates this behavior primarily through headless tests.

The runtime surface is narrower than the startup surface by design.

## Namespace

The runtime surface lives under the global `saya` object and exposes these
top-level areas.

- `saya.commands`
- `saya.buffer`
- `saya.window`
- `saya.editor`
- `saya.workspace`
- `saya.lsp`
- `saya.filer`

## Commands

The commands surface lets runtime code execute named commands through the host
capability bridge.

### `saya.commands.execute(name)`

Use this method to execute a named command.

```ts
await saya.commands.execute("write");
```

Commands may resolve to a registered runtime callback or to a host-side
command implementation, depending on the current registry and host bridge.

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

The method returns a path string when a marker matches and `null` when no
marker is found. The LSP preview plugin uses this API to resolve per-server
workspace roots from marker lists such as `go.mod`, `Cargo.toml`, and `.git`.

## LSP

The LSP surface exposes one typed host bridge for preview LSP and LSIF
requests. It is intentionally narrower than a general process or filesystem
API. Runtime code describes the request, and the Rust host owns process
lifecycle, JSON-RPC framing, document synchronization, diagnostic logging, and
LSIF index lookup.

> **Note:** This is a preview feature currently under active development.
> See [LSP preview](lsp-preview.md) for setup examples, the feature support
> matrix, LSIF limitations, and verification commands.

### `saya.lsp.request(payload)`

Use this method to send a validated LSP or LSIF bridge request to the host.
Most users call it indirectly through `setupSayaLspClient()` from
`plugins/saya-lsp-client.ts`.

```ts
const response = await saya.lsp.request({
  source: "lsp",
  lspVersion: "3.17",
  method: "textDocument/hover",
  clientName: "gopls",
  rootUri: "file:///workspace",
  languageId: "go",
  trace: "messages",
  positionEncoding: "utf-16",
  dumpPath: "",
  textDocument: { uri: "file:///workspace/main.go" },
  server: {
    name: "gopls",
    command: "gopls",
    args: ["serve"],
    env: {},
    cwd: null,
    rootMarkers: ["go.mod", ".git"],
    initializationOptions: {},
  },
  position: { line: 0, character: 0 },
  params: {},
  buffer: await saya.buffer.current(),
  editor: await saya.editor.current(),
});
```

The method returns `{ source, method, result }` when the host completes the
request. Invalid payloads and host failures surface as command errors with
user-safe messages.

## Buffer

The buffer surface lets you read the current buffer snapshot.

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
- `currentLine`

`cursorRow` and `currentLine` are read-only snapshot fields for plugins that
need display context. Dired-style commands use `saya.filer.currentEntry()` when
they need the filesystem entry associated with the cursor row.

## Window

The window surface lets you read the current window snapshot and manage
floating windows through typed host-mediated requests. Runtime code never
receives raw renderer access.

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
`focusable`, `width`, `height`, `row`, `col`, `border`, `zIndex`,
`lifecycle`, and `replacementGroup`.

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

For terminal floats, pass the command as an array. The host owns the PTY,
parses terminal output, routes focused key input to the terminal session, and
applies the close policy when the float closes.

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
> can still change before the dired API is stabilized.
> See [Dired API v1](dired-api-v1.md) for the versioned local dired contract,
> migration notes, and plugin author anti-patterns.

Directory listings are not saved as regular files. Writable directory buffers
use a save-time preview flow instead: plain `:write` prepares an operation
preview and opens a confirmation prompt. Press `y` or Enter to apply only the
latest matching preview through host-mediated filer operations. Press `n` or
Esc to cancel without changing the filesystem. The preview message includes
the preview ID and the high-risk operation count. `:write!` remains a legacy
explicit confirmation path; missing, stale, or invalid previews don't mutate
the filesystem.

Confirmed writable-buffer operations run as host-side transactions. The host
checks for path conflicts before execution, uses temporary paths to avoid
rename collisions, runs deletes after create and rename operations, and
refreshes directory metadata from the filesystem after success or failure. If a
transaction partially succeeds, the diagnostic message includes structured
counts for successful steps, failed steps, rollback results, and manual
recovery requirements.

### `saya.filer.list(path, options)`

Use this method to read a sorted directory listing. The default order preserves
the existing kind-first listing: directories, files, symlinks, and other
entries, with each group sorted by display text.

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
- `sortBy`, as `"name"`, `"kind"`, `"modifiedTime"`, or `"size"`. The default
  is `"kind"` for compatibility with earlier listings.
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
operation uses the host rename path and fails when the source path doesn't
exist or the destination path collides.

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
`{ trash: true }` fails with an unsupported-backend error until a platform
trash policy is configured.

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

Filer operation failures surface a structured error payload in the runtime
error message. The payload includes the operation, path, optional target path,
error kind, and host error message. Error kinds include `"alreadyExists"`,
`"confirmationRequired"`, `"notFound"`, and `"permissionDenied"`.

Save-time directory transaction failures also include structured counts in the
host error message so UI code and logs can distinguish successful steps,
failed steps, rollback results, and manual recovery requirements.

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
