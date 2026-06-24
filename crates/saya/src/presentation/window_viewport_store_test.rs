use std::collections::BTreeSet;

use super::WindowViewportStore;
use vim_core_rs::CoreWindowInfo;

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
