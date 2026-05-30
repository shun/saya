# Dired API v1

This page fixes the versioned public contract for the local dired preview API.
It covers the TypeScript startup helper, the runtime `saya.filer` surface,
writable directory buffer confirmation, and compatibility rules for plugin
authors.

> **Note:** Dired v1 is a preview feature currently under active development.
> The API is guarded as a v1 preview contract, but the preview label remains
> until the backend adapter work defines the long-term local and remote backend
> boundary.

## Version policy

The `v1` contract is the compatibility baseline for local filesystem dired
plugins. Compatible changes can add optional fields, optional options, new
non-default commands, or new operation kinds that old plugins can ignore.

Breaking changes require a migration note in this page and a matching update to
the declaration guard tests. Breaking changes include:

- Renaming or removing any `saya.filer.*` method.
- Renaming or removing `setupSayaDired(options)`.
- Changing a required field in `SayaFilerOperationReport`.
- Changing destructive-operation confirmation requirements.
- Changing writable directory buffer preview or apply command names.

## Startup setup contract

Use `setupSayaDired(options)` from `plugins/bundled/dired/index.ts` to register the local
dired commands at startup. It doesn't register normal-mode keymaps unless you
provide `keymap` or a legacy flat key option.

```ts
import { setupSayaDired } from "./plugins/bundled/dired/index.ts";

setupSayaDired({
  root: ".",
  hiddenFilePolicy: "hide",
  sortPolicy: "kind",
  filter: "rs",
  confirmStrategy: "preview",
  commands: {
    enter: "workspace.enter",
    refresh: "workspace.refresh",
    bulkDeletePreview: "workspace.previewDelete",
  },
  keymap: {
    enter: "<Enter>",
    refresh: "gr",
    bulkDeletePreview: "D",
  },
});
```

The public setup options are:

- `commands`, with optional names for `open`, `enter`, `up`, `refresh`, `mark`,
  `unmark`, `clearMarks`, and `bulkDeletePreview`.
- `keymap`, with optional bindings for `up`, `enter`, `refresh`, `mark`,
  `unmark`, `clearMarks`, and `bulkDeletePreview`.
- `root`, as the startup root path used by the open and refresh commands.
- `hiddenFilePolicy`, as `"show"` or `"hide"`.
- `sortPolicy`, as `"name"`, `"kind"`, `"modifiedTime"`, or `"size"`. The
  `"kind"` value groups directories before other entries.
- `filter`, as the case-insensitive listing filter.
- `confirmStrategy`, as `"preview"` or `"disabled"` for the bulk-delete preview
  command.

`setupSayaDired()` always registers the dired command callbacks. Commands are
named entry points, and they don't affect editor behavior until the user maps a
key, runs a command, or a lazy plugin trigger calls them. The default command
names are `dired.open`, `dired.enter`, `dired.up`, `dired.refresh`,
`dired.mark`, `dired.unmark`, `dired.clearMarks`, and `dired.bulkDeletePreview`.

When `keymap` is present, omitted key entries use the standard dired bindings:
`-`, `<Enter>`, `gr`, `m`, `M`, `gM`, and `D`. When `keymap` is omitted, no
normal-mode mappings are registered.

`hiddenFilePolicy`, `sortPolicy`, and `confirmStrategy` keep behavior defaults
because they only affect commands that the user explicitly invokes. The defaults
are `show`, `kind`, and `preview`.

Legacy flat option names such as `commandName`, `enterCommandName`, `key`,
`enterKey`, and `bulkDeletePreviewKey` remain accepted for compatibility. New
plugins must prefer the grouped `commands` and `keymap` options.

## Runtime filer contract

The runtime API exposes a narrow local filesystem surface under `saya.filer`. It
does not expose broad filesystem, network, backend registry, or Neovim
compatibility APIs.

The v1 preview runtime methods are:

- `saya.filer.list(path, options)`
- `saya.filer.currentEntry()`
- `saya.filer.createFile(path)`
- `saya.filer.createDirectory(path)`
- `saya.filer.copy(from, to)`
- `saya.filer.move(from, to)`
- `saya.filer.rename(from, to)`
- `saya.filer.delete(path, options)`
- `saya.filer.mark(path)`
- `saya.filer.unmark(path)`
- `saya.filer.clearMarks()`
- `saya.filer.bulkDeletePreview()`
- `saya.filer.bulkDelete(options)`

`SayaFilerDeleteOptions` contains `confirm`, `recursive`, and `trash`.
`confirm: true` is required for delete. Recursive delete and trash requests are
rejected in v1 preview instead of being silently downgraded.

`SayaFilerBulkDeleteOptions` contains `confirm` and `previewId`.
`bulkDelete(options)` requires `{ confirm: true, previewId }` from the latest
`bulkDeletePreview()` report.

Each successful mutation returns `SayaFilerOperationReport`:

```ts
type SayaFilerOperationKind =
  | "createFile"
  | "createDirectory"
  | "copy"
  | "move"
  | "rename"
  | "delete"
  | "mark"
  | "unmark"
  | "clearMarks"
  | "bulkDeletePreview"
  | "bulkDelete";

interface SayaFilerOperationReport {
  operation: SayaFilerOperationKind;
  path: string;
  targetPath?: string | null;
  entries: SayaCurrentFilerEntry[];
  previewId?: string | null;
}
```

## Operation examples

Use `currentEntry()` when a command needs the entry under the cursor. Do not
parse the rendered directory listing text.

```ts
saya.commands.register("workspace.renameCurrent", async () => {
  const entry = await saya.filer.currentEntry();
  if (!entry) return;

  await saya.filer.rename(entry.path, `${entry.rootPath}/renamed.md`);
  await saya.commands.execute(`edit ${entry.rootPath}`);
});
```

Use the preview report when deleting multiple marked entries.

```ts
saya.commands.register("workspace.deleteMarked", async () => {
  const preview = await saya.filer.bulkDeletePreview();
  await saya.filer.bulkDelete({
    confirm: true,
    previewId: preview.previewId ?? "",
  });
});
```

## Writable directory buffers

Writable directory buffers convert listing edits into a host-side operation
preview. Plain `:write` prepares the preview and opens a confirmation prompt.
Press `y` or Enter to apply only the latest matching preview. Press `n` or Esc
to cancel without changing the filesystem. `:write!` remains a legacy explicit
confirmation path for the latest matching preview.

The guarded preview and prompt types are:

```ts
interface SayaDirectoryBufferOperationPreview {
  id: string;
  rootPath: string;
  operationCount: number;
  highRiskCount: number;
  operations: SayaDirectoryBufferPreviewOperation[];
}

interface SayaDirectoryBufferOperationPrompt {
  previewId: string;
  statusLine: string;
  detailLines: string[];
  confirmCommand: "OK";
  cancelCommand: "Cancel";
  recoveryHint: string;
}
```

The guarded apply report type is:

```ts
interface SayaDirectoryBufferApplyReport {
  rootPath: string;
  operationCount: number;
  successfulSteps: number;
  failedSteps: number;
  rollbackSucceeded: number;
  rollbackFailed: number;
  manualRecoveryRequired: boolean;
}
```

The preview shape is part of the public policy even though the current runtime
does not expose a direct TypeScript method to fetch it. Host messages and docs
must keep using the same command names, preview ID semantics, and high-risk
counting rules. Apply reports must keep enough structured counts to distinguish
successful steps, failed steps, rollback results, and manual recovery
requirements.

## Anti-patterns

Avoid these plugin patterns:

- Parsing rendered dired lines instead of using `saya.filer.currentEntry()`.
- Calling delete without `{ confirm: true }`.
- Calling `bulkDelete()` without a fresh `previewId` from `bulkDeletePreview()`.
- Treating `:write!` as a generic force-save bypass for directory buffers.
- Sending normal editing keys while a directory operation confirmation prompt is
  active instead of answering OK or Cancel.
- Expecting recursive delete, trash, remote filesystems, archive browsing, or
  broad filesystem access from the v1 preview API.
- Adding Neovim dired compatibility shims or Vim script setup requirements.

## Migration notes

The current migration baseline is from the earlier unversioned preview API to
`Dired API v1`.

- Prefer grouped `commands` and `keymap` options over legacy flat option names.
- Treat `saya.filer.list(path, options)` defaults as `showHidden: true` and
  `sortBy: "kind"`, with directories grouped before other entries.
- Expect operation reports to use `operation`, `path`, `targetPath`, `entries`,
  and `previewId`.
- Keep destructive operations behind explicit confirmation and preview flows.
- Keep local dired plugin code on the narrow `saya.filer` surface.

## Stabilization exit criteria

The preview note can be removed only after these conditions are true:

- The backend adapter design fixes the local backend boundary without exposing
  broad filesystem access to TypeScript.
- The v1 preview contract has a release cycle without breaking changes.
- Public API declaration guards cover the runtime filer methods, destructive
  operation options, operation reports, writable directory preview, and prompt
  types.
- Plugin author docs include setup, operation, writable buffer, migration, and
  anti-pattern examples.
- Release notes state whether the preview label is removed or why it remains.
