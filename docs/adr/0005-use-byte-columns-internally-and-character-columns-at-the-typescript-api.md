# ADR 0005: Use byte columns internally and character columns at the TypeScript API

## Status

Accepted.

## Context

`saya` represents buffer positions with a row and a column in several places:
the cursor position from `vim-core-rs`, search match ranges, visual selection
endpoints, syntax chunk spans, and the display column used to place the
terminal cursor.

A column can be expressed in three different units, and these units are not
interchangeable for multibyte text.

- Byte offset: the UTF-8 byte index inside the line. This is what Vim stores in
  `curwin->w_cursor.col` and what `col()` returns. Neovim's
  `nvim_win_get_cursor()` also returns the column as a byte index.
- Character index: the count of Unicode scalar values. Vim exposes this through
  `charcol()`, which was added later as a secondary convenience and did not
  replace `col()`.
- Display column: the terminal cell position after expanding tabs and wide
  characters. Vim exposes this through `virtcol()`.

Two facts drive the decision.

First, the editing engine and the host language are both byte-native. Vim, and
therefore `vim-core-rs`, stores cursor and range columns as byte offsets. Rust
`&str` is byte-indexed: slicing, `len()`, and `find()` all operate on bytes, so
byte offsets are the natural coordinate for any code that reads line text.

Second, the display column is not a pure function of the buffer. It depends on
`saya`-side presentation configuration: the configured tab size, the
line-number gutter width, and terminal cell widths from `unicode-width`. Per
`docs/architecture.md`, projecting raw buffer text into display cells is a
`saya` presentation responsibility, not a `vim-core-rs` responsibility.

A regression motivated this record. In one `vim-core-rs` build the cursor
column was reported as a character index while search ranges, visual selection,
and syntax spans were still byte offsets. The mixed contract caused the
multibyte cursor to render at the wrong display column, because `saya`'s
projection assumed byte offsets. Standardizing on a single internal unit
prevents that class of failure.

A character or UTF-16 column is still the friendlier unit at the public
TypeScript surface, because JavaScript strings are UTF-16 and plugin authors do
not work in UTF-8 byte offsets. That concern belongs at the outermost API
boundary, not in the internal representation.

## Decision

All internal buffer column coordinates use UTF-8 byte offsets, uniformly.

- `vim-core-rs` reports cursor columns and range columns as byte offsets,
  consistent with `col()` and `nvim_win_get_cursor()` and with its own search,
  selection, and syntax column fields.
- `saya`'s core adapter and presentation layers treat every incoming column as
  a byte offset. They never mix byte and character units in one path.
- `saya` owns the projection from byte offset to display column, using its own
  tab size, line-number gutter, and `unicode-width` data. Display columns stay
  a presentation concern and are not requested from `vim-core-rs`.

The public TypeScript plugin API may expose character or UTF-16 columns. When
it does, the conversion happens explicitly at that boundary, and the internal
representation passed to it remains byte-based.

A cursor column reported to `saya` as a character index is treated as a
`vim-core-rs` regression, not as a contract `saya` should adapt to in its
presentation layer.

## Consequences

`saya` keeps byte-based column handling, including character-boundary clamping
when a raw byte offset must be mapped to a display column. No character-index
shim is added to the core adapter or the presentation layer.

`vim-core-rs` must keep cursor and range columns byte-based so that the cursor,
search overlays, visual selection, and syntax spans share one coordinate
system. If a future change makes the core report character indices, the fix
belongs in `vim-core-rs`, restoring the byte contract.

When the public TypeScript API later needs character or UTF-16 columns, that
work is an explicit conversion at the API boundary. It must not change the
internal byte representation or push display-column computation into
`vim-core-rs`.
