# Selector design

This page defines the `saya` selector design. The initial preview API is
implemented for host-managed static and `rg`-backed selection workflows, and
the later sections keep future expansion notes for files, buffer lines, LSP
locations, diagnostics, sessions, and additional item sources.

The design borrows the useful split points from `ddu.vim` and `fall.vim`
without importing Vim or Neovim compatibility layers. `saya` owns the visible
UI, focus routing, storage policy, and typed host actions. TypeScript plugins
own item collection, matching policy, rendering, preview content, and item
actions through narrow interfaces.

> **Note:** This is a preview design currently under active development.

## Design position

The selector must not be a grep-only feature. A grep-only implementation would
duplicate the same input, list, preview, focus, resume, and action handling for
file search, buffer-line search, LSP references, diagnostics, and command
selection.

The selector also must not become a broad UI framework. `ddu.vim` is valuable as
a reference because it separates source, filter, kind, and UI concerns, but
`saya` does not need a compatibility-shaped UI framework where plugins can
drive arbitrary editor windows. `fall.vim` is valuable as a reference because
it models a selector as a pipeline of collect, match, sort, render, preview, and
action processors.

The chosen position is:

```text
Typed, host-managed, extensible selector.
```

This means:

- The selector is generic enough to serve grep, file search, buffer-line search,
  LSP references, diagnostics, command selection, and session resume.
- The host owns UI surfaces, input routing, focus, float and split placement,
  cancellation, storage, and typed editor navigation.
- TypeScript plugins define sources, matchers, sorters, renderers, previewers,
  actions, and kind defaults.
- Plugins do not receive raw renderer access, terminal drawing access, or
  Neovim-compatible window primitives.

## Differentiation

`saya` should compete on integration quality and trust in search completeness,
not on maximum UI freedom.

The selector differs from `ddu.vim` and `fall.vim` in these ways:

- It avoids Vim and Neovim compatibility contracts.
- It uses the `saya` TypeScript runtime and host capability bridge instead of
  `denops`.
- It keeps UI rendering and focus routing in the Rust host.
- It supports floating, split, vertical split, and future tab-like views as
  view backends for the same selector session.
- It can adapt result storage from memory to a temporary file so grep results
  remain fully searchable without unbounded memory growth.
- It reports collection and matching status explicitly, including total seen
  results, matched results, shown results, completion, cancellation, failure,
  and storage mode.

## Layer ownership

The selector crosses the TypeScript runtime and application layers, so ownership
must stay explicit.

```text
TypeScript plugin layer
  -> source / matcher / sorter / renderer / previewer / action definitions
  -> runtime requests through the typed `saya` surface

Application layer
  -> SelectorSession
  -> SelectorController
  -> ResultStore
  -> SelectorViewBackend
  -> FloatingWindowManager and split integration
  -> typed editor navigation and host actions

Editing core
  -> buffer mutation, cursor movement, file editing, and window semantics
     through `vim-core-rs`
```

The selector must not reimplement editing semantics. Opening a file, jumping to a
line, editing a buffer-backed split, and routing normal editing keys must go
through `vim-core-rs` via existing application-layer adapters.

## Pipeline

The selector session processes items through a stable pipeline. Each stage can be
implemented independently and tested without a TUI.

```text
collect
  -> store
  -> match
  -> sort
  -> render
  -> preview selected item
  -> invoke action
```

The stages have these responsibilities:

- `CollectProcessor` starts a source and appends every collected item to the
  result store.
- `ResultStore` owns collected item storage and scan behavior.
- `MatchProcessor` scans the result store and produces matched items for the
  active query.
- `SortProcessor` orders matched items when the matcher or selector requests an
  ordering policy.
- `RenderProcessor` converts matched items into display rows and decorations.
- `PreviewProcessor` produces preview content for the selected item with
  debounce and cancellation.
- `ActionProcessor` resolves the default or requested action from the item
  kind and invokes it.

The pipeline must support cancellation. When the user closes a selector, reloads
the source, or changes input while a long match is running, stale work must
stop or become unable to update the visible session.

## Item model

The item model separates searchable value, source-specific detail, display
shape, and action behavior.

```ts
type SelectorItem<TDetail = unknown> = {
  id: string;
  value: string;
  kind: string;
  detail: TDetail;
};

type MatchedItem<TDetail = unknown> = {
  item: SelectorItem<TDetail>;
  score?: number;
  highlights?: SelectorHighlight[];
};

type SelectorHighlight = {
  column: number;
  width: number;
  kind: "match" | "selection" | "diagnostic";
};

type RenderedItem = {
  id: string;
  label: string;
  highlights?: SelectorHighlight[];
};
```

`value` is the primary matcher input. `detail` is the typed payload used by
previewers and actions. `kind` selects default actions and common behavior.

Example `rg` result detail:

```ts
type RgLocationDetail = {
  path: string;
  line: number;
  column: number;
  text: string;
};
```

Example file result detail:

```ts
type FileDetail = {
  path: string;
  entryKind: "file" | "directory" | "symlink" | "other";
};
```

Example LSP location detail:

```ts
type LocationDetail = {
  uri: string;
  path: string;
  line: number;
  column: number;
};
```

## Kinds and actions

Kinds keep sources from owning action policy. A source only describes what an
item is. The action registry decides what can happen to that item.

Recommended initial kinds:

- `file` for filesystem entries.
- `location` for file, line, and column targets.
- `line` for current-buffer lines, implemented as a narrow location variant if
  that keeps the first implementation simpler.
- `command` for command selection.
- `session` for selector session resume.
- `diagnostic` for diagnostic locations with severity metadata.

Recommended initial actions:

- `open` for `file`.
- `jump` for `location`, `line`, and `diagnostic`.
- `execute` for `command`.
- `resume` for `session`.
- `reload` for `session`.
- `yank` as a non-default action after the action selector exists.

The initial UI can invoke only the default action. An action selector can be
added later without changing the item and kind model.

## Matching model

The first matchers must serve exact search refinement rather than fuzzy search.
Fuzzy matching remains a future matcher, not a different selector.

Initial matchers:

- `prefixAnd`
- `substringAnd`
- `suffixAnd`

Each matcher splits input on ASCII spaces and applies AND semantics. Empty
terms are ignored.

```text
input: "query parser"
meaning: item matches "query" AND item matches "parser"
```

The matcher API must include optional scores and highlights from the beginning.
Prefix, substring, and suffix matchers can return simple scores or omit scores.
A later fuzzy matcher can return ranking scores and highlight ranges through
the same shape.

## Source model

Sources produce items and must not own the selector UI. A source can stream items
so the selector can render partial results while collection continues.

```ts
type SelectorSource<TDetail = unknown> = {
  name: string;
  collect(args: {
    params: unknown;
    signal: AbortSignal;
  }): AsyncIterable<SelectorItem<TDetail>>;
};
```

Recommended initial sources:

- `rg` for workspace grep.
- `file` for file name search.
- `line` for current-buffer line search.

The `rg` source should read ripgrep output to completion unless the user
cancels. `rg --json` is preferred over `--vimgrep` when implementation time
allows it because JSON avoids ambiguous parsing around paths and separators.

The `file` source can use `rg --files` first. A future host file-listing API can
replace that implementation without changing selector behavior.

The `line` source needs a runtime API that can read current buffer lines. The
current `saya.buffer.current()` snapshot is not enough because it exposes only
the current line and metadata.

## Result storage

The result store is the most important difference between a trustworthy grep
selector and a simple UI list. Displaying only 1,000 rows must not mean matching
only the first 1,000 collected results.

The selector must collect and store every result that remains in the active
session, then render only a bounded subset.

```text
collect all results
store all results
match against the full store
render the first visible page or max rendered items
```

The initial implementation can use memory for normal cases. It must reserve
the design path for automatic migration to a temporary file when memory usage
crosses a configured threshold.

```ts
interface SelectorResultStore<TDetail = unknown> {
  append(item: SelectorItem<TDetail>): Promise<void>;
  scan(args: {
    signal: AbortSignal;
  }): AsyncIterable<SelectorItem<TDetail>>;
  get(id: string): Promise<SelectorItem<TDetail> | undefined>;
  count(): number;
  status(): SelectorResultStoreStatus;
  dispose(): Promise<void>;
}

type SelectorResultStoreStatus = {
  storage: "memory" | "tempFile";
  totalStored: number;
  estimatedBytes?: number;
  tempFilePath?: string;
};
```

The memory store keeps an item array. The temporary-file store writes JSONL.

```json
{
  "id": "src-main-rs-42",
  "value": "src/main.rs parser",
  "kind": "location",
  "detail": {
    "path": "src/main.rs",
    "line": 42,
    "column": 10,
    "text": "parser"
  }
}
```

JSONL is intentionally simple. It supports append, line-by-line scan, and
session cleanup without adding a database dependency.

### Memory-to-file migration

The store starts in memory and migrates when the configured memory threshold is
exceeded.

1. Create a session-owned temporary file.
2. Write existing memory items to the file as JSONL.
3. Append future items to the file.
4. Drop the memory item array.
5. Keep lightweight metadata such as count, storage mode, and optional ID
   offsets only when needed.

The selector must keep matching against the complete store after migration. If
the store is file-backed, matching scans JSONL from the beginning and streams
matched rows back to the session.

## Limits

Limits must describe different costs. A single `max` option is ambiguous and
can cause users to mistake a display limit for a search limit.

Recommended options:

```ts
type SelectorLimits = {
  maxRenderedItems: number;
  maxHighlightItems: number;
  maxSessions: number;
  maxSessionItems?: number | null;
  memoryStoreThresholdBytes: number;
  maxCollectItems?: number | null;
  previewMaxBytes: number;
  collectChunkSize: number;
  collectChunkIntervalMs: number;
  matchChunkSize: number;
  matchChunkIntervalMs: number;
  previewDebounceMs: number;
};
```

Recommended defaults:

```ts
const defaultSelectorLimits: SelectorLimits = {
  maxRenderedItems: 1000,
  maxHighlightItems: 100,
  maxSessions: 50,
  maxSessionItems: null,
  memoryStoreThresholdBytes: 64 * 1024 * 1024,
  maxCollectItems: null,
  previewMaxBytes: 1_000_000,
  collectChunkSize: 1000,
  collectChunkIntervalMs: 50,
  matchChunkSize: 1000,
  matchChunkIntervalMs: 50,
  previewDebounceMs: 120,
};
```

`maxRenderedItems` limits what the UI receives. It must not limit collection
or matching. `maxCollectItems` is a safety valve only. The default should keep
collection unlimited and rely on cancellation, store migration, and explicit
status reporting.

## Status reporting

The UI must make search completeness visible. Users need to know whether a
selector searched everything, is still collecting, was cancelled, failed, or is
using a file-backed store.

```ts
type SelectorCollectStatus = {
  state: "idle" | "running" | "completed" | "cancelled" | "failed";
  totalSeen: number;
  totalStored: number;
  storage: "memory" | "tempFile";
  errorMessage?: string;
};

type SelectorMatchStatus = {
  state: "idle" | "running" | "completed" | "cancelled" | "failed";
  totalMatched: number;
  totalRendered: number;
  errorMessage?: string;
};
```

Example status text:

```text
running    1,000 shown / 12,340 matched so far / 180,000 seen
completed  1,000 shown / 42,318 matched / 180,000 total
file store 1,000 shown / 42,318 matched / 180,000 total
cancelled  1,000 shown / 8,112 matched / 34,700 seen
```

The status must distinguish `totalSeen` from `totalStored`. If a safety valve
ever truncates storage, the UI must say that the result store is limited and
matching no longer covers every seen result.

## View backends

The selector session and the visible selector view must be separate. Closing a
view should not necessarily destroy the session.

```ts
type SelectorViewKind = "floating" | "split" | "vsplit" | "tab";

type SelectorPreviewViewKind = "floating" | "split" | "vsplit" | "inline" | "none";

type SelectorViewOptions = {
  kind: SelectorViewKind;
  width?: number;
  height?: number;
  position?: "top" | "bottom" | "left" | "right" | "center";
  preview?: {
    kind: SelectorPreviewViewKind;
    width?: number;
    height?: number;
  };
};
```

The first backend can use floating surfaces:

```text
input float
list float
preview float
```

Split and vertical split backends should use the same session and pipeline.
They differ only in how the host presents the input, list, status, and preview
surfaces.

The view backend owns:

- Surface creation and disposal.
- Focus policy.
- Geometry updates.
- Rendering `RenderedItem` rows and status text.
- Routing selector keys to controller commands.

The session owns:

- Source parameters.
- Query.
- Result store.
- Match results.
- Cursor and offset.
- Selected items.
- Active matcher, sorter, renderer, previewer, and action names.

## Key binding model

Selector key bindings should be as configurable as `ddu.vim` mappings without
making plugins own raw input routing. The host should translate normalized
selector-local keys into selector controller commands.

Selector bindings are scoped to selector focus. They must not override global
normal-mode mappings while the selector is closed. They also must not require
Vim script, `<Plug>` mappings, or autocommands.

```ts
type SelectorCommand =
  | "selector.cancel"
  | "selector.hide"
  | "selector.accept"
  | "selector.reload"
  | "selector.resumeLast"
  | "selector.cursorNext"
  | "selector.cursorPrevious"
  | "selector.cursorFirst"
  | "selector.cursorLast"
  | "selector.pageDown"
  | "selector.pageUp"
  | "selector.previewScrollDown"
  | "selector.previewScrollUp"
  | "selector.inputDeleteBackward"
  | "selector.inputDeleteForward"
  | "selector.inputMoveLeft"
  | "selector.inputMoveRight"
  | "selector.inputMoveStart"
  | "selector.inputMoveEnd"
  | "selector.action";

type SelectorKeyBinding = {
  key: string;
  command: SelectorCommand;
  when?: "input" | "list" | "preview" | "any";
};
```

The public setup surface should allow full replacement and patch-style
configuration:

```ts
saya.selector.configure({
  keymap: {
    preset: "default",
    bindings: [
      { key: "<C-n>", command: "selector.cursorNext" },
      { key: "<C-p>", command: "selector.cursorPrevious" },
      { key: "<Enter>", command: "selector.accept" },
      { key: "<Esc>", command: "selector.hide" },
    ],
  },
});
```

The default policy is a product decision, but the implementation should support
all three modes:

- `preset: "default"` installs a small ergonomic set of bindings.
- `preset: "none"` installs no selector-local bindings.
- `preset: "custom"` installs only the provided bindings.

We recommend shipping a default preset. A selector that opens with no usable
bindings is hard to discover, hard to test manually, and unfriendly for first
use. The default preset should stay small and conventional:

- `<Enter>` accepts the selected item.
- `<Esc>` hides the selector and preserves the session.
- `<C-c>` cancels the selector and disposes active work.
- `<C-n>` and `<Down>` move to the next item.
- `<C-p>` and `<Up>` move to the previous item.
- `<PageDown>` and `<PageUp>` scroll the list.
- `<C-r>` reloads the source.

Users who want a framework-like setup can set `preset: "none"` and define every
binding explicitly. This keeps the ddu-style customization path without making
the default experience empty.

## Session resume and reload

Session resume is a first-class feature. It must work for floating and split
views because resume belongs to the session, not to the view backend.

Closing a selector view should:

- Close input, list, status, and preview surfaces.
- Cancel active collection and matching when the close action means abandon.
- Preserve the session and result store when the close action means hide.
- Keep temporary files until the session is evicted or disposed.

Resume and reload have different meanings:

- `resume` reopens the latest saved session with the existing result store,
  query, cursor, matcher, renderer, previewer, and view state.
- `reload` discards the old result store and reruns the source with the same
  source parameters.

Session eviction must remove temporary files. The session manager should keep a
bounded number of sessions, with `maxSessions` defaulting to 50.

## Public API sketch

The exact public API can evolve, but it should keep registration, opening, and
runtime control distinct.

Startup registration:

```ts
saya.selector.defineSource("rg", rgSource);
saya.selector.defineMatcher("substringAnd", substringAndMatcher);
saya.selector.defineRenderer("location", locationRenderer);
saya.selector.definePreviewer("file", filePreviewer);
saya.selector.defineAction("jump", jumpAction);
saya.selector.defineKind("location", {
  defaultAction: "jump",
  actions: ["jump", "yank"],
  previewer: "file",
  renderer: "location",
});
saya.selector.configure({
  keymap: {
    preset: "default",
  },
});
```

Runtime opening:

```ts
await saya.selector.open({
  name: "grep",
  source: {
    name: "rg",
    params: {
      root: ".",
      pattern: "parser",
    },
  },
  matcher: "substringAnd",
  view: {
    kind: "floating",
    preview: {
      kind: "floating",
    },
  },
});
```

Future runtime resume sketch:

```ts
await saya.selector.resumeLast();
await saya.selector.reloadLast();
```

The implemented preview runtime capabilities are:

- `saya.selector.open(...)`
- `saya.selector.update(...)`
- `saya.selector.current(...)`
- `saya.selector.control(...)`
- `saya.selector.cancel(...)`
- `saya.selector.dispose(...)`

The future sketch may add:

- `saya.selector.resumeLast(...)`
- `saya.selector.reloadLast(...)`
- `saya.editor.jump(...)`
- A buffer line-reading API such as `saya.buffer.lines(...)`

Those capabilities must remain typed and host-mediated. They must not expose
raw renderer access or compatibility string APIs.

## Initial implementation phases

The implementation can be incremental without changing the final architecture.

### Phase 1: In-memory selector

Phase 1 proves the generic selector model with a bounded visible UI.

- Add selector item, matched item, rendered item, status, and session data
  models.
- Add an in-memory `ResultStore`.
- Add `CollectProcessor`, `MatchProcessor`, `RenderProcessor`, and
  `PreviewProcessor`.
- Add `substringAnd`, `prefixAnd`, and `suffixAnd` matchers.
- Add `rg`, `file`, and current-buffer `line` sources when the required host
  APIs exist.
- Add `location` and `file` renderers.
- Add `file` previewer with `previewMaxBytes`.
- Add `jump` and `open` actions.
- Add selector-local key binding resolution with the default, none, and custom
  presets.
- Add a floating view backend.
- Report total seen, matched, rendered, and completion state.

### Phase 2: Adaptive result store and split views

Phase 2 handles large result sets and alternate presentation.

- Add memory-to-temporary-file migration.
- Scan JSONL stores for matching and counting.
- Keep temporary files across hidden sessions.
- Remove temporary files when sessions are evicted.
- Add split and vertical split view backends.
- Add `resumeLast` and `reloadLast`.
- Add session selector support when the `session` kind exists.

### Deferred features

These features are intentionally deferred:

- Fuzzy matcher.
- Action selector.
- Multi-select actions.
- Quickfix-style export.
- Renderer, matcher, sorter, and previewer switching UI.
- Persistent database-backed indexes.

The design must leave room for these features, but the first implementation
does not need them.

## Testing strategy

Tests should follow the repository's existing architecture split.

Unit tests must cover pure and deterministic behavior:

- ASCII-space AND query parsing.
- Prefix, substring, and suffix matcher behavior.
- Highlight ranges.
- Renderer output.
- Result store append, scan, and count.
- Memory-to-file migration.
- Session eviction and temporary-file cleanup.

Headless integration tests must cover application boundaries:

- Opening a selector from a runtime command.
- Collecting `rg` results through `saya.process.spawn`.
- Rendering only `maxRenderedItems` while matching the full store.
- Showing a result beyond the first rendered page after query refinement.
- Preview debounce and stale preview cancellation.
- Selector-local key binding customization and `preset: "none"`.
- Jump action through typed editor navigation.
- Closing and resuming a session without recollecting.
- Reloading a session by rerunning its source.

PTY or renderer-level tests should cover visible behavior when the TUI path is
available:

- Floating selector layout.
- Split selector layout.
- Preview layout.
- Status text for running, completed, cancelled, failed, and file-backed
  stores.
- No overlapping input, list, status, and preview surfaces.

## Implementation guardrails

The selector must preserve the repository's architectural boundaries.

- Do not add Vim script setup paths.
- Do not add Neovim-compatible window APIs.
- Do not let TypeScript plugins draw directly into the terminal.
- Do not implement selector key bindings through global Vim-style mappings.
- Do not let source implementations own UI state.
- Do not truncate collection silently.
- Do not treat `maxRenderedItems` as a collection or matching limit.
- Do not add a database-backed index for the selector.
- Do not route editing or cursor movement outside `vim-core-rs`.

The first useful selector can be small, but it must keep these boundaries so
grep, file search, buffer-line search, references, diagnostics, and sessions
can share the same workflow instead of growing separate implementations.
