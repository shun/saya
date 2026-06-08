use std::sync::{Mutex, MutexGuard, OnceLock};
use vim_core_rs::{CoreCommandError, CoreMarkPosition, VimCoreSession};

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
fn mark_api_returns_none_for_unset_marks() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("alpha\nbeta\ngamma\n").expect("session should initialize");

    session
        .execute_ex_command(":delmarks!")
        .expect("local marks should be clearable");
    session
        .execute_ex_command(":delmarks A-Z 0-9")
        .expect("global and numeric marks should be clearable");

    assert_eq!(session.mark('a'), None);
    assert_eq!(session.mark('A'), None);
    assert_eq!(session.mark('0'), None);
}

#[test]
fn mark_api_reads_local_global_numeric_and_special_marks() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("alpha\nbeta\ngamma\n").expect("session should initialize");

    let current_buf_id = session
        .buffers()
        .into_iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist")
        .id;

    session
        .execute_normal_command("2G2lma")
        .expect("local mark command should succeed");
    session
        .execute_normal_command("3GmA")
        .expect("global mark command should succeed");
    session
        .execute_ex_command(":call setpos(\"'0\", [bufnr('%'), 1, 3, 0])")
        .expect("numeric mark setup should succeed");
    session
        .execute_ex_command(":call setpos(\"'<\", [bufnr('%'), 1, 1, 0])")
        .expect("visual start mark setup should succeed");
    session
        .execute_ex_command(":call setpos(\"'>\", [bufnr('%'), 2, 4, 0])")
        .expect("visual end mark setup should succeed");

    assert_eq!(
        session.mark('a'),
        Some(CoreMarkPosition {
            buf_id: current_buf_id,
            row: 1,
            col: 2,
        })
    );
    assert_eq!(
        session.mark('A'),
        Some(CoreMarkPosition {
            buf_id: current_buf_id,
            row: 2,
            col: 0,
        })
    );
    assert_eq!(
        session.mark('0'),
        Some(CoreMarkPosition {
            buf_id: current_buf_id,
            row: 0,
            col: 2,
        })
    );
    assert_eq!(
        session.mark('<'),
        Some(CoreMarkPosition {
            buf_id: current_buf_id,
            row: 0,
            col: 0,
        })
    );
    assert_eq!(
        session.mark('>'),
        Some(CoreMarkPosition {
            buf_id: current_buf_id,
            row: 1,
            col: 3,
        })
    );
}

#[test]
fn mark_api_reflects_updated_positions_after_vim_changes() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("alpha\nbeta\ngamma\n").expect("session should initialize");

    session
        .execute_normal_command("ma")
        .expect("initial mark command should succeed");
    assert_eq!(
        session.mark('a'),
        Some(CoreMarkPosition {
            buf_id: session.buffers()[0].id,
            row: 0,
            col: 0,
        })
    );

    session
        .execute_normal_command("3G1lma")
        .expect("updated mark command should succeed");

    assert_eq!(
        session.mark('a'),
        Some(CoreMarkPosition {
            buf_id: session.buffers()[0].id,
            row: 2,
            col: 1,
        })
    );
}

#[test]
fn set_mark_round_trips_local_and_global_marks() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("alpha\nbeta\ngamma\n").expect("session should initialize");

    let current_buf_id = session
        .buffers()
        .into_iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist")
        .id;

    session
        .set_mark('a', current_buf_id, 1, 3)
        .expect("local mark should be set");
    session
        .set_mark('A', current_buf_id, 2, 1)
        .expect("global mark should be set");

    assert_eq!(
        session.mark('a'),
        Some(CoreMarkPosition {
            buf_id: current_buf_id,
            row: 1,
            col: 3,
        })
    );
    assert_eq!(
        session.mark('A'),
        Some(CoreMarkPosition {
            buf_id: current_buf_id,
            row: 2,
            col: 1,
        })
    );

    session
        .execute_normal_command("gg0ma")
        .expect("vim local mark update should succeed");
    assert_eq!(
        session.mark('a'),
        Some(CoreMarkPosition {
            buf_id: current_buf_id,
            row: 0,
            col: 0,
        })
    );

    session
        .set_mark('A', current_buf_id, 0, 4)
        .expect("rust global mark update should succeed");
    assert_eq!(
        session.mark('A'),
        Some(CoreMarkPosition {
            buf_id: current_buf_id,
            row: 0,
            col: 4,
        })
    );
}

#[test]
fn set_mark_rejects_invalid_mark_names() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("alpha\nbeta\ngamma\n").expect("session should initialize");
    let current_buf_id = session.buffers()[0].id;

    let result = session.set_mark('0', current_buf_id, 0, 0);

    assert!(
        matches!(result, Err(CoreCommandError::InvalidInput)),
        "設定不可マーク名は InvalidInput で落ちてほしい: {result:?}"
    );
}

#[test]
fn set_mark_rejects_out_of_range_positions() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("alpha\nbeta\ngamma\n").expect("session should initialize");
    let current_buf_id = session.buffers()[0].id;

    let bad_row = session.set_mark('a', current_buf_id, 99, 0);
    assert!(
        matches!(bad_row, Err(CoreCommandError::InvalidInput)),
        "範囲外行は InvalidInput で落ちてほしい: {bad_row:?}"
    );

    let bad_col = session.set_mark('A', current_buf_id, 0, 99);
    assert!(
        matches!(bad_col, Err(CoreCommandError::InvalidInput)),
        "範囲外列は InvalidInput で落ちてほしい: {bad_col:?}"
    );
}
