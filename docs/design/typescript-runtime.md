# TypeScript runtime design

This page explains the two TypeScript execution phases in `saya`: startup
evaluation and runtime callback execution. The separation between these phases
is one of the most important design decisions in the repository.

The repository treats startup registration and live callback execution as
different problems with different public surfaces.

## Phase 1: Startup evaluation

Startup evaluation happens before the editor session begins. The implementation
lives in `src/startup_runtime.rs`.

This phase gives `init.ts` a narrow `saya` surface for declaring initial state
and callback registrations.

- `saya.options.tabSize`
- `saya.options.lineNumbers`
- `saya.keymap.set(...)`
- `saya.commands.register(...)`
- `saya.events.on(...)`
- `saya.commands.execute(...)` for command references

The startup phase does not expose runtime-only state readers such as
`saya.buffer.current()`.

## Startup output model

The startup runtime does not directly mutate editor state. Instead, it collects
a normalized `StartupRegistry` and hands that registry back to Rust.

That design gives the repository these benefits.

- Deterministic order preservation
- Clear startup error handling
- Rust-side application of startup state
- No hidden mutation through arbitrary script execution paths

## Phase 2: Runtime callback execution

Runtime callback execution happens in `src/saya_live_runtime.rs`. This phase
hosts command dispatch and event handling through a separate runtime boundary.

The runtime surface is narrower than the startup surface and focuses on typed,
read-only state plus explicit command execution.

- `saya.commands.execute(name)`
- `saya.buffer.current()`
- `saya.window.current()`
- `saya.editor.current()`
- `saya.editor.mode()`

The runtime phase does not expose startup registration APIs.

## Host capability bridge

Runtime callbacks do not reach directly into application internals. Instead,
they execute against a `HostCapabilityBridge`.

That bridge lets the runtime ask for these operations.

- Execute a host command
- Read the current buffer snapshot
- Read the current window snapshot
- Read the current editor snapshot

This keeps the runtime decoupled from the TUI loop and makes headless testing
practical.

## Boundary rules

The repository enforces several rules around the `saya` namespace.

- Startup and runtime surfaces remain separate
- Filesystem and network capabilities stay out of the public MVP surface
- Compatibility-oriented string DSLs do not define the public contract
- Typed payloads are preferred over loosely structured callback arguments

## Current implementation state

The runtime layer is real and tested, but it is not yet fully wired into the
main TUI lifecycle. Today, the repository proves the model through unit and
integration tests, especially headless callback dispatch tests.

Use [Status](../status.md) to see the current integration status and known
limitations.

## Next steps

If you want the concrete public contracts after this design overview, read the
API pages.

1. Read [Startup API](../api/startup-api.md).
2. Read [Runtime API](../api/runtime-api.md).
