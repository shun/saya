//! 統合テスト: terminal lifecycle と表示更新の検証
//!
//! terminal 切り替えと restore が起動終了で成立することを確認する。
//! file name、mode、dirty、status message が表示へ反映されることを確認する。
//! Requirements: 2.5, 3.1, 3.4

use std::io;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use saya::bootstrap::prepare_launch;
use saya::cli::{ConfigSource, InputSource, LaunchRequest};
use saya::editor_session::EditorSessionState;
use saya::screen_model::{ProjectionInput, project};
use saya::terminal_lifecycle::{TerminalBackend, TerminalLifecycle};

fn unique_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-integ-terminal-{name}-{nanos}"))
}

#[derive(Default)]
struct DummyBackend {
    calls: Vec<&'static str>,
}

impl TerminalBackend for DummyBackend {
    fn enable_raw_mode(&mut self) -> io::Result<()> {
        self.calls.push("enable_raw_mode");
        Ok(())
    }

    fn enter_alternate_screen(&mut self) -> io::Result<()> {
        self.calls.push("enter_alternate_screen");
        Ok(())
    }

    fn leave_alternate_screen(&mut self) -> io::Result<()> {
        self.calls.push("leave_alternate_screen");
        Ok(())
    }

    fn disable_raw_mode(&mut self) -> io::Result<()> {
        self.calls.push("disable_raw_mode");
        Ok(())
    }
}

// ---- 9.4.1: terminal 切り替えと restore の検証 ----

#[test]
fn terminal_lifecycle_start_and_restore() {
    let mut backend = DummyBackend::default();

    let session = TerminalLifecycle::start(&mut backend).expect("Terminal start");

    assert!(session.is_raw_mode_enabled());
    assert!(session.is_alternate_screen_enabled());

    let restore_result = session.restore();
    assert!(restore_result.is_ok());

    assert_eq!(
        backend.calls,
        vec![
            "enable_raw_mode",
            "enter_alternate_screen",
            "leave_alternate_screen",
            "disable_raw_mode",
        ]
    );
}

// ---- 9.4.2: file name, mode, dirty, status message が表示へ反映されること ----

#[test]
fn display_model_reflects_editor_state_and_messages() {
    let target_path = unique_path("display");
    std::fs::write(&target_path, "Hello\nWorld\n").unwrap();

    let mut outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::File(target_path.clone()),
        config_source: ConfigSource::Default,
        ..LaunchRequest::default()
    })
    .unwrap();

    let mut session_state = EditorSessionState::new(outcome.target_path.clone());

    // 初期状態
    let model = project(&ProjectionInput::new(
        &outcome.core_bridge.snapshot(),
        &session_state,
        None,
    ));

    assert_eq!(model.file_name, target_path.display().to_string());
    assert_eq!(model.mode_label, "NORMAL");
    assert!(!model.dirty);
    assert_eq!(model.status_message, None);

    // 編集とメッセージ設定
    outcome.core_bridge.dispatch_key("i").unwrap();
    outcome.core_bridge.dispatch_key("A").unwrap();
    outcome.core_bridge.dispatch_key("\x1b").unwrap();

    session_state.update_dirty(outcome.core_bridge.snapshot().dirty);
    session_state.record_save_failure("Permission denied".to_string());

    let model2 = project(&ProjectionInput::new(
        &outcome.core_bridge.snapshot(),
        &session_state,
        Some("Action failed"),
    ));

    assert_eq!(model2.mode_label, "NORMAL");
    assert!(model2.dirty);

    // transient_message が優先される想定
    assert_eq!(model2.status_message, Some("Action failed".to_string()));

    // transient なしなら save error が出るはず
    let model3 = project(&ProjectionInput::new(
        &outcome.core_bridge.snapshot(),
        &session_state,
        None,
    ));

    assert_eq!(
        model3.status_message,
        Some("保存失敗: Permission denied".to_string())
    );
}
