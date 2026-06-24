use super::ViewportState;
use vim_core_rs::CoreWindowInfo;

#[test]
fn keeps_cursor_visible_when_moving_below_viewport() {
    let mut viewport = ViewportState::new();

    viewport.ensure_cursor_visible(5, 3, 10);

    assert_eq!(viewport.top_line(), 3);
}

#[test]
fn moves_viewport_up_when_cursor_moves_above_visible_range() {
    let mut viewport = ViewportState::new();
    viewport.ensure_cursor_visible(6, 3, 10);

    viewport.ensure_cursor_visible(1, 3, 10);

    assert_eq!(viewport.top_line(), 1);
}

#[test]
fn clamps_viewport_when_terminal_becomes_taller() {
    let mut viewport = ViewportState::new();
    viewport.ensure_cursor_visible(8, 3, 10);

    viewport.ensure_cursor_visible(8, 6, 10);

    assert_eq!(viewport.top_line(), 4);
}

#[test]
fn syncs_from_core_topline_using_one_based_coordinates() {
    let mut viewport = ViewportState::new();

    viewport.sync_from_core_topline(11, 4, 100);

    assert_eq!(viewport.top_line(), 10);
    assert_eq!(viewport.bottom_line(), 13);
}

#[test]
fn syncs_from_core_topline_clamping_to_buffer_end() {
    let mut viewport = ViewportState::new();

    viewport.sync_from_core_topline(99, 4, 100);

    assert_eq!(viewport.top_line(), 96);
    assert_eq!(viewport.bottom_line(), 99);
}

#[test]
fn syncs_full_window_viewport_state_from_core_metadata() {
    let mut viewport = ViewportState::new();

    viewport.sync_from_core_window(&CoreWindowInfo {
        id: 7,
        buf_id: 3,
        row: 0,
        col: 0,
        width: 80,
        height: 12,
        topline: 11,
        botline: 22,
        leftcol: 4,
        skipcol: 2,
        cursor_row: 14,
        cursor_col: 9,
        is_active: true,
    });

    assert_eq!(viewport.top_line(), 10);
    assert_eq!(viewport.bottom_line(), 21);
    assert_eq!(viewport.left_col(), 4);
    assert_eq!(viewport.skip_col(), 2);
}
