use std::sync::{Mutex, OnceLock};
use vim_core_rs::{CoreCommandOutcome, VimCoreSession};

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
fn e182_repro_multiple_sessions_should_all_have_custom_commands() {
    let _guard = acquire_session_test_lock();

    // First session
    {
        let mut session1 =
            VimCoreSession::new("session 1").expect("first session should initialize");

        let direct_outcome = session1
            .execute_ex_command(":CoreInternalWrite direct.txt")
            .expect("direct internal command should work in session 1");
        assert!(
            matches!(direct_outcome.outcome, CoreCommandOutcome::HostActionQueued),
            "Session 1 should have internal command"
        );

        let outcome = session1
            .execute_ex_command(":write test1.txt")
            .expect("write should work in session 1");
        println!("Outcome 1: {:?}", outcome);
        assert!(
            matches!(outcome.outcome, CoreCommandOutcome::HostActionQueued),
            "Session 1 should intercept write"
        );
    }

    // Second session
    {
        let mut session2 =
            VimCoreSession::new("session 2").expect("second session should initialize");

        // Check direct internal command first
        let direct_outcome = session2
            .execute_ex_command(":CoreInternalWrite direct.txt")
            .expect("direct internal command should work in session 2");
        assert!(
            matches!(direct_outcome.outcome, CoreCommandOutcome::HostActionQueued),
            "Session 2 should still have internal command"
        );

        let outcome = session2
            .execute_ex_command(":write test2.txt")
            .expect("write should work in session 2");
        println!("Outcome 2: {:?}", outcome);
        assert!(
            matches!(outcome.outcome, CoreCommandOutcome::HostActionQueued),
            "Session 2 should also intercept write"
        );
    }
}
