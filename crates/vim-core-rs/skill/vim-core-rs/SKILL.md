---
name: vim-core-rs
description: Comprehensive operating guide for the vim-core-rs crate and repository. Use this skill when Codex must implement or fix vim-core-rs features, write or review tests, integrate a host application with VimCoreSession, work on VFS/VFD or job control, inspect search or syntax or completion behavior, understand repository architecture or scope boundaries, or update the vendored upstream Vim integration and build pipeline.
---

# vim-core-rs development guide

Use this skill to work on the crate as both a library user and a repository
maintainer. `vim-core-rs` is not a generic editor toolkit. It is a Rust-facing
host integration layer over an embedded upstream Vim runtime, with strict
session, host-action, message-routing, and repository-contract constraints.

For split panes and renderer-facing work, keep one boundary explicit:
`vim-core-rs` owns Vim-compatible window semantics, while the host owns pane
projection, viewport caches, and rendering. Expose read-only observations of
Vim state; do not move host policy or layout projection into the core.

## Start here

Read only the materials needed for the current task.

- Start with the bundled references in this skill. They must remain sufficient
  even when the skill is copied out of the repository and used standalone.
- For the standalone canonical API set bundled with this skill, start with
  [references/api-index.md](references/api-index.md).
- For repository purpose, hard invariants, host obligations, and dangerous
  assumptions, read [references/readme-summary.md](references/readme-summary.md).
- For project scope and non-goals, read `docs/SCOPE.md` when you are inside
  the repository. Keep those boundaries in mind before adding host-owned
  orchestration, terminal behavior, semantic parsing, or plugin-surface work.
- For current implementation gaps and exposed-but-incomplete APIs, read
  [references/known-limitations.md](references/known-limitations.md) before
  relying on a method just because it exists.
- For exhaustive public symbols and callable surface area, read
  [references/public-api-reference.md](references/public-api-reference.md).
- For the data model behind those symbols, especially message events and
  transaction payloads, read [references/types.md](references/types.md).
- For private helpers, coordination layers, and implementation-only APIs, read
  [references/internal-api-reference.md](references/internal-api-reference.md).
- For sequencing rules and invariants such as VFS save flow, message polling,
  and host-action obligations, read
  [references/api-contracts.md](references/api-contracts.md).
- If you are working inside the original `vim-core-rs` repository and a `docs/`
  directory exists, treat these repository docs as deeper companion material:
  `README.md`, `docs/SCOPE.md`, `docs/known-limitations.md`,
  `docs/api-index.md`, `docs/public-api-reference.md`,
  `docs/internal-api-reference.md`, and `docs/api-contracts.md`.
- If the task touches packaging, prebuilt artifacts, release sequencing, or
  GitHub Actions workflow choice, read the repository `README.md` even when the
  bundled references seem sufficient. That operational detail intentionally
  lives in the repository docs.
- When you need those repository docs, discover them from the repository root
  instead of relying on links embedded in this skill. For example, use
  `rg --files docs` or open the files directly after confirming they exist.
- For public API usage and session methods, read
  [references/api.md](references/api.md) as a shorter task-oriented summary.
- For enums, snapshots, errors, host actions, search matches, and PUM data,
  read [references/types.md](references/types.md).
- For VFS, VFD, job control, deferred quit, and host obligations, read
  [references/vfs-vfd.md](references/vfs-vfd.md).
- For architecture boundaries, non-goals, build pipeline, and upstream vendor
  maintenance, read [references/architecture.md](references/architecture.md).
- For task-to-test mapping and verification strategy, read
  [references/testing.md](references/testing.md).
- For end-to-end usage patterns, read
  [references/examples.md](references/examples.md).

## Keep these repository truths in mind

`vim-core-rs` has a few constraints that dominate implementation decisions.

- Maintain the single-session contract. Only one live `VimCoreSession` may
  exist per process. The crate enforces this with a global `AtomicBool`.
- Treat host actions as part of the public contract. `Write`, `Quit`,
  `Redraw`, `RequestInput`, `VfsRequest`, `JobStart`, and `JobStop` are not
  incidental side effects.
- Treat debug logging as configurable session behavior. When
  `CoreSessionOptions.debug_log_path` is set, Rust-side debug output is
  appended to that file instead of stderr, and the configured path is also
  forwarded into the native bridge.
- Keep host responsibilities outside the core. VFS responses, process spawning,
  screen sizing, message handling, and redraw behavior belong to the host.
- For repository development, default to `VIM_CORE_FROM_SOURCE=1`. A bare
  `cargo test` follows the consumer path, expects a released prebuilt artifact
  for the crate version in `Cargo.toml`, and can fail with a 404 before a
  matching GitHub Release exists.
- Respect the documented project non-goals. In the repository, they live in
  `docs/SCOPE.md`. Do not extend the crate toward full Vimscript embedding,
  rich terminal emulation, or a generic semantic parsing platform.
- If the task touches Tree-sitter syntax extraction, follow repository
  `docs/adr/0003-versioned-tree-sitter-extraction.md`. Keep Tree-sitter output
  separate from `get_line_syntax()` / `CoreSyntaxChunk`, use versioned language
  packages and source revisions, return normalized chunks for the standard host
  surface, and keep grammar, query, capture-overlap resolution, and cache
  invalidation ownership inside `vim-core-rs`.
- Use contract tests as the source of truth for behavior. If API docs and tests
  disagree, assume the tests reflect the intended repository contract until the
  code proves otherwise.
- Prefer repository `docs/api-*.md` files when they are available and you need
  exhaustive coverage. Prefer `skill/vim-core-rs/references/` when you need a
  faster execution path or when the skill is being used standalone. The
  bundled `references/api-*.md` set is the standalone canonical source.

## Pick the right workflow

Choose the workflow that matches the user task instead of reading everything.

### Implement or fix a public API behavior

1. Inspect the relevant method and nearby types in `src/lib.rs`.
2. Read the matching section in `references/api.md` and
   `references/types.md`.
3. Open the most specific contract test from `references/testing.md`.
4. Change code in the Rust layer first. Touch `native/` only when behavior
   truly crosses the FFI boundary.
5. Run the narrowest relevant test before broader test suites.

### Add or debug host integration behavior

1. Identify which `CoreHostAction` variant is the source of truth.
2. Read [references/vfs-vfd.md](references/vfs-vfd.md) and the matching tests.
3. Verify both the command result and the queued host-action sequence.
4. Preserve ordering. Many repository tests assert observable sequencing, not
   only final state.

### Add or debug editing semantics

1. Start with the highest-level contract test covering the feature area.
2. Confirm whether the behavior belongs to Normal commands, Ex commands,
   snapshot extraction, or host-action emission.
3. Prefer existing public methods and snapshots over adding one-off probes.
4. Verify mode transitions, pending input, revision updates, and dirty state,
   not only buffer text.

### Add or debug window, split, or search-rendering behavior

1. Treat `CoreSnapshot.windows` as the source of truth for window geometry,
   active-window state, viewport state, and per-window cursor state.
2. Keep window semantics in the core. Hosts may project panes from window
   metadata, but they must not infer `Ctrl-w` behavior by reimplementing Vim.
3. Use `query_visible_search_state()` or
   `query_visible_search_state_for_window()` for pane-local search rendering
   instead of composing `hlsearch`, `incsearch`, and match ranges manually.
4. Remember that search columns are byte offsets, not display-cell widths.
5. If the task mentions inactive panes, verify the target API works on a
   non-active window instead of assuming active-window-only behavior.

### Update vendored Vim or build pipeline behavior

1. Read [references/architecture.md](references/architecture.md) first.
2. Read the release and workflow sections in `README.md` so you choose the
   right local verification path and the right GitHub Actions workflow.
3. Inspect `build.rs`, the `build_*.rs` audit helpers, and
   `scripts/vendor-sync.sh`.
4. Run targeted quality-gate tests before broad feature suites.
5. Preserve allowlist and traceability artifacts. The repository treats them as
   required evidence, not optional metadata.

## Testing rules that matter

Apply these rules whenever you write or modify tests.

- Serialize tests that create `VimCoreSession`. The repository commonly uses a
  `Mutex` plus `OnceLock` helper because concurrent session creation is invalid.
- Prefer contract-style tests that assert externally visible behavior:
  snapshots, host actions, errors, logs, and public return values.
- When changing VFS behavior, verify request ledger or transaction log entries
  in addition to buffer state.
- When changing job or VFD behavior, verify both injected data and job-status
  completion paths.
- When changing UI-derived state such as search matches, syntax, or PUM,
  validate the extracted Rust structs instead of assuming screen rendering.
- If the change touches repository maintenance or native build inputs, include
  the relevant quality-gate test.

## Implementation heuristics

- Prefer `execute_normal_command` for modal editing semantics and
  `execute_ex_command` for command-line behaviors.
  Inspect the returned `CoreCommandTransaction` when the change may emit
  events or host actions.
- Inspect `CoreCommandOutcome` before assuming a command changed text. Some
  commands only move the cursor, switch modes, or queue host work.
- Use `snapshot()` when the task needs a coherent state capture; use focused
  getters when tests only need one property.
- When debugging logging behavior, initialize the session with
  `VimCoreSession::new_with_options(...)` and set
  `CoreSessionOptions.debug_log_path` explicitly. If the path is `None`,
  debug output still goes to stderr.
- For VFS-backed buffers, expect multi-step flows: resolve, load, edit, save,
  deferred close, and possible resume or denial.
- For jobs, treat `JobStart` as a request for the host to spawn a real process
  and bridge stdio via VFD ids. The core does not own that process.
- For message capture, register `set_message_handler` before the command that
  emits messages.
  Use `CoreMessageSeverity` and `CoreMessageCategory` to filter the returned
  events instead of parsing message text.

## Validation checklist

Before finishing, check the smallest relevant set below.

- Repository development baseline:
  `VIM_CORE_FROM_SOURCE=1 cargo test`
  Use this when the task crosses multiple behavior areas or when you need the
  standard repository path instead of the default prebuilt-consumer path.

- Public API or session semantics:
  `cargo test --test public_api_contract`
  This suite also covers `CoreSessionOptions.debug_log_path` and verifies that
  debug log output is written to the configured file.
- Ex and Normal behavior:
  `cargo test --test integration_contract`
- VFS flows:
  `cargo test --test vfs_contract`
- Jobs and VFD:
  `cargo test --test job_api_contract`
- Messages and Vim expression evaluation:
  `cargo test --test message_log_contract`
- Search, syntax, or completion:
  `cargo test --test search_highlight_contract`
  `cargo test --test incsearch_contract`
  `cargo test --test syntax_contract`
  `cargo test --test pum_contract`
- Window semantics, viewport extraction, or `Ctrl-w` behavior:
  `env VIM_CORE_FROM_SOURCE=1 gtimeout 600 cargo test --test multi_buffer_window_contract ctrl_w_ -- --nocapture`
  `env VIM_CORE_FROM_SOURCE=1 gtimeout 600 cargo test --test multi_buffer_window_contract split_and_vsplit_preserve_window_cursor_state -- --nocapture`
- Native build and vendor integrity:
  `cargo test --test quality_gate_contract`

Run broader `cargo test` only after the narrow task-specific suite passes or
when the user explicitly wants full verification.
