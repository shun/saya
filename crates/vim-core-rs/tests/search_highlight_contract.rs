use std::sync::{Mutex, OnceLock};
use vim_core_rs::*;

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
fn test_search_pattern_and_hlsearch_state() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("").unwrap();

    // Clear search
    session.execute_ex_command("let @/ = ''").unwrap();
    session.execute_ex_command("set nohlsearch").unwrap();

    assert_eq!(session.get_search_pattern(), None);
    assert!(!session.is_hlsearch_active());

    // Set search
    session
        .execute_ex_command("let @/ = 'test_pattern'")
        .unwrap();
    session.execute_ex_command("set hlsearch").unwrap();

    assert_eq!(
        session.get_search_pattern(),
        Some("test_pattern".to_string())
    );
    assert!(session.is_hlsearch_active());

    // Disable highlight temporarily
    session.execute_ex_command("nohlsearch").unwrap();
    assert!(!session.is_hlsearch_active());
    assert_eq!(
        session.get_search_pattern(),
        Some("test_pattern".to_string())
    );
}

#[test]
fn test_search_direction() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("").unwrap();

    // Default or forward
    session.execute_ex_command("let @/ = 'test'").unwrap();
    session
        .execute_ex_command("let v:searchforward = 1")
        .unwrap();
    assert!(matches!(
        session.get_search_direction(),
        CoreSearchDirection::Forward
    ));

    session
        .execute_ex_command("let v:searchforward = 0")
        .unwrap();
    assert!(matches!(
        session.get_search_direction(),
        CoreSearchDirection::Backward
    ));
}

#[test]
fn test_search_highlights() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("").unwrap();

    session.execute_ex_command("enew!").unwrap();
    session
        .execute_ex_command("call setline(1, 'hello world hello')")
        .unwrap();
    session.execute_ex_command("let @/ = 'hello'").unwrap();
    session.execute_ex_command("set hlsearch").unwrap();

    let win_id = session.snapshot().windows[0].id;
    // Fetch highlights for line 1 (window_id, row=1)
    let highlights = session.get_search_highlights(win_id, 1, 1);

    // Should have 2 matches
    assert_eq!(highlights.len(), 2);

    assert_eq!(highlights[0].start_row, 1);
    assert_eq!(highlights[0].start_col, 0);
    assert_eq!(highlights[0].end_row, 1);
    assert_eq!(highlights[0].end_col, 5);

    assert_eq!(highlights[1].start_row, 1);
    assert_eq!(highlights[1].start_col, 12);
    assert_eq!(highlights[1].end_row, 1);
    assert_eq!(highlights[1].end_col, 17);
}

#[test]
fn test_cursor_match_info() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("").unwrap();

    session.execute_ex_command("enew!").unwrap();
    session
        .execute_ex_command("call setline(1, 'hello world hello')")
        .unwrap();
    session.execute_ex_command("let @/ = 'hello'").unwrap();
    session.execute_ex_command("set hlsearch").unwrap();

    let win_id = session.snapshot().windows[0].id;
    // Cursor on first 'hello'
    let info = session.get_cursor_match_info(win_id, 1, 0, 100, 100);
    assert!(info.is_on_match);
    assert_eq!(info.current_match_index, 1);
    if let MatchCountResult::Calculated(count) = info.total_matches {
        assert_eq!(count, 2);
    } else {
        panic!("Expected Calculated(2)");
    }

    // Cursor not on match
    let info_none = session.get_cursor_match_info(win_id, 1, 6, 100, 100);
    assert!(!info_none.is_on_match);
    assert_eq!(info_none.current_match_index, 1); // 1 because it's after the first match
}
