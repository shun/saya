//! 統合テスト: terminal lifecycle と表示更新の検証
//!
//! このファイルは `saya` の terminal and presentation integration suite です。
//!
//! 責務は host/application 層の terminal lifecycle、viewport、projection、
//! renderer、input/event-loop 連携に限定する。詳細な editing semantics は
//! ADR 0001 に従って `vim-core-rs` に委ねる。
//!
//! terminal 切り替えと restore が起動終了で成立することを確認する。
//! file name、mode、dirty、message line が表示へ反映されることを確認する。
//! Requirements: 2.5, 3.1, 3.4

use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use saya::bootstrap::prepare_launch;
use saya::cli::{ConfigSource, InitialCursorPosition, InputSource, LaunchRequest};
use saya::editor_session::EditorSessionState;
use saya::event_loop::{EventLoopCoordinator, LoopAction, UiEvent};
use saya::input_loop::{TerminalEventSource, run_terminal_input_loop};
use saya::input_router::{EditorIntent, KeyInput, resolve_intent};
use saya::screen_model::{ProjectionInput, project};
use saya::terminal_lifecycle::{TerminalBackend, TerminalLifecycle};
use saya::viewport::ViewportState;

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

// ---- 9.4.2: file name, mode, dirty, message line が表示へ反映されること ----

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
    assert_eq!(model.message_line, None);

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
    assert_eq!(model2.message_line, Some("Action failed".to_string()));

    // transient なしなら save error が出るはず
    let model3 = project(&ProjectionInput::new(
        &outcome.core_bridge.snapshot(),
        &session_state,
        None,
    ));

    assert_eq!(
        model3.message_line,
        Some("保存失敗: Permission denied".to_string())
    );
}

#[test]
fn ctrl_c_guidance_projects_into_message_line() {
    let mut outcome = prepare_launch(LaunchRequest::default()).unwrap();
    let mut session_state = EditorSessionState::new(outcome.target_path.clone());

    outcome.core_bridge.dispatch_key("i").unwrap();
    outcome.core_bridge.dispatch_key("X").unwrap();
    outcome.core_bridge.dispatch_key("\x1b").unwrap();
    outcome.core_bridge.dispatch_key("\u{3}").unwrap();

    let latest_message = outcome
        .core_bridge
        .take_pending_messages()
        .into_iter()
        .filter_map(|message| {
            let trimmed = message.content.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        })
        .last();

    session_state.update_dirty(outcome.core_bridge.snapshot().dirty);
    let model = project(&ProjectionInput::new(
        &outcome.core_bridge.snapshot(),
        &session_state,
        latest_message.as_deref(),
    ));

    assert!(
        model
            .message_line
            .as_deref()
            .is_some_and(|message| message.contains(":qa!")),
        "Ctrl+C guidance should surface in the message line: {:?}",
        model.message_line
    );
}

struct MockTerminalEventSource {
    polls: Vec<io::Result<bool>>,
    reads: Vec<io::Result<Event>>,
}

impl MockTerminalEventSource {
    fn new(polls: Vec<io::Result<bool>>, reads: Vec<io::Result<Event>>) -> Self {
        Self { polls, reads }
    }
}

impl TerminalEventSource for MockTerminalEventSource {
    fn poll(&mut self, _timeout: std::time::Duration) -> io::Result<bool> {
        if self.polls.is_empty() {
            return Ok(false);
        }
        self.polls.remove(0)
    }

    fn read(&mut self) -> io::Result<Event> {
        self.reads.remove(0)
    }
}

fn char_key_event(ch: char) -> Event {
    Event::Key(KeyEvent {
        code: KeyCode::Char(ch),
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    })
}

fn escape_key_event() -> Event {
    Event::Key(KeyEvent {
        code: KeyCode::Esc,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    })
}

fn terminal_suite_scope_statement() -> &'static str {
    "terminal and presentation integration suite for terminal lifecycle, viewport, projection, renderer, and input-event-loop integration"
}

#[test]
fn presentation_related_test_files_use_presentation_prefix_instead_of_wave6_prefix() {
    let tests_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let file_names: Vec<String> = std::fs::read_dir(&tests_dir)
        .expect("tests directory should be readable")
        .map(|entry| {
            entry
                .expect("test directory entry should be readable")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();

    assert!(
        file_names.contains(&"integration_terminal.rs".to_string()),
        "terminal suite should remain the anchor for presentation integration"
    );
    assert!(
        file_names.contains(&"integration_presentation_line_numbers.rs".to_string()),
        "line-number presentation suite should use the presentation-oriented naming convention"
    );
    assert!(
        !file_names.contains(&"integration_wave6_line_numbers.rs".to_string()),
        "wave6 naming should be retired from the presentation suite"
    );
}

#[test]
fn terminal_suite_scope_statement_stays_pinned_to_host_layer_presentation() {
    let statement = terminal_suite_scope_statement();

    assert!(
        statement.contains("terminal and presentation integration suite"),
        "suite ownership statement should stay explicit"
    );
    assert!(
        statement.contains("terminal lifecycle"),
        "suite ownership statement should keep terminal lifecycle responsibility visible"
    );
    assert!(
        statement.contains("viewport"),
        "suite ownership statement should keep viewport responsibility visible"
    );
    assert!(
        statement.contains("projection"),
        "suite ownership statement should keep projection responsibility visible"
    );
    assert!(
        statement.contains("input-event-loop"),
        "suite ownership statement should keep input/event-loop responsibility visible"
    );
    assert!(
        statement.contains("renderer"),
        "suite ownership statement should keep renderer responsibility visible"
    );
    assert!(
        !statement.contains("editing semantics"),
        "suite ownership statement must not drift into core-editing ownership"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn input_event_loop_and_projection_stay_consistent_across_one_edit_cycle() {
    let mut outcome = prepare_launch(LaunchRequest::default()).expect("起動が成功すること");
    let mut session_state = EditorSessionState::new(outcome.target_path.clone());
    let (mut coordinator, sender) = EventLoopCoordinator::with_capacity(8);
    let stop_requested = Arc::new(AtomicBool::new(false));
    let stop_for_thread = stop_requested.clone();
    let mut source = MockTerminalEventSource::new(
        vec![Ok(true), Ok(true), Ok(true), Ok(false), Ok(false)],
        vec![
            Ok(char_key_event('i')),
            Ok(char_key_event('H')),
            Ok(escape_key_event()),
        ],
    );

    let input_thread = thread::spawn(move || {
        run_terminal_input_loop(&mut source, sender, stop_for_thread);
    });

    let mut processed_inputs = 0usize;
    while processed_inputs < 3 {
        let action = coordinator.next_action().await;
        assert_eq!(action, LoopAction::NeedRedraw);

        for event in coordinator.drain_pending() {
            match event {
                UiEvent::Input(key) => {
                    processed_inputs += 1;
                    let intent = resolve_intent(&key);
                    match intent {
                        EditorIntent::EditKey(key_text) => {
                            outcome
                                .core_bridge
                                .dispatch_key(&key_text)
                                .expect("core への key dispatch が成功すること");
                        }
                        other => panic!("編集イベントだけを期待していたが {:?} を受信", other),
                    }
                }
                other => panic!("入力イベントだけを期待していたが {:?} を受信", other),
            }
        }
    }

    stop_requested.store(true, Ordering::Relaxed);
    input_thread.join().expect("input loop thread should stop");

    session_state.update_dirty(outcome.core_bridge.snapshot().dirty);
    let snapshot = outcome.core_bridge.snapshot();
    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

    assert_eq!(model.mode_label, "NORMAL");
    assert!(model.dirty, "統合更新サイクル後に dirty が投影されること");
    assert!(
        model.lines.iter().any(|line| line.contains('H')),
        "input_loop -> event_loop -> core_bridge -> screen_model で入力文字が反映されること: {:?}",
        model.lines
    );
    assert_eq!(usize::from(model.cursor_row), snapshot.cursor_row);
    assert_eq!(usize::from(model.cursor_col), snapshot.cursor_col);
}

#[tokio::test(flavor = "current_thread")]
async fn redraw_events_coalesce_without_dropping_non_redraw_events() {
    let mut outcome = prepare_launch(LaunchRequest::default()).expect("起動が成功すること");
    let mut session_state = EditorSessionState::new(outcome.target_path.clone());
    let (mut coordinator, sender) = EventLoopCoordinator::with_capacity(8);

    sender.send(UiEvent::Redraw).await.expect("redraw 1");
    sender.send(UiEvent::Redraw).await.expect("redraw 2");
    sender
        .send(UiEvent::Input(KeyInput::Char('i')))
        .await
        .expect("input 1");
    sender
        .send(UiEvent::Resize {
            columns: 80,
            rows: 5,
        })
        .await
        .expect("resize");
    sender.send(UiEvent::Redraw).await.expect("redraw 3");
    sender
        .send(UiEvent::Input(KeyInput::Char('H')))
        .await
        .expect("input 2");
    sender
        .send(UiEvent::Input(KeyInput::Escape))
        .await
        .expect("escape");

    let action = coordinator.next_action().await;
    assert_eq!(action, LoopAction::NeedRedraw);

    let drained = coordinator.drain_pending();
    log::debug!(
        "[test] drained mixed events after redraw coalescing: {:?}",
        drained
    );

    assert!(
        coordinator.take_redraw_pending(),
        "複数の redraw が 1 回分の redraw pending に集約されること"
    );
    assert!(
        !coordinator.take_redraw_pending(),
        "redraw pending は 1 回の消費で解除されること"
    );
    assert_eq!(
        drained.len(),
        4,
        "non-redraw イベントが欠落せず保持されること"
    );
    assert_eq!(
        drained[0],
        UiEvent::Input(KeyInput::Char('i')),
        "redraw の後に最初の入力が残ること"
    );
    assert_eq!(
        drained[1],
        UiEvent::Resize {
            columns: 80,
            rows: 5,
        },
        "resize イベントが残ること"
    );
    assert_eq!(
        drained[2],
        UiEvent::Input(KeyInput::Char('H')),
        "後続の入力が残ること"
    );
    assert_eq!(
        drained[3],
        UiEvent::Input(KeyInput::Escape),
        "終了系の入力も落ちないこと"
    );
    assert!(
        drained
            .iter()
            .all(|event| !matches!(event, UiEvent::Redraw))
    );

    let mut viewport = ViewportState::new();
    let mut body_height = 3usize;
    for event in drained {
        match event {
            UiEvent::Input(key) => {
                let intent = resolve_intent(&key);
                if let EditorIntent::EditKey(key_text) = intent {
                    outcome
                        .core_bridge
                        .dispatch_key(&key_text)
                        .expect("core への key dispatch が成功すること");
                }
            }
            UiEvent::Resize { rows, .. } => {
                body_height = usize::from(rows.saturating_sub(2).max(1));
            }
            UiEvent::Redraw | UiEvent::Shutdown(_) => unreachable!(),
        }
    }

    session_state.update_dirty(outcome.core_bridge.snapshot().dirty);
    let snapshot = outcome.core_bridge.snapshot();
    viewport.ensure_cursor_visible(
        snapshot.cursor_row,
        body_height,
        snapshot.text.lines().count().max(1),
    );
    let model = project(
        &ProjectionInput::new(&snapshot, &session_state, None)
            .with_viewport(viewport.top_line(), body_height),
    );

    assert!(model.dirty, "入力後の dirty 状態が投影されること");
    assert!(
        model.lines.iter().any(|line| line.contains('H')),
        "redraw が coalesce されても non-redraw の入力が画面へ反映されること"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn resize_event_refreshes_viewport_and_projection() {
    let target_path = unique_path("resize");
    std::fs::write(&target_path, "L1\nL2\nL3\nL4\nL5\nL6\n")
        .expect("resize 用のテストファイルの作成");

    let outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::File(target_path.clone()),
        initial_cursor: InitialCursorPosition::Line(5),
        ..LaunchRequest::default()
    })
    .expect("起動が成功すること");
    let session_state = EditorSessionState::new(outcome.target_path.clone());
    let snapshot = outcome.core_bridge.snapshot();
    let total_lines = snapshot.text.lines().count().max(1);

    let mut viewport = ViewportState::new();
    let initial_body_height = 4usize;
    viewport.ensure_cursor_visible(snapshot.cursor_row, initial_body_height, total_lines);
    let initial_model = project(
        &ProjectionInput::new(&snapshot, &session_state, None)
            .with_viewport(viewport.top_line(), initial_body_height),
    );

    assert_eq!(viewport.top_line(), 1);
    assert_eq!(
        initial_model.lines.first().map(String::as_str),
        Some("L2"),
        "初期描画は viewport に応じた可視範囲を投影すること"
    );
    assert_eq!(initial_model.cursor_row, 3);

    let (mut coordinator, sender) = EventLoopCoordinator::new();
    let stop_requested = Arc::new(AtomicBool::new(false));
    let stop_for_thread = stop_requested.clone();
    let mut source =
        MockTerminalEventSource::new(vec![Ok(true), Ok(false)], vec![Ok(Event::Resize(80, 4))]);

    let input_thread = thread::spawn(move || {
        run_terminal_input_loop(&mut source, sender, stop_for_thread);
    });

    let action = coordinator.next_action().await;
    assert_eq!(action, LoopAction::NeedRedraw);

    let mut resized_body_height = initial_body_height;
    for event in coordinator.drain_pending() {
        match event {
            UiEvent::Resize { columns, rows } => {
                log::debug!(
                    "[test] resize event observed: columns={}, rows={}",
                    columns,
                    rows
                );
                resized_body_height = usize::from(rows.saturating_sub(2).max(1));
            }
            other => panic!("resize 以外のイベントを想定していない: {:?}", other),
        }
    }

    assert_eq!(resized_body_height, 2);

    viewport.ensure_cursor_visible(snapshot.cursor_row, resized_body_height, total_lines);
    let resized_model = project(
        &ProjectionInput::new(&snapshot, &session_state, None)
            .with_viewport(viewport.top_line(), resized_body_height),
    );

    assert_eq!(viewport.top_line(), 3);
    assert_eq!(
        resized_model.lines.first().map(String::as_str),
        Some("L4"),
        "resize 後は viewport の先頭行が更新されること"
    );
    assert_eq!(resized_model.cursor_row, 1);

    stop_requested.store(true, Ordering::Relaxed);
    input_thread
        .join()
        .expect("input loop thread should stop after resize test");
    std::fs::remove_file(&target_path).expect("resize 用のテストファイルの削除");
}
