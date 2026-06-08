use std::sync::{Mutex, MutexGuard, OnceLock};
use vim_core_rs::{CorePendingArgumentKind, CorePendingInput, VimCoreSession};

fn session_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn acquire_session_test_lock() -> MutexGuard<'static, ()> {
    session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn pending(
    keys: &str,
    count: Option<usize>,
    awaited_argument: Option<CorePendingArgumentKind>,
) -> CorePendingInput {
    CorePendingInput {
        pending_keys: keys.to_string(),
        count,
        awaited_argument,
    }
}

#[test]
fn snapshot_exposes_pending_input_state() {
    let _guard = acquire_session_test_lock();
    let session = VimCoreSession::new("alpha beta gamma\n").expect("session should initialize");

    assert_eq!(session.pending_input(), CorePendingInput::none());
    assert_eq!(session.snapshot().pending_input, CorePendingInput::none());
}

#[test]
fn find_and_till_commands_report_char_pending_until_completed() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("alpha beta gamma\n").expect("session should initialize");

    session
        .execute_normal_command("f")
        .expect("f should succeed");
    assert_eq!(
        session.pending_input(),
        pending("f", None, Some(CorePendingArgumentKind::Char))
    );
    assert_eq!(
        session.snapshot().pending_input,
        pending("f", None, Some(CorePendingArgumentKind::Char))
    );

    session
        .execute_normal_command("b")
        .expect("pending find target should succeed");
    assert_eq!(session.pending_input(), CorePendingInput::none());
    assert_eq!(session.snapshot().pending_input, CorePendingInput::none());

    session
        .execute_normal_command("0")
        .expect("0 should succeed");
    session
        .execute_normal_command("t")
        .expect("t should succeed");
    assert_eq!(
        session.pending_input(),
        pending("t", None, Some(CorePendingArgumentKind::Char))
    );

    session
        .execute_normal_command("g")
        .expect("pending till target should succeed");
    assert_eq!(session.pending_input(), CorePendingInput::none());
}

#[test]
fn replace_mark_and_register_commands_report_specialized_pending_kinds() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("alpha beta gamma\n").expect("session should initialize");

    session
        .execute_normal_command("r")
        .expect("r should succeed");
    assert_eq!(
        session.pending_input(),
        pending("r", None, Some(CorePendingArgumentKind::ReplaceChar))
    );
    assert_eq!(
        session.snapshot().pending_input,
        pending("r", None, Some(CorePendingArgumentKind::ReplaceChar))
    );

    session
        .execute_normal_command("Z")
        .expect("replace target should succeed");
    assert_eq!(session.pending_input(), CorePendingInput::none());

    session
        .execute_normal_command("m")
        .expect("m should succeed");
    assert_eq!(
        session.pending_input(),
        pending("m", None, Some(CorePendingArgumentKind::MarkSet))
    );
    session
        .execute_normal_command("a")
        .expect("mark name should succeed");
    assert_eq!(session.pending_input(), CorePendingInput::none());

    session
        .execute_normal_command("\"")
        .expect("\" should succeed");
    assert_eq!(
        session.pending_input(),
        pending("\"", None, Some(CorePendingArgumentKind::Register))
    );
    session
        .execute_normal_command("a")
        .expect("register name should succeed");
    assert_eq!(
        session.pending_input(),
        pending("\"a", None, Some(CorePendingArgumentKind::NormalCommand))
    );
}

#[test]
fn mark_jump_commands_report_pending_when_current_input_model_can_observe_them() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("one\ntwo\nthree\n").expect("session should initialize");

    session
        .execute_normal_command("'")
        .expect("' should succeed");
    assert_eq!(
        session.pending_input(),
        pending("'", None, Some(CorePendingArgumentKind::MarkJump))
    );
    assert_eq!(
        session.snapshot().pending_input,
        pending("'", None, Some(CorePendingArgumentKind::MarkJump))
    );

    session
        .execute_normal_command("a")
        .expect("mark jump target should succeed");
    assert_eq!(session.pending_input(), CorePendingInput::none());

    session
        .execute_normal_command("`")
        .expect("` should succeed");
    assert_eq!(
        session.pending_input(),
        pending("`", None, Some(CorePendingArgumentKind::MarkJump))
    );
    session
        .execute_normal_command("a")
        .expect("mark jump target should succeed");
    assert_eq!(session.pending_input(), CorePendingInput::none());
}

#[test]
fn fully_satisfied_command_does_not_leave_pending_input() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("alpha beta gamma\n").expect("session should initialize");

    session
        .execute_normal_command("fb")
        .expect("find command with target should succeed");

    assert_eq!(session.pending_input(), CorePendingInput::none());
    assert_eq!(session.snapshot().pending_input, CorePendingInput::none());
}
