/// 統合テスト: 編集フローの検証
///
/// モード遷移、画面投影、入力、dirty 状態が一連で動くことを確認する。
/// 代表的な editing-flow smoke のみを残す。
/// Requirements: 2.1, 2.2, 2.3, 2.4, 2.5
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use saya::bootstrap::{BootstrapOutcome, prepare_launch};
use saya::cli::{ConfigSource, InputSource, LaunchRequest};
use saya::editor_session::EditorSessionState;
use saya::input_router::{EditorIntent, KeyInput, resolve_intent};
use saya::screen_model::{ProjectionInput, project};
use saya::viewport::ViewportState;

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
        input_source: InputSource::File(target_path),
        config_source: ConfigSource::Default,
        ..LaunchRequest::default()
    })
    .expect("テスト用の起動が成功すること")
}

/// テスト用に新規バッファで起動するヘルパー。
fn launch_empty() -> BootstrapOutcome {
    prepare_launch(LaunchRequest {
        input_source: InputSource::Empty,
        config_source: ConfigSource::Default,
        ..LaunchRequest::default()
    })
    .expect("テスト用の新規バッファ起動が成功すること")
}

// host-integration: input routing, core bridge, and screen projection as a
// representative smoke flow.
// ---- 9.2.1: モード遷移が一連で動くことを確認する ----

/// 起動 → i でインサート → テキスト入力 → Esc でノーマル復帰の流れが
/// CoreBridge + InputRouter + ScreenModel を横断して成立する。
#[test]
fn mode_transition_flow_through_input_router_to_screen_model() {
    let mut outcome = launch_with_content("hello\n");

    // 起動直後はノーマルモード
    let session_state = EditorSessionState::new(outcome.target_path.clone());
    let model = project(&ProjectionInput::new(
        &outcome.initial_snapshot,
        &session_state,
        None,
    ));
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

    // ScreenModel 投影
    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));
    assert_eq!(model.mode_label, "INSERT");

    // Esc でノーマルモードに復帰
    let esc_intent = resolve_intent(&KeyInput::Escape);
    assert_eq!(esc_intent, EditorIntent::EditKey("\x1b".to_string()));

    outcome
        .core_bridge
        .dispatch_key("\x1b")
        .expect("Escape dispatch");
    let snapshot = outcome.core_bridge.snapshot();

    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));
    assert_eq!(model.mode_label, "NORMAL");
}

// ---- 9.2.2: 画面投影が ScreenModel に追随する ----

/// viewport と visual selection の投影が ScreenModel に反映される。
// host-integration: viewport projection and terminal-visible cursor handling
// are application responsibilities.
#[test]
fn viewport_auto_scroll_keeps_cursor_visible_during_vertical_motion() {
    let mut outcome = launch_with_content("line1\nline2\nline3\nline4\nline5\nline6\n");
    let session_state = EditorSessionState::new(outcome.target_path.clone());
    let mut viewport = ViewportState::new();
    let body_height = 3usize;

    for _ in 0..4 {
        outcome.core_bridge.dispatch_key("j").expect("j dispatch");
        let snapshot = outcome.core_bridge.snapshot();
        viewport.ensure_cursor_visible(
            snapshot.cursor_row,
            body_height,
            snapshot.text.lines().count(),
        );
    }

    let snapshot = outcome.core_bridge.snapshot();
    let model = project(
        &ProjectionInput::new(&snapshot, &session_state, None)
            .with_viewport(viewport.top_line(), body_height),
    );

    assert_eq!(
        viewport.top_line(),
        2,
        "4 行目移動時に viewport が追従すること"
    );
    assert_eq!(model.lines, vec!["line3", "line4", "line5"]);
    assert_eq!(model.cursor_row, 2, "カーソルが本文領域内へ保たれること");
}

// host-integration: visual selection projection for rendering is host-side
// coverage.
#[test]
fn visual_inner_word_selection_is_projected_for_rendering() {
    let mut outcome = launch_with_content("alpha beta gamma\n");
    let session_state = EditorSessionState::new(outcome.target_path.clone());

    outcome.core_bridge.dispatch_key("w").expect("w dispatch");
    outcome.core_bridge.dispatch_key("v").expect("v dispatch");
    outcome.core_bridge.dispatch_key("i").expect("i dispatch");
    outcome.core_bridge.dispatch_key("w").expect("w dispatch");

    let snapshot = outcome.core_bridge.snapshot();
    let visual_selection = outcome.core_bridge.current_visual_selection();
    let model = project(
        &ProjectionInput::new(&snapshot, &session_state, None)
            .with_visual_selection(visual_selection.as_ref()),
    );

    assert!(
        visual_selection.is_some(),
        "host smoke should confirm that a core-owned visual selection can be handed off"
    );
    assert_eq!(model.mode_label, "VISUAL");
    assert!(
        model.visual_selection.is_some(),
        "visual selection should be projected once the host receives it"
    );
}

// ---- 9.2.3: テキスト入力が ScreenModel の行データに反映される ----

/// インサートモードで入力した文字が ScreenModel の lines に反映される。
// host-integration: basic input-to-screen-model smoke coverage.
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
    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

    assert!(
        model.lines.iter().any(|line| line.contains("Hi")),
        "入力した 'Hi' が行データに含まれること: {:?}",
        model.lines
    );
}

// host-integration: tab-size driven projection is an application-layer
// presentation concern.
#[test]
fn tab_size_setting_changes_screen_projection_for_tabs() {
    let mut outcome = launch_with_content("\ta\n");
    let session_state = EditorSessionState::new_with_tab_size(outcome.target_path.clone(), 4);

    outcome.core_bridge.dispatch_key("l").expect("l dispatch");
    let snapshot = outcome.core_bridge.snapshot();
    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

    assert_eq!(model.lines[0], "    a");
    assert_eq!(model.cursor_col, 4);
}

// host-integration: dirty projection is part of the application state the UI
// renders.
// ---- 9.2.5: dirty 状態が編集結果に追随する ----

/// 編集操作を通じて dirty 状態が ScreenModel に正しく追随する。
#[test]
fn dirty_state_follows_editing_in_screen_model() {
    let mut outcome = launch_with_content("initial\n");
    let session_state = EditorSessionState::new(outcome.target_path.clone());

    // 起動直後は clean
    let model = project(&ProjectionInput::new(
        &outcome.initial_snapshot,
        &session_state,
        None,
    ));
    assert!(!model.dirty, "起動直後は dirty=false");

    // 文字入力で dirty になる
    outcome.core_bridge.dispatch_key("i").expect("i dispatch");
    outcome.core_bridge.dispatch_key("X").expect("X input");
    outcome
        .core_bridge
        .dispatch_key("\x1b")
        .expect("Esc dispatch");

    let snapshot = outcome.core_bridge.snapshot();
    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));
    assert!(model.dirty, "編集後は dirty=true");
}

/// 削除操作でも dirty 状態になる。
// host-integration: dirty projection remains the application concern even for
// delete-driven edits.
#[test]
fn dirty_state_set_after_delete_operation() {
    let mut outcome = launch_with_content("hello\n");
    let session_state = EditorSessionState::new(outcome.target_path.clone());

    assert!(!outcome.initial_snapshot.dirty, "起動直後は clean");

    // x で文字削除
    outcome.core_bridge.dispatch_key("x").expect("x dispatch");
    let snapshot = outcome.core_bridge.snapshot();
    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));
    assert!(model.dirty, "削除操作後は dirty=true");
}

// host-integration: representative end-to-end editing flow smoke coverage.
// ---- 9.2.6: モード遷移 → 移動 → 入力 → 削除の一連フロー ----

/// 完全な編集フロー: モード遷移、移動、入力、削除が連続して
/// 正しく反映されることを確認する。
#[test]
fn full_editing_flow_mode_move_insert_delete() {
    let mut outcome = launch_with_content("line1\nline2\nline3\n");
    let session_state = EditorSessionState::new(outcome.target_path.clone());
    let initial_model = project(&ProjectionInput::new(
        &outcome.initial_snapshot,
        &session_state,
        None,
    ));

    // Step 1: 起動直後の projection は clean
    assert!(!initial_model.dirty);
    assert!(!initial_model.lines.is_empty());

    // Step 2: j で別行へ移動してから編集する
    outcome.core_bridge.dispatch_key("j").expect("j dispatch");

    // Step 3: i で編集開始 → テキスト入力
    outcome.core_bridge.dispatch_key("i").expect("i dispatch");
    outcome.core_bridge.dispatch_key("X").expect("X input");
    outcome.core_bridge.dispatch_key("Y").expect("Y input");
    outcome
        .core_bridge
        .dispatch_key("\x1b")
        .expect("Esc dispatch");

    // Step 4: ScreenModel に編集後の状態が反映されることを確認
    let edited_snapshot = outcome.core_bridge.snapshot();
    let edited_model = project(&ProjectionInput::new(
        &edited_snapshot,
        &session_state,
        None,
    ));
    assert!(edited_model.dirty);
    assert!(
        edited_model.lines != initial_model.lines,
        "integrated edit flow should change the projected lines: initial={:?}, edited={:?}",
        initial_model.lines,
        edited_model.lines
    );

    // Step 5: dd で行削除
    outcome.core_bridge.dispatch_key("dd").expect("dd dispatch");
    let final_snapshot = outcome.core_bridge.snapshot();
    let final_model = project(&ProjectionInput::new(&final_snapshot, &session_state, None));
    assert!(final_model.dirty);
    assert!(
        final_model.lines != edited_model.lines,
        "delete step should trigger another projected update: edited={:?}, final={:?}",
        edited_model.lines,
        final_model.lines
    );

    // 削除後の行データの検証
    eprintln!(
        "[integ-test] host smoke projection changed across edit flow: initial={:?} edited={:?} final={:?}",
        initial_model.lines, edited_model.lines, final_model.lines
    );
}
