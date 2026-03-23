# Startup API

This page documents the public TypeScript startup surface that `saya` exposes
while evaluating `init.ts`. This API exists only during startup evaluation and
is intentionally narrow.

If you need live editor state access during callback execution, use the runtime
API instead of this page.

## Availability

The startup API is available only while `saya` evaluates the startup module
that you pass through `--config`.

The startup surface is designed for declaration, not for broad live editor
control.

## Namespace

The startup surface lives under the global `saya` object and exposes these
top-level areas.

- `saya.options`
- `saya.keymap`
- `saya.commands`
- `saya.events`

## Options

The options surface lets you set initial editor options before the session
starts.

### `saya.options.tabSize`

Use this number property to control tab expansion width in the projected TUI.

```ts
saya.options.tabSize = 4;
```

### `saya.options.lineNumbers`

Use this boolean property to enable line-number prefixes in the projected TUI.

```ts
saya.options.lineNumbers = true;
```

## Keymaps

The keymap surface lets you define startup keymaps as normalized registry
entries.

### `saya.keymap.set(mode, lhs, action)`

Use this method to register a keymap at startup time.

- `mode` accepts `"normal"`, `"insert"`, or `"visual"`
- `lhs` is the left-hand-side key sequence
- `action` is either a literal string or a startup command reference

```ts
saya.keymap.set("normal", "<leader>w", saya.commands.execute("writeCurrent"));
```

## Commands

The commands surface lets you register named startup callbacks and refer to
them from keymaps.

### `saya.commands.register(name, callback)`

Use this method to register a named callback during startup.

```ts
saya.commands.register("writeCurrent", () => {
  return saya.commands.execute("write");
});
```

### `saya.commands.execute(name)`

During startup, this method returns a command reference rather than executing a
runtime command immediately.

```ts
const writeRef = saya.commands.execute("writeCurrent");
saya.keymap.set("normal", "<leader>w", writeRef);
```

## Events

The events surface lets you register startup-time event handlers that later
seed the runtime callback layer.

### `saya.events.on(name, callback)`

Use this method to register an event callback during startup.

The current typed event names are:

- `"bufferOpen"`
- `"bufferWritePost"`

```ts
saya.events.on("bufferOpen", (payload) => {
  console.log(payload.buffer.id);
});
```

## What the startup API does not expose

The startup surface deliberately excludes runtime-only and high-risk features.

- `saya.buffer.current()`
- `saya.window.current()`
- `saya.editor.current()`
- `saya.editor.mode()`
- Filesystem access
- Network access
- Vim-compatibility string DSLs

## Next steps

If you need the live callback contract after startup completes, read
[Runtime API](runtime-api.md).
