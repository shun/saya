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

Startup evaluation supports static local TypeScript imports so user config can
load project plugins without copying plugin source into `init.ts`. The loader
inlines local file imports before evaluation and rejects non-local import
specifiers.

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
- `saya.filer.list(path, options)`
- `saya.filer.currentEntry()`
- `saya.filer.createFile(path)`
- `saya.filer.createDirectory(path)`
- `saya.filer.copy(from, to)`
- `saya.filer.move(from, to)`
- `saya.filer.rename(from, to)`
- `saya.filer.delete(path, options)`

The runtime phase does not expose startup registration APIs.

## Host capability bridge

Runtime callbacks do not reach directly into application internals. Instead,
they execute against a `HostCapabilityBridge`.

That bridge lets the runtime ask for these operations.

- Execute a host command
- Read the current buffer snapshot
- Read the current window snapshot
- Read the current editor snapshot
- Read a directory listing for filer plugins, including narrow sort, hidden,
  and filter options
- Read the current directory buffer entry from host-side metadata
- Execute explicit filer operations without exposing broad filesystem access

This keeps the runtime decoupled from the TUI loop and makes headless testing
practical.

## Boundary rules

The repository enforces several rules around the `saya` namespace.

- Startup and runtime surfaces remain separate
- Filesystem and network capabilities stay out of the public MVP surface
- Filer plugins use the narrow `saya.filer` surface instead of broad
  filesystem access
- Compatibility-oriented string DSLs do not define the public contract
- Typed payloads are preferred over loosely structured callback arguments

## Current implementation state

The runtime layer is real and tested, but it is not yet fully wired into the
main TUI lifecycle. Today, the repository proves the model through unit and
integration tests, especially headless callback dispatch tests.

## Target live integration design

The unresolved work is not whether `SayaLiveRuntime` should exist. It already
exists. The unresolved work is how the main TUI lifecycle should own and drive
that runtime without collapsing the repository layers.

The repository considered three integration shapes.

- Drive runtime callbacks directly inside the main event loop and let the loop
  await each dispatch inline.
- Push runtime callback ownership down into `vim-core-rs` so the embedded core
  can trigger application callbacks as part of core execution.
- Keep `SayaLiveRuntime` as a separate worker-owned runtime and add an
  application-layer integration boundary in `saya` that translates live editor
  events into runtime dispatches and translates runtime outcomes back into UI
  updates.

The first option looks small, but it weakens the existing worker-boundary
design. The main loop would become responsible for runtime execution timing,
error shaping, and redraw policy. That is exactly the kind of cross-layer
ownership that makes the live path harder to reason about.

The second option is worse for this repository. ADR 0001 says `saya` is the
host application layer around `vim-core-rs`, not a second editing core and not
the place where embedded-core semantics should absorb application scripting
policy. Moving runtime callback ownership into `vim-core-rs` would pull
application orchestration across the boundary in the wrong direction.

The third option is the intended design.

### Chosen ownership model

The main TUI lifecycle should own a long-lived runtime integration component at
the application layer. That component should be responsible for these jobs.

- Create `SayaLiveRuntime` from the startup callback registry after bootstrap
  succeeds.
- Hold the runtime for the entire terminal session lifetime.
- Translate application events such as buffer-open and buffer-write-post into
  runtime dispatch requests.
- Translate runtime completion into application-level redraw and message
  updates.
- Shut the runtime down by dropping the integration component when the session
  ends.

The main loop itself should not understand runtime callback internals. It
should only coordinate application events and consume normalized runtime
outcomes.

### Required application components

The live integration path should be split into three application-layer pieces.

- A runtime session owner that lives next to the main TUI loop and owns the
  long-lived `SayaLiveRuntime` instance plus the host bridge it uses.
- A runtime event mapper that converts application moments such as startup
  completion, file open, and write completion into explicit
  `RuntimeEventPayload` values.
- A runtime outcome projector that converts `RuntimeDispatchReport` and
  `RuntimeDispatchError` into redraw requests and transient message updates.

This keeps the boundary explicit. The runtime layer stays responsible for
callback execution. The application layer stays responsible for event choice,
UI projection, and session lifecycle.

### Main-loop contract

The main TUI loop should remain the single owner of visible application state,
but it should not execute runtime logic directly. The main loop contract should
therefore look like this.

1. The loop mutates editor state through `CoreBridge` and host save or quit
   policy.
2. When a host-visible integration event occurs, the loop asks the runtime
   session owner to dispatch the corresponding `RuntimeEventPayload`.
3. Runtime dispatch completion returns a normalized application outcome rather
   than raw runtime internals.
4. The loop applies that normalized outcome by updating transient messages and
   scheduling redraw or projection refresh.

This keeps runtime work asynchronous without making the main loop itself a
runtime host.

### Host bridge rules

The `HostCapabilityBridge` must stay read-oriented and command-oriented. It
should not become a back door for direct TUI mutation.

- Buffer, window, and editor snapshot readers stay on the bridge.
- Explicit host command execution stays on the bridge.
- Transient-message writes, redraw flags, viewport updates, and terminal
  rendering stay out of the bridge and remain in the application loop.

That split matters because a redraw request is an application concern, not a
runtime concern. The existing helpers in `runtime_message.rs` and
`runtime_refresh.rs` already point in this direction and should remain
application-side translators.

### Event coverage strategy

The first live integration pass should stay narrow.

- Dispatch `bufferOpen` after bootstrap succeeds and the application session is
  ready.
- Dispatch `bufferWritePost` only after host-side write coordination succeeds.
- Surface callback failures as transient application messages.
- Request redraw only when runtime dispatch reports that at least one handler
  ran.

That coverage is enough to prove the live path without turning the first
integration step into a wide event-system rewrite.

### Testing implications

The repository should prove this design with host-layer integration tests, not
with duplicated editor-semantics tests.

- Add headless tests that run the main application path with a live runtime
  attached.
- Prove runtime callback failure projection through the same message line used
  by the TUI.
- Prove runtime-triggered redraw through the same event-loop and projection
  path used by the application.
- Keep detailed callback-language semantics and runtime worker internals in the
  runtime module tests unless the main application path is the behavior under
  test.

Use [Status](../status.md) to see the current integration status and known
limitations.

## Next steps

If you want the concrete public contracts after this design overview, read the
API pages.

1. Read [Startup API](../api/startup-api.md).
2. Read [Runtime API](../api/runtime-api.md).
