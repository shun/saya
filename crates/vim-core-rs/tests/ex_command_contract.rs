use std::fs;
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
fn ex_write_does_not_create_file_on_disk() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("some content").expect("session should initialize");

    let test_file = "Xtest_write_not_created.txt";
    if fs::metadata(test_file).is_ok() {
        fs::remove_file(test_file).ok();
    }

    let outcome = session
        .execute_ex_command(&format!(":write {}", test_file))
        .expect("write command should succeed");

    assert!(matches!(
        outcome.outcome,
        CoreCommandOutcome::HostActionQueued
    ));

    // Verify host action is queued with correct path
    let action = outcome
        .host_actions
        .into_iter()
        .next()
        .expect("host action should be queued");
    if let CoreHostAction::Write { path, force, .. } = action {
        assert_eq!(path, test_file);
        assert!(!force);
    } else {
        panic!("Expected Write action, got {:?}", action);
    }

    // Verify file was NOT created on disk
    assert!(
        fs::metadata(test_file).is_err(),
        "File should NOT be created on disk by Vim runtime"
    );
}

#[test]
fn ex_write_bang_queues_force_write_action() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("some content").expect("session should initialize");

    let test_file = "Xtest_write_bang.txt";
    let outcome = session
        .execute_ex_command(&format!(":write! {}", test_file))
        .expect("write! command should succeed");

    assert!(matches!(
        outcome.outcome,
        CoreCommandOutcome::HostActionQueued
    ));

    let action = outcome
        .host_actions
        .into_iter()
        .next()
        .expect("host action should be queued");
    if let CoreHostAction::Write { path, force, .. } = action {
        assert_eq!(path, test_file);
        assert!(force);
    } else {
        panic!("Expected Write action, got {:?}", action);
    }
}

#[test]
fn ex_write_no_filename_queues_empty_path_action() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("some content").expect("session should initialize");

    let outcome = session
        .execute_ex_command(":write")
        .expect("write command should succeed");

    assert!(matches!(
        outcome.outcome,
        CoreCommandOutcome::HostActionQueued
    ));

    let action = outcome
        .host_actions
        .into_iter()
        .next()
        .expect("host action should be queued");
    if let CoreHostAction::Write { path, .. } = action {
        assert_eq!(path, "");
    } else {
        panic!("Expected Write action, got {:?}", action);
    }
}

#[test]
fn ex_update_is_intercepted_as_write_action() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("some content").expect("session should initialize");

    // :update はブリッジ層で常に write としてインターセプトされる
    // dirty 判定はホスト（Rust）側の責務
    let test_file = "Xtest_update_intercepted.txt";
    let outcome = session
        .execute_ex_command(&format!(":update {}", test_file))
        .expect("update command should succeed");

    assert!(matches!(
        outcome.outcome,
        CoreCommandOutcome::HostActionQueued
    ));

    let action = outcome
        .host_actions
        .into_iter()
        .next()
        .expect("host action should be queued");
    if let CoreHostAction::Write { path, .. } = action {
        assert_eq!(path, test_file);
    } else {
        panic!("Expected Write action (from update), got {:?}", action);
    }
}

#[test]
fn ex_quit_bang_queues_force_quit_action() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("some content").expect("session should initialize");

    let outcome = session
        .execute_ex_command(":quit!")
        .expect("quit! command should succeed");

    assert!(matches!(
        outcome.outcome,
        CoreCommandOutcome::HostActionQueued
    ));

    let action = outcome
        .host_actions
        .into_iter()
        .next()
        .expect("host action should be queued");
    if let CoreHostAction::Quit { force, .. } = action {
        assert!(force);
    } else {
        panic!("Expected Quit action, got {:?}", action);
    }
}

#[test]
fn ex_suspend_queues_suspend_action() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("some content").expect("session should initialize");

    let outcome = session
        .execute_ex_command(":suspend")
        .expect("suspend command should be host-mediated");

    assert!(matches!(
        outcome.outcome,
        CoreCommandOutcome::HostActionQueued
    ));
    assert_eq!(outcome.host_actions, vec![CoreHostAction::Suspend]);
}

#[test]
fn ex_stop_queues_suspend_action() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("some content").expect("session should initialize");

    let outcome = session
        .execute_ex_command(":stop")
        .expect("stop command should be host-mediated");

    assert!(matches!(
        outcome.outcome,
        CoreCommandOutcome::HostActionQueued
    ));
    assert_eq!(outcome.host_actions, vec![CoreHostAction::Suspend]);
}

#[test]
fn ex_wq_is_intercepted_as_write_then_quit_action() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("some content").expect("session should initialize");

    let outcome = session
        .execute_ex_command(":wq")
        .expect("wq command should succeed");

    assert!(matches!(
        outcome.outcome,
        CoreCommandOutcome::HostActionQueued
    ));

    // :wq は Write → Quit の順でホストアクションをキューする
    assert!(
        matches!(
            outcome.host_actions.as_slice(),
            [CoreHostAction::Write { .. }, CoreHostAction::Quit { .. }]
        ),
        "Expected [Write, Quit], got {:?}",
        outcome.host_actions
    );

    // Write の path は空文字列（カレントバッファ対象）
    if let CoreHostAction::Write { path, .. } = &outcome.host_actions[0] {
        assert_eq!(path, "");
    }
}

#[test]
fn ex_xit_is_intercepted_as_quit_action_on_clean_buffer() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("some content").expect("session should initialize");

    // clean buffer では :xit は Quit のみ（Write は不要）
    let outcome = session
        .execute_ex_command(":x")
        .expect("xit command should succeed");

    assert!(matches!(
        outcome.outcome,
        CoreCommandOutcome::HostActionQueued
    ));

    assert!(
        matches!(
            outcome.host_actions.as_slice(),
            [CoreHostAction::Quit { .. }]
        ),
        "Expected [Quit] on clean buffer, got {:?}",
        outcome.host_actions
    );
}

#[test]
fn ex_xit_is_intercepted_as_write_then_quit_on_dirty_buffer() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("some content").expect("session should initialize");

    // バッファを dirty にする
    session
        .execute_ex_command(":normal! ihello")
        .expect("insert should succeed");

    let outcome = session
        .execute_ex_command(":x")
        .expect("xit command should succeed");

    assert!(matches!(
        outcome.outcome,
        CoreCommandOutcome::HostActionQueued
    ));

    // dirty buffer では :xit は Write → Quit の順でキューする
    assert!(
        matches!(
            outcome.host_actions.as_slice(),
            [CoreHostAction::Write { .. }, CoreHostAction::Quit { .. }]
        ),
        "Expected [Write, Quit] on dirty buffer, got {:?}",
        outcome.host_actions
    );
}

#[test]
fn ex_redraw_bang_surfaces_event_without_host_action() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("some content").expect("session should initialize");

    let outcome = session
        .execute_ex_command(":redraw!")
        .expect("redraw! command should succeed");

    assert!(matches!(outcome.outcome, CoreCommandOutcome::NoChange));

    let event = outcome
        .events
        .into_iter()
        .next()
        .expect("redraw event should be queued");
    assert_eq!(
        event,
        CoreEvent::Redraw {
            full: true,
            clear_before_draw: true,
        }
    );
    assert!(outcome.host_actions.is_empty());
}

#[test]
fn ex_redraw_short_form_surfaces_event_without_host_action() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("some content").expect("session should initialize");

    let outcome = session
        .execute_ex_command(":redr")
        .expect("redr command should succeed");

    assert!(matches!(outcome.outcome, CoreCommandOutcome::NoChange));

    let event = outcome
        .events
        .into_iter()
        .next()
        .expect("redraw event should be queued");
    assert_eq!(
        event,
        CoreEvent::Redraw {
            full: true,
            clear_before_draw: false,
        }
    );
    assert!(outcome.host_actions.is_empty());
}

#[test]
fn multiple_ex_commands_can_queue_multiple_actions() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("some content").expect("session should initialize");

    let write_tx = session.execute_ex_command(":write file1.txt").unwrap();
    let redraw_tx = session.execute_ex_command(":redraw").unwrap();
    let quit_tx = session.execute_ex_command(":quit").unwrap();

    assert!(matches!(
        write_tx.host_actions.as_slice(),
        [CoreHostAction::Write { .. }]
    ));
    assert!(matches!(
        redraw_tx.events.as_slice(),
        [CoreEvent::Redraw { .. }]
    ));
    assert!(matches!(
        quit_tx.host_actions.as_slice(),
        [CoreHostAction::Quit { .. }]
    ));
}

#[test]
fn ex_compound_command_with_write_is_intercepted() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("some content").expect("session should initialize");

    let test_file = "Xtest_compound_write.txt";
    if fs::metadata(test_file).is_ok() {
        fs::remove_file(test_file).ok();
    }

    // 複合コマンド内の write はブリッジ層でインターセプトされる
    let outcome = session
        .execute_ex_command(&format!(":set number | write! {}", test_file))
        .expect("compound command should succeed");

    assert!(matches!(
        outcome.outcome,
        CoreCommandOutcome::HostActionQueued
    ));

    // Write ホストアクションがキューされていることを確認
    let write_action = outcome
        .host_actions
        .iter()
        .find(|a| matches!(a, CoreHostAction::Write { .. }));
    assert!(
        write_action.is_some(),
        "Expected Write action in compound command, got {:?}",
        outcome.host_actions
    );

    if let Some(CoreHostAction::Write { path, force, .. }) = write_action {
        assert_eq!(path, test_file);
        assert!(force, "write! should have force=true");
    }

    // ファイルがディスク上に作成されていないことを確認
    assert!(
        fs::metadata(test_file).is_err(),
        "File should NOT be created on disk by Vim runtime"
    );
}

#[test]
fn ex_compound_write_then_quit_preserves_host_coordination_on_local_buffer() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("some content").expect("session should initialize");

    let outcome = session
        .execute_ex_command(":write compound.txt | quit")
        .expect("compound write|quit should succeed");

    assert!(matches!(
        outcome.outcome,
        CoreCommandOutcome::HostActionQueued
    ));
    assert!(
        matches!(
            outcome.host_actions.as_slice(),
            [CoreHostAction::Write { path, force: false, .. }, CoreHostAction::Quit { force: false, .. }]
            if path == "compound.txt"
        ),
        "Expected [Write, Quit] for local :write | quit, got {:?}",
        outcome.host_actions
    );
}

#[test]
fn ex_compound_update_then_quit_preserves_host_coordination_on_local_buffer() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("some content").expect("session should initialize");

    let outcome = session
        .execute_ex_command(":update compound.txt | quit")
        .expect("compound update|quit should succeed");

    assert!(matches!(
        outcome.outcome,
        CoreCommandOutcome::HostActionQueued
    ));
    assert!(
        matches!(
            outcome.host_actions.as_slice(),
            [CoreHostAction::Write { path, force: false, .. }, CoreHostAction::Quit { force: false, .. }]
            if path == "compound.txt"
        ),
        "Expected [Write, Quit] for local :update | quit, got {:?}",
        outcome.host_actions
    );
}

#[test]
fn ex_compound_non_slash_repeat_last_then_write_and_quit_preserves_local_coordination() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("foo|bar foo|bar").expect("session should initialize");

    session
        .execute_ex_command(":s#foo|bar#baz#")
        .expect("non-slash substitute should succeed");

    let outcome = session
        .execute_ex_command(":s g | write compound_repeat.txt | quit")
        .expect("repeat-last compound command should succeed");

    assert_eq!(outcome.snapshot.text.trim_end_matches('\n'), "baz baz");
    assert!(matches!(
        outcome.outcome,
        CoreCommandOutcome::HostActionQueued
    ));
    assert!(
        matches!(
            outcome.host_actions.as_slice(),
            [CoreHostAction::Write { path, force: false, .. }, CoreHostAction::Quit { force: false, .. }]
            if path == "compound_repeat.txt"
        ),
        "Expected [Write, Quit] after repeat-last substitute, got {:?}",
        outcome.host_actions
    );
}

#[test]
fn ex_compound_magic_abbreviation_then_update_and_quit_preserves_local_coordination() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("foo|bar").expect("session should initialize");

    let outcome = session
        .execute_ex_command(":sm#foo|bar#baz# | update compound_magic.txt | quit")
        .expect("magic abbreviation compound command should succeed");

    assert_eq!(outcome.snapshot.text.trim_end_matches('\n'), "baz");
    assert!(matches!(
        outcome.outcome,
        CoreCommandOutcome::HostActionQueued
    ));
    assert!(
        matches!(
            outcome.host_actions.as_slice(),
            [CoreHostAction::Write { path, force: false, .. }, CoreHostAction::Quit { force: false, .. }]
            if path == "compound_magic.txt"
        ),
        "Expected [Write, Quit] after sm#...#...# | update | quit, got {:?}",
        outcome.host_actions
    );
}

#[test]
fn ex_range_prefixed_write_then_quit_keeps_range_write_on_native_path() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("alpha\nbeta\ngamma\n").expect("session should initialize");

    let test_file = "Xtest_range_prefixed_write.txt";
    if fs::metadata(test_file).is_ok() {
        fs::remove_file(test_file).ok();
    }

    let outcome = session
        .execute_ex_command(&format!(":1,2write! {} | quit", test_file))
        .expect("range-prefixed write|quit should succeed");

    assert!(matches!(
        outcome.host_actions.as_slice(),
        [CoreHostAction::Quit { force: false, .. }]
    ));

    let file_content =
        fs::read_to_string(test_file).expect("range-prefixed write should run on native path");
    assert_eq!(file_content, "alpha\nbeta\n");

    fs::remove_file(test_file).ok();
}
