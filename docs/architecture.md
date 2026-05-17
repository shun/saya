# Architecture

`saya` uses a layered architecture that keeps editing semantics, application
orchestration, rendering, and TypeScript execution separate. This page explains
the current architecture as implemented in the repository, not an aspirational
future workspace layout.

The most important design rule is simple: higher layers may depend on lower
layers, but lower layers must not take on upper-layer responsibilities.

ADR 0001 records the repository-level boundary between `saya` and
`vim-core-rs`. Use that decision when you need to decide whether a feature or
test belongs in the application layer or in the embedded editing core.

## Layer model

The current codebase is easiest to understand as four layers.

```text
[Layer 4] User configuration and future extensions
    ├── init.ts
    └── callback code evaluated in the runtime layer
         │
         ▼
[Layer 3] TypeScript execution layer
    └── src/runtime/
         │
         ▼
[Layer 2] Application orchestration and presentation
    ├── src/app/
    ├── src/input/
    ├── src/presentation/
    ├── src/terminal/
    └── src/features/
         │
         ▼
[Layer 1] Editing core
    ├── vim-core-rs
    └── src/core/ as the repository-local adapter boundary
```

## Responsibilities by layer

Each layer exists to own one type of problem and reject the rest.

### Layer 1: Editing core

The editing core is `vim-core-rs`. It owns modal editing behavior, command
interpretation, buffer mutations, and host actions that describe operations
such as save or quit.

This repository must not duplicate editor semantics in the application layer
unless the behavior is clearly presentation-only.

### Layer 2: Application orchestration and presentation

The Rust application layer owns session startup, terminal lifecycle, rendering,
save and quit policy, and viewport projection. It turns core snapshots into a
screen model and mediates between core host actions and repository-local host
logic.

This layer also owns presentation metadata that is specific to `saya`. The
Markdown WYSIWYG path is the current example: `main.rs` builds
`MarkdownDocumentMap` values outside the draw loop,
`src/presentation/screen_model.rs` projects raw buffer text into display text
and display-space mappings, and `src/presentation/render/renderer.rs` renders
the projected screen model. This path must not mutate buffer text or
re-implement Vim motion.

Syntax, highlight, and conceal have a different boundary. `vim-core-rs` owns
the extraction semantics and public core data. `saya` can collect visible
`CoreSyntaxChunk` values, project them into display cells, and render them, but
it must not define Vim-compatible syntax extraction, `:highlight` tables,
resolved highlight attributes, or `matchadd()` conceal parity.

The `src/` tree mirrors this ownership model so new code has an obvious home.

- `src/app/` owns startup assembly, bootstrapping, session state, host I/O, and
  the event loop.
- `src/input/` owns key routing, command-line editing, command history, and
  local ex-command routing.
- `src/presentation/` owns screen projection, viewport state, Markdown
  presentation metadata, render-ready models, theme resolution, overlays, and
  TUI rendering.
- `src/terminal/` owns terminal lifecycle, terminal capability detection,
  terminal input, signal handling, terminal I/O brokering, and terminal-backed
  floating surfaces.
- `src/features/` owns feature-level orchestration that crosses lower-level
  primitives, such as selector, search, LSP, and completion workflows.

### Layer 3: TypeScript execution

The TypeScript layer exists in two phases.

- `src/runtime/startup.rs` evaluates `init.ts` before session startup and
  collects a normalized startup registry.
- `src/runtime/live.rs` hosts runtime callbacks, typed payload dispatch, and
  command execution against a host capability bridge.

The surrounding runtime modules keep capability setup, configuration parsing,
worker-boundary integration, process operations, dispatch messages, and redraw
refresh decisions out of the TUI drawing path.

This layer must stay isolated from the TUI main loop. The repository already
tests worker-boundary execution and phase separation, even though full live
integration is still incomplete.

The LSP preview follows this boundary. TypeScript startup code declares server
configuration through `plugins/saya-lsp-client.ts`, and runtime callbacks send
typed requests through `saya.lsp.request`. The Rust application layer owns the
host-side feature boundary in `src/features/lsp/` and runtime bridging through
`src/features/lsp/runtime_bridge.rs`, including LSIF index lookup and typed
request or response routing. TypeScript plugins don't receive raw process
handles or broad filesystem access.

### Layer 4: User configuration and future extensions

The top layer contains user-authored `init.ts` files and future extension code.
This is the only layer that should define end-user customization behavior.

The public contract for that layer is the `saya` namespace documented in the
API pages under `docs/api/`.

## Thread and lifecycle model

The repository tries to keep the UI path responsive and deterministic.

- The TUI loop reads terminal events and redraws based on a projected screen
  model.
- Startup TypeScript evaluation is isolated from the UI flow and normalized
  before boot continues.
- Runtime callback execution is designed around a separate worker boundary.
- Save and quit behavior remain explicit host-side operations instead of hidden
  side effects inside the configuration runtime.

## Test architecture

The repository uses a three-layer test structure that matches the layered
application design.

- Layer 1 keeps unit and narrow integration checks close to individual modules.
- Layer 2 uses a shared functional harness to exercise deterministic
  host-application paths without a PTY.
- Layer 3 launches the real executable under a PTY to validate the true
  terminal and process boundary.

This split keeps most application verification fast and deterministic while
still preserving a real-terminal confidence layer.

## Current implementation notes

This page describes the current implementation rather than the larger
architecture vision documented in older planning notes. In the present
repository state:

- The code is a single Rust crate with a layered `src/` module tree, not a split
  Cargo workspace.
- `vim-core-rs` is consumed as a published crates.io dependency.
- The TUI uses `ratatui` and `crossterm`.
- The TypeScript runtime uses `deno_core`.
- The main TUI loop does not yet host the full long-lived runtime callback
  lifecycle.

Use [Status](status.md) for the current implementation state, and use the
design pages for flow-level details. Use
[ADR 0002](adr/0002-adopt-three-layer-headless-test-architecture.md) and
[Test architecture design](design/test-architecture.md) for the testing
structure that supports this architecture.
Use [Theme API design](design/theme-api.md) for the proposed TypeScript-first
theme model for Markdown presentation, palettes, and plugin-provided themes.
Use [Floating windows design](design/floating-windows.md) for the proposed
application-layer model for hover, completion, terminal, and buffer-backed
floating surfaces.
Use [Selector design](design/selector.md) for the proposed
host-managed selector workflow for grep, file search, buffer-line search,
preview, actions, and resumable selector sessions.
Use [LSP preview](api/lsp-preview.md) for the current LSP setup surface,
runtime boundary, supported feature matrix, LSIF limitations, and headless
verification commands.
Use [Plugin model](design/plugin-model.md) for the distinction between bundled
TypeScript plugins that ship with `saya` and external plugins installed through
the plugin manager.

The plugin manager foundation follows the same boundary. Rust provides only the
host primitives for cache files under `~/.cache/saya`, cached startup plan
loading, bundled manifest fallback, lazy trigger bridging, and failure
reporting. Bundled plugin code lives under `plugins/bundled/` with
`manifest.json` files that Rust can read on a cache miss. External plugin
policy, source and protocol resolution, dependency metadata, artifact
generation, and operational workflow live in `plugins/manager/`. See
[Plugin manager API](api/plugin-manager.md) for the cache layout and public
TypeScript surface.

## Next steps

If you want flow-level detail after this architectural view, continue with
these pages.

1. Read [Boot flow design](design/boot-flow.md).
2. Read [Editing flow design](design/editing-flow.md).
3. Read [Floating windows design](design/floating-windows.md).
4. Read [TypeScript runtime design](design/typescript-runtime.md).
5. Read [Selector design](design/selector.md).
6. Read [LSP preview](api/lsp-preview.md).
