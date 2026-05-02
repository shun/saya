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
- Terminal lifecycle management, input routing, and event-loop coordination
- Startup TypeScript evaluation through `deno_core`
- Normalized startup registries for options, keymaps, commands, and events
- Runtime callback dispatch with typed payloads in headless tests
- Headless integration coverage for the main application path around
  `vim-core-rs`

## In progress

The current repository also has visible work in progress.

- Full integration of `SayaLiveRuntime` into the main TUI lifecycle
- Broader command and event coverage in the live application path
- More polished documentation around the evolving API surface
- Cleanup of known bootstrap test inconsistencies

## Constraints

Several repository-level constraints are intentional and must remain visible in
documentation and code review.

- `vim-core-rs` remains the editing-semantics source of truth
- `vim-core-rs` owns syntax, highlight, and conceal extraction semantics;
  `saya` only projects and renders the public extracted data
- `saya` remains the host application layer around that editing core
- Public TypeScript APIs remain under the `saya` namespace
- Filesystem and network capabilities remain out of the public MVP API
- Neovim compatibility is not a goal
- Vim script compatibility is not a goal
- The current binary does not initialize a logger backend even though many code
  paths emit log calls

## Known testing issues

You must know two testing caveats when you assess repository health.

- Parallel test execution can trigger `SessionAlreadyActive` failures because of
  the single-session contract.
- The full serial suite currently has one known failing test:
  `bootstrap::tests::extracts_initial_tab_size_from_config_file`

Use [Testing](testing.md) for the exact commands and context.

## Next steps

If you want to understand why these implementation choices exist, continue with
[Architecture](architecture.md),
[ADR 0001](adr/0001-define-saya-scope-against-vim-core-rs.md), and the design
pages under [docs/design](design).
