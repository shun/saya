mod support;

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use saya::app::bootstrap::{BootstrapOutcome, prepare_launch};
use saya::app::cli::{ConfigSource, InputSource, LaunchRequest};
use saya::app::session::EditorSessionState;
use saya::presentation::screen_model::{ProjectionInput, project};
use vim_core_rs::CoreMode;

fn unique_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-integ-search-{name}-{nanos}"))
}

fn launch_with_content(content: &str) -> BootstrapOutcome {
    let target_path = unique_path("search-content");
    std::fs::write(&target_path, content).expect("テストファイルの作成");

    prepare_launch(LaunchRequest {
        input_source: InputSource::File(target_path),
        config_source: ConfigSource::Default,
        ..LaunchRequest::default()
    })
    .expect("テスト用の起動が成功すること")
}

/// core の検索実行契約を検証する。`/` 入口＆Enter コミットのキー到達性は
/// E2E `slash_search_moves_cursor_to_match_*`
/// (`integration_input_pipeline_e2e.rs`) が担保するため、ここでは
/// `dispatch_key("/search\r")` を core へ直送して検索実行結果のみを検証する。
#[test]
fn search_starts_and_executes() {
    let _lock = support::session::launch_serial_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let mut outcome = launch_with_content("hello\nsearch test\nworld\n");
    let _session_state = EditorSessionState::new(outcome.target_path.clone());

    outcome
        .core_bridge
        .dispatch_key("/search\r")
        .expect("dispatch search");
    let snapshot = outcome.core_bridge.snapshot();
    assert_eq!(snapshot.mode, CoreMode::Normal);
    assert_eq!(snapshot.cursor_row, 1); // 0-indexed, "search test" is on line 2 (index 1)
}

/// core の n/N 検索繰り返し実行契約を検証する。`n`/`N` キーの入口到達性は
/// E2E `n_and_capital_n_repeat_search_*`
/// (`integration_input_pipeline_e2e.rs`) が担保するため、ここでは
/// core へ直送した際のカーソル移動結果のみを検証する。
#[test]
fn next_previous_search_results() {
    let _lock = support::session::launch_serial_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let mut outcome = launch_with_content("word\ntext\nword\nhello\nword\n");

    outcome
        .core_bridge
        .dispatch_key("/word\r")
        .expect("dispatch search");
    let snapshot = outcome.core_bridge.snapshot();
    assert_eq!(snapshot.cursor_row, 2); // skips the first one because we are at line 0, next is line 2

    outcome.core_bridge.dispatch_key("n").expect("next");
    let snapshot = outcome.core_bridge.snapshot();
    assert_eq!(snapshot.cursor_row, 4);

    outcome.core_bridge.dispatch_key("N").expect("prev");
    let snapshot = outcome.core_bridge.snapshot();
    assert_eq!(snapshot.cursor_row, 2);
}

/// 検索失敗時の E486 メッセージを、core 契約（`take_pending_messages`）と
/// presentation 契約（`project()` の `message_line` 反映）の両面で検証する。
/// 画面への実描画とキー到達性は E2E `slash_search_not_found_reports_e486_*`
/// (`integration_input_pipeline_e2e.rs`) が担保する。
#[test]
fn search_not_found_message() {
    let _lock = support::session::launch_serial_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let mut outcome = launch_with_content("hello\nworld\n");
    let _session_state = EditorSessionState::new(outcome.target_path.clone());

    // search for non-existent word
    outcome.core_bridge.dispatch_key("/missing\r").unwrap();
    let messages = outcome.core_bridge.take_pending_messages();

    let error_msg = messages
        .into_iter()
        .filter(|msg| msg.category.is_user_visible())
        .last()
        .expect("Should have a user visible message");

    assert!(
        error_msg
            .content
            .contains("E486: Pattern not found: missing")
    );

    let snapshot = outcome.core_bridge.snapshot();
    let model = project(&ProjectionInput::new(
        &snapshot,
        &_session_state,
        Some(error_msg.content.trim()),
    ));

    assert_eq!(
        model.message_line,
        Some("E486: Pattern not found: missing".to_string())
    );
}

#[test]
fn search_navigation_failure() {
    let _lock = support::session::launch_serial_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let mut outcome = launch_with_content("hello\nworld\n");
    let _session_state = EditorSessionState::new(outcome.target_path.clone());

    // search navigation 'n' without prior search
    outcome.core_bridge.dispatch_key("n").unwrap();
    let messages = outcome.core_bridge.take_pending_messages();

    let error_msg = messages
        .into_iter()
        .filter(|msg| msg.category.is_user_visible())
        .last()
        .expect("Should have a user visible message");

    assert!(
        error_msg
            .content
            .contains("E35: No previous regular expression")
            || error_msg.content.contains("E486: Pattern not found"),
        "Unexpected error message: {}",
        error_msg.content
    );
}

#[test]
fn search_exact_word() {
    let _lock = support::session::launch_serial_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let mut outcome = launch_with_content("first second first\n");
    outcome.core_bridge.dispatch_key("/\\<first\\>\r").unwrap();
    let snapshot = outcome.core_bridge.snapshot();
    assert_eq!(snapshot.cursor_col, 13);

    // search for the next "first"
    outcome.core_bridge.dispatch_key("n").unwrap();
    let snapshot2 = outcome.core_bridge.snapshot();
    assert_eq!(snapshot2.cursor_col, 0);
}

#[test]
fn search_asterisk() {
    let _lock = support::session::launch_serial_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let mut outcome = launch_with_content("first second first\n");
    // move right a bit
    outcome.core_bridge.dispatch_key("l").unwrap();
    outcome.core_bridge.dispatch_key("l").unwrap();
    // try asterisk
    outcome.core_bridge.dispatch_key("*").unwrap();
    let snapshot = outcome.core_bridge.snapshot();
    assert_eq!(snapshot.cursor_col, 13);
}

#[test]
fn search_hash() {
    let _lock = support::session::launch_serial_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let mut outcome = launch_with_content("first second first\n");
    // move right to the second "first" (index 13)
    for _ in 0..13 {
        outcome.core_bridge.dispatch_key("l").unwrap();
    }
    // try hash
    outcome.core_bridge.dispatch_key("#").unwrap();
    let snapshot = outcome.core_bridge.snapshot();
    assert_eq!(snapshot.cursor_col, 0);
}
