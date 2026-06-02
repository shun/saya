![saya CLI editor banner](images/saya-banner.png)

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
TypeScript configuration runtime, and a long-lived runtime callback foundation.
The TUI now wires the runtime into current command and buffer-event paths, while
the public API and feature coverage remain preview work.

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

This checkout depends on the sibling `vim-core-rs` repository through the local
path configured in `Cargo.toml`. Keep
`/Users/skudo/ghq/github.com/shun/saya_ws/vim-core-rs` available when you build
this development tree.

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
saya.options.shiftwidth = 4;
saya.options.expandtab = true;
saya.options.smartindent = true;
saya.options.number = true;
saya.statusline.set({
  left: ["fileName", "mode"],
  right: ["filetype", "modified"],
});
saya.ftplugin.set("go", {
  extensions: ["go"],
  options: {
    expandtab: false,
    softtabstop: 0,
    shiftwidth: 0,
  },
});

saya.keymap.set("normal", "<leader>w", saya.commands.execute("writeCurrent"));

saya.commands.register("writeCurrent", () => {
  return saya.commands.execute("write");
});

saya.events.on("bufferOpen", (payload) => {
  console.log(payload.buffer.id);
});
```

You can import local TypeScript plugins from `init.ts`. Static local imports
are resolved before startup evaluation. Use `./` or `../` for paths relative
to the importing file, `~/` for paths relative to your home directory, or a
leading environment variable such as `$SAYA_HOME/` or `${SAYA_HOME}/`.

```ts
import { setupSayaDired } from "./plugins/bundled/dired/index.ts";

setupSayaDired();
```

### Preview dired setup

`setupSayaDired()` registers the preview directory editor commands by default.
Normal-mode keymaps are opt-in: add the `keymap` option or legacy flat key
options when you want mappings such as `-`, `<Enter>`, `gr`, `m`, `M`, `gM`, or
`D`. The defaults avoid binding `u` so normal-mode undo remains available while
editing writable directory listings.

You can customize the public setup surface without exposing broad filesystem
access to TypeScript plugins.

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
  },
  keymap: {
    enter: "<Enter>",
    refresh: "gr",
  },
});
```

> **Note:** Dired is a preview feature. The command names, keymap shape,
> `hiddenFilePolicy`, `sortPolicy`, `filter`, and `confirmStrategy` options are
> public setup points, but they can still change before the dired API is
> stabilized.
> The versioned local contract is documented in
> [`docs/api/dired-api-v1.md`](docs/api/dired-api-v1.md), including migration
> notes and plugin author anti-patterns.

At the moment, `tabstop`, `number`, and indentation options such as
`smartindent` are the most visible startup settings. You can also use Vim-style
option aliases such as `si` for `smartindent`, `sw` for `shiftwidth`, and `et`
for `expandtab`. Command and event registration are implemented, tested
headlessly, and wired into the current live runtime paths.

Startup also applies a small ftplugin layer after user options. Go files use
the Vim Go defaults, so `*.go` buffers get `noexpandtab`, `softtabstop=0`, and
`shiftwidth=0` even when global startup options prefer spaces.
The ftplugin layer is data-driven: startup config and plugins can disable it
with `saya.ftplugin.enabled = false`, disable one filetype with
`saya.ftplugin.disable("go")`, or replace/add a filetype definition with
`saya.ftplugin.set(filetype, { extensions, options })`.

Plugins and startup config can customize the status line with
`saya.statusline.set({ left, right })`. The initial segment set includes
`fileName`, `mode`, `filetype`, and `modified`, which gives Go buffers a visible
`go` filetype after the ftplugin layer resolves.

## Current scope

The current implementation gives you these capabilities.

- Existing-file startup and new-buffer startup
- Normal mode and Insert mode editing
- `hjkl` movement, insert input, and delete operations
- Save and quit flows through host actions
- Dirty-state tracking in the UI
- Tab expansion and line number projection
- TypeScript startup evaluation through `deno_core`
- A live runtime callback layer with typed buffer and editor snapshots

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
