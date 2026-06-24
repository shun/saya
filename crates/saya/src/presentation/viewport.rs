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
#[path = "viewport_state_test.rs"]
mod viewport_state_tests;

#[cfg(test)]
#[path = "window_viewport_store_test.rs"]
mod window_viewport_store_tests;

#[cfg(test)]
#[path = "viewport_smooth_line_motion_test.rs"]
mod viewport_smooth_line_motion_tests;
