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
available, and falls back to words from the current buffer.

```ts
import { setupSayaCompletion } from "/path/to/plugins/bundled/completion/index.ts";

setupSayaCompletion();
```

Load `setupSayaCompletion()` before `setupSayaLspClient()` if both plugins use
their defaults. The completion plugin binds the insert-mode completion key, and
the LSP plugin registers the `lsp.completion` command that the LSP source calls
at trigger time.

## Sources

A source returns completion candidates for the current buffer snapshot, editor
snapshot, word prefix, and replace range. Sources can be synchronous or
asynchronous.

```ts
setupSayaCompletion({
  sources: [{
    name: "keywords",
    complete(context) {
      return ["function", "const", "return"]
        .filter((label) => label.startsWith(context.prefix))
        .map((label) => ({ label, insertText: label, kind: "Keyword" }));
    },
  }],
});
```

The default source list is:

- `createLspCompletionSource()`: Calls `lsp.completion`, converts LSP completion
  items to typed candidates, and uses the first LSP `textEdit.range` as the
  replacement range when present.
- `createBufferWordSource()`: Extracts identifier-like words from the current
  buffer, excludes the current prefix as a standalone candidate, and
  deduplicates labels.

Use `sourceTimeoutMs` to cap how long one source can block a trigger. A timed
out source contributes no candidates for that request.

```ts
setupSayaCompletion({
  sourceTimeoutMs: 750,
});
```

Set `sourceTimeoutMs` to `0` to disable this timeout.

## Filters and sorters

Filters receive the combined candidate list and can remove or rewrite
candidates. Sorters receive the filtered list and can reorder candidates.

```ts
setupSayaCompletion({
  filters: [
    (candidates, context) =>
      candidates.filter((candidate) =>
        candidate.label.toLowerCase().startsWith(context.prefix.toLowerCase())
      ),
  ],
  sorters: [
    (candidates) =>
      [...candidates].sort((left, right) =>
        left.label.localeCompare(right.label)
      ),
  ],
});
```

The built-in defaults are `prefixFilter` and `labelSorter`. `prefixFilter` keeps
prefix matches and removes exact-prefix no-op candidates. `labelSorter` prefers
nearby buffer words, then shorter labels, then label order. The plugin
deduplicates final candidates by label, limits the menu to `maxItems`, and then
calls the typed runtime API.

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
