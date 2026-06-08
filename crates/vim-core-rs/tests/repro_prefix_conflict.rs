use std::sync::{Mutex, OnceLock};
use vim_core_rs::{CoreCommandOutcome, CoreEvent, CoreHostAction, VimCoreSession};

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
fn input_command_prefix_full_match() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("").expect("session should initialize");

    // ":input" should work (5 chars)
    // CURRENT implementation only matches "inp" exactly (3 chars + space/null)
    let outcome = session
        .execute_ex_command(":input Please enter something")
        .expect("input command should succeed");

    assert!(
        matches!(outcome.outcome, CoreCommandOutcome::HostActionQueued),
        "Expected HostActionQueued for :input, but got {:?}",
        outcome
    );

    let action = outcome
        .host_actions
        .into_iter()
        .next()
        .expect("host action should be queued");
    if let CoreHostAction::RequestInput { prompt, .. } = action {
        assert_eq!(prompt, "Please enter something");
    } else {
        panic!("Expected RequestInput action, got {:?}", action);
    }
}

#[test]
fn input_command_shorthand_match() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("").expect("session should initialize");

    // ":inp" should work (3 chars)
    let outcome = session
        .execute_ex_command(":inp hello")
        .expect("inp command should succeed");

    assert!(matches!(
        outcome.outcome,
        CoreCommandOutcome::HostActionQueued
    ));

    let action = outcome
        .host_actions
        .into_iter()
        .next()
        .expect("host action should be queued");
    if let CoreHostAction::RequestInput { prompt, .. } = action {
        assert_eq!(prompt, "hello");
    } else {
        panic!("Expected RequestInput action, got {:?}", action);
    }
}

#[test]
fn bell_command_prefix_match() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("").expect("session should initialize");

    // ":bell" should work
    let outcome = session
        .execute_ex_command(":bell")
        .expect("bell command should succeed");

    assert!(matches!(outcome.outcome, CoreCommandOutcome::NoChange));

    assert!(matches!(outcome.events.as_slice(), [CoreEvent::Bell]));
    assert!(outcome.host_actions.is_empty());
}
