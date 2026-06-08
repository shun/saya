use std::sync::{Mutex, OnceLock};
use vim_core_rs::VimCoreSession;

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
fn test_register_access_basic() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").unwrap();

    // Set register 'a' from Rust
    session.set_register('a', "yanked from rust");

    // Verify we can get it back
    assert_eq!(session.register('a'), Some("yanked from rust".to_string()));

    // Put register 'a' into buffer
    session.execute_normal_command("\"ap").unwrap();

    // "p" puts after the cursor. Initial buffer "hello" (cursor at 1,1)
    // "hello" -> "hyanked from rustello"
    // Wait, let's check exact behavior of "p" on a character-wise register.
    // If cursor is at 'h', "p" puts after 'h'.
    assert_eq!(session.snapshot().text, "hyanked from rustello\n");
}

#[test]
fn test_register_yank_capture() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("original text").unwrap();

    // Yank "original" into register 'b'
    // "original" is 8 characters.
    session.execute_normal_command("v7l\"by").unwrap();

    // Verify register 'b' contains "original"
    assert_eq!(session.register('b'), Some("original".to_string()));
}

#[test]
fn test_register_multiline() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("line1\nline2").unwrap();

    // Set multi-line register
    session.set_register('m', "rust line 1\nrust line 2\n");

    // Put it (it should be MLINE because of trailing newline)
    session.execute_normal_command("\"mp").unwrap();

    // Current buffer:
    // line1
    // line2
    // After "p" on line 1:
    // line1
    // rust line 1
    // rust line 2
    // line2
    assert_eq!(
        session.snapshot().text,
        "line1\nrust line 1\nrust line 2\nline2\n"
    );
}

#[test]
fn test_register_multiline_readback_returns_full_contents() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("line1\nline2").unwrap();

    session.set_register('m', "rust line 1\nrust line 2\n");

    assert_eq!(
        session.register('m'),
        Some("rust line 1\nrust line 2\n".to_string())
    );
}

#[test]
fn test_register_readback_preserves_trailing_newline() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("line1\nline2").unwrap();

    session.set_register('m', "rust line 1\nrust line 2\n");

    assert_eq!(
        session.register('m'),
        Some("rust line 1\nrust line 2\n".to_string())
    );
}

#[test]
fn test_register_readback_does_not_add_trailing_newline() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("line1\nline2").unwrap();

    session.set_register('m', "rust line 1\nrust line 2");

    assert_eq!(
        session.register('m'),
        Some("rust line 1\nrust line 2".to_string())
    );
}

#[test]
fn test_unnamed_register() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("top secret").unwrap();

    // Yank into unnamed register
    session.execute_normal_command("yiw").unwrap();

    // register '"' should have "top"
    assert_eq!(session.register('"'), Some("top".to_string()));
}

#[test]
fn test_black_hole_register() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("delete me").unwrap();

    session.set_register('"', "baseline");

    // Delete into black hole register
    session.execute_normal_command("\"_diw").unwrap();

    // Unnamed register should remain unchanged.
    assert_eq!(session.register('"'), Some("baseline".to_string()));
}
