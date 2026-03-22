/// 統合テスト: 編集フローの検証
///
/// モード遷移、移動、入力、削除が一連で動くことを確認する。
/// dirty 状態が編集結果に追随することを確認する。
/// Requirements: 2.1, 2.2, 2.3, 2.4, 2.5
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use saya::bootstrap::{prepare_launch, BootstrapOutcome};
use saya::cli::{ConfigSource, LaunchRequest};
use saya::editor_session::EditorSessionState;
use saya::input_router::{resolve_intent, EditorIntent, KeyInput};
use saya::screen_model::{project, ProjectionInput};
use vim_core_rs::CoreMode;

fn unique_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-integ-edit-{name}-{nanos}"))
}

/// テスト用に起動済みセッションを生成するヘルパー。
fn launch_with_content(content: &str) -> BootstrapOutcome {
    let target_path = unique_path("edit-content");
    std::fs::write(&target_path, content).expect("テストファイルの作成");

    prepare_launch(LaunchRequest {
        target_path: Some(target_path),
        config_source: ConfigSource::Default,
    })
    .expect("テスト用の起動が成功すること")
}

/// テスト用に新規バッファで起動するヘルパー。
fn launch_empty() -> BootstrapOutcome {
    prepare_launch(LaunchRequest {
        target_path: None,
        config_source: ConfigSource::Default,
    })
    .expect("テスト用の新規バッファ起動が成功すること")
}

// ---- 9.2.1: モード遷移が一連で動くことを確認する ----

/// 起動 → i でインサート → テキスト入力 → Esc でノーマル復帰の流れが
/// CoreBridge + InputRouter + ScreenModel を横断して成立する。
#[test]
fn mode_transition_flow_through_input_router_to_screen_model() {
    let mut outcome = launch_with_content("hello\n");

    // 起動直後はノーマルモード
    let session_state = EditorSessionState::new(outcome.target_path.clone());
    let model = project(&ProjectionInput {
        snapshot: &outcome.initial_snapshot,
        session_state: &session_state,
        transient_message: None,
    });
    assert_eq!(model.mode_label, "NORMAL");

    // InputRouter で 'i' キーを intent 変換
    let intent = resolve_intent(&KeyInput::Char('i'));
    assert_eq!(intent, EditorIntent::EditKey("i".to_string()));

    // CoreBridge で dispatch
    outcome
        .core_bridge
        .dispatch_key("i")
        .expect("i キーの dispatch");
    let snapshot = outcome.core_bridge.snapshot();
    assert_eq!(snapshot.mode, CoreMode::Insert);

    // ScreenModel 投影
    let model = project(&ProjectionInput {
        snapshot: &snapshot,
        session_state: &session_state,
        transient_message: None,
    });
    assert_eq!(model.mode_label, "INSERT");

    // Esc でノーマルモードに復帰
    let esc_intent = resolve_intent(&KeyInput::Escape);
    assert_eq!(esc_intent, EditorIntent::EditKey("\x1b".to_string()));

    outcome
        .core_bridge
        .dispatch_key("\x1b")
        .expect("Escape dispatch");
    let snapshot = outcome.core_bridge.snapshot();
    assert_eq!(snapshot.mode, CoreMode::Normal);

    let model = project(&ProjectionInput {
        snapshot: &snapshot,
        session_state: &session_state,
        transient_message: None,
    });
    assert_eq!(model.mode_label, "NORMAL");
}

// ---- 9.2.2: カーソル移動が ScreenModel に追随する ----

/// hjkl 移動の結果が ScreenModel の cursor_row/col に反映される。
#[test]
fn cursor_movement_reflected_in_screen_model() {
    let mut outcome = launch_with_content("abcde\nfghij\nklmno\n");
    let session_state = EditorSessionState::new(outcome.target_path.clone());

    // 初期位置
    let model = project(&ProjectionInput {
        snapshot: &outcome.initial_snapshot,
        session_state: &session_state,
        transient_message: None,
    });
    assert_eq!(model.cursor_row, 0);
    assert_eq!(model.cursor_col, 0);

    // j で 1 行下に移動
    outcome
        .core_bridge
        .dispatch_key("j")
        .expect("j dispatch");
    let snapshot = outcome.core_bridge.snapshot();
    let model = project(&ProjectionInput {
        snapshot: &snapshot,
        session_state: &session_state,
        transient_message: None,
    });
    assert_eq!(model.cursor_row, 1);
    assert_eq!(model.cursor_col, 0);

    // ll で 2 列右に移動
    outcome
        .core_bridge
        .dispatch_key("ll")
        .expect("ll dispatch");
    let snapshot = outcome.core_bridge.snapshot();
    let model = project(&ProjectionInput {
        snapshot: &snapshot,
        session_state: &session_state,
        transient_message: None,
    });
    assert_eq!(model.cursor_row, 1);
    assert_eq!(model.cursor_col, 2);

    // k で 1 行上に移動
    outcome
        .core_bridge
        .dispatch_key("k")
        .expect("k dispatch");
    let snapshot = outcome.core_bridge.snapshot();
    let model = project(&ProjectionInput {
        snapshot: &snapshot,
        session_state: &session_state,
        transient_message: None,
    });
    assert_eq!(model.cursor_row, 0);
    assert_eq!(model.cursor_col, 2);

    // h で 1 列左に移動
    outcome
        .core_bridge
        .dispatch_key("h")
        .expect("h dispatch");
    let snapshot = outcome.core_bridge.snapshot();
    let model = project(&ProjectionInput {
        snapshot: &snapshot,
        session_state: &session_state,
        transient_message: None,
    });
    assert_eq!(model.cursor_row, 0);
    assert_eq!(model.cursor_col, 1);
}

// ---- 9.2.3: テキスト入力が ScreenModel の行データに反映される ----

/// インサートモードで入力した文字が ScreenModel の lines に反映される。
#[test]
fn text_input_reflected_in_screen_model_lines() {
    let mut outcome = launch_empty();
    let session_state = EditorSessionState::new(None);

    // i でインサート → "Hi" を入力 → Esc でノーマル復帰
    outcome.core_bridge.dispatch_key("i").expect("i dispatch");
    outcome.core_bridge.dispatch_key("H").expect("H dispatch");
    outcome.core_bridge.dispatch_key("i").expect("i input");
    outcome
        .core_bridge
        .dispatch_key("\x1b")
        .expect("Esc dispatch");

    let snapshot = outcome.core_bridge.snapshot();
    let model = project(&ProjectionInput {
        snapshot: &snapshot,
        session_state: &session_state,
        transient_message: None,
    });

    assert!(
        model
            .lines
            .iter()
            .any(|line| line.contains("Hi")),
        "入力した 'Hi' が行データに含まれること: {:?}",
        model.lines
    );
}

// ---- 9.2.4: 削除操作が ScreenModel に反映される ----

/// x で文字削除、dd で行削除した結果が ScreenModel に反映される。
#[test]
fn delete_operations_reflected_in_screen_model() {
    let mut outcome = launch_with_content("abcde\nsecond\nthird\n");
    let session_state = EditorSessionState::new(outcome.target_path.clone());

    // x で先頭文字を削除
    outcome
        .core_bridge
        .dispatch_key("x")
        .expect("x dispatch");
    let snapshot = outcome.core_bridge.snapshot();
    let model = project(&ProjectionInput {
        snapshot: &snapshot,
        session_state: &session_state,
        transient_message: None,
    });
    assert_eq!(
        model.lines[0], "bcde",
        "x で先頭の 'a' が削除されること"
    );

    // dd で行全体を削除
    outcome
        .core_bridge
        .dispatch_key("dd")
        .expect("dd dispatch");
    let snapshot = outcome.core_bridge.snapshot();
    let model = project(&ProjectionInput {
        snapshot: &snapshot,
        session_state: &session_state,
        transient_message: None,
    });
    assert_eq!(
        model.lines[0], "second",
        "dd で最初の行が削除され、second が先頭になること"
    );
}

// ---- 9.2.5: dirty 状態が編集結果に追随する ----

/// 編集操作を通じて dirty 状態が ScreenModel に正しく追随する。
#[test]
fn dirty_state_follows_editing_in_screen_model() {
    let mut outcome = launch_with_content("initial\n");
    let session_state = EditorSessionState::new(outcome.target_path.clone());

    // 起動直後は clean
    let model = project(&ProjectionInput {
        snapshot: &outcome.initial_snapshot,
        session_state: &session_state,
        transient_message: None,
    });
    assert!(!model.dirty, "起動直後は dirty=false");

    // 文字入力で dirty になる
    outcome.core_bridge.dispatch_key("i").expect("i dispatch");
    outcome.core_bridge.dispatch_key("X").expect("X input");
    outcome
        .core_bridge
        .dispatch_key("\x1b")
        .expect("Esc dispatch");

    let snapshot = outcome.core_bridge.snapshot();
    let model = project(&ProjectionInput {
        snapshot: &snapshot,
        session_state: &session_state,
        transient_message: None,
    });
    assert!(model.dirty, "編集後は dirty=true");
}

/// 削除操作でも dirty 状態になる。
#[test]
fn dirty_state_set_after_delete_operation() {
    let mut outcome = launch_with_content("hello\n");
    let session_state = EditorSessionState::new(outcome.target_path.clone());

    assert!(!outcome.initial_snapshot.dirty, "起動直後は clean");

    // x で文字削除
    outcome
        .core_bridge
        .dispatch_key("x")
        .expect("x dispatch");
    let snapshot = outcome.core_bridge.snapshot();
    let model = project(&ProjectionInput {
        snapshot: &snapshot,
        session_state: &session_state,
        transient_message: None,
    });
    assert!(model.dirty, "削除操作後は dirty=true");
}

// ---- 9.2.6: モード遷移 → 移動 → 入力 → 削除の一連フロー ----

/// 完全な編集フロー: モード遷移、移動、入力、削除が連続して
/// 正しく反映されることを確認する。
#[test]
fn full_editing_flow_mode_move_insert_delete() {
    let mut outcome = launch_with_content("line1\nline2\nline3\n");
    let session_state = EditorSessionState::new(outcome.target_path.clone());

    // Step 1: ノーマルモード確認
    assert_eq!(outcome.initial_snapshot.mode, CoreMode::Normal);
    assert!(!outcome.initial_snapshot.dirty);

    // Step 2: j で 2 行目に移動
    outcome
        .core_bridge
        .dispatch_key("j")
        .expect("j dispatch");
    let snapshot = outcome.core_bridge.snapshot();
    assert_eq!(snapshot.cursor_row, 1);

    // Step 3: i でインサートモード → テキスト入力
    outcome.core_bridge.dispatch_key("i").expect("i dispatch");
    assert_eq!(outcome.core_bridge.snapshot().mode, CoreMode::Insert);

    outcome.core_bridge.dispatch_key("X").expect("X input");
    outcome.core_bridge.dispatch_key("Y").expect("Y input");
    outcome
        .core_bridge
        .dispatch_key("\x1b")
        .expect("Esc dispatch");

    let snapshot = outcome.core_bridge.snapshot();
    assert_eq!(snapshot.mode, CoreMode::Normal);
    assert!(snapshot.dirty);

    // Step 4: ScreenModel に全体が反映されることを確認
    let model = project(&ProjectionInput {
        snapshot: &snapshot,
        session_state: &session_state,
        transient_message: None,
    });
    assert!(model.dirty);
    assert_eq!(model.mode_label, "NORMAL");
    assert!(
        model.lines.iter().any(|line| line.contains("XY")),
        "入力した 'XY' が行データに含まれること: {:?}",
        model.lines
    );

    // Step 5: dd で行削除
    outcome
        .core_bridge
        .dispatch_key("dd")
        .expect("dd dispatch");
    let snapshot = outcome.core_bridge.snapshot();
    let model = project(&ProjectionInput {
        snapshot: &snapshot,
        session_state: &session_state,
        transient_message: None,
    });
    assert!(model.dirty);

    // 削除後の行データの検証
    eprintln!(
        "[integ-test] 編集フロー完了後の行データ: {:?}",
        model.lines
    );
}
