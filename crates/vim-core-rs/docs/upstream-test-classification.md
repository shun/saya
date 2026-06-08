# Upstream test classification

This page explains how `vim-core-rs` classifies vendored upstream Vim
`src/testdir/test_*.vim` files and how it tracks repository-owned
adaptation coverage. It follows
`docs/adr/0002-define-compatibility-boundaries.md` and points to the
machine-readable manifest in `upstream-test-classification.json`.

## Summary

- Vendored upstream files: `313`
- In-scope compatibility denominator: `274`
- Preserve directly files: `231`
- Preserve through adaptation files: `43`
- Out of scope files: `39`
- Temporarily excluded files: `0`
- Adapted behaviors: `51`
- Covered adapted behaviors: `27`
- Uncovered adapted behaviors: `24`

The in-scope denominator is the upstream-derived embedded Vim core
baseline. It is computed as `231 preserve_directly + 43
preserve_through_adaptation = 274`, and it excludes the `39
out_of_scope` files.

## Manifest model

The classification manifest intentionally tracks two different units.

- `cases`: vendored upstream `test_*.vim` files and their generated-runner
  selection policy
- `adapted_behaviors`: repository-owned adaptation coverage units

This split exists because upstream Vim is tested as one integrated
binary, while `vim-core-rs` only owns the embedded core and its host
boundary contracts. Some upstream files map cleanly to one adapted
behavior. Others, such as `test_expand.vim`, mix multiple
responsibilities and have to be split into multiple adapted behaviors.

`Rendering State Family` phase 1 uses the same vocabulary across this
document, the scope page, and the contract tests. The authoritative source
for the boundary is the docs, tests, and classification metadata named in
`docs/SCOPE.md`. `Search` and `Syntax` stay current members, `Annotations`
stays the deferred placeholder for text-property extraction, and `popupwin`
is the exclusion because it remains host-owned presentation. This feature
does not add a new family descriptor or facade.
The family is a Vim-owned read-only extraction boundary.

Within that Search family boundary, `incsearch` stays part of the
repository's search contract coverage. The contract keeps inactive window
queries and byte columns observable through `VimCoreSession`, while popup
ownership remains host-owned presentation instead of a crate rendering
contract. `search_capability_contract()` is the typed summary of that
boundary, so classification text and docs should agree with the public
contract fields instead of introducing a parallel vocabulary.

Some adapted behaviors also mix Vim-owned state with host-owned
presentation. For saya-like hosts that implement their own popup UI,
`popupwin` rendering stays host-owned and is not a crate extraction
contract. pum stays separate from popupwin exclusion because it is
completion payload extraction, not popup-window presentation. `highlight` is
currently traceable through search highlight
ranges and syntax chunk extraction, but `:highlight` definition tables
and resolved attribute tables remain outside the public contract.
`textprop` remains Vim-owned annotation state and is deferred until the
repository defines a narrow read-only extraction surface.
Popup placement, popup composition, popup borders, and overlay layout also
remain host-owned presentation policy rather than crate extraction
contracts.

`build_test_runner.rs` consumes the manifest and emits a generated
manifest with:

- `compatibility_baseline` for file-level scope and generated-runner
  selection
- `adaptation_coverage` for behavior-level contract coverage

Generated upstream tests still cover only `preserve_directly` files.
Repository contract tests remain the source of truth for
`preserve_through_adaptation` behavior, including the repository-owned
contracts for VFS, VFD, session, events, runtime discovery, and other
host-boundary flows. `out_of_scope` files stay out of the generated
runner, and there are currently no `temporarily_excluded` files.
In the generated manifest, `adaptation_coverage.tracking_unit` is
`repo-owned adapted behavior` to make that behavior-level ownership
explicit.

Each `adapted_behaviors` entry carries machine-readable
`coverage_status` and optional `coverage_evidence`. Any behavior marked
`covered` must include `coverage_evidence` with a contract suite plus a
locator such as `test_name` or `evidence_ref`. The manifest validation
also requires the referenced contract suite to be declared in
`related_contract_suites`, and `test_name` locators must resolve to a
real repository contract test. When a rendering-state family member or
accessor is promoted, add the repository contract test before treating
that adapted behavior as covered.

## Filesystem and environment promotion

The current promotion pass moves filesystem and environment behavior out
of the broad integration gate and into dedicated contract suites when the
crate can observe the behavior directly.

- `test_file_perm.vim`, `test_file_size.vim`, and `test_filecopy.vim` are
  now tracked through `vfs_contract.rs`, with `integration_contract.rs`
  kept as supporting cross-cutting coverage rather than the primary
  authority.
- `test_xdg.vim` is now tracked through `runtime_path_contract.rs`, with
  `integration_contract.rs` retained only as a broad gate and not the
  primary authority.
- `test_filechanged.vim`, `test_menu.vim`, `test_shortpathname.vim`, and
  `test_windows_home.vim` stay outside dedicated promotion because their
  observable boundary remains host-owned, out of scope, or
  environment/platform dependent.

## Path, expansion, and script-context promotion

Issue #15 continues that work for the path, expansion, and script-context
behaviors that were previously left `uncovered`.

- The environment bucket now promotes
  `environment.chdir_literal_tilde_path`,
  `environment.expand_env_pathsep`, and
  `environment.expand_tilde_filename` through
  `runtimepath_contract_supports_tilde_and_env_path_expansion`.
- The expansion bucket now promotes `expansion.expandcmd_general`
  through `runtimepath_contract_supports_expandcmd_general_cases`.
- The runtime-path bucket now promotes
  `runtimepath.environ_home_and_environment_expansion`,
  `runtimepath.escaped_glob_and_globpath`,
  `runtimepath.expand_function_semantics`, and
  `runtimepath.glob2regpat_conversion` through dedicated
  `runtime_path_contract.rs` tests.
- The script-context bucket now promotes
  `script_context.expand_script_source_levels` and
  `script_context.source_placeholders_outside_source` through
  `runtimepath_contract_supports_script_context_source_placeholders`.

The remaining deferred behaviors stay `uncovered` on purpose:

- `expansion.expandcmd_shell_nonomatch` remains uncovered because the
  repository does not treat shell or platform-dependent command
  expansion as a runtime/environment guarantee.
- `expansion.filename_multicmd_reexpansion` remains uncovered because it
  is compound editor-core command-line parsing semantics rather than a
  dedicated runtime/environment contract.
- `runtimepath.expand_dllpath_options` remains uncovered because current
  source builds do not guarantee optional interpreter-specific `*dll`
  option surfaces.
- `runtimepath.global_command_path_sensitive_flows` remains uncovered
  because the mapped `:global` cases are editor-core command semantics,
  not a dedicated runtime/environment contract.

## Runtime-path bucket

The runtime-path bucket now has behavior-level traceability in
repository contract tests and the generated manifest. The bucket
contains `17` adapted behaviors: `15` covered and `2` uncovered.

Covered runtime-path behaviors currently map to these contract tests:

- `runtimepath.autoload_source` from `test_autoload.vim`:
  `runtimepath_contract_supports_runtime_and_autoload_loading`
- `runtimepath.checkpath_includeexpr_recursion` from
  `test_checkpath.vim`:
  `runtimepath_contract_supports_checkpath_includeexpr_recursion`
- `runtimepath.expand.directory_wildcard_buffer_selection` from
  `test_expand.vim::Test_with_directories`:
  `runtimepath_contract_supports_wildcard_path_expansion_for_buffer_selection`
- `runtimepath.filetype_detection_from_runtime` from `test_filetype.vim`:
  `runtimepath_contract_supports_filetype_detection_from_runtime`
- `runtimepath.findfile_path_discovery` from `test_findfile.vim`,
  `runtimepath.fnameescape_path_quoting` from `test_fnameescape.vim`,
  `runtimepath.fnamemodify_path_transforms` from `test_fnamemodify.vim`,
  and `runtimepath.getcwd_working_directory_queries` from
  `test_getcwd.vim`:
  `runtimepath_contract_supports_path_discovery_and_fnameescape`
- `runtimepath.environ_home_and_environment_expansion` from
  `test_environ.vim` and `runtimepath.escaped_glob_and_globpath` from
  `test_escaped_glob.vim`:
  `runtimepath_contract_supports_environment_mutation_and_escaped_globbing`
- `runtimepath.expand_function_semantics` from `test_expand_func.vim`
  and `runtimepath.glob2regpat_conversion` from
  `test_glob2regpat.vim`:
  `runtimepath_contract_supports_expand_function_semantics_and_glob2regpat`
- `runtimepath.help_local_additions_from_runtime_docs` from
  `test_help.vim`:
  `runtimepath_contract_supports_help_local_additions_from_runtime_docs`
- `runtimepath.help_tagjump_from_runtime_docs` from
  `test_help_tagjump.vim`:
  `runtimepath_contract_supports_help_tagjump_from_runtime_docs`
- `runtimepath.xdg_user_runtime_dirs` from `test_xdg.vim`:
  `runtimepath_honors_xdg_config_home_for_user_runtime_dirs`

The remaining runtime-path behaviors stay `uncovered` for now:

- `runtimepath.expand_dllpath_options` from `test_expand_dllpath.vim`:
  current source builds do not guarantee the optional interpreter
  `*dll` option surface as a runtime-path contract
- `runtimepath.global_command_path_sensitive_flows` from `test_global.vim`:
  the mapped `:global` cases are editor-core command semantics, not a
  runtime-path adaptation contract

`test_expand.vim` is no longer tracked as one indivisible runtime-path
case. The manifest now splits it into behavior entries, and only
`Test_with_directories` remains in the runtime-path bucket. The tilde,
environment, `expandcmd()`, and script-context functions from that file
now live in their own adaptation buckets.

## Classification rules

- **Preserve directly**: modal editing semantics and runtime behavior that
  `vim-core-rs` preserves as embedded Vim compatibility
- **Preserve through adaptation**: behavior that crosses the host
  boundary and is validated mainly through repository contract tests
- **Out of scope**: behavior outside the embedded-core product boundary,
  such as GUI, terminal-emulator parity, plugin hosting, clientserver,
  and external language interpreter integrations
- **Temporarily excluded**: files that are conceptually adjacent to the
  scope but are currently skipped for explicit repository reasons

## Operating rules

- Keep vendored upstream coverage in the generated runner only for
  `preserve_directly` files
- Track adapted coverage by repository-owned behavior, not by upstream
  file when a file mixes multiple responsibilities
- Reclassify cases when host-owned presentation and Vim-owned state need
  separate treatment, especially for popup windows, highlight state, and
  text properties
- Move host-boundary behavior into repository contract tests, even when
  upstream Vim has script coverage for similar scenarios
- Record policy exclusions in the classification manifest, not in the
  skiplist
- Reserve the skiplist for future `temporarily_excluded` files that
  still fit the repository boundary but cannot run yet for explicit
  reasons

## Next steps

- Split additional mixed upstream files into multiple adapted behaviors
  when their responsibilities cross buckets
- Refine per-behavior rationales as feature areas gain or lose scope
- Add or tighten repository contract tests when an adapted behavior
  gains new host-facing coverage
