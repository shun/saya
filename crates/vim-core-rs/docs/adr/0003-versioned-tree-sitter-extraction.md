# ADR 0003: Keep Tree-sitter extraction versioned, separate, and
# host-verifiable

This ADR records the repository-level decision for how future Tree-sitter
syntax extraction must relate to existing Vim-derived syntax extraction. It
exists to keep `saya` integration stable while `vim-core-rs` explores
Tree-sitter as an experimental highlighting foundation.

- Status: Accepted
- Date: May 3, 2026

## Context

`vim-core-rs` currently exposes Vim-derived syntax data through
`get_line_syntax()` and `CoreSyntaxChunk`. That API is a public extraction
contract for hosts such as `saya`, and it carries Vim syntax group semantics.

`saya` owns display-space projection and rendering. It must not own Vim
syntax compatibility, Tree-sitter grammars, Tree-sitter queries, capture
mapping, or syntax cache invalidation. Markdown WYSIWYG metadata is
presentation metadata in `saya`; it is not ordinary syntax highlight data.

Tree-sitter can improve highlighting for Markdown, Rust, TypeScript, and
other languages, but it has different semantics from Vim syntax:

- Tree-sitter produces captures and parse trees, not Vim syntax IDs.
- Tree-sitter highlight output is not Vim conceal or `:highlight` parity.
- Parser, query, and injection behavior affect output and must be versioned.
- Large files, incomplete TypeScript code, and language injections can make
  parsing or highlighting stale, partial, or budget-limited.

Neovim is useful prior art, but `vim-core-rs` must not copy every Neovim
tradeoff. In particular, runtimepath-based parser or query ownership,
capture-to-highlight-group coupling, and Tree-sitter conceal metadata can
blur the boundary between extraction and presentation.

## Decision

`vim-core-rs` will keep Vim-derived syntax extraction and Tree-sitter
extraction as separate public surfaces.

`get_line_syntax()` and `CoreSyntaxChunk` remain Vim-derived. Tree-sitter
output must not be routed through `CoreSyntaxChunk` unless a later ADR changes
the compatibility model.

The Tree-sitter MVP will start with Markdown and Rust. TypeScript and TSX are
the next target languages, and the MVP API must not block that extension.

Tree-sitter dependencies are opt-in. The default build must not include the
Tree-sitter runtime or grammar crates. The stable opt-in surface is enabled by
a root `tree-sitter-syntax` Cargo feature, and built-in language packages are
enabled by per-language package features such as `tree-sitter-markdown`,
`tree-sitter-rust`, and `tree-sitter-typescript`. The older
`experimental-tree-sitter` feature remains as a compatibility alias for hosts
that adopted the preview feature name.

`vim-core-rs` will own a versioned language package registry. The MVP may only
ship built-in Markdown and Rust packages, but the architecture must support
registered packages with explicit package, parser, and query versions. Hosts
must not pass raw grammars, raw queries, or capture mappings directly into the
syntax extraction path.

Markdown fenced blocks are part of the extraction boundary, but only as
data-only embedded block detection in the MVP. `vim-core-rs` may classify a
block as syntax, diagram, media, or unknown based on the Markdown parse result
and info string. It must not render Mermaid, drawio, SVG, PNG, or other
embedded media. Rendering and layout remain host-owned presentation.

Language resolution is region-aware. `vim-core-rs` must not assume that a
buffer has exactly one syntax language. It resolves the root document language
and zero or more embedded region languages through the versioned language
package registry. Vim `filetype`, buffer name, optional host hints, and
Markdown info strings are inputs to this resolver, not final authority.

This is required for Markdown and similar container formats. A Markdown buffer
may have a Markdown root language while fenced regions resolve to TypeScript,
TSX, Mermaid, drawio, SVG, PNG, Rust, or other registered syntax, diagram,
media, or unknown block kinds.

Tree-sitter extraction will use a crate-owned buffer revision model:

- `vim-core-rs` owns the public concept, named `source_revision` or
  `buffer_revision`.
- The implementation may use Vim's buffer-local changed tick as an input.
- Public APIs must not expose Vim's internal `CHANGEDTICK` name or semantics
  directly.
- Cache keys must be buffer-local and include the source revision.

Both buffer snapshots and Tree-sitter extraction results must expose the
source revision so hosts can verify freshness:

```text
buffer.source_revision == tree_sitter_result.source_revision
```

When the revisions differ, the host must treat the extraction result as stale
and either discard it, request fresh data, or render without that layer.

Tree-sitter extraction results must also report parse and coverage state. The
MVP must include:

- `source_revision`
- `language_id`
- `package_id`
- `package_version`
- `parser_version`
- `query_version`
- `parse_status`
- `has_error`
- data-only byte-range chunks

The default public API remains the existing Vim-derived surface. When
`tree-sitter-syntax` is disabled, Tree-sitter APIs and types are not compiled
into the public surface.

For Markdown fenced blocks, the MVP may also expose embedded block records:

- block range
- content range
- embedded region source
- raw info string
- normalized info string
- normalized embedded block kind
- optional syntax language ID for syntax blocks

Embedded block records are not rendered output. They are extraction data that
lets the host choose an appropriate presentation path.

Embedded block records are also language-resolution inputs. The resolver may
map an info string such as `ts` to a TypeScript syntax package, `mermaid` to a
diagram block kind, or `svg` to a media block kind. The host must not choose
Tree-sitter grammars or queries directly from those strings.

Embedded region classification uses a two-layer public shape. The first layer
separates syntax, diagram, media, and unknown regions. The second layer carries
the normalized detail for diagram or media regions, such as Mermaid, SVG, or
PNG. Syntax package identity stays in the resolved language result rather than
inside the embedded block kind.

The MVP may detect linked Markdown media in addition to fenced blocks. Linked
`*.drawio.svg` assets are treated as SVG media, not as drawio diagram
rendering. The classifier may retain a drawio SVG flavor for provenance and
future behavior, but the host can render it through the same SVG media path as
plain linked SVG. Inline SVG, raw `.drawio` XML, drawio-specific rendering,
and HTML embedded media are follow-up issues.

The TypeScript, TSX, and injection phases must extend the result model with
coverage and budget details, such as:

- covered ranges
- error ranges
- budget status
- partial-result status

Tree-sitter captures may overlap internally, but the standard public
rendering surface must return normalized, non-overlapping chunks.
`vim-core-rs` owns capture overlap resolution, range splitting, priority, and
normalization. Hosts must not need Tree-sitter overlap semantics to render the
standard output.

The internal implementation may keep raw capture spans or a capture graph.
Those raw captures are not part of the MVP public host API. If they become
necessary later, they must be exposed through a separate diagnostic or debug
surface rather than the normal rendering contract.

Capture priority is defined by the versioned language package. Built-in and
registered packages should declare explicit capture priorities. If a capture
does not have an explicit priority, the implementation may fall back to query
order as a deterministic tie-breaker.

Each public chunk must include both normalized semantic data and provenance:

- the normalized text range
- the original capture name
- the normalized syntax category
- zero or more syntax modifiers

The normalized category and modifiers form the primary rendering contract.
The capture name is retained for provenance, debugging, and optional advanced
styling. Standard host rendering should not require raw capture taxonomy
knowledge.

Tree-sitter preparation uses a request/response model. The public design must
support delayed, budgeted, or background preparation even if the first
implementation completes requests synchronously in the same thread.

Background parsing, when introduced, must not touch the embedded Vim runtime.
Workers may receive immutable text snapshots, language package metadata,
query data, and request budgets. Prepared results are returned with source
revision metadata and then committed to the session-owned cache.

`VimCoreSession` owns an immutable text snapshot store for syntax preparation.
Snapshots are keyed by `(buffer_id, source_revision)`. Request handling must
resolve the target source revision and either reuse an existing immutable
snapshot or create one from the embedded Vim buffer before handing work to a
parser or worker. Workers must never read mutable Vim buffer state directly.

Snapshot retention uses pinned in-flight snapshots plus bounded completed
snapshot caching. In-flight snapshots are pinned and cannot be evicted.
Completed snapshots may be retained for the latest revisions per buffer and
within a global byte budget. A per-snapshot size guard may reject or skip
syntax preparation for very large buffers. Eviction must only remove unpinned
snapshots, and eviction must never make stale or missing data appear fresh.

Neovim's Tree-sitter `LanguageTree` is useful prior art for buffer-local
parser/tree ownership and region-aware parsing, but `vim-core-rs` must not
copy a mutable buffer-attached source model into background workers. The
worker boundary is immutable text snapshot data plus package/query metadata.

## Target shape

The exact Rust names may change during implementation, but the public shape
must preserve this meaning:

```rust
pub struct CoreBufferRevision {
    pub value: u64,
}

pub struct CoreBufferInfo {
    pub id: i32,
    // existing fields...
    pub source_revision: CoreBufferRevision,
}

pub struct CoreTreeSitterRangeSyntax {
    pub buffer_id: i32,
    pub source_revision: CoreBufferRevision,
    pub language_id: String,
    pub package_id: String,
    pub package_version: String,
    pub parser_version: String,
    pub query_version: String,
    pub parse_status: CoreTreeSitterParseStatus,
    pub has_error: bool,
    pub chunks: Vec<CoreTreeSitterChunk>,
}

pub struct CoreTreeSitterChunk {
    pub range: CoreTextRange,
    pub capture_name: String,
    pub category: CoreSyntaxCategory,
    pub modifiers: Vec<CoreSyntaxModifier>,
}

pub enum CoreLanguageResolutionScope {
    Document,
    EmbeddedRegion {
        range: CoreTextRange,
        info_string: Option<String>,
    },
}

pub struct CoreResolvedLanguage {
    pub region: CoreTextRange,
    pub role: CoreLanguageRole,
    pub language_id: Option<String>,
    pub package_id: Option<String>,
    pub package_version: Option<String>,
    pub kind: CoreEmbeddedBlockKind,
    pub confidence: CoreResolutionConfidence,
    pub source: CoreLanguageResolutionSource,
}

pub struct CoreEmbeddedRegion {
    pub range: CoreTextRange,
    pub content_range: CoreTextRange,
    pub source: CoreEmbeddedRegionSource,
    pub raw_info_string: Option<String>,
    pub normalized_info_string: Option<String>,
    pub kind: CoreEmbeddedBlockKind,
    pub resolved_language: Option<CoreResolvedLanguage>,
}

pub enum CoreEmbeddedBlockKind {
    Syntax,
    Diagram { diagram_kind: CoreDiagramKind },
    Media {
        media_kind: CoreMediaKind,
        flavor: Option<CoreMediaFlavor>,
    },
    Unknown,
}
```

The concrete API may use request, poll, and query methods. It must preserve
this separation:

- request methods may schedule or perform syntax preparation
- poll or drain methods may return completed preparation results
- range query methods read committed cache state and must not perform heavy
  parsing

The first implementation may complete requests synchronously, but the public
contract must not require drawing code to perform parsing.

## Cache and invalidation model

Tree-sitter parsing must not run in the host's draw hot path.

`VimCoreSession` owns:

- parser selection,
- region-aware language resolution,
- embedded region classification,
- immutable text snapshot storage,
- grammar registration,
- query assets,
- versioned language packages,
- capture mapping,
- parser lifecycle,
- parse cache,
- invalidation, and
- visible range extraction.

The parse cache key must include:

- buffer ID,
- source revision,
- language ID,
- package ID,
- package version,
- parser version, and
- query version.

The MVP may use bounded full reparse when the source revision changes. The
architecture must leave room for incremental parsing when edit deltas are
available.

Visible range extraction must query cached parse state. If the cache is stale
or parsing exceeds the configured budget, the result must make that explicit
instead of returning data that appears fresh.

The cache stores normalized chunks for the standard public rendering surface.
It may also keep raw capture spans internally to support normalization,
priority resolution, diagnostics, and future advanced surfaces.

The text snapshot store follows these retention rules:

- Pin snapshots referenced by queued or running syntax preparation requests.
- Keep a bounded number of completed revisions per buffer.
- Enforce a global byte budget for unpinned snapshots.
- Optionally enforce a per-snapshot byte limit for very large buffers.
- Evict only unpinned snapshots, preferring least-recently-used completed
  revisions.
- Report explicit budget or snapshot-too-large status when preparation cannot
  obtain a valid snapshot under policy.

The language package registry registers only packages whose Cargo features are
enabled. If a request names a language or package that is not available in the
current build, the result must report an explicit unavailable or unsupported
status rather than silently falling back to another package.

## Host integration contract

Hosts such as `saya` may rely on the following contract:

- `get_line_syntax()` remains the Vim-derived syntax extraction API.
- Tree-sitter extraction is an optional, separate layer.
- Hosts compare buffer and syntax `source_revision` values before rendering.
- Hosts render only data-only byte ranges and capture names.
- Hosts render standard output from normalized, non-overlapping chunks.
- Hosts use normalized categories and modifiers as the primary rendering
  contract.
- Hosts may use capture names for optional advanced styling, but they must not
  depend on raw capture overlap semantics for normal rendering.
- Hosts own projection into display space and final styling.
- Hosts do not own grammar, query, capture mapping, or cache invalidation.
- Hosts do not inject raw grammars or raw queries into the extraction path.
- Hosts may provide language hints, but the registry-backed resolver in
  `vim-core-rs` owns the final root and embedded-region language resolution.
- Markdown WYSIWYG metadata remains presentation metadata, separate from
  syntax extraction.
- Vim conceal, Tree-sitter metadata, and Markdown presentation metadata are
  separate concepts.
- Markdown fenced block detection is data-only extraction.
- Markdown image or media link detection may also be data-only extraction.
- Mermaid, drawio, SVG, PNG, and other embedded media rendering remain
  host-owned presentation concerns and must be tracked as separate integration
  issues.
- Linked `*.drawio.svg` assets are classified as SVG media with optional drawio
  SVG flavor, not as drawio diagram rendering.

## Consequences

This decision has the following positive effects:

- It preserves the existing `saya` contract for Vim-derived syntax chunks.
- It prevents Tree-sitter captures from being confused with Vim syntax IDs.
- It lets hosts reject stale highlighting deterministically.
- It gives large files and incomplete TypeScript code an explicit degraded
  state instead of silently broken highlighting.
- It creates a shared revision model for future annotations, diagnostics,
  semantic tokens, and TypeScript extension surfaces.
- It gives future TypeScript extensions a controlled registration path without
  moving raw query ownership into `saya`.
- It creates a safe entry point for Markdown embedded content without treating
  diagrams or media as Tree-sitter syntax.
- It avoids a buffer-level-only language model that would fail for Markdown
  fenced code and other embedded regions.
- It lets `saya` route Mermaid, SVG, PNG, and linked drawio SVG to distinct
  presentation paths without parsing raw info strings as compatibility logic.
- It keeps capture overlap and priority semantics inside the engine.
- It gives hosts a stable category/modifier interface without dropping capture
  provenance.
- It keeps syntax preparation outside the draw hot path and leaves room for
  background parsing.
- It prevents background workers from observing mutable Vim buffer state.
- It bounds memory growth from immutable text snapshots while preserving
  correctness for in-flight requests.
- It keeps the default dependency graph and public API stable for hosts that
  do not opt into experimental Tree-sitter extraction.
- It lets users opt into Markdown, Rust, TypeScript, and future packages
  independently instead of forcing all grammars into one feature.

This decision also has explicit costs:

- The public API grows before Tree-sitter parsing itself is implemented.
- Buffer snapshot contracts need a source revision field.
- Tree-sitter results need status and provenance fields.
- Parser/query versioning becomes part of the contract.
- A language package registry must be designed before third-party extension
  support.
- Markdown embedded block detection needs its own contract tests.
- Region-aware language resolution needs tests for root documents, fenced
  syntax blocks, diagram blocks, media blocks, unknown info strings, and host
  hint conflicts.
- Embedded region classification needs tests for fenced blocks, Markdown media
  links, linked SVG, linked PNG, and linked `*.drawio.svg` assets.
- Tests must cover freshness, stale results, and degraded parse states, not
  only chunk ranges.
- Normalization and priority rules must be tested for overlapping captures.
- Request lifecycle tests must cover queued, completed, stale, and budgeted
  preparation states.
- Snapshot store tests must cover pinning, unpinned eviction, latest revision
  retention, global byte budget enforcement, and per-snapshot size limits.
- Feature-gated tests must cover the default no-Tree-sitter build and the
  experimental build with selected language package features enabled.

## Rejected alternatives

The repository rejects the following alternatives:

- Route Tree-sitter output through `CoreSyntaxChunk`.
  This makes one API carry two incompatible meanings and risks breaking host
  assumptions about Vim-derived syntax.
- Use session-wide snapshot revision as the cache key.
  This is too coarse for multi-buffer syntax caches and can cause unnecessary
  reparsing after non-text state changes.
- Expose Vim `CHANGEDTICK` directly as the public contract.
  The crate may use it internally, but the public model must remain owned by
  `vim-core-rs`.
- Keep source revision internal only.
  Hosts need to verify whether delayed or cached syntax data matches the
  displayed buffer text.
- Return chunks without parse or freshness status.
  That repeats the class of failures where stale or partial highlight output
  appears fresh to the renderer.
- Return raw overlapping captures as the standard host API.
  That would force hosts to own extraction semantics and priority rules.
- Return only normalized categories without capture provenance.
  That would make debugging, package evolution, and advanced styling harder.
- Let draw-time query APIs perform heavy parsing.
  That would reintroduce parser work into the host rendering hot path.
- Let parser workers read from the embedded Vim buffer directly.
  That would violate the `VimCoreSession` ownership model and make
  `source_revision` freshness unverifiable.
- Keep every immutable text snapshot indefinitely.
  That would make large Markdown and TypeScript files a memory growth risk.
- Keep only the latest buffer snapshot without pinning in-flight requests.
  That could invalidate work that is still preparing against an older source
  revision.
- Include Tree-sitter runtime and grammar crates in the default build.
  That would impose experimental dependency, build-time, and binary-size costs
  on hosts that only need the existing Vim-derived extraction surface.
- Put all built-in language packages behind only one root feature.
  That would make Markdown, Rust, TypeScript, and future grammars an
  all-or-nothing dependency choice.
- Split runtime, built-in packages, and external packages into a fully generic
  package system in the MVP.
  That is the likely long-term direction, but it needs package trust,
  versioning, ABI, loading, and lifecycle policies that are not part of the
  first implementation.
- Adopt Neovim-style runtimepath query ownership for the MVP.
  That would make extraction results environment-dependent and harder to
  contract-test.
- Let `saya` pass raw Tree-sitter queries or grammars.
  That would move extraction semantics out of `vim-core-rs` and recreate a
  host-side compatibility layer.
- Treat buffer-level Vim `filetype` as the only language selector.
  That cannot support Markdown fenced code blocks, injected TypeScript, or
  embedded media without later redesign.
- Let hosts resolve fenced code info strings directly into syntax packages.
  That would move language compatibility and alias semantics out of
  `vim-core-rs`.
- Treat Mermaid, drawio, SVG, or PNG as Tree-sitter syntax injection.
  These are embedded document or media presentation concerns. They need
  separate contracts from syntax highlighting.
- Treat linked `*.drawio.svg` as requiring drawio-specific rendering.
  The MVP can classify it as SVG media and preserve drawio flavor as
  provenance. Raw drawio XML and drawio-specific rendering belong in follow-up
  issues.
- Make hosts interpret raw fenced-block info strings or media filenames as the
  primary classification contract.
  Hosts must use the normalized embedded region kind and resolved language
  data instead.
- Implement Markdown fenced TypeScript injection in the MVP.
  The MVP must first establish block detection, freshness, parser cache, and
  package versioning. Bounded syntax injection can build on that foundation in
  a later issue.

## Implementation notes

Implementation is expected to proceed in phases:

1. Add the source revision model and documentation contracts.
2. Add the `tree-sitter-syntax` root Cargo feature.
3. Add per-language package features for Markdown and Rust.
4. Add feature-gated Tree-sitter public types with provenance and parse
   status.
5. Add the versioned built-in language package registry.
6. Add the region-aware language resolver for root documents and embedded
   regions.
7. Add Markdown and Rust parser/query support behind package features.
8. Add Markdown fenced block detection with data-only embedded region records.
9. Add parse cache invalidation keyed by buffer source revision.
10. Add visible range extraction from cached parse state.
11. Add capture priority and normalization tests for overlapping captures.
12. Add request, poll, and cache-query APIs for syntax preparation.
13. Add immutable text snapshot store policy, including in-flight pinning,
    latest revision retention, global byte budget, and per-snapshot size
    guard.
14. Add linked SVG and PNG media detection, including linked `*.drawio.svg` as
    SVG media with drawio SVG flavor.
15. Add TypeScript and TSX support behind package features.
16. Add bounded syntax injection support with coverage and budget reporting.
17. Track Mermaid rendering, linked SVG and PNG rendering, raw drawio XML,
    inline SVG, and HTML embedded media as separate host integration issues.

## Status note

This ADR is accepted as the design target for Tree-sitter extraction. It does
not require immediate parser implementation, but future implementation work
must preserve the separation between Vim-derived syntax extraction,
Tree-sitter extraction, and host-owned presentation.
