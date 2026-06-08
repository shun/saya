use std::sync::{Mutex, MutexGuard, OnceLock};
use vim_core_rs::{CoreJumpList, CoreJumpListEntry, VimCoreSession};

fn session_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn acquire_session_test_lock() -> MutexGuard<'static, ()> {
    session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn jumplist_api_returns_empty_list_after_clearjumps() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("one\ntwo\nthree\n").expect("session should initialize");

    session
        .execute_ex_command(":clearjumps")
        .expect("clearjumps should succeed");

    assert_eq!(
        session.jumplist(),
        CoreJumpList {
            current_index: 0,
            entries: Vec::new(),
        }
    );
}

#[test]
fn jumplist_api_exposes_entries_for_jump_commands() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("one\ntwo\nthree\nfour\n").expect("session should initialize");

    let current_buf_id = session
        .buffers()
        .into_iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist")
        .id;

    session
        .execute_ex_command(":clearjumps")
        .expect("clearjumps should succeed");
    session
        .execute_normal_command("4G")
        .expect("4G should succeed");
    session
        .execute_normal_command("gg")
        .expect("gg should succeed");

    let jumplist = session.jumplist();

    assert_eq!(jumplist.current_index, jumplist.entries.len());
    assert!(
        jumplist.entries.len() >= 2,
        "jump commands should create at least two entries: {jumplist:?}"
    );
    assert!(
        jumplist.entries.contains(&CoreJumpListEntry {
            buf_id: current_buf_id,
            row: 0,
            col: 0,
        }),
        "jumplist should contain the original cursor position: {jumplist:?}"
    );
    assert!(
        jumplist.entries.contains(&CoreJumpListEntry {
            buf_id: current_buf_id,
            row: 3,
            col: 0,
        }),
        "jumplist should contain the jumped-to line: {jumplist:?}"
    );
}

#[test]
fn jumplist_current_index_tracks_ctrl_o_and_ctrl_i_navigation() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("one\ntwo\nthree\nfour\n").expect("session should initialize");

    session
        .execute_ex_command(":clearjumps")
        .expect("clearjumps should succeed");
    session
        .execute_normal_command("4G")
        .expect("4G should succeed");
    session
        .execute_normal_command("gg")
        .expect("gg should succeed");

    let before_jump_back = session.jumplist();
    assert_eq!(
        before_jump_back.current_index,
        before_jump_back.entries.len()
    );

    session
        .execute_normal_command("\u{f}")
        .expect("Ctrl-O should succeed");

    let after_jump_back = session.jumplist();
    assert!(
        after_jump_back.current_index < after_jump_back.entries.len(),
        "Ctrl-O should move the current index into history: {after_jump_back:?}"
    );
    assert_eq!(
        session.snapshot().cursor_row,
        3,
        "Ctrl-O should move the cursor to an older jump target"
    );

    session
        .execute_normal_command("\u{9}")
        .expect("Ctrl-I should succeed");

    let after_jump_forward = session.jumplist();
    assert!(
        after_jump_forward.current_index > after_jump_back.current_index,
        "Ctrl-I should advance the current index: {after_jump_forward:?}"
    );
    assert_eq!(
        session.snapshot().cursor_row,
        0,
        "Ctrl-I should move the cursor forward to the newer jump target"
    );
}
