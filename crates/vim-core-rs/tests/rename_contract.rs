use std::fs;
use std::sync::{Mutex, OnceLock};
use tempfile::tempdir;
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
fn rename_missing_source_keeps_existing_destination_intact() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    let tmp_dir = tempdir().expect("tempdir should be created");
    let src = tmp_dir.path().join("Xrename_missing_source");
    let dst = tmp_dir.path().join("Xrename_destination");

    fs::write(&dst, "destination contents").expect("destination file should be created");
    assert!(
        !src.exists(),
        "source should be missing before rename is attempted"
    );

    let expr = format!("rename('{}', '{}')", src.display(), dst.display());
    let result = session
        .eval_string(&expr)
        .expect("rename() should evaluate in Vimscript");

    assert_ne!(
        result, "0",
        "rename() should fail when the source is missing"
    );
    assert_eq!(
        fs::read_to_string(&dst).expect("destination should remain readable"),
        "destination contents",
        "destination contents should remain intact when rename() fails"
    );
    assert!(
        !src.exists(),
        "missing source should still be missing after rename() fails"
    );
}

#[test]
fn rename_moves_source_contents_into_destination() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    let tmp_dir = tempdir().expect("tempdir should be created");
    let src = tmp_dir.path().join("Xrename_source");
    let dst = tmp_dir.path().join("Xrename_destination");

    fs::write(&src, "source contents").expect("source file should be created");
    assert!(
        !dst.exists(),
        "destination should be missing before rename is attempted"
    );

    let expr = format!("rename('{}', '{}')", src.display(), dst.display());
    let result = session
        .eval_string(&expr)
        .expect("rename() should evaluate in Vimscript");

    assert_eq!(
        result, "0",
        "rename() should succeed when the source exists"
    );
    assert_eq!(
        fs::read_to_string(&dst).expect("destination should remain readable"),
        "source contents",
        "destination should receive the source contents after rename() succeeds"
    );
    assert!(
        !src.exists(),
        "source should disappear after rename() succeeds"
    );
}
