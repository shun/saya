# Plugin manager API

`saya` provides a TypeScript-first plugin manager foundation. Rust keeps a thin
host boundary for cache files, startup plan loading, lazy trigger bridging, and
failure reporting. The official manager policy lives in
`plugins/manager/index.ts`. The legacy `plugins/saya-plugin-manager.ts` path
re-exports the manager for existing configuration files.

> **Note:** This is a preview feature currently under active development.

## Responsibility split

Rust owns the host primitives that must run before or during the editor session.
The TypeScript manager owns plugin declarations, protocol metadata, dependency
ordering, cache invalidation policy, and operator-facing workflow.

Rust host responsibilities:

- Read and write plugin cache files below `~/.cache/saya`.
- Load `startup-plan.json` and merge it into `StartupRegistry`.
- Load `lazy-index.json` and register command or event placeholders.
- Expose the restricted `saya.plugins.loadLazy()` runtime bridge.
- Convert plugin host failures into warnings or runtime failures.

TypeScript manager responsibilities:

- Collect user-facing plugin declarations from `saya.plugins.use()` and
  `saya.plugins.lazy()`.
- Normalize plugin specs for artifact generation.
- Resolve source declarations and protocol metadata.
- Generate `plugin-lock.json`, `startup-plan.json`, and `lazy-index.json`.
- Implement `sync`, `update`, `list`, `clean`, and `doctor` behavior.
- Own dependency metadata for `depends`, `before`, and `after`.
- Keep bundled plugins out of `plugin-lock.json` install targets.

## Cache layout

The default cache root is `~/.cache/saya`. Tests and automation can set
`SAYA_CACHE_DIR` to use another root.

```text
~/.cache/saya/
  plugins/
    plugin-lock.json
    startup-plan.json
    lazy-index.json
    operations.log
    repos/
      <external-plugin>/
```

The host logs cache hit, cache miss, invalidation, lazy placeholder, lazy
trigger, and operation decisions with `saya-plugin-host` or
`saya-plugin-manager` prefixes.

## Startup cache behavior

Cache files are disposable. A missing cache must not make bundled plugins
unavailable, and it must not force external plugin installation during startup.

The startup path has three runtime modes:

- Cache hit: Rust reads `startup-plan.json` and `lazy-index.json`, merges them
  into the startup registry, and skips TypeScript manager work.
- Bundled manifest fallback: Rust reads bundled plugin manifests and registers
  bundled lazy placeholders when generated cache files are missing or stale.
- External disabled: Rust skips external plugin entries when the lockfile,
  checkout, or generated cache data is missing, then reports that sync is
  required.

External plugin recovery belongs to `sy plugin sync` or a future explicit
runtime command. Startup must not run `git clone`, `git fetch`, dependency
resolution, or the full TypeScript manager to repair external plugin state.

This gives `saya` a dpp-style hot path while keeping vim-jetpack-style
cache-miss UX: the editor starts, bundled plugins remain available, and the user
gets a clear sync action for external plugins.

## Plugin classes

The manager supports two plugin classes. The class controls delivery and support
policy, not runtime authority.

- Bundled plugins ship with `saya`, are versioned with the editor, and are not
  installed through the plugin manager. They are still TypeScript plugins and
  must use the public `saya.*` capability surface.
- External plugins are installed through the plugin manager. They appear in the
  lockfile, use dependency metadata, and can be updated or cleaned by user
  action.

Use bundled plugins for baseline editor features such as local dired or the
official LSP client bridge. Use external plugins for optional language, theme,
formatter, navigation, and integration features. See
[Plugin model](../design/plugin-model.md) for the classification rules.

## User-facing declarations

Declare plugins from `init.ts` with `saya.plugins.use()` and
`saya.plugins.lazy()`. These methods describe desired plugin state. They don't
clone repositories or touch the network during editor startup.

Use `saya.plugins.use()` for plugins that must be active during startup. Use
`saya.plugins.lazy()` for plugins that load only after a command or event
trigger.

```ts
saya.plugins.use([
  { github: "shun/saya-theme-tokyo-night" },
  { local: "~/.config/saya/plugins/workspace-tools" },
]);

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

The declaration surface supports these source forms:

- `local`, which points to a local plugin directory and never clones.
- `github`, which uses the `owner/repository` shorthand and clones into the
  plugin cache during `sy plugin sync` or `sy plugin update`.
- `rev`, which optionally pins a GitHub plugin to a branch, tag, or commit.

When `rev` is absent, `sy plugin sync` and `sy plugin update` resolve the latest
revision from the repository's default branch. Normal editor startup uses
generated cache files and doesn't check the network.

The default plugin entry point is `mod.ts`, and the default setup export is
`setup`. You only need to specify alternate entry metadata for non-standard
plugin layouts.

These declaration defaults are metadata defaults, not bundled feature activation
defaults. A plugin still has to be declared by the user, loaded by a startup
plan, or reached through a lazy trigger before its setup code runs. For bundled
plugins, manifests can keep explicit `module` and `setup` metadata so startup
fallback can create lazy placeholders without guessing implementation paths.

## TypeScript manager

The TypeScript manager normalizes user-facing declarations into the internal
plugin spec used for artifact generation. The lower-level manager accepts local
paths, GitHub shorthand metadata, and manager-owned protocol strings.

```ts
import { definePlugins, syncPlugins } from "./plugins/manager/index.ts";

const plugins = definePlugins([
  {
    name: "workspace-tools",
    source: { kind: "local", path: "./plugins/workspace-tools.ts" },
    lazy: {
      commands: ["WorkspaceRefresh"],
      events: ["bufferOpen"],
    },
  },
]);

await syncPlugins(plugins, {
  cacheRoot: Deno.env.get("SAYA_CACHE_DIR") ??
    `${Deno.env.get("HOME")}/.cache/saya`,
  sourceHash: "manager-input-hash",
  bundled: [
    {
      version: 1,
      name: "dired",
      module: "plugins/bundled/dired/index.ts",
      setup: "setupSayaDired",
      lazy: { commands: ["dired.open"] },
    },
  ],
  writeTextFile: Deno.writeTextFile,
});
```

Bundled manifests can contribute startup and lazy artifacts, but they are not
written to `plugin-lock.json`. Lazy placeholders make bundled entry points
available after an explicit command or event trigger; they don't call setup
functions or install bundled keymaps, sources, or event handlers by themselves.
The lockfile tracks external plugins only. Complete installs distribute bundled
manifests and plugin sources under `SAYA_HOME/runtime/plugins/bundled`; see
[Install layout design](../design/install-layout.md).

## CLI commands

The Rust CLI accepts these host entrypoints:

- `sy plugin sync`
- `sy plugin update`
- `sy plugin list`
- `sy plugin clean`
- `sy plugin doctor`

The CLI entrypoints inspect or clean cache artifacts and append
`operations.log`. The TypeScript manager remains the source of truth for
artifact generation and policy.
