//! 統合テスト: 保存と終了の安全性の検証
//!
//! 保存成功、保存失敗、未保存終了警告、強制終了を個別に確認する。
//! host action と終了判定の整合が崩れないことを確認する。
//! Requirements: 1.4, 1.5, 3.2, 3.3

use std::path::PathBuf;
use std::sync::MutexGuard;
use std::time::{SystemTime, UNIX_EPOCH};

use saya::bootstrap::{BootstrapOutcome, launch_test_lock, prepare_launch};
use saya::cli::{ConfigSource, LaunchRequest};
use saya::editor_session::{EditorSessionState, QuitDecision};
use saya::host_io::{SaveResult, write_to_path};

fn unique_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-integ-save-{name}-{nanos}"))
}

fn test_lock() -> MutexGuard<'static, ()> {
    launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn launch_with_content(content: &str) -> BootstrapOutcome {
    let target_path = unique_path("content");
    std::fs::write(&target_path, content).expect("テストファイルの作成");

    prepare_launch(LaunchRequest {
        target_path: Some(target_path),
        config_source: ConfigSource::Default,
    })
    .expect("テスト用の起動が成功すること")
}

// ---- 9.3.1: 保存成功の確認 ----

#[test]
fn save_success_clears_dirty_state_and_allows_quit() {
    let _lock = test_lock();
    let mut outcome = launch_with_content("initial\n");
    let mut session_state = EditorSessionState::new(outcome.target_path.clone());

    // 編集して dirty 状態にする
    outcome.core_bridge.dispatch_key("i").unwrap();
    outcome.core_bridge.dispatch_key("X").unwrap();
    outcome.core_bridge.dispatch_key("\x1b").unwrap();
    session_state.update_dirty(outcome.core_bridge.snapshot().dirty);
    assert!(session_state.is_dirty());

    // 保存要求の生成
    let snapshot = outcome.core_bridge.snapshot();
    let request = session_state.build_save_request(&snapshot.text).unwrap();

    // ホストへの保存処理
    let result = write_to_path(&request);
    assert_eq!(result, SaveResult::Saved);

    // 成功をセッションに反映
    session_state.record_save_success();
    assert!(!session_state.is_dirty());

    // 通常終了が許可される
    assert_eq!(session_state.evaluate_quit(false), QuitDecision::Allow);
}

// ---- 9.3.2: 保存失敗の確認 ----

#[test]
fn save_failure_keeps_dirty_state_and_warns_on_quit() {
    let _lock = test_lock();
    let mut outcome = launch_with_content("initial\n");

    // 不正なパスに変更して保存失敗を引き起こす
    let bad_path = PathBuf::from("/nonexistent/dir/file.txt");
    let mut session_state = EditorSessionState::new(Some(bad_path.clone()));

    // 編集して dirty 状態にする
    outcome.core_bridge.dispatch_key("i").unwrap();
    outcome.core_bridge.dispatch_key("X").unwrap();
    outcome.core_bridge.dispatch_key("\x1b").unwrap();
    session_state.update_dirty(outcome.core_bridge.snapshot().dirty);
    assert!(session_state.is_dirty());

    // 保存要求の生成
    let snapshot = outcome.core_bridge.snapshot();
    let request = session_state.build_save_request(&snapshot.text).unwrap();

    // ホストへの保存処理（失敗する）
    let result = write_to_path(&request);

    match result {
        SaveResult::Failed { message } => {
            session_state.record_save_failure(message);
        }
        _ => panic!("Expected save failure"),
    }

    // dirty 状態は維持される
    assert!(session_state.is_dirty());
    assert!(session_state.last_save_error().is_some());

    // 通常終了は警告になる
    assert_eq!(
        session_state.evaluate_quit(false),
        QuitDecision::WarnUnsaved
    );
}

// ---- 9.3.3: 未保存終了警告の確認 ----

#[test]
fn unsaved_changes_prevent_immediate_quit() {
    let _lock = test_lock();
    let mut outcome = launch_with_content("initial\n");
    let mut session_state = EditorSessionState::new(outcome.target_path.clone());

    // 編集して dirty 状態にする
    outcome.core_bridge.dispatch_key("i").unwrap();
    outcome.core_bridge.dispatch_key("Y").unwrap();
    outcome.core_bridge.dispatch_key("\x1b").unwrap();
    session_state.update_dirty(outcome.core_bridge.snapshot().dirty);

    // quit は警告を返す
    let decision = session_state.evaluate_quit(false);
    assert_eq!(decision, QuitDecision::WarnUnsaved);

    // もう一度編集を継続できる
    outcome.core_bridge.dispatch_key("x").unwrap();
    session_state.update_dirty(outcome.core_bridge.snapshot().dirty);
    assert!(session_state.is_dirty());
}

// ---- 9.3.4: 強制終了の確認 ----

#[test]
fn force_quit_allows_exit_even_when_dirty() {
    let _lock = test_lock();
    let mut outcome = launch_with_content("initial\n");
    let mut session_state = EditorSessionState::new(outcome.target_path.clone());

    // 編集して dirty 状態にする
    outcome.core_bridge.dispatch_key("i").unwrap();
    outcome.core_bridge.dispatch_key("Z").unwrap();
    outcome.core_bridge.dispatch_key("\x1b").unwrap();
    session_state.update_dirty(outcome.core_bridge.snapshot().dirty);

    // force=true の quit は許可される
    let decision = session_state.evaluate_quit(true);
    assert_eq!(decision, QuitDecision::ForceQuit);
}

// ---- 9.3.5: quit 経路での session cleanup 確認 ----

#[test]
fn quit_host_action_allows_dropping_outcome_for_session_cleanup() {
    let _lock = test_lock();
    let mut outcome = launch_with_content("initial\n");
    let session_state = EditorSessionState::new(outcome.target_path.clone());

    outcome
        .core_bridge
        .apply_ex_command(":q")
        .expect(":q コマンドが成功すること");

    let actions = outcome.core_bridge.take_pending_host_actions();
    assert!(
        actions.iter().any(|action| matches!(
            action,
            vim_core_rs::CoreHostAction::Quit { force: false, .. }
        )),
        ":q 後に通常 quit host action が発行されること"
    );
    assert_eq!(
        session_state.evaluate_quit(false),
        QuitDecision::Allow,
        "clean buffer の :q は通常終了を許可すること"
    );

    drop(outcome);

    let relaunched = prepare_launch(LaunchRequest {
        target_path: None,
        config_source: ConfigSource::Default,
    });
    assert!(
        relaunched.is_ok(),
        "quit 後に outcome を drop すると session cleanup されて再起動できること"
    );
}
