use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, OnceLock};

use super::{ViewportSyncMode, WindowViewportStore};
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
