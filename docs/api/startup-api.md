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

`init.ts` can use static local TypeScript imports for project plugins. The
startup loader resolves local file imports before evaluation, strips type-only
declarations, and evaluates the combined startup module.

```ts
/// <reference path="./plugins/saya-startup.d.ts" />
import { setupSayaDired } from "./plugins/saya-dired.ts";

setupSayaDired();
```

Only local file imports are supported. Bare package imports and network imports
are intentionally outside the startup surface.

Use `plugins/saya-startup.d.ts` when you want an external TypeScript language
server to understand the startup-only global `saya` object while editing
`init.ts`. The reference comment is type-only and has no runtime effect.

### Preview dired setup

The repository includes `plugins/saya-dired.ts` as a preview TypeScript plugin.
Import it from `init.ts` when you want the directory editor commands and
keymaps to be registered at startup.

```ts
import { setupSayaDired } from "./plugins/saya-dired.ts";

setupSayaDired({
  root: ".",
  hiddenFilePolicy: "hide",
  sortPolicy: "kind",
  filter: "rs",
  confirmStrategy: "preview",
  commands: {
    enter: "workspace.enter",
    refresh: "workspace.refresh",
  },
  keymap: {
    enter: "<Enter>",
    refresh: "gr",
  },
});
```

`setupSayaDired()` keeps the public setup surface grouped around command names,
keymaps, root selection, hidden-file policy, sort policy, filter text, and
destructive operation confirmation strategy. Filesystem mutation still goes
through the runtime `saya.filer` surface, not through broad startup filesystem
access.
The default mark bindings are `m` for marking, `M` for unmarking, and `gM` for
clearing marks, leaving `u` available for normal-mode undo while editing a
writable directory listing.

> **Note:** Dired is a preview feature currently under active development. The
> setup options are public enough for plugin reuse, but they can change before
> the API is stabilized.
> See [Dired API v1](dired-api-v1.md) for the versioned local dired contract,
> migration notes, and plugin author anti-patterns.

### Preview LSP setup

The repository includes `plugins/saya-lsp-client.ts` as a preview TypeScript
plugin. Import it from `init.ts` when you want LSP commands, normal-mode
keymaps, document synchronization events, and language server definitions to be
registered at startup.

> **Note:** This is a preview feature currently under active development.
> See [LSP preview](lsp-preview.md) for the runtime boundary, the `gopls`
> example, supported features, LSIF limitations, and headless verification
> commands.

```ts
import { setupSayaLspClient } from "./plugins/saya-lsp-client.ts";

setupSayaLspClient({
  languageIdByExtension: {
    go: "go",
    rs: "rust",
  },
  servers: [
    {
      name: "gopls",
      command: "gopls",
      args: ["serve"],
      languages: ["go"],
      filePatterns: ["**/*.go"],
      rootMarkers: ["go.mod", ".git"],
      initializationOptions: {
        semanticTokens: true,
      },
    },
    {
      name: "rust-analyzer",
      command: "rust-analyzer",
      languages: ["rust"],
      filePatterns: ["**/*.rs"],
      rootMarkers: ["Cargo.toml", ".git"],
    },
  ],
});
```

`setupSayaLspClient()` keeps language server configuration in TypeScript. Each
server definition names the executable command, optional arguments,
initialization options, language IDs, file patterns, and workspace root markers.
At runtime, the plugin selects the matching server for the current buffer,
detects the workspace root through the narrow runtime workspace API, and sends
the selected server definition through `saya.lsp.request`.

The default command names are:

- `lsp.initialize`
- `lsp.initialized`
- `lsp.hover`
- `lsp.definition`
- `lsp.references`
- `lsp.documentSymbol`
- `lsp.nextDiagnostic`
- `lsp.previousDiagnostic`
- `lsp.shutdown`
- `lsif.hover`
- `lsif.definition`

The default normal-mode keymaps are:

- `K` for hover
- `gd` for definition
- `gR` for references
- `gO` for document symbols
- `]d` for next diagnostic
- `[d` for previous diagnostic
- `gK` for LSIF hover when LSIF is enabled
- `gD` for LSIF definition when LSIF is enabled

For a non-Go server, use the same shape with a different command and marker set.

```ts
setupSayaLspClient({
  servers: {
    python: {
      name: "pyright",
      command: "pyright-langserver",
      args: ["--stdio"],
      languages: ["python"],
      filePatterns: ["**/*.py"],
      rootMarkers: ["pyproject.toml", "setup.py", ".git"],
    },
  },
});
```

Invalid command names, non-`file://` root URIs, empty language IDs, empty
server commands, and malformed server definitions fail during startup
evaluation with a configuration error.

## Namespace

The startup surface lives under the global `saya` object and exposes these
top-level areas.

- `saya.options`
- `saya.keymap`
- `saya.commands`
- `saya.events`
- `saya.theme`
- `saya.log`
- `saya.plugins`

## Options

The options surface lets you set initial editor options before the session
starts.

`saya.options.*` uses Vim and Neovim option names. The startup file is
TypeScript, but option property names stay lowercase Vim-style names such as
`tabstop`, `number`, `numberwidth`, and `cmdheight`. JavaScript-style camelCase
names are not part of the public startup API.

The exported TypeScript declaration lets editors and `tsc` report unknown
option names while you edit `init.ts`. Startup evaluation also records a
message-area warning for unknown option assignments and ignores that option;
valid assignments in the same file still apply.

### `saya.options.tabstop`

Use this number property to control tab expansion width in the projected TUI.

```ts
saya.options.tabstop = 4;
```

### `saya.options.number`

Use this boolean property to enable line-number prefixes in the projected TUI.

```ts
saya.options.number = true;
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

## Plugins

The plugins surface lets you declare startup and lazy plugin intent from
`init.ts`. The declarations are inputs to the plugin manager. They don't install
or update external plugins during editor startup.

### `saya.plugins.use(specs)`

Use this method to declare plugins that must be active during startup.

```ts
saya.plugins.use([
  { github: "shun/saya-theme-tokyo-night" },
  { local: "~/.config/saya/plugins/workspace-tools" },
]);
```

### `saya.plugins.lazy(specs)`

Use this method to declare plugins that load after command or event triggers.

```ts
saya.plugins.lazy([
  {
    github: "shun/saya-git-tools",
    commands: ["GitStatus", "GitBlame"],
  },
  {
    local: "~/.config/saya/plugins/workspace-tools",
    events: ["bufferOpen"],
  },
]);
```

Plugin declarations support `local` for local directories and `github` for
GitHub repositories in `owner/repository` form. Add `rev` to pin a GitHub
plugin to a branch, tag, or commit. When `rev` is absent, plugin sync and update
commands resolve the latest default-branch revision and record it in the plugin
cache.

## Theme

The theme surface lets you declare UI, syntax, and Markdown presentation styles
at startup without exposing renderer internals or Vim-compatible highlight
groups.

### `saya.theme.palette`

Use this object property to define named color tokens. Values must be direct
hex colors in `#rrggbb` form.

```ts
saya.theme.palette = {
  accent: "#7aa2f7",
  heading2: "#9ece6a",
  code: "#ff9e64",
  link: "#2ac3de",
};
```

### `saya.theme.markdown`

Use this object property to define semantic Markdown styles. Color attributes
can reference palette tokens or direct hex colors. Unknown palette tokens fall
back deterministically by omitting that color.

```ts
saya.theme.markdown = {
  heading: {
    fg: "accent",
    bold: true,
  },
  heading2: {
    fg: "heading2",
    underline: true,
  },
  inlineCode: {
    fg: "code",
  },
  link: {
    fg: "link",
    underline: true,
  },
};
```

The current Markdown keys are `heading`, `heading1`, `heading2`, `heading3`,
`heading4`, `heading5`, `heading6`, `inlineCode`, `link`, `listMarker`,
`checkboxChecked`, `checkboxUnchecked`, `table`, and `fencedCodeBlock`.
Level-specific heading keys inherit from `heading` and override attributes that
they declare. Boolean attributes can also disable inherited values. For
example, `heading2: { bold: false }` turns off `heading.bold` for level-two
headings.

### `saya.theme.ui`

Use this object property to define editor UI styles. Color attributes can
reference palette tokens or direct hex colors.

```ts
saya.theme.ui = {
  text: {
    fg: "fg",
    bg: "bg",
  },
  statusActive: {
    fg: "bg",
    bg: "accent",
    bold: true,
  },
};
```

The current UI keys are `text`, `gutter`, `statusActive`, `statusInactive`,
`message`, `warningMsg`, and `prompt`. `warningMsg` is the Vim-style warning
message group. Saya uses it for startup warnings, such as ignored unknown
options. Long message-area warnings wrap to the current editor width and use
the message pager when they exceed `cmdheight`.

### `saya.theme.syntax`

Use this object property to define broad syntax styles for non-Markdown files.
Saya maps Vim syntax groups and tree-sitter categories into these semantic
syntax keys.

```ts
saya.theme.syntax = {
  comment: {
    fg: "comment",
    italic: true,
  },
  statement: {
    fg: "accent",
  },
};
```

The current syntax keys are `comment`, `string`, `constant`, `statement`,
`identifier`, `type`, `function`, `punctuation`, `markup`, and `default`.

## Log

The log surface lets you declare diagnostic logging during startup. Use it for
headless debugging and issue reproduction rather than normal editor
interaction.

### `saya.log.file`

Use this string property to write diagnostic logs to a file. If `SAYA_LOG_FILE`
is set in the environment, that environment path takes precedence over this
startup property.

```ts
saya.log.file = "/tmp/saya.log";
```

### `saya.log.level`

Use this string property to choose the minimum diagnostic log level collected
after startup configuration is evaluated.

```ts
saya.log.level = "warn";
```

The accepted levels are `"error"`, `"warn"`, `"info"`, `"debug"`, and
`"trace"`.

## What the startup API does not expose

The startup surface deliberately excludes runtime-only and high-risk features.

- `saya.buffer.current()`
- `saya.window.current()`
- `saya.editor.current()`
- `saya.editor.mode()`
- Filesystem access
- Network access
- Vim-compatibility string DSLs
- Vim or Neovim `:highlight` compatibility

## Next steps

If you need the live callback contract after startup completes, read
[Runtime API](runtime-api.md).
