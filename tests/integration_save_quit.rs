//! 統合テスト: 保存と終了の安全性の検証
//!
//! このファイルは `saya` の main host save or quit policy suite です。
//!
//! 責務は host/application 層の保存結果、quit 判定、`:wq` の
//! save-then-quit coordination に限定する。詳細な編集セマンティクスは
//! ADR 0001 に従って `vim-core-rs` に委ねる。
//!
//! 保存成功、保存失敗、未保存終了警告、強制終了を個別に確認する。
//! host action と終了判定の整合が崩れないことを確認する。
//! Requirements: 1.4, 1.5, 3.2, 3.3

use std::path::PathBuf;
use std::sync::MutexGuard;
use std::time::{SystemTime, UNIX_EPOCH};

use saya::bootstrap::{BootstrapOutcome, launch_test_lock, prepare_launch};
use saya::cli::{ConfigSource, InputSource, LaunchRequest};
use saya::editor_session::{EditorSessionState, QuitDecision};
use saya::ex_command::{LocalHostCommand, parse_local_host_command};
use saya::host_io::{SaveResult, write_to_path};
use saya::screen_model::{ProjectionInput, project};
use saya::swapfile::swapfile_path_for_target;
use vim_core_rs::CoreHostAction;

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
        input_source: InputSource::File(target_path),
        config_source: ConfigSource::Default,
        ..LaunchRequest::default()
    })
    .expect("テスト用の起動が成功すること")
}

fn save_quit_suite_scope_statement() -> &'static str {
    "main host save or quit policy suite for host/application save results, quit decisions, and save-then-quit coordination"
}

#[test]
fn save_quit_suite_scope_statement_stays_pinned_to_host_layer_policy() {
    let statement = save_quit_suite_scope_statement();

    assert!(
        statement.contains("main host save or quit policy suite"),
        "suite ownership statement should stay explicit"
    );
    assert!(
        statement.contains("save results"),
        "suite ownership statement should keep host-side save responsibility visible"
    );
    assert!(
        statement.contains("quit decisions"),
        "suite ownership statement should keep quit policy responsibility visible"
    );
    assert!(
        statement.contains("save-then-quit"),
        "suite ownership statement should mention save-then-quit coordination"
    );
    assert!(
        !statement.contains("editing semantics"),
        "suite ownership statement must not drift into core-editing ownership"
    );
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

#[test]
fn write_host_action_updates_transient_message_on_success() {
    let _lock = test_lock();
    let mut outcome = launch_with_content("initial\n");
    let mut session_state = EditorSessionState::new(outcome.target_path.clone());

    outcome.core_bridge.dispatch_key("i").expect("i dispatch");
    outcome.core_bridge.dispatch_key("X").expect("X input");
    outcome
        .core_bridge
        .dispatch_key("\x1b")
        .expect("Esc dispatch");
    session_state.update_dirty(outcome.core_bridge.snapshot().dirty);

    outcome
        .core_bridge
        .apply_ex_command(":w")
        .expect(":w コマンドが成功すること");

    let actions = outcome.core_bridge.take_pending_host_actions();
    assert!(
        matches!(actions.as_slice(), [CoreHostAction::Write { .. }]),
        ":w 後に write host action が 1 件発行されること: {:?}",
        actions
    );

    let snapshot = outcome.core_bridge.snapshot();
    let request = session_state
        .build_save_request(&snapshot.text)
        .expect("host 側が保存要求を組み立てられること");
    let result = write_to_path(&request);
    assert_eq!(result, SaveResult::Saved);

    session_state.record_save_success();
    let model = project(&ProjectionInput::new(
        &snapshot,
        &session_state,
        Some("Saved successfully"),
    ));

    assert_eq!(
        model.message_line,
        Some("Saved successfully".to_string()),
        "write 成功時の transient message が画面へ反映されること"
    );
    assert!(!session_state.is_dirty());
    assert_eq!(session_state.last_save_error(), None);
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
        input_source: InputSource::Empty,
        config_source: ConfigSource::Default,
        ..LaunchRequest::default()
    });
    assert!(
        relaunched.is_ok(),
        "quit 後に outcome を drop すると session cleanup されて再起動できること"
    );
}

#[test]
fn force_quit_removes_swapfile_when_outcome_is_dropped() {
    let _lock = test_lock();
    let target_path = unique_path("force-quit-swap-cleanup.txt");
    std::fs::write(&target_path, "initial\n").expect("テストファイルの作成");
    let swap_path = swapfile_path_for_target(&target_path).expect("swap path");

    let mut outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::File(target_path.clone()),
        config_source: ConfigSource::Default,
        ..LaunchRequest::default()
    })
    .expect("テスト用の起動が成功すること");

    assert!(
        swap_path.exists(),
        "起動後に swapfile が作成されること: {}",
        swap_path.display()
    );

    outcome
        .core_bridge
        .apply_ex_command(":q!")
        .expect(":q! コマンドが成功すること");

    let actions = outcome.core_bridge.take_pending_host_actions();
    assert!(
        actions.iter().any(|action| matches!(
            action,
            vim_core_rs::CoreHostAction::Quit { force: true, .. }
        )),
        ":q! 後に強制 quit host action が発行されること"
    );

    drop(outcome);

    assert!(
        !swap_path.exists(),
        "outcome drop 後に swapfile が削除されること: {}",
        swap_path.display()
    );
    std::fs::remove_file(&target_path).expect("テストファイルの削除");
}

#[test]
fn wq_host_coordination_saves_before_allowing_quit() {
    let _lock = test_lock();
    let mut outcome = launch_with_content("initial\n");
    let mut session_state = EditorSessionState::new(outcome.target_path.clone());

    outcome.core_bridge.dispatch_key("i").expect("i dispatch");
    outcome.core_bridge.dispatch_key("X").expect("X input");
    outcome
        .core_bridge
        .dispatch_key("\x1b")
        .expect("Esc dispatch");
    session_state.update_dirty(outcome.core_bridge.snapshot().dirty);
    assert_eq!(
        session_state.evaluate_quit(false),
        QuitDecision::WarnUnsaved,
        "save 前の dirty 状態では quit を即時許可しないこと"
    );

    assert_eq!(
        parse_local_host_command(":wq"),
        Some(LocalHostCommand::SaveThenQuit),
        ":wq は saya 側の host save-then-quit policy にルーティングされること"
    );

    outcome
        .core_bridge
        .apply_ex_command(":wq")
        .expect(":wq コマンドが成功すること");

    let actions = outcome.core_bridge.take_pending_host_actions();
    assert!(
        matches!(
            actions.as_slice(),
            [CoreHostAction::Quit { force: false, .. }]
        ),
        "core 側の :wq は save 完了を保証しないので saya 側で補う必要があること: {:?}",
        actions
    );

    let snapshot = outcome.core_bridge.snapshot();
    let request = session_state
        .build_save_request(&snapshot.text)
        .expect("host 側が保存要求を組み立てられること");
    let result = write_to_path(&request);
    assert_eq!(result, SaveResult::Saved, ":wq の保存が成功すること");

    session_state.record_save_success();
    assert_eq!(
        session_state.evaluate_quit(false),
        QuitDecision::Allow,
        "host 側で保存成功を記録した後に quit を許可すること"
    );
}
