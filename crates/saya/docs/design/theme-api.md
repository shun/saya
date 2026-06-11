# Theme API design

This page describes the intended theme customization model for Markdown
presentation, color palettes, and future plugin-provided themes in `saya`.
The startup API described here is implemented for user `init.ts`
configuration. Runtime theme mutation and plugin activation remain future
work.

> **Note:** This is a preview design currently under active development.

The goal is to give users and plugins a TypeScript-first theme surface without
adding Vim-compatible `:highlight` tables or renderer-specific plugin hooks.

## Design goals

The theme API must keep customization expressive while preserving the
repository's layer boundaries.

- Keep Markdown presentation styles in the `saya` presentation layer.
- Keep `vim-core-rs` as the owner of syntax and highlight extraction
  semantics.
- Avoid Vim script and Neovim compatibility surfaces.
- Let user config and plugins write to the same theme registry.
- Resolve colors through a palette before rendering.
- Keep terminal capability fallback inside the renderer or presentation
  resolver.

## Non-goals

The theme API must not become a compatibility layer for Vim highlight
behavior.

- It must not expose `:highlight` groups as the public contract.
- It must not implement Vim-compatible highlight resolution rules.
- It must not let plugins mutate the TUI renderer directly.
- It must not make Markdown presentation metadata behave like Vim conceal or
  syntax state.

These constraints follow the existing architecture rule that Markdown WYSIWYG
metadata belongs to `saya`, while syntax extraction and highlight semantics
belong to `vim-core-rs`.

## Public configuration shape

The user-facing API lives under `saya.theme`. The startup API can collect theme
declarations in the same way it collects options, keymaps, commands, and events
today.

The recommended shape separates palette tokens from Markdown style rules.

```ts
saya.theme.palette = {
  accent: "#7aa2f7",
  heading2: "#9ece6a",
  code: "#ff9e64",
  link: "#2ac3de",
  muted: "#565f89",
};

saya.theme.markdown = {
  heading: {
    fg: "accent",
    bold: true,
  },
  heading2: {
    fg: "heading2",
    underline: true,
  },
  inlineCode: {
    fg: "code",
  },
  link: {
    fg: "link",
    underline: true,
  },
};
```

We recommend palette tokens as the default color reference because they make
theme overrides, light and dark variants, and terminal fallback easier to
maintain. Direct color values can be accepted as a convenience when the API
needs a small escape hatch.

```ts
saya.theme.markdown = {
  heading2: {
    fg: "#9ece6a",
    underline: true,
  },
};
```

## Markdown style keys

Markdown style keys describe semantic presentation targets, not parser
implementation details.

The initial key set can stay narrow.

- `heading`
- `heading1`
- `heading2`
- `heading3`
- `heading4`
- `heading5`
- `heading6`
- `inlineCode`
- `link`
- `listMarker`
- `checkboxChecked`
- `checkboxUnchecked`
- `table`
- `fencedCodeBlock`

Level-specific heading keys override the general `heading` style. For example,
`heading2` can add underline while inheriting the color and boldness from
`heading`.

```ts
saya.theme.markdown = {
  heading: {
    fg: "accent",
    bold: true,
  },
  heading2: {
    underline: true,
  },
};
```

## Style attributes

The public style object maps to terminal-friendly attributes while remaining
independent from the renderer implementation.

```ts
type SayaThemeColor = string;

interface SayaTextStyle {
  fg?: SayaThemeColor;
  bg?: SayaThemeColor;
  bold?: boolean;
  italic?: boolean;
  underline?: boolean;
  strikethrough?: boolean;
}
```

The resolver treats missing attributes as inheritance. Boolean attributes use
explicit values, so `bold: false` on `heading2` disables inherited
`heading.bold`, while omitting `bold` keeps the inherited value.

## Language-aware syntax styles

Syntax styling must stay semantic and language-aware instead of becoming a
Vim-compatible `:highlight` layer. The public theme surface uses broad syntax
keys globally, then lets a language override only the keys that differ.

```ts
saya.theme.syntax = {
  comment: {
    fg: "muted",
    italic: true,
  },
  string: {
    fg: "green",
  },
  function: {
    fg: "purple",
  },
  statement: {
    fg: "accent",
  },
};

saya.theme.languages = {
  go: {
    syntax: {
      function: {
        fg: "#7aa2f7",
        bold: true,
      },
      string: {
        fg: "#9ece6a",
      },
    },
  },
  rust: {
    syntax: {
      type: {
        fg: "#2ac3de",
      },
    },
  },
};
```

The language key describes the syntax language for a highlighted range, not
only the buffer-level Vim `filetype`. A normal `main.go` buffer can resolve to
the `go` language, while a Markdown fenced code block can independently resolve
its info string to the same language.

````markdown
```go
func hoge() {
  log.Println("hoge")
}
```
````

For that Markdown example, the fenced block contributes Markdown structure,
while the code lines contribute embedded `go` syntax ranges. The renderer first
applies the fenced block presentation, then applies language-specific syntax
for the code tokens. A missing `saya.theme.languages.go.syntax.string` entry
falls back to `saya.theme.syntax.string`.

The initial public language map must stay optional. Themes that only define
`saya.theme.syntax` continue to style all languages through the global semantic
keys.

## Filer styles

Dired and future filer views are not syntax languages. They describe host-side
filesystem presentation metadata, so their styles belong to a separate filer
surface instead of `saya.theme.syntax`.

```ts
saya.theme.filer = {
  directory: {
    fg: "accent",
    bold: true,
  },
  file: {
    fg: "fg",
  },
  symlink: {
    fg: "purple",
  },
  other: {
    fg: "red",
  },
  marked: {
    bg: "#33467c",
  },
};
```

The directory buffer state already tracks entry kind as host metadata. The
screen model can project that metadata into filer style ranges without parsing
the rendered listing text. This keeps filesystem UI policy in `saya` and keeps
syntax extraction in `vim-core-rs`.

## Highlight resolution model

The renderer must receive normalized highlight spans and resolved theme data.
It must not perform palette lookup, language alias resolution, or Vim-style
highlight group resolution on every cell.

The internal model can use a shape like this.

```rust
struct HighlightSpan {
    raw_range: TextRange,
    source: HighlightSource,
    semantic_key: SemanticStyleKey,
    language: Option<LanguageId>,
    priority: HighlightPriority,
}
```

`source` separates syntax, Markdown, filer, search, selection, and other
presentation sources. `language` is present only when a span comes from a
language-aware syntax source. `LanguageId` must be normalized and interned
before the render path, so the draw loop does not repeatedly compare language
strings.

Style resolution follows this order.

1. Resolve transient UI overlays, such as selection, search, prompt, and
   cursor-related styling.
2. Resolve surface state overlays, such as filer marks and active rows.
3. Resolve Markdown structural presentation, such as headings, links, and
   fenced code block backgrounds.
4. Resolve language-specific syntax, such as
   `saya.theme.languages.go.syntax.function`.
5. Resolve global syntax, such as `saya.theme.syntax.function`.
6. Resolve the base UI text style.

Markdown fenced code blocks use two layers from this order. The code block
background comes from Markdown structural presentation, and the token colors
come from language-specific or global syntax styles.

## Performance model

Language-aware overrides are designed to keep the render path cheap. The costly
work is syntax extraction and raw-to-display projection, not the final theme
lookup.

The implementation must use these performance rules.

- Resolve palette tokens, language aliases, inheritance, and fallback chains
  when the theme changes, not during every draw.
- Collect syntax only for visible ranges and reuse existing
  `vim-core-rs` syntax data where possible.
- Cache Markdown fenced block metadata by buffer, revision, block range, and
  language.
- Prepare embedded language syntax only for visible fenced blocks, with
  bounded lookahead if prefetching becomes necessary.
- Keep raw highlight spans separate from display-space spans, and cache
  projection results by buffer revision and viewport.
- Intern language identifiers before highlight resolution.
- Merge overlays in priority order once per projected line instead of
  repeatedly splitting spans for each highlight source.

This makes `saya.theme.languages` a deterministic style lookup after extraction
and projection have already happened.

## Alternatives considered

The theme model intentionally avoids two simpler-looking alternatives because
they weaken long-term boundaries.

Vim-compatible `:highlight` groups would be familiar to Vim users and could
express very fine-grained themes. They also require highlight links, group
resolution, colorscheme ordering, runtime file loading, and compatibility rules
that this repository explicitly keeps out of `saya`. That approach risks moving
syntax semantics and highlight resolution from `vim-core-rs` into the host
application.

Buffer-level `saya.theme.syntaxByFiletype` would be easier to implement than
full Vim compatibility. It still treats the whole buffer as one language and
doesn't naturally cover Markdown fenced code blocks, future embedded languages,
or mixed-language preview surfaces. It also leaves dired/filer presentation
without a clear home.

Language-aware semantic styles preserve the existing architecture boundary:
`vim-core-rs` extracts syntax data, Markdown parsing identifies embedded code
ranges, filer metadata stays in the host layer, and `saya` resolves and renders
presentation styles.

## Palette resolution

The renderer must not receive unresolved palette names. The theme resolver
normalizes user and plugin declarations into a resolved presentation theme
before rendering.

```text
init.ts or plugin theme
  -> ThemeRegistry
  -> resolved theme
  -> Markdown presentation style
  -> TUI renderer
```

Resolution follows this order for each color reference.

1. Resolve a palette token such as `"accent"` to a concrete color.
2. Accept a direct color value such as `"#7aa2f7"` when supported.
3. Fall back to the inherited style or default style when the reference is
   unknown.
4. Map the resolved color to the active terminal text capability.

The terminal fallback step belongs after palette resolution. For example, a
truecolor terminal can render `"#7aa2f7"` directly, while an ANSI-only terminal
can map the same resolved color to the nearest supported ANSI color.

## Implemented scope

The first implementation covers startup configuration and renderer-ready
Markdown presentation styles.

- `saya.theme.palette` collects named color tokens during startup.
- `saya.theme.markdown` collects Markdown semantic style declarations.
- Palette tokens and direct `#rrggbb` color values resolve before rendering.
- Unknown palette tokens fall back by dropping only that unresolved color.
- `heading1` through `heading6` inherit from `heading` and override concrete
  attributes that they declare.
- Plain text terminal mode preserves projected text and removes style.

The language-aware syntax and filer style surfaces are future design work. They
must update the startup declarations, theme resolver, screen model projection,
renderer, and public API documentation together when implemented.

## Plugin-provided themes

Plugins use the same theme API as user config. A plugin can register a theme
preset, set palette tokens, and set Markdown style rules, but it must not call
renderer-specific functions.

```ts
export function activate(ctx) {
  ctx.theme.register("tokyo-night", {
    palette: {
      accent: "#7aa2f7",
      heading2: "#9ece6a",
      code: "#ff9e64",
      link: "#2ac3de",
    },
    markdown: {
      heading: {
        fg: "accent",
        bold: true,
      },
      heading2: {
        fg: "heading2",
        underline: true,
      },
      inlineCode: {
        fg: "code",
      },
      link: {
        fg: "link",
        underline: true,
      },
    },
  });
}
```

User config can select the plugin theme and override only the parts that differ
from the preset.

```ts
saya.theme.use("tokyo-night");

saya.theme.markdown.heading2 = {
  fg: "accent",
  underline: false,
  bold: true,
};
```

The registry makes plugin and user declarations converge into one resolved
theme.

```text
default theme
  < plugin theme
  < user init.ts override
  < runtime command override
```

Runtime overrides are listed as the highest-priority layer because a later
runtime command may intentionally change the active theme. The first
implementation can omit runtime theme mutation if startup-only theme
declaration is enough for the MVP.

## Ownership model

Theme support is split across the same layers used by the rest of the
application.

- The TypeScript layer defines the public `saya.theme` declaration surface.
- The startup registry stores normalized theme declarations.
- The application layer owns `ThemeRegistry` and resolved presentation styles.
- `src/presentation/screen_model/` maps Markdown metadata to semantic presentation ranges.
- `src/presentation/render/renderer/` renders already-resolved styles according to terminal
  capability.
- `vim-core-rs` remains responsible for syntax extraction, including any
  future embedded-language extraction contract.
- Filer metadata remains host-owned and projects into filer style ranges rather
  than syntax style ranges.

This model keeps plugins and config away from renderer internals and keeps
theme choices out of `vim-core-rs`.

## Testing strategy

Theme support is tested at the presentation boundary instead of duplicating
syntax extraction tests from `vim-core-rs`.

- Test that startup config collects palette and Markdown style declarations.
- Test that user overrides take precedence over plugin presets.
- Test that `heading2` overrides `heading` for level-two headings.
- Test that unresolved palette tokens fall back deterministically.
- Test that truecolor and ANSI terminal capabilities receive the expected
  resolved styles.
- Test that plain text capability removes styling without changing text
  projection.
- Test that language-specific syntax styles override global syntax styles
  without changing extracted syntax ranges.
- Test that Markdown fenced code blocks can apply Markdown block styling and
  embedded language token styling together.
- Test that filer entry kind styles come from host metadata, not rendered text
  parsing.
- Test that repeated renders reuse cached theme and projection data instead of
  recomputing palette and language fallback chains.

Headless tests assert screen model and rendered span styles. They don't require
a live terminal unless the behavior under test is terminal capability
negotiation.

## Next steps

Before implementing this API, update the startup type declaration and add
tests that describe the registry output. Then add the resolver and rendering
integration behind that tested contract.
