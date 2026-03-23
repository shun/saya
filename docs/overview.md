# Project overview

`saya` is a CLI text editor built on top of `vim-core-rs`. It preserves a
Vim-derived editing model, keeps the application shell in Rust, and treats
TypeScript as the primary public surface for configuration and future
extensions.

This page explains the product direction, the intended user experience, and the
explicit non-goals that shape the rest of the documentation.

## Product goals

The project exists to give you a portable terminal editor with a small public
surface and predictable responsibility boundaries.

- Keep editing semantics in `vim-core-rs` whenever possible
- Keep the application layer focused on orchestration, rendering, and host I/O
- Expose configuration and extension entry points through TypeScript
- Preserve startup speed, portability, and conceptual simplicity
- Avoid Neovim compatibility layers and Vim script-first workflows

## Product boundaries

The project separates editor-core behavior from application behavior. The core
editing experience comes from `vim-core-rs`, while this repository owns these
areas.

- CLI startup and argument parsing
- Session lifecycle and save or quit policies
- Terminal rendering and viewport management
- Startup TypeScript evaluation through `deno_core`
- Runtime callback infrastructure for future editor events

## Non-goals

The repository intentionally does not target several common editor-platform
goals. These exclusions are part of the design, not temporary omissions.

- Full Neovim compatibility
- Vim script as the main extension surface
- String-first compatibility APIs such as `vim.cmd(...)`, `:set`, or `:map`
- Broad filesystem or network capabilities in the public TypeScript API
- Pulling application-level concerns down into `vim-core-rs`

## Current state

The current codebase already supports a functional CLI editor MVP and a tested
TypeScript startup surface. At the same time, the long-lived runtime callback
layer is still partially isolated from the main TUI loop.

You can think of the repository as having two stable centers today.

- A headless-testable CLI editor flow
- A TypeScript-first startup and runtime foundation

Use [Status](status.md) to see what is implemented today, and use
[Architecture](architecture.md) to understand how the pieces fit together.

## Next steps

If you want to move from product intent to concrete implementation details,
continue with these pages.

1. Read [Requirements](requirements.md).
2. Read [Architecture](architecture.md).
3. Read the design pages under
   [docs/design](design).
