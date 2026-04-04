//! 画面内にカーソルを収めるための viewport 状態。
//!
//! vim-core-rs はバッファ上の絶対カーソル座標を返す。
//! このモジュールでは application 層の責務として、terminal 本文領域に
//! カーソルが常に収まるように viewport の先頭行を管理する。

use std::collections::{BTreeMap, BTreeSet};

use vim_core_rs::CoreWindowInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ViewportState {
    top_line: usize,
    bottom_line: usize,
    left_col: usize,
    skip_col: usize,
}

impl ViewportState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn top_line(&self) -> usize {
        self.top_line
    }

    pub fn bottom_line(&self) -> usize {
        self.bottom_line
    }

    pub fn left_col(&self) -> usize {
        self.left_col
    }

    pub fn skip_col(&self) -> usize {
        self.skip_col
    }

    /// core が返す window.topline (1-based) を viewport に同期する。
    ///
    /// vim 側のスクロール操作は cursor 位置だけでは表現しきれないため、
    /// 画面の先頭行は cursor ではなく core の window state を優先する。
    pub fn sync_from_core_topline(
        &mut self,
        topline: usize,
        body_height: usize,
        total_lines: usize,
    ) {
        let body_height = body_height.max(1);
        let total_lines = total_lines.max(1);
        let max_top_line = total_lines.saturating_sub(body_height);
        let next_top_line = topline.saturating_sub(1).min(max_top_line);

        log::debug!(
            "[viewport] sync from core topline: topline={}, body_height={}, total_lines={}, top_line_before={}, top_line_after={}, max_top_line={}",
            topline,
            body_height,
            total_lines,
            self.top_line,
            next_top_line,
            max_top_line
        );

        self.top_line = next_top_line;
        self.bottom_line = self.top_line.saturating_add(body_height.saturating_sub(1));
    }

    pub fn ensure_cursor_visible(
        &mut self,
        cursor_row: usize,
        body_height: usize,
        total_lines: usize,
    ) {
        let body_height = body_height.max(1);
        let total_lines = total_lines.max(1);
        let max_top_line = total_lines.saturating_sub(body_height);

        log::debug!(
            "[viewport] ensure cursor visible: cursor_row={}, body_height={}, total_lines={}, top_line_before={}",
            cursor_row,
            body_height,
            total_lines,
            self.top_line
        );

        if cursor_row < self.top_line {
            self.top_line = cursor_row;
        } else if cursor_row >= self.top_line.saturating_add(body_height) {
            self.top_line = cursor_row + 1 - body_height;
        }

        self.top_line = self.top_line.min(max_top_line);
        self.bottom_line = self.top_line.saturating_add(body_height.saturating_sub(1));

        log::debug!(
            "[viewport] cursor visible: top_line_after={}, max_top_line={}",
            self.top_line,
            max_top_line
        );
    }

    pub fn sync_from_core_window(&mut self, window: &CoreWindowInfo) {
        self.top_line = window.topline.saturating_sub(1);
        self.bottom_line = window.botline.saturating_sub(1);
        self.left_col = window.leftcol;
        self.skip_col = window.skipcol;
        log::debug!(
            "[viewport] sync from core window: window_id={}, topline={}, botline={}, leftcol={}, skipcol={}",
            window.id,
            window.topline,
            window.botline,
            window.leftcol,
            window.skipcol
        );
    }
}

#[derive(Debug, Clone, Default)]
pub struct WindowViewportStore {
    states: BTreeMap<i32, ViewportState>,
}

impl WindowViewportStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, window_id: i32) -> Option<&ViewportState> {
        self.states.get(&window_id)
    }

    pub fn get_mut_or_default(&mut self, window_id: i32) -> &mut ViewportState {
        self.states.entry(window_id).or_default()
    }

    pub fn sync_from_windows(&mut self, windows: &[CoreWindowInfo]) {
        let live_ids = windows
            .iter()
            .map(|window| window.id)
            .collect::<BTreeSet<_>>();
        self.states
            .retain(|window_id, _| live_ids.contains(window_id));
        for window in windows {
            self.get_mut_or_default(window.id)
                .sync_from_core_window(window);
        }
    }

    pub fn len(&self) -> usize {
        self.states.len()
    }
}

#[cfg(test)]
mod tests {
    use super::{ViewportState, WindowViewportStore};
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

    #[test]
    fn window_viewport_store_keeps_states_isolated_by_window_id() {
        let mut store = WindowViewportStore::new();
        store.sync_from_windows(&[
            CoreWindowInfo {
                id: 1,
                buf_id: 1,
                row: 0,
                col: 0,
                width: 40,
                height: 10,
                topline: 5,
                botline: 14,
                leftcol: 0,
                skipcol: 0,
                cursor_row: 4,
                cursor_col: 0,
                is_active: true,
            },
            CoreWindowInfo {
                id: 2,
                buf_id: 1,
                row: 0,
                col: 40,
                width: 40,
                height: 10,
                topline: 20,
                botline: 29,
                leftcol: 3,
                skipcol: 1,
                cursor_row: 20,
                cursor_col: 2,
                is_active: false,
            },
        ]);

        assert_eq!(store.len(), 2);
        assert_eq!(store.get(1).expect("window 1").top_line(), 4);
        assert_eq!(store.get(2).expect("window 2").top_line(), 19);
        assert_eq!(store.get(2).expect("window 2").left_col(), 3);
    }

    #[test]
    fn window_viewport_store_prunes_closed_windows() {
        let mut store = WindowViewportStore::new();
        store.sync_from_windows(&[
            CoreWindowInfo {
                id: 1,
                buf_id: 1,
                row: 0,
                col: 0,
                width: 40,
                height: 10,
                topline: 1,
                botline: 10,
                leftcol: 0,
                skipcol: 0,
                cursor_row: 0,
                cursor_col: 0,
                is_active: true,
            },
            CoreWindowInfo {
                id: 2,
                buf_id: 1,
                row: 0,
                col: 40,
                width: 40,
                height: 10,
                topline: 1,
                botline: 10,
                leftcol: 0,
                skipcol: 0,
                cursor_row: 0,
                cursor_col: 0,
                is_active: false,
            },
        ]);

        store.sync_from_windows(&[CoreWindowInfo {
            id: 2,
            buf_id: 1,
            row: 0,
            col: 0,
            width: 80,
            height: 10,
            topline: 3,
            botline: 12,
            leftcol: 0,
            skipcol: 0,
            cursor_row: 2,
            cursor_col: 0,
            is_active: true,
        }]);

        assert!(store.get(1).is_none(), "closed window should be pruned");
        assert_eq!(store.get(2).expect("window 2").top_line(), 2);
    }
}
