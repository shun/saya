# saya

`saya` is a CLI text editor built on top of `vim-core-rs`. It keeps a
Vim-derived editing model, uses Rust for orchestration and terminal UI, and
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
- [Boot flow design](docs/design/boot-flow.md)
- [Editing flow design](docs/design/editing-flow.md)
- [TypeScript runtime design](docs/design/typescript-runtime.md)
- [Startup API](docs/api/startup-api.md)
- [Runtime API](docs/api/runtime-api.md)
- [Testing](docs/testing.md)
- [Status](docs/status.md)

If you want the Japanese entry page, see
[README_ja.md](README_ja.md).

## Build

This repository depends on a sibling checkout of `vim-core-rs`, so you must
place both repositories under the same parent directory.

```text
workspace/
├── saya.main/
└── vim-core-rs/
```

Before you build, make sure you have these prerequisites.

- Rust stable
- `cargo`
- A C or C++ build toolchain that can build `rusty_v8`
- A local checkout of `../vim-core-rs`

Run the build from the repository root.

```bash
cargo build
```

The binary name is `sy`.

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

Load a TypeScript configuration file with this command.

```bash
cargo run --bin sy -- path/to/file.txt --config ./init.ts
```

## Minimal `init.ts`

The current startup API lets you configure options, register commands, and
declare event handlers before the editor session begins.

```ts
saya.options.tabSize = 4;
saya.options.lineNumbers = true;

saya.keymap.set("normal", "<leader>w", saya.commands.execute("writeCurrent"));

saya.commands.register("writeCurrent", () => {
  return saya.commands.execute("write");
});

saya.events.on("bufferOpen", (payload) => {
  console.log(payload.buffer.id);
});
```

At the moment, `tabSize` and `lineNumbers` are the most visible startup
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
4. Read the design pages under
   [docs/design](docs/design).
5. Read the API reference under
   [docs/api](docs/api).
