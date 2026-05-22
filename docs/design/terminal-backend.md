# Terminal backend design

Terminal panels use a host-owned terminal emulator model instead of exposing a
terminal buffer API to plugins. This keeps panels as display surfaces owned by
Saya while letting PTY-backed tools render through structured terminal cells.

> **Note:** This is a preview feature currently under active development.

## Responsibilities

The terminal backend is split into host-side components with clear ownership.
`PanelManager` owns panel layout, focus, and composition. `TerminalFloatManager`
owns PTY sessions, terminal input, output draining, redraw wakeups, and resize
propagation. `TerminalEmulator` owns VT parsing and exposes a
`TerminalScreenSnapshot` for rendering.

The current default backend is `Vt100TerminalEmulator`, which wraps the existing
`vt100` parser behind the internal `TerminalEmulator` trait. This keeps the
existing backend available while avoiding direct parser coupling in panel and
float rendering code.

## Snapshot model

Terminal output is projected as cells before it is rendered. A
`TerminalScreenSnapshot` contains rows of `TerminalCell` values, cursor
position, and cursor visibility. Each cell carries text plus a
`TerminalCellStyle` with foreground color, background color, bold, underline,
and inverse attributes.

The renderer still consumes `FloatingScreenModel` values. The terminal snapshot
is converted to rendered lines plus terminal-specific inline styles before it
enters the Rust TUI renderer. This lets terminal content reuse the existing
float and panel composition path while preserving cell metadata that was lost in
the old `Vec<String>` projection.

## Resize and redraw flow

When a terminal panel or float is refreshed, the visible content width and
height are sent to `TerminalFloatManager::resize`. The resize is propagated to
both the PTY and the terminal emulator. Matching sizes are treated as no-ops to
avoid repeated PTY resize calls during normal redraws.

PTY reader threads send `UiEvent::Redraw` when output arrives. The UI loop can
therefore redraw terminal panels after process output without waiting for user
input.

## Compatibility boundary

Saya does not model terminal content as an editor buffer and does not add
Vim-compatible or Neovim-compatible terminal-buffer APIs. Terminal content stays
a panel content kind with terminal-only input routing. Generic `panel.focus`
continues to select the panel; terminal input mode remains behavior owned by the
terminal content.

TypeScript plugins do not receive raw terminal drawing access. They can request
terminal-backed panels or floats through host APIs, but the host keeps PTY
state, emulator state, rendering composition, and resize policy.

## Ghostty backend path

The `terminal-ghostty` Cargo feature is reserved for a future
`libghostty-vt`-backed implementation of `TerminalEmulator`. It is intentionally
not a default dependency yet because build, packaging, and cross-platform
behavior still need validation.

The expected integration path is:

- Keep `TerminalEmulator` as the only panel-facing terminal emulation API.
- Add a `libghostty-vt` implementation behind `terminal-ghostty`.
- Keep the `vt100` backend available as a fallback and comparison backend.
- Promote the Ghostty backend only after portability and packaging risks are
  understood.
