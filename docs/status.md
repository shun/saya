# Status

This page summarizes what the repository implements today, what remains in
progress, and what constraints you must keep in mind when you work on the
codebase.

This page is intentionally implementation-focused. For product goals, read
[Project overview](overview.md).

ADR 0001 defines the scope boundary behind this status page. Read that
decision when you need to distinguish between behavior owned by `saya` and
behavior owned by `vim-core-rs`.

## Implemented today

The current repository already implements these behaviors.

- Existing-file startup and new-buffer startup for the CLI application
- Host-side save and quit handling around `vim-core-rs` host actions
- Dirty-state tracking, mode display, and message projection in the TUI
- Viewport-aware screen projection
- Tab-size projection and line-number projection
- Visible syntax chunk projection and TUI styling from `vim-core-rs`
  `get_line_syntax()` data
- Markdown presentation metadata, parser caching, logical-to-display projection,
  inactive rich rendering, active-block raw expansion, and headless regression
  coverage for the WYSIWYG editing path
- Terminal lifecycle management, input routing, and event-loop coordination
- Startup TypeScript evaluation through `deno_core`
- Normalized startup registries for options, keymaps, commands, and events
- Startup `saya.theme` declarations for palette tokens and Markdown semantic
  styles
- Resolved Markdown theme projection for headings, inline code, and links,
  with plain text terminal fallback that preserves text
- Runtime callback dispatch with typed payloads in headless tests and current
  live command and buffer-event paths
- Preview dired, completion, LSP, LSIF, selector, panel, plugin manager, and
  process APIs documented under `docs/api/` and `docs/design/`
- Headless integration coverage for the main application path around
  `vim-core-rs`

## In progress

The current repository also has visible work in progress.

- Stabilization of the preview TypeScript runtime API surface
- Broader command and event coverage in the live application path
- More polished documentation around the evolving API surface

## Feature status matrix

Use this matrix to separate implemented behavior from preview and design-only
work.

| Area | Status | Notes |
| --- | --- | --- |
| CLI editing and save or quit flows | Implemented | Uses `vim-core-rs` for editing semantics and host actions for I/O. |
| Startup TypeScript API | Implemented | Covers options, keymaps, commands, events, theme, and plugin declarations. |
| Live runtime callbacks | Preview | Wired for current command and buffer-event paths, with API stabilization in progress. |
| Dired v1 | Preview | Commands register by default; keymaps are opt-in. |
| Completion | Preview | Manual and automatic completion paths use the typed completion menu. |
| LSP and LSIF | Preview | Live LSP uses the TypeScript process-backed manager; LSIF uses host lookup. |
| Selector | Preview | Runtime selector APIs are implemented, while some future design notes remain open. |
| Plugin manager | Preview | Startup declarations and lazy runtime load dispatch exist. |
| Rust crate modules | Internal/test support | Public Rust modules are not documented as a stable external library API. |

## Constraints

Several repository-level constraints are intentional and must remain visible in
documentation and code review.

- `vim-core-rs` remains the editing-semantics source of truth
- `vim-core-rs` owns syntax, highlight, and conceal extraction semantics;
  `saya` only projects and renders the public extracted data from
  `get_line_syntax()` and related public core surfaces
- Markdown WYSIWYG metadata is presentation metadata owned by `saya`; it must
  not become a Vim syntax, highlight, or conceal compatibility layer
- `saya` must not own `:highlight` definition tables, resolved highlight
  attributes, `matchadd()` conceal parity, or upstream Vim syntax/conceal
  compatibility suites
- `saya` remains the host application layer around that editing core
- Public TypeScript APIs remain under the `saya` namespace
- Broad filesystem and network capabilities remain out of the public MVP API
- Neovim compatibility is not a goal
- Vim script compatibility is not a goal
- The current binary initializes diagnostic logging when the environment or
  startup path requests it

## Known testing issues

You must know this testing caveat when you assess repository health.

- Parallel test execution can trigger `SessionAlreadyActive` failures because of
  the single-session contract.

Use [Testing](testing.md) for the exact commands and context.

## Next steps

If you want to understand why these implementation choices exist, continue with
[Architecture](architecture.md),
[ADR 0001](adr/0001-define-saya-scope-against-vim-core-rs.md), and the design
pages under [docs/design](design).
