# ADR 0004: Do not expose editable scratch floats yet

## Status

Accepted.

## Context

`saya` is adding a general-purpose floating-window foundation for hover
information, completion menus, terminal overlays, picker-style lists, and
buffer-backed floating views.

The near-term use cases are:

- Show `grep` or `rg` search results, narrow them interactively, jump to the
  selected file or location, and run a replace action.
- Show and select LSP completion candidates.
- Show LSP information such as hover text, diagnostics, and related previews.

These use cases need read-only information floats, selectable list floats,
preview floats, and existing core-window-backed buffer floats. They don't need
a new editable scratch buffer that lives only inside the `saya` application
layer.

Phase 10 fixed the buffer-backed float contract so a buffer float is editable
only when it is backed by an existing `vim-core-rs` `CoreWindowInfo`. If a
buffer is not visible in a current core window, the host rejects the request
instead of switching an unrelated active window to that buffer. That contract
keeps editing semantics in `vim-core-rs` and keeps placement, lifecycle, and
composition in `saya`.

Exposing editable scratch floats before the broader buffer API is stable would
create pressure to duplicate editing state in `saya`: modes, cursor movement,
dirty state, undo, save behavior, close confirmation, and input routing. That
would blur the repository boundary defined by ADR 0001 and would reintroduce
the kind of hidden window and buffer coupling that the floating-window design
is trying to avoid.

The user-facing model is closer to `ddu.vim`-style picker behavior than to an
editable scratch buffer. `ddu.vim` separates UI, sources, filters, kinds, and
actions. `saya` should adopt the same shape where it helps: sources provide
items, filters narrow them, and actions operate on selected items. It should
not copy the Vimscript, denops, or Neovim compatibility surface.

## Decision

`saya` will not expose editable scratch floats yet.

The supported floating-window interaction models are:

- Read-only information floats for hover, diagnostics, documentation, and
  previews.
- Structured selectable list floats for completion, search results, picker
  UIs, and similar item-oriented workflows.
- Terminal floats with host-owned PTY state.
- Buffer floats backed by an existing `vim-core-rs` core window.

Interactive search and replace workflows must use a structured picker or
search-result float, not an editable scratch buffer. The picker owns query
state, selected item state, preview state, and action dispatch. Replace
operations are explicit host actions over selected items or previewed ranges.

The runtime API must not expose `content.kind: "scratch"` as an editable
surface. If a future feature needs a real editable scratch surface, it must
first define a `vim-core-rs`-backed buffer or hidden-window API that preserves
core ownership of editing semantics.

## Consequences

This keeps floating-window editing behavior aligned with `vim-core-rs` and
prevents `saya` from implementing a second text editor inside the compositor.

The immediate next feature should be a picker or search-result floating UI. It
can support `rg` results, narrowing, selection movement, preview, jump actions,
and replace actions without adding editable scratch buffers.

LSP completion should continue to use a completion-menu content owner. LSP
hover and diagnostics should continue to use read-only information floats,
with markdown-aware rendering considered separately.

The cost is that small ad hoc editable floating forms are deferred. If the
project later needs them, the work must start by extending the core-facing
buffer or window contract instead of adding application-layer editing state.
