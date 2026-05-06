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

## Buffer

The buffer surface lets you read the current buffer snapshot.

### `saya.buffer.current()`

Use this method to retrieve a typed buffer snapshot.

```ts
const buffer = await saya.buffer.current();
console.log(buffer.id, buffer.path, buffer.lineCount);
```

The returned snapshot currently includes:

- `id`
- `path`
- `lineCount`

## Window

The window surface lets you read the current window snapshot.

### `saya.window.current()`

Use this method to retrieve a typed window snapshot.

```ts
const windowState = await saya.window.current();
console.log(windowState.id);
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

The filer surface lets TypeScript plugins read a directory listing without
opening a broad filesystem API. Use it for directory-editor plugins that need
file entries while keeping writes and arbitrary filesystem access out of the
runtime surface.

### `saya.filer.list(path)`

Use this method to read a sorted directory listing.

```ts
const entries = await saya.filer.list(".");
console.log(entries.map((entry) => `${entry.kind}:${entry.name}`));
```

Each entry contains:

- `name`
- `path`
- `kind`, as `"directory"`, `"file"`, `"symlink"`, or `"other"`

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
