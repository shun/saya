use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use vim_core_rs::{CoreSessionOptions, VimCoreSession};

fn session_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn acquire_session_test_lock() -> std::sync::MutexGuard<'static, ()> {
    session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn vertical_scroll_updates_viewport_boundaries() {
    let _guard = acquire_session_test_lock();

    // Create 100 lines
    let text = (1..=100)
        .map(|i| format!("line {}", i))
        .collect::<Vec<_>>()
        .join("\n");
    let mut session = VimCoreSession::new(&text).expect("session should initialize");

    // Update layout automatically upon snapshot

    let initial_snapshot = session.snapshot();
    assert!(
        !initial_snapshot.windows.is_empty(),
        "Window list should not be empty"
    );
    let initial_win = &initial_snapshot.windows[0];

    assert_eq!(initial_win.topline, 1, "Initial topline should be 1");
    let initial_botline = initial_win.botline;
    assert!(
        initial_botline > 1 && initial_botline <= 100,
        "botline should be > 1"
    );

    // Move cursor down by 50 lines which forces a scroll
    session
        .execute_normal_command("50j")
        .expect("50j should succeed");

    // Update layout automatically upon snapshot

    let scrolled_snapshot = session.snapshot();
    let scrolled_win = &scrolled_snapshot.windows[0];

    assert!(
        scrolled_win.topline > 1,
        "topline should be > 1 after scrolling down"
    );
    assert!(
        scrolled_win.botline > initial_botline,
        "botline should be advanced"
    );
}

#[test]
fn page_scroll_back_restores_previous_topline() {
    let _guard = acquire_session_test_lock();

    let text = (1..=40)
        .map(|i| format!("line {}", i))
        .collect::<Vec<_>>()
        .join("\n");
    let mut session = VimCoreSession::new(&text).expect("session should initialize");
    session.set_screen_size(12, 80);

    let initial_snapshot = session.snapshot();
    let initial_win = &initial_snapshot.windows[0];
    assert_eq!(initial_win.topline, 1, "Initial topline should be 1");

    session
        .dispatch_key("\u{6}")
        .expect("Ctrl-F should succeed");
    let forward_snapshot = session.snapshot();
    let forward_win = &forward_snapshot.windows[0];
    assert!(
        forward_win.topline > initial_win.topline,
        "Ctrl-F should advance topline"
    );

    session
        .dispatch_key("\u{2}")
        .expect("Ctrl-B should succeed");
    let backward_snapshot = session.snapshot();
    let backward_win = &backward_snapshot.windows[0];
    assert_eq!(
        backward_win.topline, initial_win.topline,
        "Ctrl-B should restore the previous topline"
    );
}

#[test]
fn single_line_up_after_ctrl_f_reaches_last_page_scrolls_one_line() {
    let _guard = acquire_session_test_lock();

    let text = (1..=120)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut session = VimCoreSession::new(&text).expect("session should initialize");
    session.set_screen_size(12, 80);

    for _ in 0..20 {
        session
            .dispatch_key("\u{6}")
            .expect("Ctrl-F should succeed");
        let snapshot = session.snapshot();
        let window = &snapshot.windows[0];
        if window.botline >= 120 {
            break;
        }
    }

    let before = session.snapshot();
    let before_window = &before.windows[0];
    assert!(
        before_window.botline >= 120,
        "test setup should reach the last page: topline={}, botline={}, cursor_row={}",
        before_window.topline,
        before_window.botline,
        before.cursor_row
    );

    session.dispatch_key("k").expect("k should succeed");

    let after = session.snapshot();
    let after_window = &after.windows[0];
    assert_eq!(
        after_window.topline,
        before_window.topline.saturating_sub(1),
        "moving up from the last page should scroll the window up by one line: before_topline={}, after_topline={}, before_cursor={}, after_cursor={}",
        before_window.topline,
        after_window.topline,
        before.cursor_row,
        after.cursor_row
    );
}

#[test]
fn repeated_up_after_ctrl_f_reaches_last_page_scrolls_one_line_per_key() {
    let _guard = acquire_session_test_lock();

    let text = (1..=120)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut session = VimCoreSession::new(&text).expect("session should initialize");
    session.set_screen_size(12, 80);

    for _ in 0..20 {
        session
            .dispatch_key("\u{6}")
            .expect("Ctrl-F should succeed");
        let snapshot = session.snapshot();
        let window = &snapshot.windows[0];
        if window.botline >= 120 {
            break;
        }
    }

    let before = session.snapshot();
    assert!(
        before.windows[0].botline >= 120,
        "test setup should reach the last page: topline={}, botline={}, cursor_row={}",
        before.windows[0].topline,
        before.windows[0].botline,
        before.cursor_row
    );

    let mut observed = Vec::new();
    for _ in 0..11 {
        session.dispatch_key("k").expect("k should succeed");
        let snapshot = session.snapshot();
        let window = &snapshot.windows[0];
        observed.push((window.topline, snapshot.cursor_row));
    }

    assert_eq!(
        observed,
        vec![
            (108, 117),
            (107, 116),
            (106, 115),
            (105, 114),
            (104, 113),
            (103, 112),
            (102, 111),
            (101, 110),
            (100, 109),
            (99, 108),
            (98, 107),
        ],
        "k after Ctrl-F reaches the last page should scroll up one line per key while preserving the cursor screen row"
    );
}

#[test]
fn repeated_up_after_ctrl_f_reaches_last_page_should_scroll_one_line_per_key() {
    let _guard = acquire_session_test_lock();

    let text = (1..=120)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut session = VimCoreSession::new(&text).expect("session should initialize");
    session.set_screen_size(12, 80);

    for _ in 0..20 {
        session
            .dispatch_key("\u{6}")
            .expect("Ctrl-F should succeed");
        let snapshot = session.snapshot();
        let window = &snapshot.windows[0];
        if window.botline >= 120 {
            break;
        }
    }

    let before = session.snapshot();
    let before_window = &before.windows[0];
    assert!(
        before_window.botline >= 120,
        "test setup should reach the last page: topline={}, botline={}, cursor_row={}",
        before_window.topline,
        before_window.botline,
        before.cursor_row
    );

    for step in 1..=3 {
        session.dispatch_key("k").expect("k should succeed");
        let snapshot = session.snapshot();
        let window = &snapshot.windows[0];
        assert_eq!(
            window.topline,
            before_window.topline.saturating_sub(step),
            "k after Ctrl-F reaches the last page should scroll the core window up by one line per key: step={step}, before_topline={}, actual_topline={}, cursor_row={}",
            before_window.topline,
            window.topline,
            snapshot.cursor_row
        );
    }
}

#[test]
fn short_last_page_after_ctrl_f_then_k_scrolls_one_line_per_key() {
    let _guard = acquire_session_test_lock();

    let text = (1..=59)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut session = VimCoreSession::new(&text).expect("session should initialize");
    session.set_screen_size(55, 96);

    for _ in 0..5 {
        session
            .dispatch_key("\u{6}")
            .expect("Ctrl-F should succeed");
        let snapshot = session.snapshot();
        let window = &snapshot.windows[0];
        if window.topline >= 59 {
            break;
        }
    }

    let before = session.snapshot();
    let before_window = &before.windows[0];
    assert_eq!(
        before_window.topline, 59,
        "test setup should reach the one-line last page: topline={}, botline={}, cursor_row={}",
        before_window.topline, before_window.botline, before.cursor_row
    );

    let mut observed = Vec::new();
    for _ in 0..3 {
        session.dispatch_key("k").expect("k should succeed");
        let snapshot = session.snapshot();
        let window = &snapshot.windows[0];
        observed.push((window.topline, snapshot.cursor_row));
    }

    assert_eq!(
        observed,
        vec![(58, 57), (57, 56), (56, 55)],
        "k after Ctrl-F reaches a one-line last page should not jump back to the first full page"
    );
}

#[test]
fn repeated_down_after_ctrl_b_reaches_first_page_scrolls_one_line_per_key() {
    let _guard = acquire_session_test_lock();

    let text = (1..=120)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut session = VimCoreSession::new(&text).expect("session should initialize");
    session.set_screen_size(12, 80);

    for _ in 0..6 {
        session
            .dispatch_key("\u{6}")
            .expect("Ctrl-F should succeed");
    }
    for _ in 0..20 {
        session
            .dispatch_key("\u{2}")
            .expect("Ctrl-B should succeed");
        let snapshot = session.snapshot();
        let window = &snapshot.windows[0];
        if window.topline == 1 {
            break;
        }
    }

    let before = session.snapshot();
    assert_eq!(
        before.windows[0].topline, 1,
        "test setup should reach the first page: topline={}, botline={}, cursor_row={}",
        before.windows[0].topline, before.windows[0].botline, before.cursor_row
    );

    let mut observed = Vec::new();
    for _ in 0..11 {
        session.dispatch_key("j").expect("j should succeed");
        let snapshot = session.snapshot();
        let window = &snapshot.windows[0];
        observed.push((window.topline, snapshot.cursor_row));
    }

    assert_eq!(
        observed,
        vec![
            (2, 1),
            (3, 2),
            (4, 3),
            (5, 4),
            (6, 5),
            (7, 6),
            (8, 7),
            (9, 8),
            (10, 9),
            (11, 10),
            (12, 11),
        ],
        "j after Ctrl-B reaches the first page should scroll down one line per key while preserving the cursor screen row"
    );
}

#[test]
fn repeated_down_after_ctrl_b_reaches_first_page_should_scroll_one_line_per_key() {
    let _guard = acquire_session_test_lock();

    let text = (1..=120)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut session = VimCoreSession::new(&text).expect("session should initialize");
    session.set_screen_size(12, 80);

    for _ in 0..6 {
        session
            .dispatch_key("\u{6}")
            .expect("Ctrl-F should succeed");
    }
    for _ in 0..20 {
        session
            .dispatch_key("\u{2}")
            .expect("Ctrl-B should succeed");
        let snapshot = session.snapshot();
        let window = &snapshot.windows[0];
        if window.topline == 1 {
            break;
        }
    }

    let before = session.snapshot();
    let before_window = &before.windows[0];
    assert_eq!(
        before_window.topline, 1,
        "test setup should reach the first page: topline={}, botline={}, cursor_row={}",
        before_window.topline, before_window.botline, before.cursor_row
    );

    for step in 1..=3 {
        session.dispatch_key("j").expect("j should succeed");
        let snapshot = session.snapshot();
        let window = &snapshot.windows[0];
        assert_eq!(
            window.topline,
            before_window.topline.saturating_add(step),
            "j after Ctrl-B reaches the first page should scroll the core window down by one line per key: step={step}, before_topline={}, actual_topline={}, cursor_row={}",
            before_window.topline,
            window.topline,
            snapshot.cursor_row
        );
    }
}

#[test]
fn single_line_motion_keeps_topline_stable_after_screen_resize() {
    let _guard = acquire_session_test_lock();

    let text = (1..=20)
        .map(|i| format!("line {}", i))
        .collect::<Vec<_>>()
        .join("\n");
    let mut session = VimCoreSession::new(&text).expect("session should initialize");
    session.set_screen_size(12, 80);

    let initial_snapshot = session.snapshot();
    let initial_win = &initial_snapshot.windows[0];
    assert_eq!(initial_win.topline, 1, "Initial topline should be 1");
    assert!(
        initial_win.botline > initial_win.topline,
        "Initial viewport should span multiple lines"
    );

    session.dispatch_key("j").expect("j should succeed");

    let moved_snapshot = session.snapshot();
    let moved_win = &moved_snapshot.windows[0];
    assert_eq!(
        moved_snapshot.cursor_row, 1,
        "Cursor should move down one line"
    );
    assert_eq!(
        moved_win.topline, 1,
        "Single-line motion should not scroll the viewport immediately"
    );
    assert!(
        moved_win.botline > moved_win.topline,
        "Viewport height should remain valid after single-line motion"
    );
}

#[test]
fn repeated_resize_keeps_topline_stable_after_single_line_motion() {
    let _guard = acquire_session_test_lock();

    let text = (1..=20)
        .map(|i| format!("line {}", i))
        .collect::<Vec<_>>()
        .join("\n");
    let mut session = VimCoreSession::new(&text).expect("session should initialize");
    session.set_screen_size(24, 80);
    session.set_screen_size(18, 80);
    session.set_screen_size(12, 80);

    let initial_snapshot = session.snapshot();
    let initial_win = &initial_snapshot.windows[0];
    assert_eq!(initial_win.topline, 1, "Initial topline should be 1");

    session.dispatch_key("j").expect("j should succeed");

    let moved_snapshot = session.snapshot();
    let moved_win = &moved_snapshot.windows[0];
    assert_eq!(
        moved_snapshot.cursor_row, 1,
        "Cursor should move down one line"
    );
    assert_eq!(
        moved_win.topline, 1,
        "Repeated resize should not make single-line motion scroll the viewport"
    );
}

#[test]
fn split_window_keeps_topline_stable_after_single_line_motion() {
    let _guard = acquire_session_test_lock();

    let text = (1..=20)
        .map(|i| format!("line {}", i))
        .collect::<Vec<_>>()
        .join("\n");
    let mut session = VimCoreSession::new(&text).expect("session should initialize");
    session.set_screen_size(12, 80);
    session
        .execute_ex_command(":split")
        .expect("split should succeed");

    let windows = session.windows();
    assert!(
        windows.iter().any(|window| window.is_active),
        "Split should leave an active window"
    );
    let active_before = windows
        .iter()
        .find(|window| window.is_active)
        .cloned()
        .expect("split should leave an active window");
    let snapshot_before = session.snapshot();
    let cursor_before = snapshot_before.cursor_row;

    session.dispatch_key("j").expect("j should succeed");

    let snapshot_after = session.snapshot();
    let active_after = session
        .windows()
        .iter()
        .find(|window| window.is_active)
        .cloned()
        .expect("split should still leave an active window");
    assert_eq!(
        snapshot_after.cursor_row,
        cursor_before + 1,
        "Single-line motion after split should move the cursor down one line"
    );
    assert_eq!(
        active_after.topline, active_before.topline,
        "Single-line motion after split should not scroll the active window"
    );
}

#[test]
fn vertical_and_horizontal_resize_keep_topline_stable_separately() {
    let _guard = acquire_session_test_lock();

    let text = (1..=20)
        .map(|i| format!("line {}", i))
        .collect::<Vec<_>>()
        .join("\n");

    {
        let mut session = VimCoreSession::new(&text).expect("session should initialize");
        session.set_screen_size(24, 80);
        session.set_screen_size(12, 80);

        session.dispatch_key("j").expect("j should succeed");

        let snapshot = session.snapshot();
        assert_eq!(
            snapshot.windows[0].topline, 1,
            "Vertical resize should not make single-line motion scroll the viewport"
        );
    }

    {
        let mut session = VimCoreSession::new(&text).expect("session should initialize");
        session.set_screen_size(24, 80);
        session.set_screen_size(24, 120);

        session.dispatch_key("j").expect("j should succeed");

        let snapshot = session.snapshot();
        assert_eq!(
            snapshot.windows[0].topline, 1,
            "Horizontal resize should not make single-line motion scroll the viewport"
        );
    }
}

#[test]
fn recreated_session_keeps_topline_stable_after_single_line_motion() {
    let _guard = acquire_session_test_lock();

    let text = (1..=20)
        .map(|i| format!("line {}", i))
        .collect::<Vec<_>>()
        .join("\n");

    {
        let mut session = VimCoreSession::new(&text).expect("session should initialize");
        session.set_screen_size(12, 80);
        session.set_screen_size(18, 80);
        session
            .execute_ex_command(":split")
            .expect("split should succeed");

        session.dispatch_key("j").expect("j should succeed");

        let snapshot = session.snapshot();
        assert_eq!(snapshot.cursor_row, 1);
        assert_eq!(
            snapshot
                .windows
                .iter()
                .find(|w| w.is_active)
                .unwrap()
                .topline,
            1
        );
    }

    {
        let mut session = VimCoreSession::new(&text).expect("session should initialize");
        session.set_screen_size(12, 80);
        let snapshot = session.snapshot();
        assert_eq!(
            snapshot
                .windows
                .iter()
                .find(|w| w.is_active)
                .unwrap()
                .topline,
            1,
            "A recreated session should start with a stable topline even after the previous session resized and split"
        );

        session.dispatch_key("j").expect("j should succeed");

        let snapshot = session.snapshot();
        assert_eq!(
            snapshot.cursor_row, 1,
            "Recreated session should still move the cursor down one line"
        );
        assert_eq!(
            snapshot
                .windows
                .iter()
                .find(|w| w.is_active)
                .unwrap()
                .topline,
            1,
            "Recreated session should keep the viewport stable"
        );
    }
}

#[test]
fn horizontal_scroll_updates_viewport_boundaries() {
    let _guard = acquire_session_test_lock();

    // Create 1 line with 1000 characters
    let long_line = "a".repeat(1000);
    let mut session = VimCoreSession::new(&long_line).expect("session should initialize");

    // Turn off wrap so horizontal scroll occurs
    session
        .execute_ex_command("set nowrap")
        .expect("set nowrap should succeed");

    // Update layout automatically upon snapshot

    let initial_snapshot = session.snapshot();
    assert!(
        !initial_snapshot.windows.is_empty(),
        "Window list should not be empty"
    );
    let initial_win = &initial_snapshot.windows[0];

    assert_eq!(initial_win.leftcol, 0, "Initial leftcol should be 0");

    // Move cursor right by 200 columns
    session
        .execute_normal_command("200l")
        .expect("200l should succeed");

    // Update layout automatically upon snapshot

    let far_scrolled_snapshot = session.snapshot();
    let far_scrolled_win = &far_scrolled_snapshot.windows[0];

    assert!(
        far_scrolled_win.leftcol > 0,
        "leftcol should increase after moving cursor far right with nowrap"
    );
}

#[test]
fn ctrl_f_and_ctrl_b_restore_the_previous_page_viewport() {
    let _guard = acquire_session_test_lock();

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should move forward")
        .as_nanos();
    let debug_log_path =
        std::env::temp_dir().join(format!("vim-core-rs-scroll-viewport-{nanos}.log"));

    let text = (1..=100)
        .map(|i| format!("line {}", i))
        .collect::<Vec<_>>()
        .join("\n");
    let mut session = VimCoreSession::new_with_options(
        &text,
        CoreSessionOptions {
            debug_log_path: Some(debug_log_path.clone()),
            ..Default::default()
        },
    )
    .expect("session should initialize");

    let initial_snapshot = session.snapshot();
    let initial_window = &initial_snapshot.windows[0];
    let initial_topline = initial_window.topline;

    session
        .execute_normal_command("\x06")
        .expect("Ctrl+F should succeed");

    let forward_snapshot = session.snapshot();
    let forward_window = &forward_snapshot.windows[0];
    assert!(
        forward_window.topline > initial_topline,
        "Ctrl+F should advance topline: initial={}, forward={}",
        initial_topline,
        forward_window.topline
    );

    session
        .execute_normal_command("\x02")
        .expect("Ctrl+B should succeed");

    let backward_snapshot = session.snapshot();
    let backward_window = &backward_snapshot.windows[0];

    let debug_log = std::fs::read_to_string(&debug_log_path)
        .unwrap_or_else(|error| panic!("debug log should be readable: {error}"));
    eprintln!("[test] native log:\n{debug_log}");

    assert_eq!(
        backward_window.topline, initial_topline,
        "Ctrl+B should restore the original topline after a page forward"
    );
}
