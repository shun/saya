use std::sync::{Mutex, MutexGuard, OnceLock};
use vim_core_rs::{CoreMode, VimCoreSession};

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
fn test_automatic_mode_transition_via_key_injection() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("line 1\n").expect("session should initialize");

    assert_eq!(session.snapshot().mode, CoreMode::Normal);

    // Inject 'i' to enter Insert mode.
    session.execute_normal_command("i").expect("i command");
    assert_eq!(session.snapshot().mode, CoreMode::Insert);

    // Inject text and Escape to return to Normal mode.
    session
        .execute_normal_command("hello\x1b")
        .expect("hello<Esc> command");
    assert_eq!(session.snapshot().mode, CoreMode::Normal);
    assert_eq!(session.snapshot().text, "helloline 1\n");
}

#[test]
fn test_append_command_transitions_to_insert_mode() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("A").expect("session should initialize");

    // 'A' should move to end of line and enter Insert mode
    session.execute_normal_command("A").expect("A command");
    assert_eq!(session.snapshot().mode, CoreMode::Insert);

    session.execute_normal_command("BC\x1b").expect("BC<Esc>");
    assert_eq!(session.snapshot().mode, CoreMode::Normal);
    assert_eq!(session.snapshot().text, "ABC\n");
}

#[test]
fn visual_mode_variants_are_reported_in_snapshot() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("one\ntwo\nthree\n").expect("session should initialize");

    session
        .execute_normal_command("v")
        .expect("v should enter visual mode");
    assert_eq!(session.snapshot().mode, CoreMode::Visual);
    assert_eq!(session.mode(), CoreMode::Visual);

    session
        .execute_normal_command("\x1bV")
        .expect("escape then V should enter visual line mode");
    assert_eq!(session.snapshot().mode, CoreMode::VisualLine);
    assert_eq!(session.mode(), CoreMode::VisualLine);

    session
        .execute_normal_command("\x1b\x16")
        .expect("escape then Ctrl-V should enter visual block mode");
    assert_eq!(session.snapshot().mode, CoreMode::VisualBlock);
    assert_eq!(session.mode(), CoreMode::VisualBlock);
}

#[test]
fn select_mode_variants_are_reported_in_snapshot() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("one\ntwo\nthree\n").expect("session should initialize");

    session
        .execute_normal_command("gh")
        .expect("gh should enter select mode");
    assert_eq!(session.snapshot().mode, CoreMode::Select);
    assert_eq!(session.mode(), CoreMode::Select);

    session
        .execute_normal_command("\x1bgH")
        .expect("escape then gH should enter select line mode");
    assert_eq!(session.snapshot().mode, CoreMode::SelectLine);
    assert_eq!(session.mode(), CoreMode::SelectLine);

    session
        .execute_normal_command("\x1bg\x08")
        .expect("escape then g Ctrl-H should enter select block mode");
    assert_eq!(session.snapshot().mode, CoreMode::SelectBlock);
    assert_eq!(session.mode(), CoreMode::SelectBlock);
}

#[test]
fn replace_mode_is_reported_in_snapshot() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("sample text\n").expect("session should initialize");

    session
        .execute_normal_command("R")
        .expect("R should enter replace mode");
    assert_eq!(session.snapshot().mode, CoreMode::Replace);
    assert_eq!(session.mode(), CoreMode::Replace);
}

#[test]
fn command_line_mode_is_reported_in_snapshot() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("sample text\n").expect("session should initialize");

    session
        .execute_normal_command(":")
        .expect(": should enter command-line mode");
    assert_eq!(session.snapshot().mode, CoreMode::CommandLine);
    assert_eq!(session.mode(), CoreMode::CommandLine);
}

#[test]
fn operator_pending_mode_is_reported_in_snapshot() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("sample text\n").expect("session should initialize");

    session
        .execute_normal_command("d")
        .expect("d should enter operator-pending mode");
    assert_eq!(session.snapshot().mode, CoreMode::OperatorPending);
    assert_eq!(session.mode(), CoreMode::OperatorPending);
}

#[test]
fn insert_round_trip_after_motion_keeps_editing_semantics_in_core() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("line1\nline2\nline3\n").expect("session should initialize");

    session.execute_normal_command("j").expect("j command");
    assert_eq!(session.snapshot().cursor_row, 1);
    assert_eq!(session.snapshot().mode, CoreMode::Normal);

    let insert_outcome = session.execute_normal_command("i").expect("i command");
    assert_eq!(
        insert_outcome.outcome,
        vim_core_rs::CoreCommandOutcome::ModeChanged {
            mode: CoreMode::Insert
        }
    );
    assert_eq!(session.snapshot().mode, CoreMode::Insert);

    let text_outcome = session
        .execute_normal_command("XY\x1b")
        .expect("insert text then escape");
    assert_eq!(
        text_outcome.outcome,
        vim_core_rs::CoreCommandOutcome::BufferChanged { revision: 1 }
    );

    let snapshot = session.snapshot();
    assert_eq!(snapshot.mode, CoreMode::Normal);
    assert_eq!(snapshot.cursor_row, 1);
    assert_eq!(snapshot.cursor_col, 1);
    assert_eq!(snapshot.text, "line1\nXYline2\nline3\n");
    assert!(snapshot.dirty);
    assert_eq!(snapshot.revision, 1);
}
