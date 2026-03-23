# Editing flow design

This page explains how the current TUI loop turns terminal input into editor
state changes and screen updates. It covers the implemented path in the Rust
application layer.

The editing flow is deliberately small. It favors explicit transitions and
projection steps over a broad feature surface.

## Main loop structure

The current TUI loop lives in `src/main.rs` and coordinates these pieces.

- `EventLoopCoordinator` for input, redraw, resize, and shutdown ordering
- `CoreBridge` for editor-core commands
- `EditorSessionState` for save and quit policy
- `ViewportState` for visible-line management
- `ScreenModel` projection and `TuiRenderer` drawing

## Input routing

Terminal key input becomes editor intent in two steps.

1. Raw terminal events are converted into `KeyInput`.
2. `resolve_intent(...)` turns `KeyInput` into `EditorIntent`.

The current intent set distinguishes editing keys from save and quit actions.
This keeps editor mutations separate from application-level session control.

## Editing path

For edit commands, the main loop forwards input to `CoreBridge`, reads the new
snapshot, drains pending host actions, updates dirty state, and redraws.

This path keeps the application layer small.

- `vim-core-rs` owns modal behavior and text mutation
- `CoreBridge` adapts repository-local command dispatch details
- `screen_model.rs` turns core state into renderable data

## Save and quit path

The editor does not let the core write directly to disk or exit the process by
itself. Instead, the core emits host actions and the application layer decides
how to fulfill them.

- Save requests become `SaveRequest` values handled by `host_io.rs`
- Quit requests become `QuitDecision` values handled by `editor_session.rs`
- Dirty-state warnings stay in the application layer and surface through the UI

This split makes save and quit logic testable without embedding host behavior
inside the core.

## Projection model

`screen_model.rs` turns core snapshots and session state into a render-only
view model.

The projection currently handles these concerns.

- File-name resolution
- Mode labels
- Dirty-state display
- Tab expansion
- Line-number prefixes
- Cursor row and display-column conversion
- Viewport-relative visible lines
- Transient status messages

`tui_renderer.rs` consumes only `ScreenModel`, which keeps rendering detached
from core and session internals.

## Next steps

If you want to understand the TypeScript-specific execution model, continue
with [TypeScript runtime design](typescript-runtime.md).
