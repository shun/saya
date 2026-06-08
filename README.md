# saya monorepo

This repository is a Cargo workspace that hosts the `saya` CLI editor and its
embedded Vim runtime library.

## Crates

| Path | Crate | Published | Description |
| --- | --- | --- | --- |
| [`crates/saya`](crates/saya) | `saya` (bin `sy`) | no | Markdown-first CLI text editor built on `vim-core-rs`, with a TypeScript (`deno_core`) configuration runtime. See [`crates/saya/README.md`](crates/saya/README.md). |
| [`crates/vim-core-rs`](crates/vim-core-rs) | `vim-core-rs` | yes (crates.io) | Rust host integration layer for one embedded Vim runtime. |

## Build

`vim-core-rs` compiles an embedded Vim from vendored C sources that are
generated from a pinned upstream tag and are not committed. Generate them once
before the first build.

```bash
crates/vim-core-rs/scripts/vendor-sync.sh apply
cargo build
```

The workspace always builds `vim-core-rs` from source
(`.cargo/config.toml` sets `VIM_CORE_FROM_SOURCE=1`). The binary name is `sy`.

For editor usage, distribution, and design docs, see
[`crates/saya/README.md`](crates/saya/README.md) and
[`crates/saya/docs/`](crates/saya/docs).

## Layout

```
saya/
├── Cargo.toml            # workspace manifest
├── .cargo/config.toml    # workspace-wide cargo env
├── crates/               # Rust crates (publish controlled per-crate)
│   ├── saya/             # editor (bin: sy) — src, tests, docs, README
│   └── vim-core-rs/      # published library — src, native, vendor, docs, README
├── plugins/              # first-class extension code, by language
│   └── ts/               # TypeScript plugins, types, plugin manager (loaded by deno_core)
├── LICENSE
└── .github/workflows/
```
