use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use saya::bootstrap::{BootstrapOutcome, prepare_launch};
use saya::cli::{ConfigSource, InputSource, LaunchRequest};
use saya::editor_session::EditorSessionState;
use saya::screen_model::{ProjectionInput, project};
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

#[test]
fn search_starts_and_executes() {
    let _lock = saya::bootstrap::launch_test_lock().lock().unwrap_or_else(|p| p.into_inner());
    let mut outcome = launch_with_content("hello\nsearch test\nworld\n");
    let _session_state = EditorSessionState::new(outcome.target_path.clone());

    // / starts search. Since main loop handles / mapping to command_line_prompt, 
    // the integration of core_bridge handling '/' search directly can also be tested.
    // However, our `main.rs` intercepts `/` and then dispatches `/{cmd}\r`.
    
    // We can at least test `vim-core-rs` behavior through `core_bridge` 
    // to ensure dispatching `/{cmd}\r` moves the cursor.
    outcome.core_bridge.dispatch_key("/search\r").expect("dispatch search");
    let snapshot = outcome.core_bridge.snapshot();
    assert_eq!(snapshot.mode, CoreMode::Normal);
    assert_eq!(snapshot.cursor_row, 1); // 0-indexed, "search test" is on line 2 (index 1)
}

#[test]
fn next_previous_search_results() {
    let _lock = saya::bootstrap::launch_test_lock().lock().unwrap_or_else(|p| p.into_inner());
    let mut outcome = launch_with_content("word\ntext\nword\nhello\nword\n");
    
    outcome.core_bridge.dispatch_key("/word\r").expect("dispatch search");
    let snapshot = outcome.core_bridge.snapshot();
    assert_eq!(snapshot.cursor_row, 2); // skips the first one because we are at line 0, next is line 2

    outcome.core_bridge.dispatch_key("n").expect("next");
    let snapshot = outcome.core_bridge.snapshot();
    assert_eq!(snapshot.cursor_row, 4);

    outcome.core_bridge.dispatch_key("N").expect("prev");
    let snapshot = outcome.core_bridge.snapshot();
    assert_eq!(snapshot.cursor_row, 2);
}

#[test]
fn search_not_found_message() {
    let _lock = saya::bootstrap::launch_test_lock().lock().unwrap_or_else(|p| p.into_inner());
    let mut outcome = launch_with_content("hello\nworld\n");
    let _session_state = EditorSessionState::new(outcome.target_path.clone());
    
    // search for non-existent word
    outcome.core_bridge.dispatch_key("/missing\r").unwrap();
    let messages = outcome.core_bridge.take_pending_messages();
    
    let error_msg = messages.into_iter()
        .filter(|msg| msg.category.is_user_visible())
        .last()
        .expect("Should have a user visible message");
    
    assert!(error_msg.content.contains("E486: Pattern not found: missing"));

    let snapshot = outcome.core_bridge.snapshot();
    let model = project(&ProjectionInput::new(
        &snapshot,
        &_session_state,
        Some(error_msg.content.trim()),
    ));

    assert_eq!(model.message_line, Some("E486: Pattern not found: missing".to_string()));
}

#[test]
fn search_navigation_failure() {
    let _lock = saya::bootstrap::launch_test_lock().lock().unwrap_or_else(|p| p.into_inner());
    let mut outcome = launch_with_content("hello\nworld\n");
    let _session_state = EditorSessionState::new(outcome.target_path.clone());
    
    // search navigation 'n' without prior search
    outcome.core_bridge.dispatch_key("n").unwrap();
    let messages = outcome.core_bridge.take_pending_messages();
    
    let error_msg = messages.into_iter()
        .filter(|msg| msg.category.is_user_visible())
        .last()
        .expect("Should have a user visible message");
    
    assert!(
        error_msg.content.contains("E35: No previous regular expression") ||
        error_msg.content.contains("E486: Pattern not found"),
        "Unexpected error message: {}", error_msg.content
    );
}

#[test]
fn search_exact_word() {
    let _lock = saya::bootstrap::launch_test_lock().lock().unwrap_or_else(|p| p.into_inner());
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
    let _lock = saya::bootstrap::launch_test_lock().lock().unwrap_or_else(|p| p.into_inner());
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
    let _lock = saya::bootstrap::launch_test_lock().lock().unwrap_or_else(|p| p.into_inner());
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

#[test]
fn search_prompt_and_cancel_flow() {
    let _lock = saya::bootstrap::launch_test_lock().lock().unwrap_or_else(|p| p.into_inner());
    let mut outcome = launch_with_content("hello\nsearch test\nworld\n");
    let session_state = EditorSessionState::new(outcome.target_path.clone());

    // 1. Initial state: Normal mode, no prompt
    let snapshot = outcome.core_bridge.snapshot();
    assert_eq!(snapshot.mode, CoreMode::Normal);
    
    // Simulate pressing '/' to start search
    let mut command_line_prompt = Some('/');
    let mut command_line_buffer = String::new();
    
    let model = project(&ProjectionInput::new(
        &snapshot,
        &session_state,
        Some(&format!("{}{}", command_line_prompt.unwrap(), command_line_buffer)),
    ));
    assert_eq!(model.message_line.as_deref(), Some("/"));
    
    // Simulate typing "world"
    command_line_buffer.push_str("world");
    let model = project(&ProjectionInput::new(
        &snapshot,
        &session_state,
        Some(&format!("{}{}", command_line_prompt.unwrap(), command_line_buffer)),
    ));
    assert_eq!(model.message_line.as_deref(), Some("/world"));

    // Simulate pressing Esc (cancel search)
    command_line_prompt = None;
    command_line_buffer.clear();
    let model = project(&ProjectionInput::new(
        &snapshot,
        &session_state,
        None,
    ));
    assert_eq!(model.message_line, None);

    // Assert cursor hasn't moved
    assert_eq!(outcome.core_bridge.snapshot().cursor_row, 0);

    // Simulate pressing '/' again
    command_line_prompt = Some('/');
    command_line_buffer.push_str("search");

    // Simulate pressing Enter (execute search)
    let prompt = command_line_prompt.take().unwrap();
    let cmd = format!("{}{}", prompt, command_line_buffer);
    command_line_buffer.clear();

    if cmd.starts_with('/') {
        let search_keys = format!("{}\r", cmd);
        outcome.core_bridge.dispatch_key(&search_keys).unwrap();
    }

    let snapshot = outcome.core_bridge.snapshot();
    assert_eq!(snapshot.mode, CoreMode::Normal);
    assert_eq!(snapshot.cursor_row, 1); // "search test" is on row 1

    let model = project(&ProjectionInput::new(
        &snapshot,
        &session_state,
        None,
    ));
    assert_eq!(model.message_line, None);
}

