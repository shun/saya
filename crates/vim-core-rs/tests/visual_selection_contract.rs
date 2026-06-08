use std::sync::{Mutex, OnceLock};
use vim_core_rs::{CoreMode, CoreVisualSelection, VimCoreSession};

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
fn visual_inner_word_selection_exposes_exact_core_coordinates() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("alpha beta gamma\n").expect("session should initialize");

    session
        .execute_normal_command("wviw")
        .expect("iw text object should be executable after entering visual mode");

    assert_eq!(session.mode(), CoreMode::Visual);
    assert_eq!(
        session.current_visual_selection(),
        Some(CoreVisualSelection {
            mode: CoreMode::Visual,
            start_row: 0,
            start_col: 6,
            end_row: 0,
            end_col: 9,
        })
    );
}
