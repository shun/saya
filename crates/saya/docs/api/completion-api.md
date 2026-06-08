# Completion API

This page documents the bundled `saya-completion` plugin and the typed runtime
completion surface. The plugin owns source orchestration, filtering, sorting,
and timeout policy. The Rust host owns the typed menu request, session
lifecycle, stale request rejection, rendering, and confirm-time text edits.

> **Note:** This is a preview feature currently under active development.

## Setup

Use `setupSayaCompletion()` from the bundled `saya-completion` plugin in your
startup configuration. The setup registers `completion.trigger`. It doesn't
enable any completion source, manual trigger keymap, or automatic trigger event
unless you configure those options, so completion stays quiet by default.
Menus use standard operation keys such as `<Enter>`, `<Tab>`, `<C-y>`,
`<C-e>`, `<C-n>`, and `<C-p>` unless you override them. `<Esc>` and `<C-[>`
close the menu and continue as editor escape keys, so insert mode exits just as
it does in Vim and Neovim.

```ts
import {
  createBufferWordSource,
  createLspCompletionSource,
  createPathCompletionSource,
  setupSayaCompletion,
} from "/path/to/plugins/bundled/completion/index.ts";

setupSayaCompletion({
  key: "<C-Space>",
  keys: {
    confirm: ["<Enter>", "<Tab>", "<C-y>"],
    close: ["<C-e>"],
    next: ["<Down>", "<C-n>"],
    previous: ["<Up>", "<C-p>"],
    pageNext: ["<PageDown>"],
    pagePrevious: ["<PageUp>"],
  },
  sources: [
    createLspCompletionSource({ minPrefixLength: 1 }),
    createPathCompletionSource({
      minPrefixLength: 1,
      triggerCharacters: ["/", "."],
    }),
    createBufferWordSource({ minPrefixLength: 1 }),
  ],
});
```

Enable automatic completion when you want candidates to open as the user types
in insert mode:

```ts
setupSayaCompletion({
  autoTrigger: true,
  autoTriggerDelayMs: 80,
  sources: [
    createLspCompletionSource({ minPrefixLength: 1 }),
    createPathCompletionSource({
      minPrefixLength: 1,
      triggerCharacters: ["/", "."],
    }),
    createBufferWordSource({ minPrefixLength: 1 }),
  ],
});
```

Load `setupSayaCompletion()` before `setupSayaLspClient()` if the completion
sources call LSP commands. The LSP plugin registers the `lsp.completion` command
that the LSP source calls at trigger time.

The setup uses explicit behavior options:

- `commandName: "completion.trigger"`.
- `key` is unset by default. Set it to register a manual insert-mode keymap.
- `autoTrigger: false`.
- `autoTriggerDelayMs: 80`.
- `keys` defaults to standard menu operation keys. Set it to override menu keys
  for completion menus opened by this setup.
- `minPrefixLength: 1`.
- `maxItems: 50`.
- `sourceTimeoutMs: 1000`.
- `sources: []`.

Source-specific `minPrefixLength` overrides the global `minPrefixLength`. For
example, configure `createBufferWordSource({ minPrefixLength: 1 })` when you
want buffer word completion to open after one typed character. Keeping
`createBufferWordSource({ minPrefixLength: 2 })` is less aggressive for normal
typing.

## Automatic triggering

Automatic triggering subscribes to `bufferChanged` events and debounces source
execution. The debounce runs in the bundled TypeScript plugin; the Rust host
only delivers the event and opens the typed completion menu.

Automatic triggers run only when `saya.editor.current().mode` is `Insert`. The
plugin then calls the same typed menu path used by manual completion:
`saya.completion.show({ ..., selectedIndex: 0 })`.

When an automatic trigger re-evaluates to no valid candidates, or when insert
mode is no longer active, the plugin closes the active completion menu through
`saya.completion.close()`. Completion plugins should use this typed completion
surface instead of inspecting generic floating windows.

Manual completion and automatic completion share the same engine, but they use
different trigger reasons:

```ts
type SayaCompletionTriggerReason =
  | { kind: "manual" }
  | { kind: "auto"; character?: string };
```

Sources receive this reason in `SayaCompletionTriggerContext.reason`. Manual
completion is not gated by `triggerCharacters`; it only needs the source to
return a query that satisfies `minPrefixLength`. Automatic completion also
checks source `triggerCharacters`, so a source can open on a character such as
`/` or `.` even when the prefix is shorter than its minimum prefix length.

## Menu operations

Completion menu operations use standard completion menu bindings by default.
When `keys` is present, missing operation groups use the standard bindings.
Providing an empty array disables that operation for menus opened by this setup.
Key strings use Vim-like names such as `<Tab>`, `<Enter>`, `<C-e>`, `<C-n>`,
`<PageDown>`, or a single printable character such as `j`.

The `close` group means "close the menu and stay in the current editor mode."
Use `<C-e>` for Vim-style completion cancellation. `<Esc>` and `<C-[>` are
handled as modal editor escape keys even when they aren't listed in `close`:
the host closes the completion menu first, then dispatches the key to the
editor so insert mode changes to normal mode. Empty `close` arrays disable
close-only keys, but they don't disable `<Esc>` or `<C-[>`.

```ts
setupSayaCompletion({
  keys: {
    confirm: ["<Tab>", "<C-y>"],
    close: ["<C-e>"],
    next: ["<C-n>", "j"],
    previous: ["<C-p>", "k"],
    pageNext: ["<PageDown>", "<C-f>"],
    pagePrevious: ["<PageUp>", "<C-b>"],
  },
});
```

The bundled plugin passes the normalized key policy with every typed
`saya.completion.show()` request. Manual and automatic triggers use the same
menu key policy; `triggerCharacters` only affects whether automatic source
execution starts.

The Rust completion menu treats missing `request.keys` as the standard operation
keys. Once a `keys` object is present, omitted operation groups use the standard
bindings for that request, and empty arrays disable that operation.

## Sources

A source owns its trigger detection and replacement range. The engine gives each
source the current buffer, editor snapshot, and trigger reason. The source
returns `null` from `trigger()` when the cursor context doesn't belong to that
source, or a query with its own prefix and replacement range when it does. The
source then returns a result with candidates and the replacement range to use
for those candidates. Sources can be synchronous or asynchronous.

```ts
setupSayaCompletion({
  sources: [{
    id: "keywords",
    minPrefixLength: 2,
    triggerCharacters: ["."],
    trigger(context) {
      const line = context.buffer.currentLine ?? "";
      const cursor = context.buffer.cursorCol ?? 0;
      const prefix = line.slice(0, cursor).match(/[A-Za-z_]+$/)?.[0] ?? "";
      return {
        ...context,
        sourceId: "keywords",
        prefix,
        replaceRange: {
          start: {
            line: context.buffer.cursorRow,
            character: cursor - prefix.length,
          },
          end: { line: context.buffer.cursorRow, character: cursor },
        },
      };
    },
    complete(query) {
      const candidates = ["function", "const", "return"]
        .filter((label) => label.startsWith(query.prefix))
        .map((label) => ({ label, insertText: label, kind: "Keyword" }));
      return {
        sourceId: query.sourceId,
        prefix: query.prefix,
        replaceRange: query.replaceRange,
        candidates,
      };
    },
  }],
});
```

The bundled source helpers are:

- `createLspCompletionSource()`: Calls `lsp.completion`, converts LSP completion
  items to typed candidates, and uses the first LSP `textEdit.range` as the
  replacement range when present.
- `createPathCompletionSource()`: Detects path-like prefixes, lists matching
  entries through the host-mediated `saya.fs.readDir()` surface, inserts a
  trailing `/` for directory candidates, and uses the path prefix as the
  replacement range.
- `createBufferWordSource()`: Extracts identifier-like words from the current
  buffer, excludes the current prefix as a standalone candidate, and
  deduplicates labels.

These helpers are ordinary source factories. Add the ones you want to `sources`;
omitted helpers don't run. Helper defaults such as source names, optional LSP
and path failure handling, path `showHidden`, and trigger detection are internal
source policy. They don't activate completion until the helper is explicitly
listed in `sources`. Use `sourceTimeoutMs` to cap how long one source can block
a trigger. A timed out source result contributes no candidates for that request.

```ts
setupSayaCompletion({
  sourceTimeoutMs: 750,
  sources: [createBufferWordSource({ minPrefixLength: 1 })],
});
```

Set `sourceTimeoutMs` to `0` to disable this timeout.

## Ranking

Use `ranking` when you want the built-in sources to keep the startup-safe
runtime path while changing candidate order. The default behavior is unchanged
when `ranking` is omitted.

```ts
setupSayaCompletion({
  ranking: {
    sourcePriority: ["lsp", "path", "buffer"],
    deepCompletionPriority: "last",
    duplicateLabels: "preferFirstSource",
  },
  sources: [
    createLspCompletionSource({ minPrefixLength: 1 }),
    createPathCompletionSource({ minPrefixLength: 1 }),
    createBufferWordSource({ minPrefixLength: 1 }),
  ],
});
```

`sourcePriority` ranks candidates by source before the fallback label sorter.
`deepCompletionPriority: "last"` keeps deep LSP completions, such as
`Default().Println`, but ranks them after direct and fallback candidates.
`deepCompletionPriority: "afterDirect"` keeps deep completions after direct
candidates within the same source priority group. Final deduplication keeps the
first candidate for each label, so `duplicateLabels: "preferFirstSource"` works
with `sourcePriority` to keep the preferred source.

## Filters and sorters

Filters receive one source result and its query, and can remove or rewrite
candidates before the engine groups results by replacement range. Sorters
receive the selected candidate list and can reorder candidates.

```ts
setupSayaCompletion({
  filters: [
    (result, query) => ({
      ...result,
      candidates: result.candidates.filter((candidate) =>
        candidate.label.toLowerCase().startsWith(query.prefix.toLowerCase())
      ),
    }),
  ],
  sorters: [
    (candidates, _result) =>
      [...candidates].sort((left, right) =>
        left.label.localeCompare(right.label)
      ),
  ],
});
```

The built-in defaults are `prefixFilter` and `labelSorter`. `prefixFilter` keeps
prefix matches and removes exact-prefix no-op candidates. `labelSorter` prefers
nearby buffer words, then directory path candidates before files, then shorter
labels, then label order. The plugin deduplicates final candidates by label,
limits the menu to `maxItems`, and then calls the typed runtime API. When
multiple sources return candidates with different replacement ranges, the engine
selects the first non-empty replacement-range group in source order.

## Path completion

Path completion is part of the bundled `saya-completion` plugin. It treats
prefixes such as `./`, `../`, `/`, `src/main`, and quoted string content such as
`"./src/main"` as path-like text. Relative paths resolve from the current
buffer's directory. When the buffer has no path, relative paths resolve from
`.`.

Directory candidates use the same label and `insertText`, and both include a
trailing `/`. The source removes candidates that exactly match the current
prefix, deduplicates labels, applies the source `maxItems` cap when configured,
and keeps filesystem policy in TypeScript instead of the Rust completion menu
layer. Directory listing uses `saya.fs.readDir()` rather than a direct Deno
filesystem API.

## Runtime menu request

The plugin opens the completion menu through `saya.completion.show(request)`.
The request carries a session ID, monotonically increasing request ID, a single
replacement range, and typed candidates.

```ts
await saya.completion.show({
  sessionId: "custom:1",
  requestId: 1,
  replaceRange: {
    start: { line: 0, character: 0 },
    end: { line: 0, character: 4 },
  },
  candidates: [{
    label: "println",
    insertText: "println($0)",
    kind: "Function",
    detail: "macro",
    documentation: ["Prints a line."],
    source: "custom",
  }],
  selectedIndex: 0,
  keys: {
    confirm: ["<Tab>"],
    close: ["<C-e>"],
    next: ["<C-n>"],
    previous: ["<C-p>"],
    pageNext: ["<PageDown>"],
    pagePrevious: ["<PageUp>"],
  },
});
```

The plugin can close the active completion menu through the typed lifecycle
surface:

```ts
await saya.completion.close();
```

The host rejects stale requests for the same session when a newer request ID has
already been accepted. When the user confirms a candidate, the host applies
`insertText` when present, or `label` otherwise, to `replaceRange`.
