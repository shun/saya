//! 画面内にカーソルを収めるための viewport 状態。
//!
//! vim-core-rs はバッファ上の絶対カーソル座標を返す。
//! このモジュールでは application 層の責務として、terminal 本文領域に
//! カーソルが常に収まるように viewport の先頭行を管理する。

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ViewportState {
    top_line: usize,
}

impl ViewportState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn top_line(&self) -> usize {
        self.top_line
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

        log::debug!(
            "[viewport] cursor visible: top_line_after={}, max_top_line={}",
            self.top_line,
            max_top_line
        );
    }
}

#[cfg(test)]
mod tests {
    use super::ViewportState;

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
    }

    #[test]
    fn syncs_from_core_topline_clamping_to_buffer_end() {
        let mut viewport = ViewportState::new();

        viewport.sync_from_core_topline(99, 4, 100);

        assert_eq!(viewport.top_line(), 96);
    }
}
