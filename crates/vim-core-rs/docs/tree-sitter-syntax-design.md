# Tree-sitter syntax design note

This note records the current syntax, highlight, and conceal extraction
boundary, and evaluates whether `vim-core-rs` needs Tree-sitter as a future
highlighting foundation.

The recommendation is to keep the existing Vim-derived extraction contract
stable, then add Tree-sitter only as an experimental, optional, parallel
extraction surface. Tree-sitter output must not be mixed into
`get_line_syntax()` until the crate has a typed provenance model and contract
tests for cache invalidation, visible range extraction, and host integration.

## Current state

`vim-core-rs` currently exposes Vim-derived syntax data through a narrow
read-only API:

- `get_line_syntax(win_id, lnum)` returns line-scoped `CoreSyntaxChunk`
  values.
- `CoreSyntaxChunk` contains `start_col`, `end_col`, `syn_id`, and `name`.
- `get_syntax_name(syn_id)` resolves Vim syntax group names.
- Columns are byte offsets. Hosts that render by grapheme or display cell must
  convert explicitly.
- The crate does not expose `:highlight` definition tables, resolved highlight
  attribute tables, popup composition, or text properties as public syntax
  output.

The native bridge implements this contract by calling Vim's `syn_get_id()` for
each byte column in the requested line. Rust groups consecutive equal syntax
IDs into chunks. This keeps Vim syntax semantics owned by the embedded Vim
runtime and keeps rendering policy in the host.

The current contract tests cover a focused extraction slice:

- `tests/syntax_contract.rs` covers line-scoped syntax chunk extraction,
  syntax group names, `synstack()` / `synIDtrans()` parity, and a narrow
  syntax-conceal slice.
- `tests/search_highlight_contract.rs` and
  `tests/search_highlight_c_contract.rs` cover search highlight ranges.
- Documentation tests in `tests/public_api_contract.rs` and
  `tests/quality_gate_contract.rs` keep highlight tables, popup rendering, and
  text properties outside the current public surface.

The upstream Vim classification treats `test_syntax.vim`,
`test_highlight.vim`, `test_conceal.vim`, `test_matchadd_conceal.vim`, and
`test_matchadd_conceal_utf8.vim` as adaptation targets, not full generated
runner parity. The manifest still marks several of these as uncovered because
the crate does not expose full highlight-table or matchadd-conceal parity.

## Design constraints

Any Tree-sitter design must keep these constraints:

- `vim-core-rs` owns extraction semantics.
- Hosts such as `saya` own projection and rendering only.
- Hosts must not own grammar selection, query loading, highlight capture
  mapping, or cache invalidation rules.
- Tree-sitter output is not Vim syntax, Vim highlight, or Vim conceal parity.
- Tree-sitter parsing must not mutate buffer text.
- Parsing and query execution must stay outside the UI draw hot path.
- Visible range extraction must use cached parse state and return bounded
  results.
- Vim-derived syntax/conceal/highlight compatibility and Tree-sitter highlight
  coverage must be tested and classified separately.

## Option 1: Keep Vim-derived extraction only

This option keeps `get_line_syntax()` as the only syntax extraction API and
does not add Tree-sitter.

Pros:

- Keeps the public API stable for `saya`.
- Adds no dependency, build, grammar, or query maintenance cost.
- Preserves the current upstream Vim compatibility story.
- Avoids ambiguity between Vim syntax groups and Tree-sitter captures.

Cons:

- Leaves modern language highlighting quality limited by Vim runtime syntax.
- Does not create a foundation for semantic or grammar-aware extension data.
- Keeps extraction line-by-line, which is simple but not enough for richer
  language-aware features.

Compatibility:

- Fully compatible with the current API.
- No impact on `saya`.

Cache and invalidation:

- Reuses Vim's own syntax state. The crate does not need a separate parse
  cache.

Test strategy:

- Continue expanding `syntax_contract.rs` for Vim-derived extraction slices.
- Keep upstream Vim files classified as adaptation targets.

Dependency cost:

- None.

Feature flag:

- Not needed.

TypeScript extension surface:

- Weak fit. Extensions would consume Vim syntax group names only, which is not
  a stable semantic model.

## Option 2: Add Tree-sitter as an optional backend for the existing API

This option lets Tree-sitter produce chunks that flow through
`get_line_syntax()` and `CoreSyntaxChunk`.

Pros:

- Minimal host integration churn if the shape remains line chunks.
- Lets `saya` continue consuming one API.

Cons:

- Blurs Vim `syn_id` / group names with Tree-sitter capture names.
- Risks breaking `saya` assumptions about Vim-derived semantics.
- Makes upstream Vim compatibility classification harder because one API can
  mean two different extraction engines.
- Forces a lossy mapping from Tree-sitter captures into `syn_id`.

Compatibility:

- Source-compatible but semantically risky.
- Requires a provenance field or mode selection to avoid silent behavior
  changes.

Cache and invalidation:

- Needs per-buffer parse cache keyed by buffer identity and revision.
- Needs incremental edits from core-owned buffer changes or a full reparse
  fallback for the first experimental version.
- Visible range queries must execute against cached trees and query captures,
  not parse during drawing.

Test strategy:

- Existing `syntax_contract.rs` must keep forcing Vim-derived behavior.
- New Tree-sitter tests must use explicit feature flags and expected capture
  names.

Dependency cost:

- Adds `tree-sitter`, language grammars, query assets, and build maintenance.

Feature flag:

- Required.

TypeScript extension surface:

- Possible, but the mixed API makes extension semantics harder to explain.

## Option 3: Add Tree-sitter as a separate public surface

This option keeps `get_line_syntax()` Vim-derived and adds a parallel
Tree-sitter extraction family.

Pros:

- Preserves the existing `saya` contract.
- Keeps Vim compatibility and Tree-sitter highlighting as separate concepts.
- Lets the crate define typed provenance, capture names, language IDs, and
  versioned query metadata.
- Gives future hosts and extensions a cleaner semantic foundation.

Cons:

- Adds a second syntax-like API surface.
- Requires host integration work to opt into Tree-sitter data.
- Needs careful naming so hosts don't treat Tree-sitter as Vim parity.

Compatibility:

- Backward-compatible. `get_line_syntax()` and `CoreSyntaxChunk` remain stable.
- `saya` can keep rendering Vim-derived chunks while adding an explicit
  Tree-sitter layer later.

Cache and invalidation:

- Add a per-buffer `TreeSitterSyntaxStore` inside `VimCoreSession`.
- Key cache entries by buffer ID, language ID, parser version, query version,
  and buffer revision.
- Use incremental parsing when edits can be represented as byte and point
  ranges.
- Fall back to bounded full reparse for MVP when edit deltas are unavailable.
- Execute visible range extraction from cached trees and query captures.

Test strategy:

- Keep existing Vim-derived contract tests unchanged.
- Add feature-gated tests for parser selection, cache invalidation after buffer
  edits, visible range extraction, and capture-to-chunk grouping.
- Add documentation contract tests that classify Tree-sitter as a separate
  family from Vim syntax extraction.

Dependency cost:

- Moderate. The crate owns Tree-sitter runtime dependency, grammar crates,
  query assets, and versioning.

Feature flag:

- Required. Use the stable opt-in `tree-sitter-syntax` feature. The
  `experimental-tree-sitter` feature remains as a compatibility alias.

TypeScript extension surface:

- Strong fit. Extensions can consume a typed Tree-sitter extraction family with
  language ID, capture names, and query version metadata.

## Option 4: Start with a few experimental languages

This option is a staged form of option 3. The crate adds Tree-sitter behind an
experimental feature for a small language set such as Markdown and Rust.

Pros:

- Limits dependency and query maintenance cost.
- Gives `saya` a realistic integration path without destabilizing Vim-derived
  extraction.
- Tests cache, invalidation, and visible range behavior with concrete grammar
  crates.
- Keeps Markdown WYSIWYG presentation metadata separate from syntax output.

Cons:

- Users may expect broad language coverage too early.
- Requires explicit unsupported-language behavior.
- Requires careful docs to prevent treating Tree-sitter captures as Vim
  highlight parity.

Compatibility:

- Backward-compatible if implemented as a parallel surface.
- `saya` can continue using `get_line_syntax()` and opt into the experimental
  API only when ready.

Cache and invalidation:

- Same as option 3, but MVP can permit full reparse on edit and require cached
  visible range extraction before public use.

Test strategy:

- Add one small Rust fixture and one Markdown fixture.
- Test that Markdown Tree-sitter extraction does not encode WYSIWYG display
  metadata.
- Test that visible range extraction doesn't parse in the query method when
  cache is fresh.

Dependency cost:

- Lower than broad Tree-sitter adoption, but still adds runtime and grammar
  crates.

Feature flag:

- Required.

TypeScript extension surface:

- Good incremental path. The public shape can be designed for future extension
  languages before broad grammar support exists.

## Recommendation

Adopt option 4 as the MVP path, using option 3 as the target architecture.
Do not route Tree-sitter output through `get_line_syntax()` for the first
version.

The existing Vim-derived API is already a stable public boundary for `saya`.
It carries Vim syntax IDs and group names, and upstream Vim compatibility
classification depends on that meaning. Tree-sitter has different semantics:
captures, query versions, parser versions, language IDs, and no Vim conceal or
`:highlight` table parity. Mixing those results into `CoreSyntaxChunk` would
make the API easier to call but harder to reason about.

## Proposed architecture

Add a new experimental extraction family with explicit provenance:

- `CoreSyntaxChunk` remains the Vim-derived chunk type.
- `CoreTreeSitterChunk` uses byte ranges and capture names, not Vim syntax IDs.
- `CoreTreeSitterLineSyntax` or `CoreTreeSitterRangeSyntax` wraps chunks with
  buffer ID, language ID, parser version, query version, source revision, and
  cache status.
- `VimCoreSession` owns grammar selection, query assets, parser lifecycle,
  parse cache, and invalidation.
- `VimCoreSession` owns region-aware language resolution. Vim `filetype`,
  buffer name, optional host hints, and Markdown info strings are resolver
  inputs, not final grammar or query authority.
- Hosts request visible ranges and receive data-only chunks.
- Hosts never provide query text or grammar objects in the MVP.

The language model must not be buffer-level only. Markdown and similar
container formats can have a root document language plus embedded syntax,
diagram, media, or unknown regions. For example, a Markdown document may
resolve to a Markdown root language while a fenced `ts` region resolves to a
TypeScript syntax package and a fenced `mermaid` region resolves to a diagram
block kind.

The API can start with range extraction:

```rust
pub fn get_tree_sitter_syntax_range(
    &mut self,
    win_id: i32,
    start_lnum: i64,
    end_lnum: i64,
) -> Result<CoreTreeSitterRangeSyntax, CoreCommandError>;
```

The method takes `&mut self` in the MVP because cache refresh is observable
internal state, even though it doesn't mutate Vim buffer text. A later design
can split cache preparation from read-only query methods if needed.

## MVP completion criteria

The MVP is complete when the experimental feature provides:

- A feature-gated Tree-sitter module that is disabled by default.
- One or two crate-owned language integrations.
- Crate-owned highlight query assets and capture mapping.
- Per-buffer parse cache with clear invalidation behavior.
- Visible range extraction returning byte-range chunks.
- Tests proving that Vim-derived `get_line_syntax()` remains unchanged.
- Tests proving Tree-sitter output has separate provenance and capture names.
- Documentation stating that Tree-sitter output is not Vim syntax,
  `:highlight`, or conceal parity.

## Issue and task candidates

1. Document the syntax extraction boundary and Tree-sitter non-goals.
2. Add an experimental Tree-sitter feature flag and dependency skeleton.
3. Define `CoreTreeSitterChunk` and range result types.
4. Implement per-buffer parser cache and invalidation.
5. Add a region-aware language resolver for root documents and embedded
   regions.
6. Add Markdown or Rust grammar support with crate-owned queries.
7. Add visible range extraction from cached query captures.
8. Add `saya` integration contract documentation.
9. Revisit upstream test classification after Vim-derived conceal and
   matchadd-conceal extraction surfaces are clarified.

## First implementation task

The first implementation task must be the public contract design, not parser
code. Add feature-gated types and documentation tests that assert:

- `get_line_syntax()` remains Vim-derived.
- Tree-sitter extraction has a separate type and explicit provenance.
- Tree-sitter chunks use byte ranges and capture names.
- Tree-sitter output does not expose Vim `syn_id`, resolved highlight
  attributes, or conceal display substitutions.
- Language resolution is region-aware and registry-backed rather than based
  only on buffer-level `filetype`.

## Integration contract for `saya`

`saya` can rely on this contract:

- Continue using `get_line_syntax()` for Vim-derived syntax chunks.
- Treat all chunk columns as byte offsets before display-space projection.
- Do not infer Vim highlight attributes from `CoreSyntaxChunk`.
- Do not own Tree-sitter grammar, query, or capture mapping.
- If Tree-sitter is enabled later, consume it as a separate optional layer with
  explicit provenance.
- Keep Markdown WYSIWYG metadata in presentation metadata, not in syntax
  extraction.
- Treat cache status and source revision as invalidation hints, not rendering
  policy.

## Open questions

Resolve these questions before implementation:

- Which first language is most useful: Markdown, Rust, or both?
- Does the public Tree-sitter range API need `&mut self`, or must cache
  preparation be a separate explicit step?
- Which buffer revision should key the cache: current snapshot revision,
  Vim `CHANGEDTICK`, or a new buffer-local revision field?
- Should unsupported languages return an empty result, a typed unsupported
  status, or `CoreCommandError`?
- How much query customization must be available to future TypeScript
  extensions, and how can that avoid moving query ownership to `saya`?
- Do Tree-sitter chunks need priority and overlap rules in the MVP, or can
  they return normalized non-overlapping chunks only?
