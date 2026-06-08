use std::sync::{Mutex, OnceLock};
use vim_core_rs::{CoreSearchHighlightMode, CoreWindowInfo, VimCoreSession};

fn session_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn acquire_session_test_lock() -> std::sync::MutexGuard<'static, ()> {
    session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn window_by_id(session: &VimCoreSession, window_id: i32) -> CoreWindowInfo {
    session
        .windows()
        .into_iter()
        .find(|window| window.id == window_id)
        .unwrap_or_else(|| panic!("window {window_id} should exist"))
}

fn assert_valid_viewport(window: &CoreWindowInfo) {
    assert!(
        window.topline >= 1,
        "topline should stay 1-based and non-zero: {:?}",
        window
    );
    assert!(
        window.botline >= window.topline,
        "botline should not precede topline: {:?}",
        window
    );
    assert!(window.width > 0, "width should stay positive: {:?}", window);
    assert!(
        window.height > 0,
        "height should stay positive: {:?}",
        window
    );
}

#[test]
fn active_window_id_is_unique_and_tracks_focus_changes() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("alpha\nbeta\ngamma\ndelta\n").expect("session should initialize");
    session.set_screen_size(24, 80);
    session
        .execute_ex_command(":split")
        .expect("split should succeed");
    session
        .execute_ex_command(":vsplit")
        .expect("vsplit should succeed");

    let window_ids: Vec<i32> = session.windows().iter().map(|window| window.id).collect();
    let initial_active_id = session
        .active_window_id()
        .expect("an active window id should exist");
    let initial_snapshot = session.snapshot();
    eprintln!(
        "[window-contract] initial windows={:?} active={}",
        window_ids, initial_active_id
    );

    assert!(window_ids.contains(&initial_active_id));
    assert_eq!(initial_snapshot.active_window_id(), Some(initial_active_id));
    assert_eq!(
        initial_snapshot
            .window(initial_active_id)
            .map(|window| window.id),
        Some(initial_active_id)
    );

    for target_id in window_ids {
        session
            .switch_to_window(target_id)
            .expect("switch_to_window should succeed");

        let reported_active_id = session
            .active_window_id()
            .expect("an active window id should exist after switching");
        let snapshot = session.snapshot();
        let active_ids: Vec<i32> = snapshot
            .windows
            .iter()
            .filter(|window| window.is_active)
            .map(|window| window.id)
            .collect();
        eprintln!(
            "[window-contract] switch target={} reported={} active_ids={:?}",
            target_id, reported_active_id, active_ids
        );

        assert_eq!(reported_active_id, target_id);
        assert_eq!(snapshot.active_window_id(), Some(target_id));
        assert_eq!(
            snapshot.window(target_id).map(|window| window.id),
            Some(target_id)
        );
        assert_eq!(
            active_ids,
            vec![target_id],
            "exactly one active window should remain after focus changes"
        );
    }
}

#[test]
fn window_id_remains_canonical_across_move_resize_and_close() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\n")
        .expect("session should initialize");
    session.set_screen_size(24, 80);
    session
        .execute_ex_command(":split")
        .expect("split should succeed");

    let tracked_window_id = session
        .active_window_id()
        .expect("an active window id should exist");
    let initial = window_by_id(&session, tracked_window_id);
    eprintln!("[window-contract] initial tracked window={:?}", initial);
    assert_valid_viewport(&initial);

    session
        .execute_ex_command(":resize 5")
        .expect("resize should succeed");
    let after_resize = window_by_id(&session, tracked_window_id);
    eprintln!(
        "[window-contract] after resize tracked window={:?}",
        after_resize
    );
    assert_eq!(after_resize.id, tracked_window_id);
    assert_eq!(after_resize.height, 5);
    assert_valid_viewport(&after_resize);

    session
        .execute_normal_command("\u{17}K")
        .expect("Ctrl-W K should succeed");
    let after_move = window_by_id(&session, tracked_window_id);
    let topmost_row = session
        .windows()
        .iter()
        .map(|window| window.row)
        .min()
        .expect("topmost row should exist");
    eprintln!(
        "[window-contract] after move tracked window={:?} topmost_row={}",
        after_move, topmost_row
    );
    assert_eq!(after_move.id, tracked_window_id);
    assert_eq!(after_move.row, topmost_row);
    assert_valid_viewport(&after_move);

    let closed_window_id = session
        .windows()
        .into_iter()
        .find(|window| window.id != tracked_window_id)
        .expect("the sibling window should exist")
        .id;
    session
        .switch_to_window(closed_window_id)
        .expect("switch_to_window should succeed");
    session
        .execute_ex_command(":close")
        .expect("close should succeed");

    let surviving_windows = session.windows();
    eprintln!(
        "[window-contract] after close surviving windows={:?}",
        surviving_windows
            .iter()
            .map(|window| {
                (
                    window.id,
                    window.row,
                    window.col,
                    window.width,
                    window.height,
                    window.topline,
                    window.botline,
                    window.is_active,
                )
            })
            .collect::<Vec<_>>()
    );

    assert!(
        surviving_windows
            .iter()
            .all(|window| window.id != closed_window_id)
    );
    let surviving_tracked = surviving_windows
        .iter()
        .find(|window| window.id == tracked_window_id)
        .expect("tracked window should survive closing the other window");
    assert!(surviving_tracked.is_active);
    assert_valid_viewport(surviving_tracked);
    assert_eq!(session.active_window_id(), Some(tracked_window_id));
}

#[test]
fn query_visible_search_state_for_inactive_window_returns_ranges() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("needle here\nsecond needle\nplain line\n")
        .expect("session should initialize");
    session.set_screen_size(24, 80);
    session.execute_ex_command("set hlsearch").unwrap();
    session.execute_ex_command("let @/ = 'needle'").unwrap();
    session
        .execute_ex_command(":split")
        .expect("split should succeed");

    let active_window_id = session
        .active_window_id()
        .expect("an active window id should exist");
    let inactive_window_id = session
        .windows()
        .into_iter()
        .find(|window| !window.is_active)
        .expect("an inactive window should exist")
        .id;

    let active_state = session
        .query_visible_search_state_for_window(active_window_id, 1, 3)
        .expect("active window search state should be queryable");
    let inactive_state = session
        .query_visible_search_state_for_window(inactive_window_id, 1, 3)
        .expect("inactive window search state should be queryable");

    eprintln!(
        "[window-contract] active_state={:?} inactive_state={:?}",
        active_state, inactive_state
    );

    assert_eq!(inactive_state.window_id, inactive_window_id);
    assert_eq!(inactive_state.mode, CoreSearchHighlightMode::HlSearch);
    assert_eq!(inactive_state.pattern.as_deref(), Some("needle"));
    assert!(!inactive_state.ranges.is_empty());
    assert_eq!(inactive_state.ranges, active_state.ranges);
}
