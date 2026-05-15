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
- `src/presentation/screen_model.rs` maps Markdown metadata to semantic presentation ranges.
- `src/presentation/render/renderer.rs` renders already-resolved styles according to terminal
  capability.

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

Headless tests assert screen model and rendered span styles. They don't require
a live terminal unless the behavior under test is terminal capability
negotiation.

## Next steps

Before implementing this API, update the startup type declaration and add
tests that describe the registry output. Then add the resolver and rendering
integration behind that tested contract.
