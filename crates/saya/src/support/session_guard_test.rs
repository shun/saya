use super::*;
use crate::app::test_support::launch_serial_lock;

#[test]
fn rejects_second_live_session_guard() {
    let _lock = launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    SESSION_OWNED.store(false, Ordering::SeqCst);

    let first = SessionGuard::acquire().unwrap();
    let second = SessionGuard::acquire();

    assert!(matches!(second, Err(SessionGuardError::AlreadyInitialized)));

    drop(first);
    SESSION_OWNED.store(false, Ordering::SeqCst);
}

#[test]
fn allows_reacquiring_after_guard_is_released() {
    let _lock = launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    SESSION_OWNED.store(false, Ordering::SeqCst);

    let first = SessionGuard::acquire().unwrap();
    first.release();

    let second = SessionGuard::acquire();

    assert!(second.is_ok());

    drop(second);
    SESSION_OWNED.store(false, Ordering::SeqCst);
}
