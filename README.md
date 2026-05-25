# saya

`saya` is a Markdown-first CLI text editor built on top of `vim-core-rs`. It
embeds Vim-derived editing semantics instead of approximating editor behavior
in the application layer, uses Rust for orchestration and terminal UI, and
rebuilds configuration and extension surfaces around TypeScript instead of Vim
script.

This README gives you the shortest path to understand the project, build it,
run it, and find the permanent documentation under `docs/`.

> **Note:** This is a preview feature currently under active development.

## What you can find here

This repository currently contains a working CLI editor MVP, a startup
TypeScript configuration runtime, and a separate runtime callback foundation.
The TUI and the long-lived runtime are not fully integrated yet, so the
repository is best understood as a working editor plus an evolving
TypeScript-first extension model.

## Documentation

The long-lived project documentation lives under `docs/`. Start with these
pages.

- [Project overview](docs/overview.md)
- [Requirements](docs/requirements.md)
- [Architecture](docs/architecture.md)
- [Install layout design](docs/design/install-layout.md)
- [Boot flow design](docs/design/boot-flow.md)
- [Editing flow design](docs/design/editing-flow.md)
- [Floating windows design](docs/design/floating-windows.md)
- [TypeScript runtime design](docs/design/typescript-runtime.md)
- [Startup API](docs/api/startup-api.md)
- [Runtime API](docs/api/runtime-api.md)
- [LSP preview](docs/api/lsp-preview.md)
- [Testing](docs/testing.md)
- [Status](docs/status.md)

If you want the Japanese entry page, see
[README_ja.md](README_ja.md).

## Build

This repository depends on the published `vim-core-rs` crate from crates.io,
so Cargo downloads it automatically during the build.

Before you build, make sure you have these prerequisites.

- Rust stable
- `cargo`
- A C or C++ build toolchain that can build `rusty_v8`

Run the build from the repository root.

```bash
cargo build
```

The binary name is `sy`.

To stage the release layout with `bin/sy` and `share/saya`, run this command.

```bash
scripts/build-dist
```

For local installs, the default prefix is `$HOME/.local`.

```bash
scripts/install-local
scripts/link-local
```

See [Install layout design](docs/design/install-layout.md) for the release
archive, `SAYA_HOME`, and package-manager layout.

## Run

You can start the current CLI editor with or without a target file.

Open an existing file with this command.

```bash
cargo run --bin sy -- path/to/file.txt
```

Start with a new buffer with this command.

```bash
cargo run --bin sy --
```

Without `-u`, `sy` looks for the default startup config at
`$XDG_CONFIG_HOME/saya/init.ts`, then falls back to `$HOME/.config/saya/init.ts`
when `XDG_CONFIG_HOME` is unset.

Load a TypeScript configuration file with this command.

```bash
cargo run --bin sy -- path/to/file.txt -u ./init.ts
```

Read from stdin with this command.

```bash
printf 'hello\nworld\n' | cargo run --bin sy -- -
```

Start at a specific line with this command.

```bash
cargo run --bin sy -- +42 path/to/file.txt
```

Open in read-only mode with this command.

```bash
cargo run --bin sy -- -R path/to/file.txt
```

Print help with this command.

```bash
cargo run --bin sy -- --help
```

## Minimal `init.ts`

The current startup API lets you configure options, register commands, and
declare event handlers before the editor session begins.

```ts
saya.options.tabstop = 4;
saya.options.number = true;

saya.keymap.set("normal", "<leader>w", saya.commands.execute("writeCurrent"));

saya.commands.register("writeCurrent", () => {
  return saya.commands.execute("write");
});

saya.events.on("bufferOpen", (payload) => {
  console.log(payload.buffer.id);
});
```

You can import local TypeScript plugins from `init.ts`. Static local imports
are resolved before startup evaluation.

```ts
import { setupSayaDired } from "./plugins/saya-dired.ts";

setupSayaDired();
```

### Preview dired setup

`setupSayaDired()` registers the preview directory editor commands and normal
mode keymaps. The default bindings are `-` for parent directory navigation,
`<Enter>` for opening the current entry, `gr` for refresh, `m` for marking,
`M` for unmarking, `gM` for clearing marks, and `D` for a bulk-delete
preview. The defaults avoid binding `u` so normal-mode undo remains available
while editing writable directory listings.

You can customize the public setup surface without exposing broad filesystem
access to TypeScript plugins.

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

The default `sortPolicy: "kind"` groups directories before other entries.

> **Note:** Dired is a preview feature. The command names, keymap shape,
> `hiddenFilePolicy`, `sortPolicy`, `filter`, and `confirmStrategy` options are
> public setup points, but they can still change before the dired API is
> stabilized.
> The versioned local contract is documented in
> [`docs/api/dired-api-v1.md`](docs/api/dired-api-v1.md), including migration
> notes and plugin author anti-patterns.

At the moment, `tabstop` and `number` are the most visible startup
settings in the TUI. Command and event registration are implemented and tested
headlessly, but their full live integration is still in progress.

## Current scope

The current implementation gives you these capabilities.

- Existing-file startup and new-buffer startup
- Normal mode and Insert mode editing
- `hjkl` movement, insert input, and delete operations
- Save and quit flows through host actions
- Dirty-state tracking in the UI
- Tab expansion and line number projection
- TypeScript startup evaluation through `deno_core`
- A separate runtime callback layer with typed buffer and editor snapshots

## License

`saya` source code is licensed under Apache License 2.0. See
[LICENSE](LICENSE).

This repository also depends on `vim-core-rs`, which carries its own license
split and redistributes modified Vim sources under the Vim License. If you
redistribute `saya` binaries that include those components, carry the relevant
third-party notices as well. See
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## Next steps

If you want to understand the project in more detail, read the documentation in
this order.

1. Read [Project overview](docs/overview.md).
2. Read [Requirements](docs/requirements.md).
3. Read [Architecture](docs/architecture.md).
4. Read [Install layout design](docs/design/install-layout.md).
5. Read the design pages under
   [docs/design](docs/design).
6. Read the API reference under
   [docs/api](docs/api).
