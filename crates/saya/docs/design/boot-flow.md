# Boot flow design

This page explains how `saya` starts an editor session today. It focuses on the
implemented control flow rather than speculative future boot phases.

The boot flow begins with CLI argument parsing and ends with an initialized
session state plus an initial screen projection.

## Entry points

The current startup path begins in `src/main.rs` and moves through
`src/app/cli.rs` and `src/app/bootstrap/`.

- `parse_launch_request(...)` parses the target path and optional `--config`
  file.
- `prepare_launch(...)` performs startup preflight work.
- The main function initializes the terminal, event loop, and first render.

## Boot sequence

The repository uses this sequence to prepare an editor session.

1. Parse CLI arguments into a `LaunchRequest`.
2. Acquire a session guard so only one live core session exists per process.
3. Read the target file when a path is present.
4. Create a `CoreBridge` over `vim-core-rs`.
5. Load the startup configuration source or fall back to defaults.
6. Evaluate startup TypeScript when a config file is present.
7. Normalize startup results into options, keymaps, commands, and events.
8. Build `EditorSessionState` from the resolved startup state.
9. Project the first `ScreenModel` and draw it.

## Config handling

The startup path treats configuration failures as non-fatal when it can safely
continue with defaults.

- Missing or unreadable config files produce warnings and default startup state.
- Invalid or unsupported startup capabilities also fall back to defaults.
- Fatal startup errors are reserved for target file read failures and session
  guard violations.

This split keeps the editor usable even when the startup config is broken.

## Startup outputs

`prepare_launch(...)` returns a `BootstrapOutcome` that packages the data the
rest of the application needs.

- Target-path identity
- Loaded-config metadata
- Initial tab size
- Initial line-number state
- Startup registry snapshot
- Callback registry seed
- Initial core snapshot
- Live `CoreBridge`
- Startup warnings
- Session guard

## Design constraints

The boot flow keeps several boundaries explicit.

- Core editing semantics stay behind `CoreBridge`.
- Startup TypeScript evaluation happens before the terminal session begins.
- Startup results become normalized Rust data before the TUI loop uses them.
- The repository does not let startup failure silently corrupt editor state.

## Next steps

If you want to follow what happens after boot, continue with
[Editing flow design](editing-flow.md).
