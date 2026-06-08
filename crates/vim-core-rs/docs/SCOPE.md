# Scope and non-goals

This document defines what `vim-core-rs` is responsible for and what it is
not allowed to become. Read this page before extending the crate. The project
is intentionally narrow.

If you need exact signatures or behavior contracts, continue with
`api-index.md`, `public-api-reference.md`, `internal-api-reference.md`, and
`api-contracts.md`.

## Design context

This crate exists as the editing-core component of a larger editor
architecture.

- Rust hosts own rendering, the event loop, I/O orchestration, and higher-level
  presentation state.
- The extension or scripting layer belongs outside this crate.
- `vim-core-rs` owns embedded Vim editing semantics and selected runtime state
  extraction.
- Tree-sitter syntax extraction, when present, is a versioned extraction
  contract owned by this crate, not a host-owned compatibility layer or a
  general semantic platform. See
  `docs/adr/0003-versioned-tree-sitter-extraction.md`.

Because of that split, the crate must behave like a modal text-editing engine
with a host integration boundary. It must not try to become the whole editor.

## Rendering State Family boundary

`vim-core-rs` treats rendering-adjacent extraction as a family boundary, not
as a general rendering surface. This phase 1 family is governed by a shared
vocabulary so that docs, classification, and tests stay aligned. The
authoritative source for this boundary is this page, `docs/api-contracts.md`,
`docs/api-index.md`, `docs/public-api-reference.md`,
`docs/upstream-test-classification.md`, `tests/quality_gate_contract.rs`,
and `upstream-test-classification.json`. The family is a Vim-owned read-only extraction boundary.

- `Search` and `Syntax` are the current members.
- `Annotations` is the deferred placeholder for text-property style
  extraction.
- `popupwin` is the exclusion and remains host-owned presentation, not
  popup window rendering.
- This phase does not add a new family descriptor or facade.

## In scope

These capabilities are inside the intended product boundary.

### Buffer and text state

The crate owns core text-editing semantics.

- Insert, delete, and replace text through embedded Vim behavior.
- Track revisions, dirty state, cursor position, and window viewport state.
- Expose undo tree information and allow undo jumps.

### Modal input and command semantics

The crate owns Vim-like input behavior.

- Maintain mode transitions across Normal, Insert, Visual, Select, Replace,
  Command-line, and Operator-pending modes.
- Execute Normal-mode command strings.
- Execute Ex commands, including Rust-side interception of file-like Ex
  commands that must cross the host boundary.
- Expose registers, marks, jumplist state, and message events.

### Rendering-adjacent extraction

The crate may expose data that a host renderer can consume directly.

- Snapshot buffers and windows.
- Search pattern state and search match ranges for the current `Search`
  family member.
- Syntax chunks and syntax group names derived from the embedded Vim runtime
  for the current `Syntax` family member.
- Versioned Tree-sitter syntax extraction as a separate public surface from
  Vim-derived `get_line_syntax()` / `CoreSyntaxChunk`, when implemented under
  ADR 0003.
- Pop-up menu state and items.
- Text properties as Vim-owned annotation state for the future
  `Annotations` placeholder, once a narrow read-only surface is defined.

### Host-mediated file and job integration

The crate may request work from the host.

- Convert file-like Ex flows into explicit VFS requests or host write actions.
- Track VFS request ledger state, transaction logs, and deferred close flows.
- Convert Vim job requests into host-managed process actions with VFD bridging.

## Out of scope

These areas are intentionally outside the project boundary.

### Full scripting-platform exposure

The crate is not a general embedding of all Vimscript or Lua capabilities.

- Do not expand the crate toward a full plugin-hosting surface.
- Do not treat Vimscript or Lua interoperability as the primary extension
  model.
- Keep complex extension logic outside the core.

### Host-owned asynchronous orchestration

The crate is not the application's async runtime.

- Do not move general async orchestration, networking, or background task
  ownership into the core.
- Do not model the embedded Vim runtime as the authoritative event loop.
- Keep modern async coordination in the host.

### Modern semantic parsing and highlighting

The crate is not a general semantic analysis platform.

- Do not expand Vim regex syntax extraction into a full semantic pipeline.
- Treat Vim-derived syntax information as renderer input or fallback behavior.
- Keep Tree-sitter extraction separate from Vim-derived syntax extraction.
- Do not route Tree-sitter output through `CoreSyntaxChunk`.
- Do not move grammar, query, capture-overlap resolution, or syntax cache
  invalidation ownership to hosts such as `saya`.
- Do not use Tree-sitter support as a path to general plugin hosting,
  unrestricted query injection, Neovim compatibility, or semantic analysis
  ownership.
- Do not render Mermaid, drawio, SVG, PNG, or other embedded media in this
  crate. Markdown embedded block detection may be exposed as data-only
  extraction, but rendering and layout remain host-owned presentation.
- Follow `docs/adr/0003-versioned-tree-sitter-extraction.md` for any
  Tree-sitter extraction work.

### Virtual text and overlay composition

The crate is not responsible for host-side overlay rendering or popup window
layout.

- Do not move inline hints, diagnostics overlays, popup window rendering, or
  virtual text layout into the embedded Vim core.
- Keep overlay composition in Rust-side rendering systems.
- Keep popup placement, popup composition, and popup border rendering in the
  host.
- Treat text properties as Vim-owned annotation state, not host rendering
  itself. The crate may expose them later through the `Annotations` family
  placeholder.
- Do not expose `:highlight` definition tables or resolved highlight attribute tables as part of the public rendering-state family.

### Terminal emulator ownership

The crate is not the terminal subsystem.

- Do not embed a rich terminal emulator into the core.
- Do not widen the scope toward `:terminal` feature parity.
- Keep PTY ownership and terminal rendering in the host.

## Decision filter

Use this filter when a proposed change is ambiguous.

- If the feature is pure modal editing semantics, it likely belongs here.
- If the feature is state extraction from embedded Vim, it may belong here.
- If the feature is versioned Tree-sitter syntax extraction that follows ADR
  0003, it may belong here.
- If the feature is persistence, process management, rendering, plugin
  hosting, general semantic parsing, media rendering, or async orchestration,
  it likely belongs in the host instead.

## Next steps

Read `known-limitations.md` for current gaps and partially implemented areas.
Then read `api-contracts.md` for the sequencing rules that sit on top of this
scope boundary.
