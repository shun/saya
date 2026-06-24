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
