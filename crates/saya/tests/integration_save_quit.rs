//! 統合テスト: 保存と終了の安全性の検証
//!
//! このファイルは `saya` の main host save or quit policy suite です。
//!
//! 責務は host/application 層の保存結果、quit 判定、`:w / :wq / :x / :xit`
//! の host action coordination に限定する。詳細な編集セマンティクスは
//! ADR 0001 に従って `vim-core-rs` に委ねる。
//!
//! 保存成功、保存失敗、未保存終了警告、強制終了を個別に確認する。
//! host action と終了判定の整合が崩れないことを確認する。
//! Requirements: 1.4, 1.5, 3.2, 3.3

use std::path::PathBuf;
use std::sync::MutexGuard;
use std::time::{SystemTime, UNIX_EPOCH};

use saya::app::bootstrap::{BootstrapOutcome, launch_test_lock, prepare_launch};
use saya::app::cli::{ConfigSource, InputSource, LaunchRequest};
use saya::app::host_io::{SaveRequest, SaveResult, write_to_path};
use saya::app::session::{EditorSessionState, QuitDecision};
use saya::support::swapfile::swapfile_path_for_target;
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

fn explicit_save_request(path: PathBuf, contents: &str) -> SaveRequest {
    SaveRequest {
        path,
        contents: contents.to_string(),
    }
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
    assert_eq!(
        std::fs::read_to_string(outcome.target_path.as_ref().expect("target path"))
            .expect("saved file should be readable"),
        snapshot.text,
        "保存後のファイル内容は core snapshot の本文と一致すること"
    );

    // 成功をセッションに反映
    session_state.record_save_success();
    assert!(!session_state.is_dirty());

    // 通常終了が許可される
    assert_eq!(session_state.evaluate_quit(false), QuitDecision::Allow);
}

/// Write host action 機構の host 契約検証。`:w` が write host action を 1 件
/// 発行し、host 側が保存要求を組み立てて保存できることを確認する。
/// キーストロークからの `:` 入口到達性と実バイナリでの実保存は実バイナリ
/// E2E (`integration_input_pipeline_e2e.rs` の `write_command_saves_file_*`)
/// が担保する。
#[test]
fn write_host_action_saves_buffer_contents_on_success() {
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
    assert_eq!(
        std::fs::read_to_string(outcome.target_path.as_ref().expect("target path"))
            .expect("saved file should be readable"),
        snapshot.text,
        ":w 成功後のファイル内容は保存時 snapshot の本文と一致すること"
    );

    session_state.record_save_success();
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
    assert!(
        !bad_path.exists(),
        "保存失敗時は存在しない保存先ファイルを作成しないこと"
    );
    assert_eq!(
        std::fs::read_to_string(outcome.target_path.as_ref().expect("original target path"))
            .expect("original file should remain readable"),
        "initial\n",
        "保存失敗時は元のファイル内容を変更しないこと"
    );

    // 通常終了は警告になる
    assert_eq!(
        session_state.evaluate_quit(false),
        QuitDecision::WarnUnsaved
    );
}

// ---- 9.3.3: 未保存終了警告の確認 ----

/// 未保存変更時の quit 判定 (`WarnUnsaved`) を確認する host ユニット契約。
/// キー到達性と実バイナリでの保存挙動は実バイナリ E2E
/// (`integration_input_pipeline_e2e.rs` の `write_command_saves_file_*` /
/// `wq_command_*` / `x_command_*`) が担保する。
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

/// quit 経路で outcome を drop した際の session cleanup を確認する host
/// ユニット契約。キー到達性と実バイナリでの保存挙動は実バイナリ E2E
/// (`integration_input_pipeline_e2e.rs` の `write_command_saves_file_*` /
/// `wq_command_*` / `x_command_*`) が担保する。
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

/// 強制 quit 後に outcome を drop すると swapfile が削除される drop 契約の
/// 検証。キー到達性と実バイナリでの保存挙動は実バイナリ E2E
/// (`integration_input_pipeline_e2e.rs` の `write_command_saves_file_*` /
/// `wq_command_*` / `x_command_*`) が担保する。
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

    std::fs::write(&swap_path, "swap cleanup sentinel\n").expect("テスト用 swapfile の作成");
    assert!(
        swap_path.exists(),
        "テスト用 swapfile が作成されること: {}",
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

/// `:wq` の Write -> Quit 発行順を確認する core 契約 + 保存後の quit 許可を
/// 確認する host 契約。キー到達性と実バイナリでの保存挙動は実バイナリ E2E
/// (`integration_input_pipeline_e2e.rs` の `wq_command_*`) が担保する。
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
        outcome
            .core_bridge
            .apply_ex_command(":wq")
            .expect(":wq コマンドが成功すること"),
        vim_core_rs::CoreCommandOutcome::HostActionQueued,
        ":wq コマンドが成功すること"
    );

    let actions = outcome.core_bridge.take_pending_host_actions();
    assert!(
        matches!(
            actions.as_slice(),
            [
                CoreHostAction::Write { path, force: false, .. },
                CoreHostAction::Quit { force: false, .. }
            ] if path.is_empty()
        ),
        "core 側の :wq は local buffer で Write -> Quit を発行し、saya 側が save-before-quit を補完すること: {:?}",
        actions
    );

    let snapshot = outcome.core_bridge.snapshot();
    let request = session_state
        .build_save_request(&snapshot.text)
        .expect("host 側が保存要求を組み立てられること");
    let result = write_to_path(&request);
    assert_eq!(result, SaveResult::Saved, ":wq の保存が成功すること");
    assert_eq!(
        std::fs::read_to_string(outcome.target_path.as_ref().expect("target path"))
            .expect("saved file should be readable"),
        snapshot.text,
        ":wq の保存後ファイル内容は snapshot と一致すること"
    );

    session_state.record_save_success();
    assert_eq!(
        session_state.evaluate_quit(false),
        QuitDecision::Allow,
        "host 側で保存成功を記録した後に quit を許可すること"
    );
}

/// clean buffer の `:x`/`:xit`/`:exit` が Quit のみをキューする core 契約。
/// キー到達性と実バイナリでの保存挙動は実バイナリ E2E
/// (`integration_input_pipeline_e2e.rs` の `x_command_*`) が担保する。
#[test]
fn x_xit_exit_queue_quit_only_on_clean_buffer() {
    let _lock = test_lock();

    for command in [":x", ":xit", ":exit"] {
        let mut outcome = launch_with_content("initial\n");

        outcome
            .core_bridge
            .apply_ex_command(command)
            .unwrap_or_else(|error| panic!("{command} コマンドが成功すること: {error:?}"));

        let actions = outcome.core_bridge.take_pending_host_actions();
        assert!(
            matches!(
                actions.as_slice(),
                [CoreHostAction::Quit { force: false, .. }]
            ),
            "{command} は clean buffer では Quit のみをキューすること: {:?}",
            actions
        );
        assert!(!outcome.core_bridge.snapshot().dirty);
        assert_eq!(
            std::fs::read_to_string(outcome.target_path.as_ref().expect("target path"))
                .expect("target file should remain readable"),
            "initial\n",
            "{command} は clean buffer の終了でファイル内容を変更しないこと"
        );
    }
}

/// dirty buffer の `:x`/`:xit`/`:exit` が Write -> Quit をキューする core
/// 契約 + host 側の保存と quit 許可を確認する host 契約。キー到達性と実
/// バイナリでの保存挙動は実バイナリ E2E
/// (`integration_input_pipeline_e2e.rs` の `x_command_*`) が担保する。
#[test]
fn x_xit_exit_queue_write_then_quit_on_dirty_buffer() {
    let _lock = test_lock();

    for command in [":x", ":xit", ":exit"] {
        let mut outcome = launch_with_content("initial\n");
        let mut session_state = EditorSessionState::new(outcome.target_path.clone());

        outcome.core_bridge.dispatch_key("i").expect("i dispatch");
        outcome.core_bridge.dispatch_key("D").expect("D input");
        outcome
            .core_bridge
            .dispatch_key("\x1b")
            .expect("Esc dispatch");
        session_state.update_dirty(outcome.core_bridge.snapshot().dirty);
        assert!(
            session_state.is_dirty(),
            "{command} の保存前は dirty であること"
        );

        outcome
            .core_bridge
            .apply_ex_command(command)
            .unwrap_or_else(|error| panic!("{command} コマンドが成功すること: {error:?}"));

        let actions = outcome.core_bridge.take_pending_host_actions();
        assert!(
            matches!(
                actions.as_slice(),
                [
                    CoreHostAction::Write { path, force: false, .. },
                    CoreHostAction::Quit { force: false, .. }
                ] if path.is_empty()
            ),
            "{command} は dirty buffer では Write -> Quit をキューすること: {:?}",
            actions
        );

        let snapshot = outcome.core_bridge.snapshot();
        let request = session_state
            .build_save_request(&snapshot.text)
            .expect("host 側が保存要求を組み立てられること");
        assert_eq!(write_to_path(&request), SaveResult::Saved);
        assert_eq!(
            std::fs::read_to_string(outcome.target_path.as_ref().expect("target path"))
                .expect("saved file should be readable"),
            snapshot.text,
            "{command} の保存後ファイル内容は snapshot と一致すること"
        );
        session_state.record_save_success();
        assert_eq!(session_state.evaluate_quit(false), QuitDecision::Allow);
    }
}

/// `:write <path> | quit` が explicit path 付き Write -> Quit をキューする
/// core 契約 + host 側の explicit path 保存を確認する host 契約。キー到達性と
/// 実バイナリでの保存挙動は実バイナリ E2E
/// (`integration_input_pipeline_e2e.rs` の `write_command_saves_file_*`) が
/// 担保する。
#[test]
fn compound_write_file_then_quit_queues_write_before_quit_with_explicit_path() {
    let _lock = test_lock();
    let mut outcome = launch_with_content("initial\n");
    let mut session_state = EditorSessionState::new(outcome.target_path.clone());
    let alternate_path = unique_path("compound-write-quit.txt");
    let alternate_path_string = alternate_path.display().to_string();

    outcome.core_bridge.dispatch_key("i").expect("i dispatch");
    outcome.core_bridge.dispatch_key("X").expect("X input");
    outcome
        .core_bridge
        .dispatch_key("\x1b")
        .expect("Esc dispatch");
    session_state.update_dirty(outcome.core_bridge.snapshot().dirty);

    outcome
        .core_bridge
        .apply_ex_command(&format!(":write {} | quit", alternate_path.display()))
        .expect(":write file | quit コマンドが成功すること");

    let actions = outcome.core_bridge.take_pending_host_actions();
    assert!(
        matches!(
            actions.as_slice(),
            [
                CoreHostAction::Write { path, force: false, .. },
                CoreHostAction::Quit { force: false, .. }
            ] if path == &alternate_path_string
        ),
        ":write file | quit は explicit path 付きの Write -> Quit をキューすること: {:?}",
        actions
    );

    let snapshot = outcome.core_bridge.snapshot();
    let result = write_to_path(&explicit_save_request(
        alternate_path.clone(),
        &snapshot.text,
    ));
    assert_eq!(
        result,
        SaveResult::Saved,
        "alternate file への保存が成功すること"
    );

    session_state.record_save_success();
    assert_eq!(
        session_state.evaluate_quit(false),
        QuitDecision::Allow,
        "explicit path への保存成功後に quit を許可すること"
    );
    assert_eq!(
        std::fs::read_to_string(&alternate_path).expect("alternate path should exist"),
        snapshot.text,
        "host 側は Write action の explicit path に保存すること"
    );
}

/// dirty buffer の `:update <path> | quit` が Write -> Quit をキューする core
/// 契約 + host 側の explicit path 保存を確認する host 契約。キー到達性と実
/// バイナリでの保存挙動は実バイナリ E2E
/// (`integration_input_pipeline_e2e.rs` の `write_command_saves_file_*` /
/// `wq_command_*` / `x_command_*`) が担保する。
#[test]
fn compound_update_file_then_quit_on_dirty_buffer_queues_write_before_quit() {
    let _lock = test_lock();
    let mut outcome = launch_with_content("initial\n");
    let mut session_state = EditorSessionState::new(outcome.target_path.clone());
    let alternate_path = unique_path("compound-update-dirty.txt");
    let alternate_path_string = alternate_path.display().to_string();

    outcome.core_bridge.dispatch_key("i").expect("i dispatch");
    outcome.core_bridge.dispatch_key("D").expect("D input");
    outcome
        .core_bridge
        .dispatch_key("\x1b")
        .expect("Esc dispatch");
    session_state.update_dirty(outcome.core_bridge.snapshot().dirty);
    assert!(session_state.is_dirty(), "編集後は dirty であること");

    outcome
        .core_bridge
        .apply_ex_command(&format!(":update {} | quit", alternate_path.display()))
        .expect(":update file | quit コマンドが成功すること");

    let actions = outcome.core_bridge.take_pending_host_actions();
    assert!(
        matches!(
            actions.as_slice(),
            [
                CoreHostAction::Write { path, force: false, .. },
                CoreHostAction::Quit { force: false, .. }
            ] if path == &alternate_path_string
        ),
        "dirty local buffer の :update file | quit は Write -> Quit をキューすること: {:?}",
        actions
    );

    let snapshot = outcome.core_bridge.snapshot();
    let result = write_to_path(&explicit_save_request(
        alternate_path.clone(),
        &snapshot.text,
    ));
    assert_eq!(
        result,
        SaveResult::Saved,
        "dirty buffer の alternate save が成功すること"
    );

    session_state.record_save_success();
    assert_eq!(
        session_state.evaluate_quit(false),
        QuitDecision::Allow,
        "dirty buffer の alternate save 成功後に quit を許可すること"
    );
    assert_eq!(
        std::fs::read_to_string(&alternate_path).expect("alternate path should exist"),
        snapshot.text,
        "dirty buffer の :update file は explicit path に snapshot 本文を保存すること"
    );
}

/// clean buffer でも `:update <path> | quit` が Write -> Quit を保つ core
/// 契約 + host 側の explicit path 保存を確認する host 契約。キー到達性と実
/// バイナリでの保存挙動は実バイナリ E2E
/// (`integration_input_pipeline_e2e.rs` の `write_command_saves_file_*` /
/// `wq_command_*` / `x_command_*`) が担保する。
#[test]
fn compound_update_file_then_quit_on_clean_buffer_still_preserves_write_before_quit() {
    let _lock = test_lock();
    let mut outcome = launch_with_content("initial\n");
    let mut session_state = EditorSessionState::new(outcome.target_path.clone());
    let alternate_path = unique_path("compound-update-clean.txt");
    let alternate_path_string = alternate_path.display().to_string();

    assert_eq!(
        session_state.evaluate_quit(false),
        QuitDecision::Allow,
        "clean buffer は開始時点で通常 quit を許可すること"
    );

    outcome
        .core_bridge
        .apply_ex_command(&format!(":update {} | quit", alternate_path.display()))
        .expect(":update file | quit on clean buffer が成功すること");

    let actions = outcome.core_bridge.take_pending_host_actions();
    assert!(
        matches!(
            actions.as_slice(),
            [
                CoreHostAction::Write { path, force: false, .. },
                CoreHostAction::Quit { force: false, .. }
            ] if path == &alternate_path_string
        ),
        "clean local buffer の :update file | quit も Write -> Quit を保つこと: {:?}",
        actions
    );

    let snapshot = outcome.core_bridge.snapshot();
    let result = write_to_path(&explicit_save_request(
        alternate_path.clone(),
        &snapshot.text,
    ));
    assert_eq!(
        result,
        SaveResult::Saved,
        "clean buffer の alternate save が成功すること"
    );

    session_state.record_save_success();
    assert_eq!(
        session_state.evaluate_quit(false),
        QuitDecision::Allow,
        "clean buffer の alternate save 後も quit を許可すること"
    );
    assert_eq!(
        std::fs::read_to_string(&alternate_path).expect("alternate path should exist"),
        snapshot.text,
        "clean buffer の :update file は explicit path に snapshot 本文を保存すること"
    );
}

/// non-slash delimiter の substitute 変換が compound forwarding 中に壊れず
/// Write -> Quit を維持する core 契約 + host 側の保存を確認する host 契約。
/// キー到達性と実バイナリでの保存挙動は実バイナリ E2E
/// (`integration_input_pipeline_e2e.rs` の `write_command_saves_file_*` /
/// `wq_command_*` / `x_command_*`) が担保する。
#[test]
fn non_slash_delimiter_compound_update_then_quit_keeps_forwarding_intact() {
    let _lock = test_lock();
    let mut outcome = launch_with_content("foo|bar foo|bar\n");
    let mut session_state = EditorSessionState::new(outcome.target_path.clone());
    let alternate_path = unique_path("compound-update-hash-delimiter.txt");
    let alternate_path_string = alternate_path.display().to_string();

    outcome
        .core_bridge
        .apply_ex_command(&format!(
            ":sm#foo|bar#baz# | update {} | quit",
            alternate_path.display()
        ))
        .expect("non-slash delimiter compound command が成功すること");
    session_state.update_dirty(outcome.core_bridge.snapshot().dirty);

    let snapshot = outcome.core_bridge.snapshot();
    assert_eq!(
        snapshot.text.trim_end_matches('\n'),
        "baz foo|bar",
        "non-slash delimiter substitute が forwarding 中に壊れないこと"
    );

    let actions = outcome.core_bridge.take_pending_host_actions();
    assert!(
        matches!(
            actions.as_slice(),
            [
                CoreHostAction::Write { path, force: false, .. },
                CoreHostAction::Quit { force: false, .. }
            ] if path == &alternate_path_string
        ),
        "non-slash delimiter compound command 後も Write -> Quit を維持すること: {:?}",
        actions
    );

    let result = write_to_path(&explicit_save_request(
        alternate_path.clone(),
        &snapshot.text,
    ));
    assert_eq!(
        result,
        SaveResult::Saved,
        "non-slash delimiter の save が成功すること"
    );

    session_state.record_save_success();
    assert_eq!(
        session_state.evaluate_quit(false),
        QuitDecision::Allow,
        "non-slash delimiter compound save 成功後に quit を許可すること"
    );
    assert_eq!(
        std::fs::read_to_string(&alternate_path).expect("alternate path should exist"),
        snapshot.text,
        "non-slash delimiter compound command は変換後 snapshot 本文を保存すること"
    );
}
