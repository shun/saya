# vim-core-rs

`vim-core-rs` is a Rust-facing host integration layer around one embedded
upstream Vim runtime. It is written for LLM-first consumption, with humans as
the second audience. The goal of this README is to remove ambiguity before an
agent reads source code, edits behavior, or integrates the crate into a host
application.

This crate is not a generic editor toolkit. It exists to give another
application access to Vim's modal editing engine, buffer state, selected
rendering data, host-mediated virtual document I/O, and host-managed job
bridging.

## Read this first

Read these documents in order when you need the full mental model.

1. `README.md`
   This page. It explains purpose, invariants, boundaries, and the most
   dangerous assumptions to avoid.
2. `docs/SCOPE.md`
   The formal in-scope and out-of-scope boundary for the crate.
3. `docs/known-limitations.md`
   Current implementation gaps, intentionally incomplete areas, and features
   that exist in the type surface but are not fully implemented.
4. `docs/api-index.md`
   The map to the exhaustive public and internal API references.
5. `docs/api-contracts.md`
   The behavior contracts that matter more than raw signatures.
6. `docs/public-api-reference.md`
   The full crate-public symbol reference.
7. `docs/internal-api-reference.md`
   The implementation-only helper and coordination reference.

## Repository purpose

The crate owns the embedded Vim editing core. The host application owns the
environment around it.

The crate does these jobs:

- Creates exactly one embedded Vim runtime per process.
- Executes Normal-mode and Ex-mode commands against that runtime.
- Extracts coherent snapshots of text, cursor, buffers, windows, undo state,
  search data, syntax chunks, and pop-up menu state.
- Converts file-like commands into host-visible VFS requests instead of
  letting embedded Vim perform direct file I/O.
- Bridges Vim job and channel behavior into host-managed processes through
  virtual file descriptors.

The crate does not do these jobs:

- It does not own the UI event loop or rendering pipeline.
- It does not persist files by itself.
- It does not spawn or supervise real OS processes by itself.
- It does not aim to expose the whole Vim runtime as a general-purpose
  scripting platform.

## Hard invariants

These points are not optional design preferences. They are repository-level
truths enforced by code and tests.

- Only one live `VimCoreSession` may exist per process.
- `VimCoreSession` is stateful, not `Send`, and not `Sync`.
- `take_pending_host_action()` is part of the normal control flow, not an
  optional diagnostics API.
- VFS requests are explicit and host-owned. The host must answer them with
  `submit_vfs_response()`.
- Job execution is host-owned. The host must react to `JobStart`, then feed
  bytes back with `inject_vfd_data()` and lifecycle updates with
  `notify_job_status()`.
- Contract tests are the source of truth for behavior when prose and intuition
  disagree.

## Mental model

The safest way to reason about the crate is as four coupled state machines.

- Session machine
  Owns lifetime, the embedded runtime pointer, and the single-session lock.
- Command machine
  Executes Normal or Ex input and returns one coarse outcome instead of a full
  diff.
- VFS machine
  Tracks buffer-to-document bindings, request sequencing, request ledger
  status, and deferred close behavior.
- VFD machine
  Tracks virtual file descriptors and host-managed job status.

Do not model the crate as "a library that edits files." That framing leads to
incorrect assumptions about ownership, persistence, and concurrency.

## Host responsibilities

An embedding host must implement these behaviors.

- Prefer `execute_normal_command()` and `execute_ex_command()` when you
  need one transaction result that contains the final snapshot, emitted
  events, and emitted host actions.
- Drain `take_pending_host_action()` until it returns `None`.
- Drain `take_pending_event()` until it returns `None` when you consume
  asynchronous or queue-delivered editor events outside a transaction result.
- Handle `CoreHostAction::Write` and `CoreHostAction::Quit`.
- Handle every `CoreHostAction::VfsRequest` and send one matching
  `CoreVfsResponse`.
- Spawn real jobs when `CoreHostAction::JobStart` appears.
- Handle `CoreHostAction::JobWrite { vfd, data }` by forwarding the
  Vim-originated bytes to the real process stdin.
- Feed stdout and stderr bytes back through `inject_vfd_data()`.
- Report terminal job status through `notify_job_status()`.
- Set UI size with `set_screen_size()` when screen geometry matters.
- Treat `window_id` as the only canonical pane identity. Resolve focus with
  `active_window_id()` or the unique active entry in `CoreSnapshot.windows`,
  not with host-side ordering or buffer heuristics.
- Read `CoreEvent::Message`, `CoreEvent::PagerPrompt`, `CoreEvent::Bell`,
  `CoreEvent::Redraw`, and layout or buffer lifecycle events from the
  transaction result or event queue. Do not scrape `:messages` or
  `v:errmsg`.

## Dangerous assumptions to avoid

An agent reading this repository must not assume any of the following.

- Do not assume file writes happen automatically inside the crate.
- Do not assume `CoreCommandOutcome::HostActionQueued` means the side effect
  already completed.
- Do not assume every Ex command reaches native Vim unchanged. A file-like
  subset is intercepted and rerouted through Rust policy.
- Do not assume VFS request IDs are reusable or unordered. They are monotonic
  session-local identities.
- Do not assume the latest `CoreVfsResponse::Saved` always applies. Save
  responses can be rejected as stale when revisions advance.
- Do not assume the embedded runtime owns process execution. It only requests
  host action.
- Do not assume pane identity or active-pane resolution from window ordering,
  buffer identity, or host layout heuristics. Use `window_id`,
  `active_window_id()`, and per-window snapshot fields.

## Source-of-truth hierarchy

When repository artifacts disagree, use this order.

1. Contract tests in `tests/`
2. Rust implementation in `src/`
3. Native bridge implementation in `native/`
4. Documentation in `docs/`
5. README-style summaries elsewhere

This hierarchy is intentional. The crate is contract-driven, and many
observable behaviors are locked by tests rather than by Rust type signatures.

## Repository layout

These paths matter most for reasoning and maintenance.

- `src/lib.rs`
  Public API facade, command routing, snapshot conversion, event and host
  action queues, option accessors, and top-level VFS integration.
- `src/vfs.rs`
  VFS request ledger, transaction log, buffer bindings, and deferred close.
- `src/vfd.rs`
  Virtual file descriptor and job-bridge state.
- `native/`
  C bridge and embedded Vim runtime shims.
- `tests/`
  Contract suites that define large parts of the intended behavior.
- `build.rs` and `build_*.rs`
  Bindgen, native compilation, audit generation, and traceability artifacts.

## Build facts that affect behavior

The vendored Vim build is intentionally constrained.

- The generated upstream build runs with `--with-features=normal`.
- The build disables native terminal support.
- The build disables native socket server support.
- The build disables native channel support at configure time, even though the
  crate exposes host-bridged job behavior through its own control plane.

Because of that setup, do not assume this repository behaves like a full
desktop Vim or like Neovim.

## Prebuilt artifacts

By default, consumers do not build the embedded Vim runtime from source.
`build.rs` resolves a target-specific prebuilt artifact, expands it into
`OUT_DIR`, and links the bundled `libvimcore.a`. A Git checkout of this
repository is the exception: when `VIM_CORE_FROM_SOURCE` is unset, local
repository builds use the source build path so tests exercise the checked-out
native and vendored Vim sources.

The published crate still contains the allowlisted `vendor/vim_src` inputs
required for an explicit source build. That means a crates.io consumer can
override the default path with `VIM_CORE_FROM_SOURCE=1` without checking out
this repository.

This repository currently publishes prebuilt artifacts for these targets:

- `aarch64-apple-darwin`
- `x86_64-unknown-linux-gnu`

Each target artifact contains these files:

- `libvimcore.a`
- `bindings.rs`
- `artifact-manifest.json`

The archive also includes the generated traceability outputs that the
repository tests expect in `OUT_DIR`, such as the compile proof, audit
reports, generated Vim headers, and generated upstream test list.

### Environment variables

Use these environment variables to control the build path:

- `VIM_CORE_FROM_SOURCE=1`
  Builds the embedded Vim runtime from source. This is the expected mode for
  repository development, CI, and release packaging.
- `VIM_CORE_ARTIFACT_BASE_URL`
  Overrides the base URL or local directory used to resolve the prebuilt
  archive. This is intended for validation, mirrors, or local testing.
- `VIM_CORE_ARTIFACT_DIR`
  Overrides the local cache directory used to store downloaded and verified
  prebuilt artifacts.

If a prebuilt artifact is unavailable, the build fails with an explicit error.
Consumer builds do not fall back to a source build unless you set
`VIM_CORE_FROM_SOURCE=1`.

## Development and release verification

Repository development uses the source build. This section explains which
GitHub Actions workflow to use as you move from a local code change to a
published crate release.

Run the baseline checks from the repository root with these commands:

```bash
./scripts/setup-git-hooks.sh
VIM_CORE_FROM_SOURCE=1 cargo test
VIM_CORE_FROM_SOURCE=1 cargo publish --dry-run --allow-dirty
```

In a Git checkout, bare `cargo test` also uses the source build path unless you
explicitly set `VIM_CORE_FROM_SOURCE=0`.

After you run `./scripts/setup-git-hooks.sh` once, the repository installs a
`pre-commit` hook through `core.hooksPath`. That hook runs `rustfmt` on staged
`.rs` files before each commit, then re-stages those formatted Rust files so
the commit includes the formatted result.

When you validate the default consumer path before publication, point
`VIM_CORE_ARTIFACT_BASE_URL` at a local directory that contains the packaged
target archives, then run a clean build with `VIM_CORE_FROM_SOURCE=0`.

### GitHub Actions workflow map

Use the workflows according to the scope of your change. The fast path is for
day-to-day Rust and bridge work. The slow paths are for upstream Vim coverage
and publication.

- `CI`
  This workflow runs on `push`, `pull_request`, and manual dispatch. It checks
  formatting, runs clippy, and runs the repository contract tests with
  `cargo test -- --skip upstream_`. Use it for normal development and PR
  validation.
- `Upstream Vim Tests`
  This workflow runs only by manual dispatch. It runs
  `cargo test --test upstream_vim_generated` and covers the vendored upstream
  Vim test cases that are too expensive for every push. A successful local
  build or a successful `CI` run does not replace this workflow because those
  checks do not exercise the full vendored upstream Vim script suite. Use it
  when the change could affect embedded Vim behavior relative to upstream Vim.
- `Release Crate`
  This workflow runs only by manual dispatch. It updates the vendored Vim
  snapshot, verifies source builds, packages prebuilt artifacts, verifies the
  default consumer path, and optionally publishes release assets and the crate.

### When to run upstream Vim tests

`Upstream Vim Tests` is not required for every code change. Use this rule:

- Skip it for documentation-only changes, workflow-only changes, and Rust-only
  changes that do not affect vendored Vim behavior.
- Run it after changing `vendor/vim_src`, `vendor/upstream/vim`, `native/`,
  `build.rs`, or command execution paths that could change Vim script
  behavior, runtime discovery, message handling, job handling, or other
  embedded Vim behavior.

In short, if you can confidently explain that the change does not alter
embedded Vim behavior, `CI` is enough. If the change could alter behavior
relative to upstream Vim, run `Upstream Vim Tests` before release.

### Development-to-publish flow

Use this sequence when you change code and want to carry it through to a
published release.

1. Change the code locally and run `VIM_CORE_FROM_SOURCE=1 cargo test`.
2. Push the branch and wait for `CI` to pass. Treat this as the required
   baseline for ordinary Rust, VFS, VFD, and bridge-facing changes.
3. If the change touches `vendor/vim_src`, `vendor/upstream/vim`, `native/`,
   `build.rs`, or behavior that depends on upstream Vim scripts, run
   `Upstream Vim Tests` manually from the Actions tab and wait for it to pass.
   Otherwise, you can skip that workflow.
4. Before preparing a release, run
   `VIM_CORE_FROM_SOURCE=1 cargo publish --dry-run --allow-dirty` locally if
   you need an immediate packaging sanity check.
5. When you are ready to cut a release, start `Release Crate` manually and set
   these inputs:
   - `vim_version`: the upstream Vim tag to vendor, such as `v9.2.0437`
   - `crate_version`: the crate version to publish, such as `0.2.0`
   - `publish`: `false` for a full dry run without publishing, or `true` to
     continue through the approval and publish stages
6. Wait for `Release Crate` to finish its non-publishing stages:
   `update_upstream`, `verify_source`, `package_prebuilt`, `verify_prebuilt`,
   and `package_check`.
7. If you started `Release Crate` with `publish=false`, review the produced
   results and rerun the workflow with `publish=true` when you are ready to
   release the same version.
8. If you started `Release Crate` with `publish=true`, approve the
   `crates-io-publish` environment when GitHub prompts for manual approval.
9. Let `Release Crate` publish the GitHub release assets first, then publish
   the crate to crates.io. This ordering is required because the default
   consumer build expects the GitHub Releases artifact to exist before users
   install the crate version.

For a production release, publish GitHub Releases assets for every supported
target before you publish the crate to crates.io. That ordering keeps
`vim-core-rs = "<version>"` consumers from hitting a transient 404 between the
crate publish and the asset upload.

## Vim license notice

This repository vendors and modifies portions of upstream Vim in order to
embed one headless Vim runtime behind the Rust API surface. When you
redistribute this repository or binaries built from it, include the bundled
license files that apply to the shipped code.

The repository uses a split licensing model:

- Original `vim-core-rs` code is licensed under Apache License 2.0. See
  `LICENSE`.
- Vendored and modified Vim sources remain subject to the Vim License. See
  `LICENSE-vim`.

The upstream Vim baseline for the current vendor snapshot is recorded in
`upstream-metadata.json`. The repository-level notice for this modified Vim
distribution lives in `THIRD_PARTY_NOTICES.md`, including the maintainer
contact URL for change requests related to the embedded Vim sources.

## Next steps

Read `docs/SCOPE.md` to understand design boundaries. Read
`docs/known-limitations.md` before planning changes so you do not mistake a
current gap for a guaranteed feature.
