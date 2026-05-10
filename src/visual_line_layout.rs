//! 1 行分の「raw byte ↔ 画面表示列」を支配する単一の真実。
//!
//! セマンティクスは Vim 由来:
//!
//! - タブはコンテンツの先頭（col 0）からの絶対 `tab_size` 起算で次の tab stop まで展開する。
//! - ガター（行番号など）は表示の最終結果に加算するだけのオフセットであり、
//!   タブ stop 計算には介入しない。
//!
//! この設計により、レンダリング側とカーソル算出側がそれぞれ独自に
//! `display_width(line[..N], tab_size)` を再計算してずれる、という旧来の
//! 構造的バグを排除する。raw↔display 変換は本モジュールの値型のメソッド
//! のみが正解を返す。

use std::ops::Range;

use unicode_width::UnicodeWidthChar;

/// 行内のバイトオフセット（UTF-8 のバイト境界）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct RawByteCol(pub usize);

impl RawByteCol {
    pub const ZERO: RawByteCol = RawByteCol(0);

    pub fn get(self) -> usize {
        self.0
    }
}

impl From<usize> for RawByteCol {
    fn from(value: usize) -> Self {
        RawByteCol(value)
    }
}

/// ガターを除いた「コンテンツ部分」の表示列（0 = ガター直後の最初のセル）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct ContentDisplayCol(pub u16);

impl ContentDisplayCol {
    pub const ZERO: ContentDisplayCol = ContentDisplayCol(0);

    pub fn get(self) -> u16 {
        self.0
    }
}

/// ガターを含む画面全体の絶対表示列。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct ScreenDisplayCol(pub u16);

impl ScreenDisplayCol {
    pub const ZERO: ScreenDisplayCol = ScreenDisplayCol(0);

    pub fn get(self) -> u16 {
        self.0
    }
}

/// 1 セル: raw bytes の連続範囲を、コンテンツ列の連続範囲へ写像する。
///
/// - タブの場合は `raw` 幅 1、`content_display` 幅は次 tab stop までの距離。
/// - 通常文字は `unicode_width` のセル幅（半角=1, 全角=2）。
/// - 0 幅文字（結合用文字など）は `content_display` が空 Range（start == end）のセルになる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutCell {
    pub raw: Range<RawByteCol>,
    pub content_display: Range<ContentDisplayCol>,
}

impl LayoutCell {
    pub fn raw_start(&self) -> RawByteCol {
        self.raw.start
    }

    pub fn raw_end(&self) -> RawByteCol {
        self.raw.end
    }

    pub fn content_start(&self) -> ContentDisplayCol {
        self.content_display.start
    }

    pub fn content_end(&self) -> ContentDisplayCol {
        self.content_display.end
    }

    pub fn content_width(&self) -> u16 {
        self.content_display
            .end
            .0
            .saturating_sub(self.content_display.start.0)
    }
}

/// 1 行分の raw text と画面表示の対応を保持する不変値型。
///
/// `display_text` はガターを含まない、タブを空白に展開済みのコンテンツ。
/// `gutter_width` を加えれば任意のセルの絶対画面列が得られる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisualLineLayout {
    raw_text: String,
    display_text: String,
    cells: Vec<LayoutCell>,
    content_width: u16,
    gutter_width: u16,
    tab_size: u16,
}

impl VisualLineLayout {
    /// 行を解析して layout を構築する。
    ///
    /// `tab_size == 0` は 1 にクランプする（防御的）。`gutter_width` は
    /// セル写像には影響せず、`raw_to_screen` 等で結果に加算されるだけ。
    pub fn build(raw_text: &str, tab_size: u16, gutter_width: u16) -> Self {
        let tab_size = tab_size.max(1);
        let mut cells = Vec::with_capacity(raw_text.len());
        let mut display_text = String::with_capacity(raw_text.len());
        let mut content_col: u16 = 0;
        let mut byte_offset: usize = 0;

        for ch in raw_text.chars() {
            let raw_start = RawByteCol(byte_offset);
            let raw_end = RawByteCol(byte_offset + ch.len_utf8());
            let cell_width: u16 = if ch == '\t' {
                let remainder = content_col % tab_size;
                let advance = tab_size - remainder;
                let advance = advance.min(tab_size).max(1);
                display_text.extend(std::iter::repeat_n(' ', usize::from(advance)));
                advance
            } else {
                let width = u16::try_from(UnicodeWidthChar::width(ch).unwrap_or(0))
                    .unwrap_or(0);
                display_text.push(ch);
                width
            };
            let content_start = ContentDisplayCol(content_col);
            let content_end = ContentDisplayCol(content_col.saturating_add(cell_width));
            cells.push(LayoutCell {
                raw: raw_start..raw_end,
                content_display: content_start..content_end,
            });
            content_col = content_col.saturating_add(cell_width);
            byte_offset = raw_end.0;
        }

        log::debug!(
            "[visual_line_layout] built: raw_len={}, display_len={}, cells={}, content_width={}, gutter_width={}, tab_size={}",
            raw_text.len(),
            display_text.len(),
            cells.len(),
            content_col,
            gutter_width,
            tab_size,
        );

        VisualLineLayout {
            raw_text: raw_text.to_string(),
            display_text,
            cells,
            content_width: content_col,
            gutter_width,
            tab_size,
        }
    }

    pub fn raw_text(&self) -> &str {
        &self.raw_text
    }

    /// タブを空白に展開済みのコンテンツ（ガター含まず）。
    pub fn display_text(&self) -> &str {
        &self.display_text
    }

    pub fn cells(&self) -> &[LayoutCell] {
        &self.cells
    }

    pub fn gutter_width(&self) -> u16 {
        self.gutter_width
    }

    pub fn tab_size(&self) -> u16 {
        self.tab_size
    }

    /// コンテンツ部分の総表示幅（ガター含まず）。
    pub fn content_width(&self) -> ContentDisplayCol {
        ContentDisplayCol(self.content_width)
    }

    /// ガターを含む画面上の総表示幅。
    pub fn screen_width(&self) -> ScreenDisplayCol {
        ScreenDisplayCol(self.gutter_width.saturating_add(self.content_width))
    }

    /// raw バイト列の指定位置がコンテンツ列で何処に投影されるか。
    ///
    /// - `raw_col == 0` の時は常にコンテンツ列 0。
    /// - 行末（`raw_text.len()`）の時はコンテンツ末尾。
    /// - セル境界に乗らない位置（マルチバイト文字の途中など）は、その文字の
    ///   セル開始へ寄せる（Vim の `clamp_to_char_boundary` と同じ意図）。
    pub fn raw_to_content(&self, raw_col: RawByteCol) -> ContentDisplayCol {
        let raw_col = raw_col.0.min(self.raw_text.len());
        if raw_col == 0 {
            return ContentDisplayCol::ZERO;
        }
        if raw_col >= self.raw_text.len() {
            return ContentDisplayCol(self.content_width);
        }
        if let Some(cell) = self.cells.iter().find(|cell| cell.raw.start.0 == raw_col) {
            return cell.content_start();
        }
        // 文字境界の中間に raw_col が落ちた場合は、含むセルの開始へ寄せる。
        if let Some(cell) = self
            .cells
            .iter()
            .find(|cell| cell.raw.start.0 < raw_col && raw_col < cell.raw.end.0)
        {
            log::debug!(
                "[visual_line_layout] raw_to_content: raw_col {} fell mid-cell, snapping to cell start ({}, content {})",
                raw_col,
                cell.raw.start.0,
                cell.content_start().0
            );
            return cell.content_start();
        }
        // どこにも当てはまらない（理論上ありえない）場合は末尾扱い。
        ContentDisplayCol(self.content_width)
    }

    /// raw バイト列の指定位置の絶対画面列（ガター込み）。
    pub fn raw_to_screen(&self, raw_col: RawByteCol) -> ScreenDisplayCol {
        let content = self.raw_to_content(raw_col);
        ScreenDisplayCol(self.gutter_width.saturating_add(content.0))
    }

    /// 画面列が指す raw バイト位置を返す。ガターより左の場合は `None`。
    pub fn screen_to_raw(&self, screen_col: ScreenDisplayCol) -> Option<RawByteCol> {
        if screen_col.0 < self.gutter_width {
            return None;
        }
        let content_col = screen_col.0 - self.gutter_width;
        if let Some(cell) = self
            .cells
            .iter()
            .find(|cell| {
                cell.content_display.start.0 <= content_col
                    && (content_col < cell.content_display.end.0
                        || (cell.content_width() == 0
                            && cell.content_display.start.0 == content_col))
            })
        {
            return Some(cell.raw_start());
        }
        // コンテンツ末尾以降は raw 末尾を指す。
        Some(RawByteCol(self.raw_text.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(value: usize) -> RawByteCol {
        RawByteCol(value)
    }

    fn screen(value: u16) -> ScreenDisplayCol {
        ScreenDisplayCol(value)
    }

    #[test]
    fn empty_line_has_zero_widths_and_no_cells() {
        let layout = VisualLineLayout::build("", 8, 5);
        assert_eq!(layout.cells().len(), 0);
        assert_eq!(layout.content_width(), ContentDisplayCol(0));
        assert_eq!(layout.screen_width(), ScreenDisplayCol(5));
        assert_eq!(layout.raw_to_screen(raw(0)), screen(5));
    }

    #[test]
    fn ascii_only_line_maps_each_byte_to_one_cell() {
        let layout = VisualLineLayout::build("hello", 8, 0);
        assert_eq!(layout.cells().len(), 5);
        assert_eq!(layout.display_text(), "hello");
        assert_eq!(layout.content_width(), ContentDisplayCol(5));
        assert_eq!(layout.raw_to_screen(raw(0)), screen(0));
        assert_eq!(layout.raw_to_screen(raw(3)), screen(3));
        assert_eq!(layout.raw_to_screen(raw(5)), screen(5));
    }

    #[test]
    fn leading_tab_uses_full_tab_size_regardless_of_gutter() {
        // 真因の核: ガターは tab stop に介入しない。
        let layout = VisualLineLayout::build("\thello", 8, 5);
        assert_eq!(layout.display_text(), "        hello");
        assert_eq!(layout.content_width(), ContentDisplayCol(13));
        assert_eq!(layout.screen_width(), ScreenDisplayCol(18));
        assert_eq!(
            layout.raw_to_screen(raw(0)),
            screen(5),
            "tab itself starts right after gutter"
        );
        assert_eq!(
            layout.raw_to_screen(raw(1)),
            screen(13),
            "'h' must be at gutter(5) + tab(8) = 13"
        );
        assert_eq!(layout.raw_to_screen(raw(6)), screen(18), "after 'hello'");
    }

    #[test]
    fn mid_line_tab_advances_to_next_tab_stop_in_content_space() {
        // "fn\tmain" with tab_size=4, gutter=5:
        //   f@0, n@1, \t at content_col=2 -> advance to 4 (2 cols), main@4..8
        let layout = VisualLineLayout::build("fn\tmain", 4, 5);
        assert_eq!(layout.display_text(), "fn  main");
        assert_eq!(layout.content_width(), ContentDisplayCol(8));
        assert_eq!(layout.raw_to_screen(raw(2)), screen(7), "tab cell start");
        assert_eq!(
            layout.raw_to_screen(raw(3)),
            screen(9),
            "'m' starts after tab"
        );
        assert_eq!(layout.raw_to_screen(raw(7)), screen(13), "end of line");
    }

    #[test]
    fn full_width_unicode_takes_two_cells_of_display_width() {
        // 「あ」(U+3042) は east-asian wide → 表示幅 2。
        let layout = VisualLineLayout::build("aあb", 8, 0);
        assert_eq!(layout.cells().len(), 3);
        assert_eq!(layout.content_width(), ContentDisplayCol(4));
        assert_eq!(layout.raw_to_screen(raw(0)), screen(0));
        assert_eq!(layout.raw_to_screen(raw(1)), screen(1));
        assert_eq!(
            layout.raw_to_screen(raw(4)),
            screen(3),
            "byte after multibyte char"
        );
    }

    #[test]
    fn raw_to_screen_handles_position_inside_multibyte_char() {
        let layout = VisualLineLayout::build("aあb", 8, 0);
        // "あ" は 3 バイト (1..4)。raw_col=2 はそのバイト中間。
        // セル開始（=1）に寄せるべき。
        assert_eq!(layout.raw_to_screen(raw(2)), screen(1));
        assert_eq!(layout.raw_to_screen(raw(3)), screen(1));
    }

    #[test]
    fn screen_to_raw_round_trips_through_each_cell() {
        let layout = VisualLineLayout::build("\thello", 8, 5);
        // 各セルについて raw_start から raw_to_screen → screen_to_raw が同じ値になる。
        for cell in layout.cells() {
            let screen_col = layout.raw_to_screen(cell.raw_start());
            let recovered = layout.screen_to_raw(screen_col);
            assert_eq!(
                recovered,
                Some(cell.raw_start()),
                "round-trip failed for cell raw={:?}, screen={:?}",
                cell.raw_start(),
                screen_col
            );
        }
    }

    #[test]
    fn screen_to_raw_returns_none_for_columns_inside_gutter() {
        let layout = VisualLineLayout::build("hello", 8, 5);
        assert_eq!(layout.screen_to_raw(screen(0)), None);
        assert_eq!(layout.screen_to_raw(screen(4)), None);
        assert_eq!(layout.screen_to_raw(screen(5)), Some(raw(0)));
    }

    #[test]
    fn screen_to_raw_returns_line_end_for_columns_past_content() {
        let layout = VisualLineLayout::build("hi", 8, 0);
        // content_width=2、それ以降は行末バイト位置を返す。
        assert_eq!(layout.screen_to_raw(screen(2)), Some(raw(2)));
        assert_eq!(layout.screen_to_raw(screen(20)), Some(raw(2)));
    }

    #[test]
    fn tab_size_zero_is_clamped_to_one() {
        let layout = VisualLineLayout::build("\ta", 0, 0);
        // tab=1 では tab stop は常に次セルなので、advance=1。
        assert_eq!(layout.display_text(), " a");
        assert_eq!(layout.content_width(), ContentDisplayCol(2));
    }
}
