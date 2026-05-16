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
    last_cursor_row: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewportSyncMode {
    #[default]
    Core,
    SmoothLineMotion,
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
        self.last_cursor_row = Some(window.cursor_row);
        log::debug!(
            "[viewport] sync from core window: window_id={}, topline={}, botline={}, leftcol={}, skipcol={}",
            window.id,
            window.topline,
            window.botline,
            window.leftcol,
            window.skipcol
        );
    }

    pub fn sync_active_cursor_movement_from_core_window(
        &mut self,
        window: &CoreWindowInfo,
        total_lines: usize,
    ) {
        let body_height = window.height.saturating_sub(1).max(1);
        let previous_cursor_row = self.last_cursor_row;
        let cursor_moved = previous_cursor_row.is_some_and(|row| row != window.cursor_row);

        self.left_col = window.leftcol;
        self.skip_col = window.skipcol;

        if !cursor_moved {
            self.sync_from_core_window(window);
            return;
        }

        let total_lines = total_lines.max(1);
        let core_top_line = window.topline.saturating_sub(1);
        if core_top_line == self.top_line {
            log::debug!(
                "[viewport] sync active cursor movement from stable core topline: window_id={}, cursor_row={}, previous_cursor_row={:?}, body_height={}, core_topline={}, top_line={}",
                window.id,
                window.cursor_row,
                previous_cursor_row,
                body_height,
                window.topline,
                self.top_line
            );
            self.sync_from_core_window(window);
            return;
        }

        let midpoint = body_height / 2;
        let movement_delta = previous_cursor_row.map(|row| row.abs_diff(window.cursor_row));
        let is_single_line_motion = movement_delta == Some(1);
        let core_topline_delta = core_top_line.abs_diff(self.top_line);
        if is_single_line_motion && core_topline_delta == 1 {
            log::debug!(
                "[viewport] sync active cursor movement from one-line core scroll: window_id={}, cursor_row={}, previous_cursor_row={:?}, movement_delta={:?}, body_height={}, total_lines={}, core_topline={}, top_line_before={}, top_line_after={}",
                window.id,
                window.cursor_row,
                previous_cursor_row,
                movement_delta,
                body_height,
                total_lines,
                window.topline,
                self.top_line,
                core_top_line
            );
            self.top_line = core_top_line;
            self.bottom_line = self.top_line.saturating_add(body_height.saturating_sub(1));
            self.last_cursor_row = Some(window.cursor_row);
            return;
        }

        let previous_anchor = previous_cursor_row
            .and_then(|row| row.checked_sub(self.top_line))
            .filter(|relative_row| *relative_row < body_height);
        let stable_band_start = body_height / 3;
        let stable_band_end = body_height.saturating_sub(stable_band_start + 1);
        let stable_anchor = previous_anchor.filter(|relative_row| {
            *relative_row >= stable_band_start && *relative_row <= stable_band_end
        });
        let cursor_above_viewport = window.cursor_row < self.top_line;
        let cursor_below_viewport = window.cursor_row >= self.top_line.saturating_add(body_height);
        let matching_scroll_edge_anchor = previous_cursor_row
            .zip(previous_anchor)
            .map(|(previous_row, relative_row)| {
                if window.cursor_row > previous_row {
                    relative_row + 1 == body_height
                } else if window.cursor_row < previous_row {
                    relative_row == 0
                } else {
                    false
                }
            })
            .unwrap_or(false);
        let preserve_previous_anchor = is_single_line_motion || matching_scroll_edge_anchor;
        let anchor_row = if cursor_above_viewport {
            0
        } else if cursor_below_viewport {
            body_height.saturating_sub(1)
        } else if preserve_previous_anchor {
            previous_anchor.unwrap_or(midpoint)
        } else {
            stable_anchor.unwrap_or(midpoint)
        };
        let max_top_line = total_lines.saturating_sub(body_height);
        let next_top_line = window
            .cursor_row
            .saturating_sub(anchor_row)
            .min(max_top_line);

        log::debug!(
            "[viewport] sync active cursor movement: window_id={}, cursor_row={}, previous_cursor_row={:?}, movement_delta={:?}, body_height={}, total_lines={}, previous_anchor={:?}, cursor_above_viewport={}, cursor_below_viewport={}, single_line_motion={}, matching_scroll_edge_anchor={}, preserve_previous_anchor={}, stable_band=({},{}), stable_anchor={:?}, anchor_row={}, core_topline={}, top_line_before={}, top_line_after={}",
            window.id,
            window.cursor_row,
            previous_cursor_row,
            movement_delta,
            body_height,
            total_lines,
            previous_anchor,
            cursor_above_viewport,
            cursor_below_viewport,
            is_single_line_motion,
            matching_scroll_edge_anchor,
            preserve_previous_anchor,
            stable_band_start,
            stable_band_end,
            stable_anchor,
            anchor_row,
            window.topline,
            self.top_line,
            next_top_line
        );

        self.top_line = next_top_line;
        self.bottom_line = self.top_line.saturating_add(body_height.saturating_sub(1));
        self.last_cursor_row = Some(window.cursor_row);
    }
}

#[derive(Debug, Clone, Default)]
pub struct WindowViewportStore {
    states: BTreeMap<i32, ViewportState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ViewportSyncSummary {
    pub live_window_ids: BTreeSet<i32>,
    pub synced_window_ids: BTreeSet<i32>,
    pub pruned_window_ids: BTreeSet<i32>,
    pub reevaluated_window_ids: BTreeSet<i32>,
    pub invalidated_missing_window_ids: BTreeSet<i32>,
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

    pub fn sync_from_windows(&mut self, windows: &[CoreWindowInfo]) -> ViewportSyncSummary {
        self.sync_from_windows_with_invalidations(windows, &BTreeSet::new())
    }

    pub fn sync_from_windows_with_invalidations(
        &mut self,
        windows: &[CoreWindowInfo],
        invalidated_windows: &BTreeSet<i32>,
    ) -> ViewportSyncSummary {
        let live_ids = windows
            .iter()
            .map(|window| window.id)
            .collect::<BTreeSet<_>>();

        let state_ids_before = self.states.keys().copied().collect::<BTreeSet<_>>();
        let pruned_window_ids = state_ids_before
            .difference(&live_ids)
            .copied()
            .collect::<BTreeSet<_>>();
        let invalidated_missing_window_ids = invalidated_windows
            .difference(&live_ids)
            .copied()
            .collect::<BTreeSet<_>>();
        let reevaluated_window_ids = invalidated_windows
            .intersection(&live_ids)
            .copied()
            .collect::<BTreeSet<_>>();

        self.states.retain(|window_id, _| {
            let is_live = live_ids.contains(window_id);
            if !is_live {
                log::debug!(
                    "[viewport] prune closed window state: window_id={}",
                    window_id
                );
            }
            is_live
        });

        let mut synced_window_ids = BTreeSet::new();
        for window in windows {
            self.get_mut_or_default(window.id)
                .sync_from_core_window(window);
            synced_window_ids.insert(window.id);
        }

        log::debug!(
            "[viewport] live window sync summary: live={:?}, synced={:?}, pruned={:?}, reevaluated={:?}, invalidated_missing={:?}",
            live_ids,
            synced_window_ids,
            pruned_window_ids,
            reevaluated_window_ids,
            invalidated_missing_window_ids
        );

        ViewportSyncSummary {
            live_window_ids: live_ids,
            synced_window_ids,
            pruned_window_ids,
            reevaluated_window_ids,
            invalidated_missing_window_ids,
        }
    }

    pub fn sync_from_windows_for_render(
        &mut self,
        windows: &[CoreWindowInfo],
        invalidated_windows: &BTreeSet<i32>,
        line_counts_by_buffer: &BTreeMap<i32, usize>,
        sync_mode: ViewportSyncMode,
    ) -> ViewportSyncSummary {
        let live_ids = windows
            .iter()
            .map(|window| window.id)
            .collect::<BTreeSet<_>>();

        let state_ids_before = self.states.keys().copied().collect::<BTreeSet<_>>();
        let pruned_window_ids = state_ids_before
            .difference(&live_ids)
            .copied()
            .collect::<BTreeSet<_>>();
        let invalidated_missing_window_ids = invalidated_windows
            .difference(&live_ids)
            .copied()
            .collect::<BTreeSet<_>>();
        let reevaluated_window_ids = invalidated_windows
            .intersection(&live_ids)
            .copied()
            .collect::<BTreeSet<_>>();

        self.states.retain(|window_id, _| {
            let is_live = live_ids.contains(window_id);
            if !is_live {
                log::debug!(
                    "[viewport] prune closed window state: window_id={}",
                    window_id
                );
            }
            is_live
        });

        let mut synced_window_ids = BTreeSet::new();
        for window in windows {
            let state = self.get_mut_or_default(window.id);
            if window.is_active
                && !invalidated_windows.contains(&window.id)
                && sync_mode == ViewportSyncMode::SmoothLineMotion
            {
                let total_lines = line_counts_by_buffer
                    .get(&window.buf_id)
                    .copied()
                    .unwrap_or_else(|| window.botline.max(window.cursor_row.saturating_add(1)));
                state.sync_active_cursor_movement_from_core_window(window, total_lines);
            } else {
                state.sync_from_core_window(window);
            }
            synced_window_ids.insert(window.id);
        }

        log::debug!(
            "[viewport] render window sync summary: sync_mode={:?}, live={:?}, synced={:?}, pruned={:?}, reevaluated={:?}, invalidated_missing={:?}",
            sync_mode,
            live_ids,
            synced_window_ids,
            pruned_window_ids,
            reevaluated_window_ids,
            invalidated_missing_window_ids
        );

        ViewportSyncSummary {
            live_window_ids: live_ids,
            synced_window_ids,
            pruned_window_ids,
            reevaluated_window_ids,
            invalidated_missing_window_ids,
        }
    }

    pub fn len(&self) -> usize {
        self.states.len()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::{Mutex, OnceLock};

    use super::{ViewportState, ViewportSyncMode, WindowViewportStore};
    use vim_core_rs::CoreWindowInfo;

    struct TestLogger {
        lines: Mutex<Vec<String>>,
    }

    fn test_logger_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    impl TestLogger {
        fn init() -> &'static Self {
            static LOGGER: OnceLock<TestLogger> = OnceLock::new();
            let logger = LOGGER.get_or_init(|| TestLogger {
                lines: Mutex::new(Vec::new()),
            });
            let _ = log::set_logger(logger);
            log::set_max_level(log::LevelFilter::Debug);
            logger.clear();
            logger
        }

        fn clear(&self) {
            self.lines
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clear();
        }

        fn lines(&self) -> Vec<String> {
            self.lines
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        }
    }

    impl log::Log for TestLogger {
        fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
            metadata.level() <= log::Level::Debug
        }

        fn log(&self, record: &log::Record<'_>) {
            if self.enabled(record.metadata()) {
                self.lines
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(format!("{}", record.args()));
            }
        }

        fn flush(&self) {}
    }

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
    fn active_cursor_movement_clamps_to_bottom_when_smooth_line_motion_skips_past_viewport() {
        let mut store = WindowViewportStore::new();
        let mut line_counts = BTreeMap::new();
        line_counts.insert(1, 1_000);

        store.sync_from_windows_for_render(
            &[CoreWindowInfo {
                id: 1,
                buf_id: 1,
                row: 0,
                col: 0,
                width: 120,
                height: 51,
                topline: 1,
                botline: 50,
                leftcol: 0,
                skipcol: 0,
                cursor_row: 0,
                cursor_col: 0,
                is_active: true,
            }],
            &BTreeSet::new(),
            &line_counts,
            ViewportSyncMode::SmoothLineMotion,
        );

        store.sync_from_windows_for_render(
            &[CoreWindowInfo {
                id: 1,
                buf_id: 1,
                row: 0,
                col: 0,
                width: 120,
                height: 51,
                topline: 51,
                botline: 81,
                leftcol: 0,
                skipcol: 0,
                cursor_row: 79,
                cursor_col: 0,
                is_active: true,
            }],
            &BTreeSet::new(),
            &line_counts,
            ViewportSyncMode::SmoothLineMotion,
        );

        store.sync_from_windows_for_render(
            &[CoreWindowInfo {
                id: 1,
                buf_id: 1,
                row: 0,
                col: 0,
                width: 120,
                height: 51,
                topline: 131,
                botline: 156,
                leftcol: 0,
                skipcol: 0,
                cursor_row: 154,
                cursor_col: 0,
                is_active: true,
            }],
            &BTreeSet::new(),
            &line_counts,
            ViewportSyncMode::SmoothLineMotion,
        );

        let viewport = store.get(1).expect("active viewport");
        assert_eq!(
            154usize.saturating_sub(viewport.top_line()),
            49,
            "smooth line motion that skips past the visible range should keep the cursor at the bottom instead of snapping to the midpoint"
        );
    }

    #[test]
    fn active_cursor_movement_respects_core_topline_when_cursor_moves_within_same_screen() {
        let mut store = WindowViewportStore::new();
        let mut line_counts = BTreeMap::new();
        line_counts.insert(1, 1_000);

        store.sync_from_windows_for_render(
            &[CoreWindowInfo {
                id: 1,
                buf_id: 1,
                row: 0,
                col: 0,
                width: 120,
                height: 51,
                topline: 76,
                botline: 125,
                leftcol: 0,
                skipcol: 0,
                cursor_row: 100,
                cursor_col: 0,
                is_active: true,
            }],
            &BTreeSet::new(),
            &line_counts,
            ViewportSyncMode::SmoothLineMotion,
        );

        store.sync_from_windows_for_render(
            &[CoreWindowInfo {
                id: 1,
                buf_id: 1,
                row: 0,
                col: 0,
                width: 120,
                height: 51,
                topline: 76,
                botline: 125,
                leftcol: 0,
                skipcol: 0,
                cursor_row: 75,
                cursor_col: 0,
                is_active: true,
            }],
            &BTreeSet::new(),
            &line_counts,
            ViewportSyncMode::SmoothLineMotion,
        );

        let viewport = store.get(1).expect("active viewport");
        assert_eq!(
            viewport.top_line(),
            75,
            "same-topline cursor moves, such as H/M/L or mouse positioning, should preserve core screen placement"
        );
    }

    #[test]
    fn smooth_line_motion_keeps_cursor_at_bottom_when_j_scrolls_viewport() {
        let mut store = WindowViewportStore::new();
        let mut line_counts = BTreeMap::new();
        line_counts.insert(1, 100);

        store.sync_from_windows_for_render(
            &[CoreWindowInfo {
                id: 1,
                buf_id: 1,
                row: 0,
                col: 0,
                width: 80,
                height: 6,
                topline: 1,
                botline: 5,
                leftcol: 0,
                skipcol: 0,
                cursor_row: 4,
                cursor_col: 0,
                is_active: true,
            }],
            &BTreeSet::new(),
            &line_counts,
            ViewportSyncMode::SmoothLineMotion,
        );

        store.sync_from_windows_for_render(
            &[CoreWindowInfo {
                id: 1,
                buf_id: 1,
                row: 0,
                col: 0,
                width: 80,
                height: 6,
                topline: 2,
                botline: 6,
                leftcol: 0,
                skipcol: 0,
                cursor_row: 5,
                cursor_col: 0,
                is_active: true,
            }],
            &BTreeSet::new(),
            &line_counts,
            ViewportSyncMode::SmoothLineMotion,
        );

        let viewport = store.get(1).expect("active viewport");
        assert_eq!(viewport.top_line(), 1);
        assert_eq!(
            5usize.saturating_sub(viewport.top_line()),
            4,
            "holding j at the bottom should scroll by one line without jumping the cursor to the midpoint"
        );
    }

    #[test]
    fn smooth_line_motion_keeps_cursor_at_bottom_when_repeated_j_coalesces_before_render() {
        let _guard = test_logger_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let logger = TestLogger::init();
        let mut store = WindowViewportStore::new();
        let mut line_counts = BTreeMap::new();
        line_counts.insert(1, 100);

        store.sync_from_windows_for_render(
            &[CoreWindowInfo {
                id: 1,
                buf_id: 1,
                row: 0,
                col: 0,
                width: 80,
                height: 6,
                topline: 1,
                botline: 5,
                leftcol: 0,
                skipcol: 0,
                cursor_row: 4,
                cursor_col: 0,
                is_active: true,
            }],
            &BTreeSet::new(),
            &line_counts,
            ViewportSyncMode::SmoothLineMotion,
        );
        logger.clear();

        store.sync_from_windows_for_render(
            &[CoreWindowInfo {
                id: 1,
                buf_id: 1,
                row: 0,
                col: 0,
                width: 80,
                height: 6,
                topline: 5,
                botline: 9,
                leftcol: 0,
                skipcol: 0,
                cursor_row: 8,
                cursor_col: 0,
                is_active: true,
            }],
            &BTreeSet::new(),
            &line_counts,
            ViewportSyncMode::SmoothLineMotion,
        );

        let viewport = store.get(1).expect("active viewport");
        assert_eq!(viewport.top_line(), 4);
        assert_eq!(
            8usize.saturating_sub(viewport.top_line()),
            4,
            "coalesced repeated j input should keep the cursor at the bottom instead of snapping to the midpoint"
        );
        let logs = logger.lines().join("\n");
        assert!(
            logs.contains("movement_delta=Some(4)")
                && logs.contains("matching_scroll_edge_anchor=true")
                && logs.contains("preserve_previous_anchor=true")
                && logs.contains("anchor_row=4"),
            "viewport log should expose the coalesced j diagnosis and selected anchor: {logs}"
        );
    }

    #[test]
    fn smooth_line_motion_clamps_to_bottom_when_repeated_j_skips_past_viewport() {
        let _guard = test_logger_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let logger = TestLogger::init();
        let mut store = WindowViewportStore::new();
        let mut line_counts = BTreeMap::new();
        line_counts.insert(1, 200);

        store.sync_from_windows_for_render(
            &[CoreWindowInfo {
                id: 1,
                buf_id: 1,
                row: 0,
                col: 0,
                width: 80,
                height: 23,
                topline: 1,
                botline: 22,
                leftcol: 0,
                skipcol: 0,
                cursor_row: 10,
                cursor_col: 0,
                is_active: true,
            }],
            &BTreeSet::new(),
            &line_counts,
            ViewportSyncMode::SmoothLineMotion,
        );
        logger.clear();

        store.sync_from_windows_for_render(
            &[CoreWindowInfo {
                id: 1,
                buf_id: 1,
                row: 0,
                col: 0,
                width: 80,
                height: 23,
                topline: 20,
                botline: 41,
                leftcol: 0,
                skipcol: 0,
                cursor_row: 40,
                cursor_col: 0,
                is_active: true,
            }],
            &BTreeSet::new(),
            &line_counts,
            ViewportSyncMode::SmoothLineMotion,
        );

        let viewport = store.get(1).expect("active viewport");
        assert_eq!(viewport.top_line(), 19);
        assert_eq!(
            40usize.saturating_sub(viewport.top_line()),
            21,
            "if repeated j moves past the visible bottom before the next render, the cursor should clamp to the bottom instead of preserving a stale midpoint anchor"
        );
        let logs = logger.lines().join("\n");
        assert!(
            logs.contains("movement_delta=Some(30)")
                && logs.contains("cursor_below_viewport=true")
                && logs.contains("anchor_row=21")
                && logs.contains("top_line_after=19"),
            "viewport log should expose skipped-bottom j diagnosis and selected bottom anchor: {logs}"
        );
    }

    #[test]
    fn smooth_line_motion_keeps_cursor_at_top_when_k_scrolls_viewport() {
        let mut store = WindowViewportStore::new();
        let mut line_counts = BTreeMap::new();
        line_counts.insert(1, 100);

        store.sync_from_windows_for_render(
            &[CoreWindowInfo {
                id: 1,
                buf_id: 1,
                row: 0,
                col: 0,
                width: 80,
                height: 6,
                topline: 11,
                botline: 15,
                leftcol: 0,
                skipcol: 0,
                cursor_row: 10,
                cursor_col: 0,
                is_active: true,
            }],
            &BTreeSet::new(),
            &line_counts,
            ViewportSyncMode::SmoothLineMotion,
        );

        store.sync_from_windows_for_render(
            &[CoreWindowInfo {
                id: 1,
                buf_id: 1,
                row: 0,
                col: 0,
                width: 80,
                height: 6,
                topline: 10,
                botline: 14,
                leftcol: 0,
                skipcol: 0,
                cursor_row: 9,
                cursor_col: 0,
                is_active: true,
            }],
            &BTreeSet::new(),
            &line_counts,
            ViewportSyncMode::SmoothLineMotion,
        );

        let viewport = store.get(1).expect("active viewport");
        assert_eq!(viewport.top_line(), 9);
        assert_eq!(
            9usize.saturating_sub(viewport.top_line()),
            0,
            "holding k at the top should scroll by one line without jumping the cursor to the midpoint"
        );
    }

    #[test]
    fn core_sync_mode_preserves_page_scroll_topline_even_when_cursor_moves() {
        let mut store = WindowViewportStore::new();
        let mut line_counts = BTreeMap::new();
        line_counts.insert(1, 1_000);

        store.sync_from_windows_for_render(
            &[CoreWindowInfo {
                id: 1,
                buf_id: 1,
                row: 0,
                col: 0,
                width: 120,
                height: 51,
                topline: 1,
                botline: 50,
                leftcol: 0,
                skipcol: 0,
                cursor_row: 0,
                cursor_col: 0,
                is_active: true,
            }],
            &BTreeSet::new(),
            &line_counts,
            ViewportSyncMode::Core,
        );

        store.sync_from_windows_for_render(
            &[CoreWindowInfo {
                id: 1,
                buf_id: 1,
                row: 0,
                col: 0,
                width: 120,
                height: 51,
                topline: 32,
                botline: 81,
                leftcol: 0,
                skipcol: 0,
                cursor_row: 49,
                cursor_col: 0,
                is_active: true,
            }],
            &BTreeSet::new(),
            &line_counts,
            ViewportSyncMode::Core,
        );

        let viewport = store.get(1).expect("active viewport");
        assert_eq!(
            viewport.top_line(),
            31,
            "page and explicit scroll commands should keep core-owned topline instead of smooth-line centering"
        );
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

    #[test]
    fn window_viewport_store_reports_live_sync_and_missing_invalidations() {
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
                id: 9,
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

        let invalidated_windows = BTreeSet::from([2, 9]);
        let summary = store.sync_from_windows_with_invalidations(
            &[
                CoreWindowInfo {
                    id: 1,
                    buf_id: 1,
                    row: 0,
                    col: 0,
                    width: 80,
                    height: 10,
                    topline: 4,
                    botline: 13,
                    leftcol: 2,
                    skipcol: 0,
                    cursor_row: 3,
                    cursor_col: 0,
                    is_active: true,
                },
                CoreWindowInfo {
                    id: 2,
                    buf_id: 2,
                    row: 10,
                    col: 0,
                    width: 80,
                    height: 8,
                    topline: 7,
                    botline: 14,
                    leftcol: 0,
                    skipcol: 1,
                    cursor_row: 6,
                    cursor_col: 0,
                    is_active: false,
                },
            ],
            &invalidated_windows,
        );

        assert_eq!(summary.live_window_ids, BTreeSet::from([1, 2]));
        assert_eq!(summary.synced_window_ids, BTreeSet::from([1, 2]));
        assert_eq!(summary.pruned_window_ids, BTreeSet::from([9]));
        assert_eq!(summary.reevaluated_window_ids, BTreeSet::from([2]));
        assert_eq!(summary.invalidated_missing_window_ids, BTreeSet::from([9]));

        assert!(store.get(9).is_none(), "closed window should be pruned");
        assert!(
            store.get(2).is_some(),
            "live invalidated window should sync"
        );
    }
}
