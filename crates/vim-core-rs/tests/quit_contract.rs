use std::sync::{Mutex, OnceLock};
use vim_core_rs::{CoreCommandOutcome, CoreHostAction, VimCoreSession};

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
fn quit_should_not_exit_process() {
    let _guard = acquire_session_test_lock();

    let mut session = VimCoreSession::new("test\n").expect("Failed to create session");

    // This should currently exit the process if it's not trapped.
    let outcome = session
        .execute_ex_command(":quit")
        .expect("Failed to apply :quit");

    assert!(matches!(
        outcome.outcome,
        CoreCommandOutcome::HostActionQueued
    ));
    let action = outcome
        .host_actions
        .into_iter()
        .next()
        .expect("Expected host action");
    assert!(matches!(action, CoreHostAction::Quit { .. }));
}

#[test]
fn quit_bang_should_not_exit_process() {
    let _guard = acquire_session_test_lock();

    let mut session = VimCoreSession::new("test\n").expect("Failed to create session");

    // Make the buffer dirty to ensure :quit! is different from :quit
    session
        .execute_normal_command("Aadded text\x1b")
        .expect("Failed to edit");

    let outcome = session
        .execute_ex_command(":quit!")
        .expect("Failed to apply :quit!");

    assert!(matches!(
        outcome.outcome,
        CoreCommandOutcome::HostActionQueued
    ));
    let action = outcome
        .host_actions
        .into_iter()
        .next()
        .expect("Expected host action");
    assert!(matches!(action, CoreHostAction::Quit { force: true, .. }));
}
