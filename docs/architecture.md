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
    ├── startup_runtime.rs
    └── saya_live_runtime.rs
         │
         ▼
[Layer 2] Application orchestration and presentation
    ├── bootstrap.rs
    ├── event_loop.rs
    ├── editor_session.rs
    ├── screen_model.rs
    ├── tui_renderer.rs
    └── host_io.rs
         │
         ▼
[Layer 1] Editing core
    ├── vim-core-rs
    └── core_bridge.rs as the repository-local adapter
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
`MarkdownDocumentMap` values outside the draw loop, `screen_model.rs` projects
raw buffer text into display text and display-space mappings, and
`tui_renderer.rs` renders the projected screen model. This path must not mutate
buffer text or re-implement Vim motion.

Syntax, highlight, and conceal have a different boundary. `vim-core-rs` owns
the extraction semantics and public core data. `saya` can collect visible
`CoreSyntaxChunk` values, project them into display cells, and render them, but
it must not define Vim-compatible syntax extraction, `:highlight` tables,
resolved highlight attributes, or `matchadd()` conceal parity.

This layer includes these key modules.

- `bootstrap.rs`
- `event_loop.rs`
- `editor_session.rs`
- `markdown_structure.rs`
- `screen_model.rs`
- `tui_renderer.rs`
- `host_io.rs`

### Layer 3: TypeScript execution

The TypeScript layer exists in two phases.

- `startup_runtime.rs` evaluates `init.ts` before session startup and collects a
  normalized startup registry.
- `saya_live_runtime.rs` hosts runtime callbacks, typed payload dispatch, and
  command execution against a host capability bridge.

This layer must stay isolated from the TUI main loop. The repository already
tests worker-boundary execution and phase separation, even though full live
integration is still incomplete.

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

- The code is a single Rust crate, not a split Cargo workspace.
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

## Next steps

If you want flow-level detail after this architectural view, continue with
these pages.

1. Read [Boot flow design](design/boot-flow.md).
2. Read [Editing flow design](design/editing-flow.md).
3. Read [TypeScript runtime design](design/typescript-runtime.md).
