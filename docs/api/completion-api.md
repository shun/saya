# Completion API

This page documents the bundled `saya-completion` plugin and the typed runtime
completion surface. The plugin owns source orchestration, filtering, sorting,
and timeout policy. The Rust host owns the typed menu request, session
lifecycle, stale request rejection, rendering, and confirm-time text edits.

> **Note:** This is a preview feature currently under active development.

## Setup

Use `setupSayaCompletion()` from the bundled `saya-completion` plugin in your
startup configuration. The default setup registers `completion.trigger`, maps
`<C-Space>` in insert mode, queries LSP completions when `lsp.completion` is
available, completes path-like text through `saya.filer.list()`, and falls back
to words from the current buffer.

```ts
import { setupSayaCompletion } from "/path/to/plugins/bundled/completion/index.ts";

setupSayaCompletion();
```

Load `setupSayaCompletion()` before `setupSayaLspClient()` if both plugins use
their defaults. The completion plugin binds the insert-mode completion key, and
the LSP plugin registers the `lsp.completion` command that the LSP source calls
at trigger time.

## Sources

A source owns its trigger detection and replacement range. The engine gives each
source the current buffer and editor snapshot. The source returns `null` from
`trigger()` when the cursor context doesn't belong to that source, or a query
with its own prefix and replacement range when it does. The source then returns
a result with candidates and the replacement range to use for those candidates.
Sources can be synchronous or asynchronous.

```ts
setupSayaCompletion({
  sources: [{
    id: "keywords",
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

The default source list is:

- `createLspCompletionSource()`: Calls `lsp.completion`, converts LSP completion
  items to typed candidates, and uses the first LSP `textEdit.range` as the
  replacement range when present.
- `createPathCompletionSource()`: Detects path-like prefixes, lists matching
  entries through the host-mediated `saya.filer.list()` surface, inserts a
  trailing `/` for directory candidates, and uses the path prefix as the
  replacement range.
- `createBufferWordSource()`: Extracts identifier-like words from the current
  buffer, excludes the current prefix as a standalone candidate, and
  deduplicates labels.

Use `sourceTimeoutMs` to cap how long one source can block a trigger. A timed
out source result contributes no candidates for that request.

```ts
setupSayaCompletion({
  sourceTimeoutMs: 750,
});
```

Set `sourceTimeoutMs` to `0` to disable this timeout.

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
layer. Directory listing uses `saya.filer.list()` rather than a direct Deno
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
});
```

The host rejects stale requests for the same session when a newer request ID has
already been accepted. When the user confirms a candidate, the host applies
`insertText` when present, or `label` otherwise, to `replaceRange`.
