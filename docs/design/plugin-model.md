# Plugin model

`saya` uses two plugin classes so core editor distribution and user-installed
extensions can evolve without moving feature policy into Rust.

> **Note:** This is a preview feature currently under active development.

## Plugin classes

The plugin class describes how a plugin is delivered and supported. It does
not grant extra runtime authority by itself.

### Bundled plugins

Bundled plugins ship with `saya` and are versioned with the editor. They are
implemented in TypeScript, but they are not installed through the plugin
manager.

Use a bundled plugin when all of these conditions are true:

- The editor feels incomplete without the feature.
- The feature policy belongs above `vim-core-rs`.
- The implementation can use the public `saya.*` startup and runtime APIs.
- The feature must work without a network fetch or first-run installation.
- The plugin can still be represented in startup plans and lazy indexes.

Examples include local dired setup, the official LSP client bridge, and future
official workflow plugins that are part of the baseline editor experience.

### External plugins

External plugins are installed, updated, listed, and cleaned through the
TypeScript plugin manager. They extend the editor, but the editor remains
usable when they are absent.

Use an external plugin when any of these conditions are true:

- The feature is optional for normal editing.
- The feature depends on third-party release cadence or repository ownership.
- The plugin adds language, theme, navigation, formatter, integration, or
  workflow behavior outside the baseline editor experience.
- The plugin needs lockfile tracking, dependency metadata, or user-controlled
  updates.

External plugins are the default for community plugins and optional official
extensions.

## Authority model

Bundled and external plugins use the same capability model. A bundled plugin
does not get private Rust access just because it ships with `saya`.

The host must keep these rules:

- Rust owns narrow capabilities such as filer operations, process handles,
  cache files, and lazy trigger bridging.
- TypeScript plugins own setup, commands, events, and orchestration.
- Public `saya.*` APIs are the preferred integration surface for both plugin
  classes.
- A capability required by a bundled plugin must be designed as a public or
  intentionally previewed capability before the plugin uses it.
- Plugins must not depend on Neovim compatibility APIs, Vim script hooks,
  runtimepath, packpath, or `vim-core-rs` plugin manager behavior.

This keeps bundled plugins honest: they can provide first-party behavior
without becoming hidden Rust features.

## Startup and lazy loading

Both plugin classes can contribute to the same generated artifacts.

- `startup-plan.json` records eager startup entries.
- `lazy-index.json` records lazy command and event triggers.
- `plugin-lock.json` records external plugin sources and dependency metadata.
- Bundled plugins can appear in startup plans and lazy indexes without
  appearing as installable lockfile entries.
- External plugins appear in the lockfile and can also contribute startup plan
  and lazy index entries.

The repository keeps plugin code in responsibility-oriented directories.

```text
plugins/
  bundled/
    dired/
      index.ts
      manifest.json
    lsp-client/
      index.ts
      manifest.json
  manager/
    index.ts
    spec.ts
    lockfile.ts
    startup-plan.ts
    lazy-index.ts
    protocols/
  types/
    startup.d.ts
    runtime.d.ts
    plugin-manager.d.ts
```

The legacy `plugins/saya-dired.ts`, `plugins/saya-lsp-client.ts`, and
`plugins/saya-plugin-manager.ts` paths are re-export shims for existing
configuration files. New code must import from the structured directories.

The Rust host only reads these artifacts and merges them into the existing
startup and runtime registries. The TypeScript manager remains responsible for
artifact generation, dependency ordering, cache invalidation, and protocol
policy.

User configuration declares plugins with two startup methods:

- `saya.plugins.use(...)` declares plugins that must be active during startup.
- `saya.plugins.lazy(...)` declares plugins that load after command or event
  triggers.

The names are intentionally user-facing. `use` means the user wants the plugin
enabled as part of the normal editor session. `lazy` means the plugin is
available through generated lazy placeholders without loading during startup.
The manager normalizes both forms into startup plan, lazy index, and lockfile
artifacts.

## Startup cache policy

`saya` optimizes for a hot startup path while keeping cache-miss behavior
usable. The model combines two ideas from existing Vim plugin managers:

- dpp-style hot start: read generated state and avoid recomputing plugin
  policy during startup.
- vim-jetpack-style miss handling: keep the editor usable and ask the user to
  sync external plugins instead of blocking startup on installation.

The host uses these startup modes:

| Mode | Condition | Startup behavior | Target |
| --- | --- | --- | --- |
| Cache hit | `startup-plan.json` and `lazy-index.json` are valid | Read JSON, merge registries, skip TypeScript manager work | 50-100 ms |
| Bundled manifest fallback | Cache is missing or stale, but bundled manifests are available | Read bundled manifests, register bundled lazy placeholders, continue startup | Usable without seconds of delay |
| External disabled | External lock or checkout data is missing | Skip external plugin entries, show a sync message, continue startup | Do not block editor use |
| Explicit sync | User runs `sy plugin sync` or the equivalent command | Resolve external plugins and regenerate artifacts | Outside startup path |

Cache files are disposable. If the cache is deleted, bundled plugins are
recovered from the distribution manifests. External plugins are not restored
during startup because repository resolution, dependency solving, and network
or process work would make startup unpredictable.

The cache layout is intentionally disposable and contains only generated state
or external checkouts.

```text
~/.cache/saya/plugins/
  plugin-lock.json
  startup-plan.json
  lazy-index.json
  operations.log
  repos/
    <external-plugin>/
```

The host must log the selected mode and elapsed time. Example log labels:

- `[PERF][plugin-host] cache_hit elapsed_ms=...`
- `[PERF][plugin-host] bundled_manifest_fallback elapsed_ms=...`
- `[saya-plugin-host] external plugins disabled: reason=missing_lockfile`
- `[saya-plugin-manager] sync required: reason=cache_missing`

This policy makes cache hit startup fast, keeps first run acceptable, and
preserves explicit user control over external plugin installation.

## Classification guide

Use this guide when deciding where a feature belongs.

| Question | Bundled plugin | External plugin |
| --- | --- | --- |
| Is the feature part of the baseline editor experience? | Yes | No |
| Must it work before any network fetch? | Yes | Optional |
| Is it versioned with `saya`? | Yes | No |
| Is install/update policy user-controlled? | No | Yes |
| Can it fail without blocking normal editing? | Usually no | Yes |
| Can it use private Rust APIs? | No | No |

If a feature needs Rust-only behavior, first decide whether that behavior is a
host capability. The capability can live in Rust, but the feature policy must
remain in TypeScript when it belongs to the plugin layer.

## Dired classification

Local dired is a bundled plugin candidate. It is a core part of the expected
editor workflow, but the directory UI and command orchestration do not belong
in `vim-core-rs` or thick Rust application code.

The Rust side provides host capabilities such as directory listing, current
entry lookup, and explicit file operations. The TypeScript dired plugin owns
startup registration, keymaps, command names, and workflow policy.

This split keeps `saya` usable out of the box while preserving the thin Rust
host boundary.

## Migration rules

When moving existing `./plugins` code into this model, follow these rules:

1. Classify the plugin as bundled or external before changing code.
2. Keep bundled plugin imports stable for existing `init.ts` files when
   possible.
3. Move shared host needs into narrow `saya.*` capabilities, not private Rust
   calls.
4. Generate or update startup plan and lazy index coverage for eager and lazy
   entry points.
5. Add headless tests that assert the relevant `saya-plugin-host` or
   `saya-plugin-manager` logs.
