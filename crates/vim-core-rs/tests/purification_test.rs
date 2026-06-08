use std::sync::{Mutex, OnceLock};
use vim_core_rs::{CoreMode, VimCoreSession};

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
fn test_normal_command_injection_sequence() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("").expect("session should initialize");

    // i: enter insert mode, hello: text, <Esc>: return to normal mode
    // We use \x1b for <Esc>
    session
        .execute_normal_command("ihello\x1b")
        .expect("command should succeed");

    let snapshot = session.snapshot();
    assert_eq!(snapshot.text, "hello\n");
    assert_eq!(snapshot.mode, CoreMode::Normal);
}

#[test]
fn test_normal_command_in_insert_mode() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("").expect("session should initialize");

    // Enter insert mode
    session
        .execute_normal_command("i")
        .expect("enter insert mode");
    assert_eq!(session.snapshot().mode, CoreMode::Insert);

    // Inject "world" while in insert mode
    // If it's pure key injection, it should just work.
    session
        .execute_normal_command("world")
        .expect("inject world");

    let snapshot = session.snapshot();
    assert_eq!(snapshot.text, "world\n");
    // Should still be in insert mode if we didn't send <Esc>
    assert_eq!(snapshot.mode, CoreMode::Insert);

    // Return to normal mode
    session
        .execute_normal_command("\x1b")
        .expect("return to normal");
    assert_eq!(session.snapshot().mode, CoreMode::Normal);
}

#[test]
fn test_normal_command_with_mapping() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("").expect("Failed to create session");

    // Define a mapping: x -> ihello<Esc>
    session
        .execute_ex_command(":nmap x ihello\x1b")
        .expect("define mapping");

    // Execute 'x'
    session
        .execute_normal_command("x")
        .expect("execute mapping");

    let snapshot = session.snapshot();
    assert_eq!(snapshot.text, "hello\n");
    assert_eq!(snapshot.mode, CoreMode::Normal);
}
