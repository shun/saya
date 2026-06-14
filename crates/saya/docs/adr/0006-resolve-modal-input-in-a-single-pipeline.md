# ADR 0006: Resolve modal input in a single pipeline

## Status

Accepted

This ADR refines ADR 0001. ADR 0001 keeps editing semantics and command
execution in `vim-core-rs`. This ADR narrows where *input resolution* lives,
namely key decoding, mapping and typeahead, and the modal grammar that decides
when a command is complete and which command it is. It also defines how a
resolved command crosses the host boundary.

## Context

`saya` currently resolves modal input with two independent pending-state
machines.

- The host TUI keymap layer (`startup_keymap_pending_lhs`) resolves
  plugin- and user-defined mappings in Rust.
- The editing core keeps its own pending prediction (`pending_input.rs`) on top
  of the native Vim typeahead buffer.

These two machines do not share state, and they drift by one keystroke. When a
`g`-prefixed mapping exists, such as `gd`, `gR`, or `gq` registered by the
bundled LSP client, or `gr` and `gM` registered by `dired`, the first `g` is
consumed by the host keymap layer as a possible mapping prefix and is never
forwarded to the core. The core pending state therefore lags by one key, so
`gg` fails to jump to the first line while `ggg` succeeds.

This is not an isolated defect. It is a structural consequence of duplicating
input-resolution state across two owners. The same root cause produces related
gaps.

- There is no `timeoutlen` equivalent for the host keymap layer, so an
  ambiguous prefix waits for the next key indefinitely.
- Counts do not compose with mappings, because the keymap layer matches a raw
  string while the count lives in the core.
- Ambiguity resolution between a builtin command and a mapping cannot follow a
  single, well-defined rule when the deciders are split.

The following facts were verified while diagnosing the defect.

- The editing core interprets complete commands correctly. Feeding `g` then `g`
  directly to the core moves the cursor to the first line. The defect is
  upstream of the core, in host-side input routing.
- Plugin keymaps are resolved entirely in the host layer. The native engine
  exposes no keymap-registration FFI, but it does expose pending state and a
  host-action channel from engine to host.

## Decision

Resolve all modal input in a single pipeline with one typeahead and one pending
state. The editing backend receives only complete, structured commands. It
never holds partial multi-key state on behalf of the host.

The pipeline is a sequence of stages over the keystroke stream.

```text
terminal bytes
  -> [1] key decode          bytes to logical keys (<C-a>, <Esc>, multibyte)
  -> [2] typeahead + mapping  expand user and plugin maps, timeout, noremap
  -> [3] modal grammar        keys to a resolved Command (count, register,
                              operator, motion or text object, or named action)
  -> [4] Command              BuiltinEdit(intent) | HostCommand(name, args)
  -> [5] execution
         |- BuiltinEdit -> editing backend (buffer, motion, undo)  synchronous
         '- HostCommand -> effect queue -> async worker (TypeScript or Wasm)
  -> [6] view projection      buffer and state to a screen model
  -> [7] render
```

Stages 2 and 3 are distinct but composed in one pipeline. They are not two
competing resolvers. A command is a first-class value, and a builtin edit and a
host command are two variants of the same type. A mapping right-hand side
produces either keys or a `Command`, handled uniformly. There is no separate
interception step for plugin commands.

## Invariants

These rules are what make the defect class impossible by construction.

1. One typeahead and one pending state exist in the whole input path. Partial
   command state lives in exactly one place.
2. `Command` is a first-class type with `BuiltinEdit` and `HostCommand`
   variants. A mapping right-hand side resolves to keys or to a `Command`.
3. Only complete commands cross the host-to-backend boundary. A pending prefix
   never reaches the backend.
4. Pending-time UI is rendered from the pipeline's local state, such as the
   partial-command display, the pending-operator indicator, and incremental
   search. It does not require a backend round trip.
5. Incremental feedback is modeled as a per-keystroke complete command, such as
   extending a visual selection by a motion or updating a search pattern. It is
   not modeled as buffering until the interaction is done.
6. Effects, including plugin and TypeScript commands, language-server work, and
   IO, cross a single asynchronous queue. They are never run inline on the
   input path.

## Ownership

The single input brain, that is stages 1 through 3, is owned by the host in
Rust.

- A mapping right-hand side must be able to produce a host or TypeScript command
  as a first-class result, which the native engine cannot express.
- The product rejects Vim script and Neovim compatibility, so the grammar is a
  bounded, specifiable subset rather than a bug-for-bug reproduction.
- A pure-function pipeline matches the startup-speed, simplicity, and
  portability goals, and it keeps the execution backend swappable.

`vim-core-rs` is the execution backend. It executes complete commands such as
motions, operators, and edits. It owns buffer state, undo, registers, and
marks, and it remains the source of truth for editing semantics, consistent
with ADR 0001.

## Performance

Input resolution is not a performance bottleneck, and the single pipeline is
faster than the current split design.

- The grammar step is a few branches over a small buffer, on the order of tens
  of nanoseconds. An FFI call is negligible. Snapshot projection and rendering
  dominate the frame budget, so input resolution is well under one tenth of one
  percent of it.
- The single pipeline removes today's double parse and the per-keystroke heap
  allocation in the current resolver, which builds a `String` per key. The hot
  path can be allocation-free.
- Passing only complete commands does not add latency. A pending prefix
  produces no buffer change to render, and pending-time UI is local to the
  pipeline.
- Batch input such as macros, paste, and `:g` feeds the same pipeline with
  rendering suppressed. The single-pipeline shape makes batch replay clean.
- The boundary must carry structured commands. Raw key strings must not be sent
  across the boundary to be re-parsed per command.

## Relationship to ADR 0001

ADR 0001 continues to hold for execution and editing semantics. This ADR
consolidates input resolution and clarifies that only complete commands cross
into the backend. ADR 0001 already lists input routing as a `saya` concern.
This ADR makes the grammar part of that routing explicit and removes the
duplicated core-side pending prediction.

## Migration

The target is reached in phases, not in a single rewrite.

0. Stop-gap. Forward the swallowed prefix key to the backend so that `gg`
   works immediately while the larger change is in progress.
1. Make the host pipeline the single source of pending state, and disable the
   duplicated core-side pending prediction.
2. Merge mapping resolution into the same pipeline. Model a mapping right-hand
   side as a `Command` with `BuiltinEdit` and `HostCommand` variants, and
   delete the separate keymap resolver.
3. Pass only complete commands to the backend, and remove partial-key feeding.
4. Later, optionally move motion execution to Rust or Wasm so that the backend
   becomes swappable.

## Testing implications

- The host layer gains tests for the input pipeline: ambiguity between a
  mapping and a builtin, prefix completion such as `gg` while `g`-prefixed
  mappings are present, counts with mappings such as `3gd` and `2gg`, timeout
  behavior, and per-keystroke visual and incremental-search feedback.
- The core remains the source of truth for what a complete command does, so
  core tests do not move.

## Consequences

- The `gg` defect class disappears by construction, and related gaps such as
  timeout and count composition get a single home.
- Net code decreases. The duplicated grammar in `pending_input.rs` and the
  separate keymap resolver collapse into one pipeline.
- New input features have one obvious place to live.
- The main risk is that implementing the grammar subset is classically
  underestimated. This is mitigated by scoping out Vim script and Neovim
  compatibility and by following the phased migration.

## Next steps

- Record the bounded modal-grammar subset as a specification that the pipeline
  implements.
- Land phase 0 as a focused fix, then proceed through phases 1 to 3.
