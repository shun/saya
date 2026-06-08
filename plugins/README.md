# plugins

First-class extension/runtime code for `saya`, organized by language.

| Subdir | Language | Description |
| --- | --- | --- |
| [`ts/`](ts) | TypeScript | Plugins, public type declarations (`types/`), the plugin manager, and bundled plugins (`bundled/`). Loaded by the `deno_core` runtime. |

## Future

Wasm offload is a stated direction (see `crates/saya/docs/architecture.md`).
Where it lands depends on its role, decided when it actually exists:

- If it is a **plugin runtime** (extensions authored as Wasm) → add `plugins/wasm/`.
- If it is **Rust-compiled compute offload** (heavy processing) → its source is a
  Rust crate under `crates/` (e.g. `cdylib` targeting `wasm32`), not here.

This directory is intentionally not reserving `plugins/wasm/` yet, to avoid
fixing the wrong assumption before the role is settled.

## Notes

- Dev source lives here; the installed layout copies it into
  `$SAYA_HOME/runtime/plugins/` (the `ts` segment is dropped on install).
- Rust resolves the dev location through a single helper:
  `crates/saya/src/support/paths.rs::dev_ts_plugins_dir()`.
