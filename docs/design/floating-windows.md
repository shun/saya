# Floating windows design

This page evaluates Neovim floating windows and proposes a `saya` design for
general-purpose floating surfaces. The goal is to support hover hints,
completion menus, terminal overlays, and buffer-backed floating views without
adding a Neovim compatibility layer.

The design keeps editing semantics in `vim-core-rs` and keeps floating
placement, focus arbitration, and terminal drawing in the `saya` application
and presentation layers.

> **Note:** This is a preview design currently under active development.

## Requirements

Floating windows must behave like first-class editor surfaces while staying
small enough for a TUI editor.

- Show transient hint content such as LSP hover, signature help, and
  diagnostics.
- Show completion candidates with a selected row and optional documentation.
- Show a terminal session inside a bounded floating rectangle.
- Show a specific buffer in a floating rectangle.
- Let a focused float receive scrolling, editing, or terminal input.
- Let non-focused or non-focusable floats pass normal input to the underlying
  editor.
- Keep placement deterministic in headless tests.
- Avoid Neovim compatibility promises in the public API.

## Neovim model

Neovim implements floats as real windows with additional window configuration.
The main API is `nvim_open_win(buf, enter, config)`, and the same window can be
reconfigured later with `nvim_win_set_config(...)`.

The important design choices are:

- A float displays a normal buffer.
- A float is still a window, so common window APIs, editing, scrolling, and
  most window options continue to work.
- The config describes placement separately from buffer content.
- `relative`, `row`, `col`, `width`, `height`, `anchor`, and `bufpos` decide
  geometry.
- `focusable` and `mouse` decide whether user actions can enter the float.
- `zindex` decides composition order.
- `border`, `title`, `footer`, and `style=minimal` are presentation options.
- LSP hover and diagnostic helpers are thin wrappers over the generic float
  API. They create a scratch buffer, size it, choose placement near the cursor,
  and close it on cursor or insert events.
- Completion and preview UI also use float-like placement and separate z-index
  conventions for built-in surfaces.

This is a good shape because the editor does not need a separate "tooltip"
primitive, "preview" primitive, and "terminal popup" primitive. The same
window identity and placement model can express all of them.

## What not to copy

Neovim's exact public API is not the right contract for `saya`.

- `saya` does not aim for Neovim API compatibility.
- `external` windows depend on Neovim's multigrid UI model and do not match
  the TUI-only policy.
- Vim window options such as `winhighlight`, `statusline`, and
  `style=minimal` option side effects should not become public compatibility
  behavior.
- Autocommand and session-restoration details should not be imported as hidden
  requirements.
- The public TypeScript surface should use `saya` naming and typed options,
  not `nvim_open_win(...)` shapes.

The transferable idea is the split between content identity, placement config,
focusability, and composition order.

## Neovim debt and `saya` countermeasures

Neovim's float design is useful, but it also carries complexity from extending
the existing Vim window model. `saya` should learn from that design without
copying the historical coupling.

The main risks and countermeasures are:

- Neovim uses one broad window model for normal splits, floats, external
  windows, previews, and popup-like UI. `saya` should keep normal panes and
  floating surfaces separate in the application layer, then compose them only
  in the screen model and renderer.
- `nvim_open_win(...)` accepts split, float, and external-window configuration
  in one API. `saya` should expose a float-specific TypeScript API instead of a
  compatibility-shaped multi-purpose API.
- Neovim's `style=minimal` works by mutating many Vim window options. `saya`
  should represent visual chrome, borders, and minimal presentation as
  explicit renderer-ready settings.
- Neovim's `external` windows depend on multigrid GUI behavior. `saya` should
  reject that direction under the existing TUI-only surface policy.
- Neovim LSP hover, diagnostics, completion, and preview helpers each assemble
  float behavior around the generic API. `saya` should centralize placement,
  lifecycle, focus, and z-index policy in `FloatingWindowManager` so features
  do not duplicate that logic.
- Neovim focus behavior is tied to current-window state, focusability, mouse
  settings, and window commands. `saya` should use an explicit
  `WorkspaceFocus` value that can target panes, floats, prompts, or the command
  line.
- Buffer-backed floats can tempt the application layer to reimplement editing.
  `saya` must route editing in focused buffer floats through `vim-core-rs` and
  use `core_bridge.rs` only as an adapter.
- Neovim has z-index conventions for built-in UI elements. `saya` should use
  documented z-index bands so hover, completion, terminal, prompt, and pager
  surfaces do not accidentally cover each other.
- Neovim placement accepts rich relative targets and out-of-bounds values.
  `saya` should keep placement resolution as a deterministic, testable
  application-layer function with integer terminal-cell output.

## Recommended architecture

Floating windows should be application-layer surfaces that reference content
owned by the appropriate lower layer.

```text
TypeScript runtime or host feature
  -> FloatingWindowCommand
  -> FloatingWindowManager
  -> WorkspaceScreenModel floats
  -> TuiRenderer compositor
```

The recommended module boundary is:

- `vim-core-rs` owns text buffer mutation, cursor movement, modes, and edit
  commands for buffer-backed surfaces.
- `core_bridge.rs` exposes enough window and buffer operations for `saya` to
  bind a float to a core buffer or window.
- `FloatingWindowManager` in the application layer owns float IDs, placement
  specs, focus state, lifecycle, scroll state for non-core content, and z-index
  ordering.
- `screen_model.rs` projects normal panes plus floating surfaces into a single
  render model.
- `tui_renderer.rs` draws normal panes first, then floats sorted by z-index.
- `input_router.rs` keeps key normalization, but the main loop dispatches each
  key to the active focus target.

This keeps the generic float mechanism in `saya` without moving editor
semantics out of `vim-core-rs`.

## Core data model

Use one typed model for all floats.

```rust
pub struct FloatingWindow {
    pub id: FloatingWindowId,
    pub content: FloatingContentRef,
    pub placement: FloatingPlacement,
    pub size: FloatingSize,
    pub focus: FloatingFocusPolicy,
    pub chrome: FloatingChrome,
    pub zindex: i32,
    pub lifecycle: FloatingLifecycle,
}
```

`FloatingContentRef` should distinguish the content owner.

- `CoreWindow { window_id }`: a buffer-backed surface edited through
  `vim-core-rs`.
- `ScratchBuffer { buffer_id }`: a temporary buffer that can be read-only or
  editable depending on the API.
- `Terminal { terminal_id }`: a host-owned terminal process view.
- `StaticLines { content_id }`: read-only text used for hover, diagnostics,
  and simple previews.
- `CompletionMenu { menu_id }`: a structured selectable list, still rendered
  through the same compositor.

The first implementation can start with `StaticLines` and `CompletionMenu`,
but the model must reserve the content-owner boundary now. Otherwise terminal
and editable buffer floats will force a later rewrite.

## Placement model

Use a smaller placement set than Neovim at first.

```rust
pub enum FloatingRelativeTo {
    Editor,
    Window { window_id: i32 },
    Cursor { window_id: i32 },
    BufferPosition {
        window_id: i32,
        line: usize,
        column: usize,
    },
}
```

Placement should also include:

- `anchor`: `NorthWest`, `NorthEast`, `SouthWest`, or `SouthEast`.
- `row` and `col`: signed cell offsets from the anchor target.
- `width` and `height`: positive cell dimensions.
- `fit`: truncate to the editor grid by default.

`BufferPosition` is important for LSP and diagnostics because a hint anchored
to a symbol must move when the parent window scrolls. The position resolver can
translate it through the existing viewport and display-column projection.

Fractional coordinates can be omitted until there is a real need. A TUI
renderer ultimately draws integer cells, and integer-only placement keeps
tests simpler.

## Focus and input routing

Introduce a focus target that is separate from normal pane selection.

```rust
pub enum WorkspaceFocus {
    Pane { window_id: i32 },
    Float { float_id: FloatingWindowId },
    CommandLine,
    Prompt,
}
```

The main loop should route input by focus target.

- A focused `CoreWindow` float sends edit keys to `vim-core-rs` against that
  core window.
- A focused `StaticLines` float handles local scroll keys and close keys.
- A focused `Terminal` float sends raw terminal input to the terminal broker.
- A non-focusable float never captures keyboard input.
- Mouse dispatch chooses the topmost float under the cell when that float has
  `mouse=true`; otherwise the event falls through to the pane below.

This model supports "press hover key once to show, press again to focus" as a
feature-level policy rather than a renderer special case.

## Rendering model

Extend `WorkspaceScreenModel` with a `floats` field.

```rust
pub struct WorkspaceScreenModel {
    pub panes: Vec<PaneScreenModel>,
    pub floats: Vec<FloatingScreenModel>,
    pub active_window_id: i32,
    pub focus: WorkspaceFocus,
    ...
}
```

`FloatingScreenModel` should contain renderer-ready geometry and lines after
placement resolution. The renderer should not inspect editor state or runtime
state while drawing.

Rendering order should be:

1. Clear and draw normal panes.
2. Draw message, prompt, and command-line surfaces that belong below floats.
3. Draw floats sorted by `(zindex, creation_order)`.
4. Set the cursor for the active focus target.

Use default z-index bands instead of exposing Neovim values:

- `40`: hover, diagnostics, and lightweight previews.
- `80`: user-created floating buffers and terminals.
- `100`: completion menu.
- `120`: completion documentation.
- `200`: blocking prompts or pagers.

The exact values can change, but the bands prevent features from accidentally
covering each other.

## TypeScript API shape

The public API should be TypeScript-first and explicit.

```ts
const float = await saya.window.openFloat({
  content: { kind: "lines", lines: hoverLines },
  relativeTo: { kind: "cursor" },
  width: 60,
  height: 12,
  anchor: "nw",
  focusable: true,
  border: "rounded",
  zIndex: "hover",
});

await saya.window.focus(float.id);
await saya.window.close(float.id);
```

For buffer and terminal content:

```ts
await saya.window.openFloat({
  content: { kind: "buffer", bufferId },
  relativeTo: { kind: "editor" },
  width: 80,
  height: 24,
  focusable: true,
});

await saya.window.openFloat({
  content: { kind: "terminal", command: ["git", "status"] },
  relativeTo: { kind: "editor" },
  width: 90,
  height: 20,
  focusable: true,
});
```

The `bufferId` form requires the buffer to already be visible in a current
core window. Hidden buffer-to-window creation is deliberately not implicit.

The API must not expose raw renderer access. Runtime calls should enqueue typed
host commands and let the application layer update the float manager on the UI
thread.

## Lifecycle

Floats need explicit lifecycle policies because many use cases are transient.

- `Manual`: close only through API, command, or explicit user action.
- `CloseOnCursorMove`: useful for hover and diagnostics.
- `CloseOnInsert`: useful for passive hints.
- `CloseOnBufferChange`: useful for stale previews.
- `ReplaceByGroup { group }`: useful for "only one hover for this source."

The manager should log lifecycle decisions with float ID, content kind,
trigger, and focus target. These logs make headless debugging possible without
depending on terminal screenshots.

## Implementation phases

### Phase 1: non-editable floats and compositor

Build the base mechanism first.

- Add `FloatingWindowManager`.
- Add `FloatingScreenModel` to `WorkspaceScreenModel`.
- Render bordered and borderless `StaticLines` floats.
- Resolve `Editor`, `Window`, and `Cursor` placement.
- Add z-index sorting and topmost hit testing.
- Add headless tests for placement, truncation, z-index, and non-focusable
  pass-through behavior.

This phase enables LSP hover-like hints and diagnostics.

### Phase 2: focusable local floats

Add focused scrolling and close behavior for non-core content.

- Add `WorkspaceFocus`.
- Route keys through focus target before falling back to editor input.
- Implement local scroll state for `StaticLines`.
- Add focus commands and mouse focus for focusable floats.
- Add tests for `PageUp`, `PageDown`, arrow scrolling, Escape close, and
  focus restoration.

### Phase 3: buffer-backed floats

Connect floats to real editor buffers and windows.

- Add or expose core operations needed to create a buffer-backed view.
- Make focused buffer floats dispatch edit keys through `vim-core-rs`.
- Keep floating placement metadata in `saya`, not in the core.
- Add tests proving insert, normal-mode movement, scrolling, and dirty-state
  behavior inside the focused float.

If `vim-core-rs` cannot represent the required window identity yet, this phase
must extend `vim-core-rs` or `core_bridge.rs` intentionally instead of
duplicating editing semantics in `saya`.

Status: implemented for existing core-window-backed buffers.

The Phase 10 contract keeps buffer floats tied to an existing
`CoreWindowInfo`. `saya.window.openFloat({ content: { kind: "buffer",
bufferId } })` resolves `bufferId` to a current core window that already
displays the buffer. If no such window exists, the host rejects the request
with an explicit hidden core-window error instead of switching an unrelated
active window to that buffer.

This avoids the Neovim-style debt where a UI float implicitly mutates global
window state. Arbitrary hidden core-window creation remains a `vim-core-rs`
API design task, not a `saya` compositor workaround.

### Phase 4: terminal floats

Reuse the existing terminal broker direction and keep process I/O in the host
layer.

Status: implemented for internal host commands and the public runtime surface.

- `FloatingContentRef::Terminal { terminal_id }` identifies terminal-owned
  content without making the renderer inspect process state.
- `TerminalFloatManager` owns the PTY session, `vt100` parser, viewport state,
  input writer, and explicit `KillOnClose` or `DetachOnClose` close behavior.
- The main loop routes focused terminal-float keys to the PTY session before
  normal editor dispatch. `PageUp` and `PageDown` adjust the terminal viewport.
- Workspace projection drains PTY output and replaces the float's renderer-ready
  lines before composing floats above normal panes.
- PTY-backed tests cover command output rendering, focused input routing,
  detach-versus-kill close policy, and `saya.window.openFloat(...)` terminal
  requests.

The public entry point is `saya.window.openFloat(...)` with
`content.kind: "terminal"`. The older `terminal.float ...` host command remains
an internal integration path while the runtime API is hardened.

### Phase 5: public runtime API

Expose the stable subset to TypeScript.

- Add `saya.window.openFloat(...)`.
- Add `saya.window.close(...)`.
- Add `saya.window.focus(...)`.
- Add `saya.window.floats()` read-only float snapshots for plugins.
- Keep completion-specific APIs separate from the low-level float primitive if
  structured selection semantics are needed.

Status: implemented for the public runtime surface.

The runtime API sends typed request and response values through the host
capability bridge. The application layer opens `lines`, `buffer`, and
`terminal` content by updating `FloatingWindowManager` and, for terminal
content, `TerminalFloatManager`. TypeScript receives read-only snapshots and
cannot access renderer internals such as draw cells or `FloatingScreenModel`.

### Phase 6: production cleanup

Prepare the feature for normal use instead of demo-driven validation.

Status: implemented for the Phase 9 cleanup scope.

- The `SAYA_FLOAT_DEMO` production render hook was removed. Workspace
  projection now composes only floats that were opened through feature entry
  points such as host commands or `saya.window.openFloat(...)`.
- Demo-specific helper names in the main input path were replaced with generic
  floating-window terminology.
- High-frequency per-redraw float refresh logs are trace-level. Lifecycle,
  open, close, focus, and error-path diagnostics remain debug-level so
  headless failures are still diagnosable.
- PTY-backed coverage now includes a runtime `openFloat` terminal scenario.

## Testing strategy

Floating windows should be tested primarily below the PTY layer.

- Unit tests: placement resolution, anchor math, truncation, z-index ordering,
  lifecycle policy, and hit testing.
- Functional harness tests: focus routing, scroll behavior, close events,
  buffer-backed editing, and runtime commands.
- Renderer tests: bordered text layout, clipping, cursor placement, and
  overlapping floats.
- PTY tests: a small number of real-terminal scenarios for redraw artifacts,
  terminal float input, and process-boundary behavior.

Every test that executes commands must use a timeout wrapper in local runs so
failures do not hang the suite.

## Open decisions

These decisions need confirmation before implementation.

- Whether `vim-core-rs` should grow a first-class "floating window" concept or
  only expose enough buffer/window identity for `saya` to place the view.
- Editable scratch floats are not exposed yet. ADR 0004 records the decision
  and points picker/search-result floats at structured list state instead of
  application-layer editing state.
- Whether terminal floats close the terminal process by default or detach the
  view by default.
- Which runtime API names become public and which stay internal until LSP and
  completion integrations exist.

## Recommended first milestone

The first milestone should implement the compositor and `StaticLines` floats
only. That gives LSP hover and diagnostics a real rendering surface while
keeping editable buffer and terminal behavior behind explicit later milestones.

The milestone is valuable on its own and creates the correct shape for the
harder features: focus targets, content ownership, z-index composition, and
placement resolution.
