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

use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use saya::bootstrap::{launch_test_lock, prepare_launch};
use saya::cli::{ConfigSource, InitialCursorPosition, InputSource, LaunchRequest};
use saya::editor_session::EditorSessionState;
use saya::event_loop::{EventLoopCoordinator, LoopAction, UiEvent};
use saya::input_loop::{TerminalEventSource, run_terminal_input_loop};
use saya::input_router::{EditorIntent, KeyInput, resolve_intent};
use saya::screen_model::{
    PaneRect, ProjectionInput, WorkspaceProjectionInput, project, project_workspace,
};
use saya::search_query::{SearchVisibleQuery, SearchVisibleState};
use saya::terminal_lifecycle::{TerminalBackend, TerminalLifecycle};
use saya::viewport::ViewportState;
use saya::viewport::WindowViewportStore;

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
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
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
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
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

fn ctrl_char_key_event(ch: char) -> Event {
    Event::Key(KeyEvent {
        code: KeyCode::Char(ch),
        modifiers: KeyModifiers::CONTROL,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    })
}

fn terminal_suite_scope_statement() -> &'static str {
    "terminal and presentation integration suite for terminal lifecycle, viewport, projection, renderer, and input-event-loop integration"
}

fn project_workspace_from_snapshot(
    snapshot: &vim_core_rs::CoreSnapshot,
    session_state: &EditorSessionState,
) -> Result<saya::screen_model::WorkspaceScreenModel, saya::screen_model::WorkspaceProjectionError>
{
    let mut viewport_store = WindowViewportStore::new();
    viewport_store.sync_from_windows(&snapshot.windows);
    let search_states = BTreeMap::new();
    project_workspace(&WorkspaceProjectionInput {
        snapshot,
        session_state,
        visual_selection: None,
        search_states: &search_states,
        command_preview: None,
        core_message: None,
        system_warning: None,
        transient_info: None,
        viewport_store: &viewport_store,
        terminal_width: 240,
        terminal_height: 60,
    })
}

fn collect_search_states_for_snapshot(
    outcome: &mut saya::bootstrap::BootstrapOutcome,
    snapshot: &vim_core_rs::CoreSnapshot,
) -> BTreeMap<i32, SearchVisibleState> {
    snapshot
        .windows
        .iter()
        .map(|window| {
            let body_height = window.height.saturating_sub(1).max(1);
            let start_row = window.topline.max(1);
            let end_row = start_row + body_height.saturating_sub(1);
            let search_state = outcome
                .core_bridge
                .query_visible_search_state_for_window(
                    window.id,
                    SearchVisibleQuery { start_row, end_row },
                )
                .unwrap_or_else(|error| {
                    panic!(
                        "search state query should succeed for window_id={}: {:?}",
                        window.id, error
                    )
                });
            (window.id, search_state)
        })
        .collect()
}

fn assert_workspace_tracks_snapshot(
    snapshot: &vim_core_rs::CoreSnapshot,
    session_state: &EditorSessionState,
) {
    let model = project_workspace_from_snapshot(snapshot, session_state)
        .expect("workspace projection should succeed for tracked snapshots");
    assert_eq!(
        model.panes.len(),
        snapshot.windows.len(),
        "workspace projection should keep pane count in sync with the core snapshot"
    );
    assert_eq!(
        model.active_window_id,
        snapshot
            .active_window_id()
            .expect("active window should exist when split tracking is valid"),
        "workspace active pane should stay tied to snapshot.active_window_id()"
    );

    for window in &snapshot.windows {
        let pane = model
            .panes
            .iter()
            .find(|pane| pane.window_id == window.id)
            .unwrap_or_else(|| panic!("missing pane for window_id={}", window.id));
        assert_eq!(
            pane.rect,
            PaneRect::from_core_window(window),
            "renderer input should reflect core geometry for window_id={}",
            window.id
        );
        assert_eq!(
            pane.is_active,
            snapshot.active_window_id() == Some(window.id),
            "pane active flag should only mirror the active window id"
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WindowLayoutSummary {
    row: usize,
    col: usize,
    width: usize,
    height: usize,
    is_active: bool,
}

fn summarize_layout(snapshot: &vim_core_rs::CoreSnapshot) -> Vec<WindowLayoutSummary> {
    let active_window_id = snapshot.active_window_id();
    let mut summary = snapshot
        .windows
        .iter()
        .map(|window| WindowLayoutSummary {
            row: window.row,
            col: window.col,
            width: window.width,
            height: window.height,
            is_active: Some(window.id) == active_window_id,
        })
        .collect::<Vec<_>>();
    summary.sort_by_key(|window| (window.row, window.col, window.width, window.height));
    summary
}

fn active_window(snapshot: &vim_core_rs::CoreSnapshot) -> &vim_core_rs::CoreWindowInfo {
    let active_window_id = snapshot
        .active_window_id()
        .expect("active window should exist");
    snapshot
        .window(active_window_id)
        .unwrap_or_else(|| panic!("missing active window: window_id={active_window_id}"))
}

fn split_summary_for_command(split_command: &str) -> Vec<WindowLayoutSummary> {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut outcome = prepare_launch(LaunchRequest::default()).expect("起動が成功すること");
    outcome.core_bridge.set_screen_size(24, 80);
    if split_command.starts_with('\u{17}') {
        outcome
            .core_bridge
            .dispatch_key(split_command)
            .unwrap_or_else(|_| panic!("split key sequence should succeed: {split_command:?}"));
    } else {
        outcome
            .core_bridge
            .apply_ex_command(split_command)
            .unwrap_or_else(|_| panic!("split command should succeed: {split_command}"));
    }
    summarize_layout(&outcome.core_bridge.snapshot())
}

fn assert_active_window_matches_direction(
    snapshot: &vim_core_rs::CoreSnapshot,
    expected_direction: char,
) {
    let active = active_window(snapshot);
    match expected_direction {
        'h' => {
            let min_col = snapshot
                .windows
                .iter()
                .map(|window| window.col)
                .min()
                .unwrap();
            assert_eq!(
                active.col, min_col,
                "Ctrl-w h should move to the left neighbor"
            );
        }
        'l' => {
            let max_col = snapshot
                .windows
                .iter()
                .map(|window| window.col)
                .max()
                .unwrap();
            assert_eq!(
                active.col, max_col,
                "Ctrl-w l should move to the right neighbor"
            );
        }
        'k' => {
            let min_row = snapshot
                .windows
                .iter()
                .map(|window| window.row)
                .min()
                .unwrap();
            assert_eq!(
                active.row, min_row,
                "Ctrl-w k should move to the upper neighbor"
            );
        }
        'j' => {
            let max_row = snapshot
                .windows
                .iter()
                .map(|window| window.row)
                .max()
                .unwrap();
            assert_eq!(
                active.row, max_row,
                "Ctrl-w j should move to the lower neighbor"
            );
        }
        other => panic!("unsupported direction: {other}"),
    }
}

#[test]
fn workspace_projection_returns_explicit_failure_when_active_window_is_missing() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut outcome = prepare_launch(LaunchRequest::default()).expect("起動が成功すること");
    let session_state = EditorSessionState::new(outcome.target_path.clone());

    outcome.core_bridge.set_screen_size(24, 80);
    outcome
        .core_bridge
        .apply_ex_command(":split")
        .expect("split should succeed");
    let mut snapshot = outcome.core_bridge.snapshot();
    for window in snapshot.windows.iter_mut() {
        window.is_active = false;
    }

    let model = project_workspace_from_snapshot(&snapshot, &session_state);
    assert_eq!(
        model,
        Err(saya::screen_model::WorkspaceProjectionError::ActiveWindowMissing)
    );
}

#[test]
fn pty_smoke_renders_split_and_rollback_display() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let target_path = unique_path("pty-smoke");
    std::fs::write(&target_path, "alpha\nbeta\ngamma\n").expect("test target should be writable");
    let transcript_path = unique_path("pty-smoke-transcript");
    let cargo_target_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target");

    let build_output = Command::new("gtimeout")
        .arg("120")
        .env("VIM_CORE_FROM_SOURCE", "1")
        .arg("cargo")
        .arg("build")
        .arg("--bin")
        .arg("sy")
        .output()
        .expect("PTY smoke build should spawn");
    assert!(
        build_output.status.success(),
        "PTY smoke binary build should succeed: status={:?}\nstdout={}\nstderr={}",
        build_output.status,
        String::from_utf8_lossy(&build_output.stdout),
        String::from_utf8_lossy(&build_output.stderr)
    );

    let binary_path = cargo_target_dir.join("debug").join("sy");

    let output = Command::new("gtimeout")
        .arg("20")
        .arg("script")
        .arg("-q")
        .arg(&transcript_path)
        .arg("sh")
        .arg("-c")
        .arg("stty rows 24 cols 80; env TERM=xterm-256color VIM_CORE_FROM_SOURCE=1 SAYA_PTY_SMOKE=1 \"$1\" \"$2\"")
        .arg("sh")
        .arg(&binary_path)
        .arg(&target_path)
        .output()
        .expect("PTY smoke command should spawn");

    let transcript = std::fs::read_to_string(&transcript_path).unwrap_or_default();
    let _ = std::fs::remove_file(&target_path);
    let _ = std::fs::remove_file(&transcript_path);

    assert!(
        output.status.success(),
        "PTY smoke should exit cleanly: status={:?}\nstdout={}\nstderr={}\ntranscript={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        transcript
    );
    assert!(
        transcript.contains("[pty-smoke] initial draw"),
        "transcript should include the initial draw marker: {transcript}"
    );
    assert!(
        transcript.contains("[pty-smoke] split draw"),
        "transcript should include the split draw marker: {transcript}"
    );
    assert!(
        transcript.contains("[pty-smoke] rollback draw"),
        "transcript should include the rollback draw marker: {transcript}"
    );
    assert!(
        transcript.contains("[pty-smoke] resize draw"),
        "transcript should include the resize draw marker: {transcript}"
    );
    assert!(
        transcript.contains("[pty-smoke] save result"),
        "transcript should include the save result marker: {transcript}"
    );
    assert!(
        transcript.contains("[pty-smoke] quit reason: UserQuit"),
        "transcript should include the normal quit marker: {transcript}"
    );
    assert!(
        transcript.contains("[pty-smoke] force quit reason: UserForceQuit"),
        "transcript should include the force quit marker: {transcript}"
    );
    assert!(
        transcript.contains("pty-smoke") && transcript.contains("NORMAL"),
        "transcript should include rendered status line text: {transcript}"
    );
    assert!(
        transcript.contains("workspace projection failed: active window could not be resolved"),
        "transcript should include the rollback message line: {transcript}"
    );
}

fn dispatch_ctrl_w(outcome: &mut saya::bootstrap::BootstrapOutcome, command: char) {
    log::debug!("[test] dispatching Ctrl-w command: {}", command);
    let sequence = format!("\u{17}{command}");
    outcome
        .core_bridge
        .dispatch_key(&sequence)
        .expect("Ctrl-w command should be accepted");
}

fn dispatch_key_input(outcome: &mut saya::bootstrap::BootstrapOutcome, key: KeyInput) {
    let intent = resolve_intent(&key);
    match intent {
        EditorIntent::EditKey(key_text) => {
            log::debug!(
                "[test] dispatching key input through intent router: key={:?}, vim_key={:?}",
                key,
                key_text
            );
            outcome
                .core_bridge
                .dispatch_key(&key_text)
                .expect("key input should be accepted");
        }
        other => panic!(
            "unexpected non-edit intent in Ctrl-w sequence test: {:?}",
            other
        ),
    }
}

async fn dispatch_terminal_key_events_through_user_path(
    outcome: &mut saya::bootstrap::BootstrapOutcome,
    terminal_events: Vec<Event>,
) {
    let (mut coordinator, sender) = EventLoopCoordinator::with_capacity(8);
    let stop_requested = Arc::new(AtomicBool::new(false));
    let stop_for_thread = stop_requested.clone();
    let event_count = terminal_events.len();
    let mut source = MockTerminalEventSource::new(
        std::iter::repeat_with(|| Ok(true))
            .take(event_count)
            .chain([Ok(false), Ok(false)])
            .collect(),
        terminal_events.into_iter().map(Ok).collect(),
    );

    let input_thread = thread::spawn(move || {
        run_terminal_input_loop(&mut source, sender, stop_for_thread);
    });

    let mut processed_inputs = 0usize;
    while processed_inputs < event_count {
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
                                .expect("core should accept terminal-routed key input");
                        }
                        other => {
                            panic!(
                                "Ctrl-w integration path should stay on EditKey, got {:?}",
                                other
                            )
                        }
                    }
                }
                other => panic!(
                    "unexpected non-input event in key-only terminal path: {:?}",
                    other
                ),
            }
        }
    }

    stop_requested.store(true, Ordering::Relaxed);
    input_thread
        .join()
        .expect("input loop thread should stop after terminal key integration");
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
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
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
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
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
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
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

#[test]
fn split_and_ctrl_w_navigation_keep_window_ids_and_active_pane_in_sync() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut outcome = prepare_launch(LaunchRequest::default()).expect("起動が成功すること");
    let session_state = EditorSessionState::new(outcome.target_path.clone());

    outcome.core_bridge.set_screen_size(24, 80);
    outcome
        .core_bridge
        .apply_ex_command(":split")
        .expect("split should succeed");

    let split_snapshot = outcome.core_bridge.snapshot();
    log::debug!(
        "[test] split snapshot: active_window_id={:?}, windows={:?}",
        split_snapshot.active_window_id(),
        split_snapshot
            .windows
            .iter()
            .map(|window| (
                window.id,
                window.row,
                window.col,
                window.width,
                window.height
            ))
            .collect::<Vec<_>>()
    );
    assert_workspace_tracks_snapshot(&split_snapshot, &session_state);

    let active_window_id = split_snapshot
        .active_window_id()
        .expect("split should produce an active window");
    let mut moved_snapshot = None;
    let mut moved_key = None;
    for target_key in ['h', 'j', 'k', 'l'] {
        let before = outcome.core_bridge.snapshot();
        dispatch_ctrl_w(&mut outcome, target_key);
        let after = outcome.core_bridge.snapshot();
        if after.active_window_id() != before.active_window_id() {
            moved_key = Some(target_key);
            moved_snapshot = Some(after);
            break;
        }
    }

    let moved_snapshot =
        moved_snapshot.expect("one of h/j/k/l should switch the active window id after a split");
    let moved_key = moved_key.expect("a successful movement key should be recorded");
    log::debug!(
        "[test] moved snapshot: active_window_id={:?}, target_key={}, windows={:?}",
        moved_snapshot.active_window_id(),
        moved_key,
        moved_snapshot
            .windows
            .iter()
            .map(|window| (
                window.id,
                window.row,
                window.col,
                window.width,
                window.height
            ))
            .collect::<Vec<_>>()
    );
    assert_workspace_tracks_snapshot(&moved_snapshot, &session_state);

    assert_ne!(
        moved_snapshot.active_window_id(),
        Some(active_window_id),
        "Ctrl-w movement should switch the active window id"
    );
}

#[test]
fn ctrl_w_split_commands_match_ex_split_layout_and_active_pane() {
    let ex_split = split_summary_for_command(":split");
    let ctrl_w_split = split_summary_for_command("\u{17}s");
    let ex_vsplit = split_summary_for_command(":vsplit");
    let ctrl_w_vsplit = split_summary_for_command("\u{17}v");

    assert_eq!(
        ctrl_w_split, ex_split,
        "Ctrl-w s should match :split for pane count, geometry, and active pane"
    );
    assert_eq!(
        ctrl_w_vsplit, ex_vsplit,
        "Ctrl-w v should match :vsplit for pane count, geometry, and active pane"
    );
}

#[test]
fn ctrl_w_split_commands_work_when_prefix_and_target_arrive_as_separate_key_inputs() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut split_outcome = prepare_launch(LaunchRequest::default()).expect("起動が成功すること");
    split_outcome.core_bridge.set_screen_size(24, 80);
    dispatch_key_input(&mut split_outcome, KeyInput::Ctrl('w'));
    dispatch_key_input(&mut split_outcome, KeyInput::Char('s'));
    let split_snapshot = split_outcome.core_bridge.snapshot();
    drop(split_outcome);

    let mut vsplit_outcome = prepare_launch(LaunchRequest::default()).expect("起動が成功すること");
    vsplit_outcome.core_bridge.set_screen_size(24, 80);
    dispatch_key_input(&mut vsplit_outcome, KeyInput::Ctrl('w'));
    dispatch_key_input(&mut vsplit_outcome, KeyInput::Char('v'));
    let vsplit_snapshot = vsplit_outcome.core_bridge.snapshot();

    assert_eq!(
        split_snapshot.windows.len(),
        2,
        "Ctrl-w then s as separate input events should still create a horizontal split"
    );
    assert_eq!(
        vsplit_snapshot.windows.len(),
        2,
        "Ctrl-w then v as separate input events should still create a vertical split"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn ctrl_w_split_commands_work_when_prefix_and_target_arrive_as_separate_terminal_events() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut split_outcome = prepare_launch(LaunchRequest::default()).expect("起動が成功すること");
    split_outcome.core_bridge.set_screen_size(24, 80);
    dispatch_terminal_key_events_through_user_path(
        &mut split_outcome,
        vec![ctrl_char_key_event('w'), char_key_event('s')],
    )
    .await;
    let split_snapshot = split_outcome.core_bridge.snapshot();
    drop(split_outcome);

    let mut vsplit_outcome = prepare_launch(LaunchRequest::default()).expect("起動が成功すること");
    vsplit_outcome.core_bridge.set_screen_size(24, 80);
    dispatch_terminal_key_events_through_user_path(
        &mut vsplit_outcome,
        vec![ctrl_char_key_event('w'), char_key_event('v')],
    )
    .await;
    let vsplit_snapshot = vsplit_outcome.core_bridge.snapshot();

    assert_eq!(
        split_snapshot.windows.len(),
        2,
        "Ctrl-w then s as separate terminal events should still create a horizontal split"
    );
    assert_eq!(
        vsplit_snapshot.windows.len(),
        2,
        "Ctrl-w then v as separate terminal events should still create a vertical split"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn ctrl_w_navigation_commands_work_when_prefix_and_target_arrive_as_separate_terminal_events()
{
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut horizontal = prepare_launch(LaunchRequest::default()).expect("起動が成功すること");
    let horizontal_session_state = EditorSessionState::new(horizontal.target_path.clone());
    horizontal.core_bridge.set_screen_size(24, 80);
    horizontal
        .core_bridge
        .apply_ex_command(":vsplit")
        .expect("vsplit should succeed");

    dispatch_terminal_key_events_through_user_path(
        &mut horizontal,
        vec![ctrl_char_key_event('w'), char_key_event('h')],
    )
    .await;
    let after_h = horizontal.core_bridge.snapshot();
    assert_workspace_tracks_snapshot(&after_h, &horizontal_session_state);
    assert_active_window_matches_direction(&after_h, 'h');

    dispatch_terminal_key_events_through_user_path(
        &mut horizontal,
        vec![ctrl_char_key_event('w'), char_key_event('l')],
    )
    .await;
    let after_l = horizontal.core_bridge.snapshot();
    assert_workspace_tracks_snapshot(&after_l, &horizontal_session_state);
    assert_active_window_matches_direction(&after_l, 'l');
    drop(horizontal);

    let mut vertical = prepare_launch(LaunchRequest::default()).expect("起動が成功すること");
    let vertical_session_state = EditorSessionState::new(vertical.target_path.clone());
    vertical.core_bridge.set_screen_size(24, 80);
    vertical
        .core_bridge
        .apply_ex_command(":split")
        .expect("split should succeed");

    dispatch_terminal_key_events_through_user_path(
        &mut vertical,
        vec![ctrl_char_key_event('w'), char_key_event('k')],
    )
    .await;
    let after_k = vertical.core_bridge.snapshot();
    assert_workspace_tracks_snapshot(&after_k, &vertical_session_state);
    assert_active_window_matches_direction(&after_k, 'k');

    dispatch_terminal_key_events_through_user_path(
        &mut vertical,
        vec![ctrl_char_key_event('w'), char_key_event('j')],
    )
    .await;
    let after_j = vertical.core_bridge.snapshot();
    assert_workspace_tracks_snapshot(&after_j, &vertical_session_state);
    assert_active_window_matches_direction(&after_j, 'j');
}

#[tokio::test(flavor = "current_thread")]
async fn ctrl_w_reposition_commands_work_when_prefix_and_target_arrive_as_separate_terminal_events()
{
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut outcome = prepare_launch(LaunchRequest::default()).expect("起動が成功すること");
    let session_state = EditorSessionState::new(outcome.target_path.clone());

    outcome.core_bridge.set_screen_size(30, 120);
    outcome
        .core_bridge
        .apply_ex_command(":split")
        .expect("split should succeed");
    outcome
        .core_bridge
        .apply_ex_command(":vsplit")
        .expect("vsplit should succeed");

    for command in ['H', 'J', 'K', 'L'] {
        dispatch_terminal_key_events_through_user_path(
            &mut outcome,
            vec![ctrl_char_key_event('w'), char_key_event(command)],
        )
        .await;
        let snapshot = outcome.core_bridge.snapshot();
        assert_workspace_tracks_snapshot(&snapshot, &session_state);
        assert_active_window_matches_direction(&snapshot, command.to_ascii_lowercase());
    }
}

#[tokio::test(flavor = "current_thread")]
async fn ctrl_w_close_and_geometry_commands_work_when_prefix_and_target_arrive_as_separate_terminal_events()
 {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut close_outcome = prepare_launch(LaunchRequest::default()).expect("起動が成功すること");
    let close_session_state = EditorSessionState::new(close_outcome.target_path.clone());
    close_outcome.core_bridge.set_screen_size(24, 80);
    close_outcome
        .core_bridge
        .apply_ex_command(":vsplit")
        .expect("vsplit should succeed");

    dispatch_terminal_key_events_through_user_path(
        &mut close_outcome,
        vec![ctrl_char_key_event('w'), char_key_event('c')],
    )
    .await;
    let closed_snapshot = close_outcome.core_bridge.snapshot();
    assert_workspace_tracks_snapshot(&closed_snapshot, &close_session_state);
    assert_eq!(
        closed_snapshot.windows.len(),
        1,
        "Ctrl-w then c as separate terminal events should still close the active pane"
    );
    drop(close_outcome);

    let mut horizontal = prepare_launch(LaunchRequest::default()).expect("起動が成功すること");
    let horizontal_session_state = EditorSessionState::new(horizontal.target_path.clone());
    horizontal.core_bridge.set_screen_size(30, 120);
    horizontal
        .core_bridge
        .apply_ex_command(":split")
        .expect("split should succeed");

    dispatch_terminal_key_events_through_user_path(
        &mut horizontal,
        vec![ctrl_char_key_event('w'), char_key_event('+')],
    )
    .await;
    let after_grow = horizontal.core_bridge.snapshot();
    assert_workspace_tracks_snapshot(&after_grow, &horizontal_session_state);
    let grown_active_height = active_window(&after_grow).height;

    dispatch_terminal_key_events_through_user_path(
        &mut horizontal,
        vec![ctrl_char_key_event('w'), char_key_event('-')],
    )
    .await;
    let after_shrink = horizontal.core_bridge.snapshot();
    assert_workspace_tracks_snapshot(&after_shrink, &horizontal_session_state);
    let shrunk_active_height = active_window(&after_shrink).height;
    assert!(
        grown_active_height > shrunk_active_height,
        "Ctrl-w then +/- as separate terminal events should still change active pane height"
    );

    dispatch_terminal_key_events_through_user_path(
        &mut horizontal,
        vec![ctrl_char_key_event('w'), char_key_event('=')],
    )
    .await;
    let after_equalize = horizontal.core_bridge.snapshot();
    assert_workspace_tracks_snapshot(&after_equalize, &horizontal_session_state);
    let heights = after_equalize
        .windows
        .iter()
        .map(|window| window.height)
        .collect::<Vec<_>>();
    let min_height = *heights.iter().min().expect("split should keep heights");
    let max_height = *heights.iter().max().expect("split should keep heights");
    assert!(
        max_height.saturating_sub(min_height) <= 1,
        "Ctrl-w then = as separate terminal events should still rebalance heights: {:?}",
        heights
    );
    drop(horizontal);

    let mut vertical = prepare_launch(LaunchRequest::default()).expect("起動が成功すること");
    let vertical_session_state = EditorSessionState::new(vertical.target_path.clone());
    vertical.core_bridge.set_screen_size(30, 120);
    vertical
        .core_bridge
        .apply_ex_command(":vsplit")
        .expect("vsplit should succeed");

    dispatch_terminal_key_events_through_user_path(
        &mut vertical,
        vec![ctrl_char_key_event('w'), char_key_event('>')],
    )
    .await;
    let after_widen = vertical.core_bridge.snapshot();
    assert_workspace_tracks_snapshot(&after_widen, &vertical_session_state);
    let widened_active_width = active_window(&after_widen).width;

    dispatch_terminal_key_events_through_user_path(
        &mut vertical,
        vec![ctrl_char_key_event('w'), char_key_event('<')],
    )
    .await;
    let after_narrow = vertical.core_bridge.snapshot();
    assert_workspace_tracks_snapshot(&after_narrow, &vertical_session_state);
    let narrowed_active_width = active_window(&after_narrow).width;
    assert!(
        widened_active_width > narrowed_active_width,
        "Ctrl-w then <> as separate terminal events should still change active pane width"
    );
}

#[test]
fn ctrl_w_h_and_l_move_to_expected_horizontal_neighbors() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut outcome = prepare_launch(LaunchRequest::default()).expect("起動が成功すること");
    let session_state = EditorSessionState::new(outcome.target_path.clone());

    outcome.core_bridge.set_screen_size(24, 80);
    outcome
        .core_bridge
        .apply_ex_command(":vsplit")
        .expect("vsplit should succeed");

    let after_split = outcome.core_bridge.snapshot();
    assert_workspace_tracks_snapshot(&after_split, &session_state);

    dispatch_ctrl_w(&mut outcome, 'h');
    let after_h = outcome.core_bridge.snapshot();
    assert_workspace_tracks_snapshot(&after_h, &session_state);
    assert_active_window_matches_direction(&after_h, 'h');

    dispatch_ctrl_w(&mut outcome, 'l');
    let after_l = outcome.core_bridge.snapshot();
    assert_workspace_tracks_snapshot(&after_l, &session_state);
    assert_active_window_matches_direction(&after_l, 'l');
}

#[test]
fn ctrl_w_j_and_k_move_to_expected_vertical_neighbors() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut outcome = prepare_launch(LaunchRequest::default()).expect("起動が成功すること");
    let session_state = EditorSessionState::new(outcome.target_path.clone());

    outcome.core_bridge.set_screen_size(24, 80);
    outcome
        .core_bridge
        .apply_ex_command(":split")
        .expect("split should succeed");

    let after_split = outcome.core_bridge.snapshot();
    assert_workspace_tracks_snapshot(&after_split, &session_state);

    dispatch_ctrl_w(&mut outcome, 'k');
    let after_k = outcome.core_bridge.snapshot();
    assert_workspace_tracks_snapshot(&after_k, &session_state);
    assert_active_window_matches_direction(&after_k, 'k');

    dispatch_ctrl_w(&mut outcome, 'j');
    let after_j = outcome.core_bridge.snapshot();
    assert_workspace_tracks_snapshot(&after_j, &session_state);
    assert_active_window_matches_direction(&after_j, 'j');
}

#[test]
fn ctrl_w_close_removes_closed_window_from_viewport_tracking_and_workspace_projection() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut outcome = prepare_launch(LaunchRequest::default()).expect("起動が成功すること");
    let session_state = EditorSessionState::new(outcome.target_path.clone());
    let mut viewport_store = WindowViewportStore::new();

    outcome.core_bridge.set_screen_size(24, 80);
    outcome
        .core_bridge
        .apply_ex_command(":vsplit")
        .expect("vsplit should succeed");

    let split_snapshot = outcome.core_bridge.snapshot();
    viewport_store.sync_from_windows(&split_snapshot.windows);
    assert_eq!(viewport_store.len(), split_snapshot.windows.len());
    assert_workspace_tracks_snapshot(&split_snapshot, &session_state);

    let closed_window_id = split_snapshot
        .active_window_id()
        .expect("split should produce an active window");

    dispatch_ctrl_w(&mut outcome, 'c');

    let closed_snapshot = outcome.core_bridge.snapshot();
    viewport_store.sync_from_windows(&closed_snapshot.windows);
    log::debug!(
        "[test] close snapshot: active_window_id={:?}, windows={:?}",
        closed_snapshot.active_window_id(),
        closed_snapshot
            .windows
            .iter()
            .map(|window| (
                window.id,
                window.row,
                window.col,
                window.width,
                window.height
            ))
            .collect::<Vec<_>>()
    );

    assert_eq!(
        closed_snapshot.windows.len(),
        1,
        "Ctrl-w c should close exactly one window"
    );
    assert_eq!(
        viewport_store.len(),
        1,
        "closed windows should be pruned from viewport tracking"
    );
    assert!(
        viewport_store.get(closed_window_id).is_none(),
        "viewport tracking must drop the closed window id"
    );
    assert_workspace_tracks_snapshot(&closed_snapshot, &session_state);
}

#[test]
fn ctrl_w_geometry_commands_are_reflected_in_workspace_projection() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut outcome = prepare_launch(LaunchRequest::default()).expect("起動が成功すること");
    let session_state = EditorSessionState::new(outcome.target_path.clone());

    outcome.core_bridge.set_screen_size(30, 120);
    outcome
        .core_bridge
        .apply_ex_command(":split")
        .expect("split should succeed");
    assert_workspace_tracks_snapshot(&outcome.core_bridge.snapshot(), &session_state);

    for command in ['=', 'H', 'J', 'K', 'L', '+', '-', '<', '>'] {
        dispatch_ctrl_w(&mut outcome, command);
        let snapshot = outcome.core_bridge.snapshot();
        log::debug!(
            "[test] geometry command snapshot: command={}, active_window_id={:?}, windows={:?}",
            command,
            snapshot.active_window_id(),
            snapshot
                .windows
                .iter()
                .map(|window| (
                    window.id,
                    window.row,
                    window.col,
                    window.width,
                    window.height
                ))
                .collect::<Vec<_>>()
        );
        assert!(
            snapshot.windows.len() >= 2,
            "geometry commands should keep the split layout alive"
        );
        assert_workspace_tracks_snapshot(&snapshot, &session_state);
    }
}

#[test]
fn ctrl_w_reposition_commands_move_active_pane_to_requested_edge() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut outcome = prepare_launch(LaunchRequest::default()).expect("起動が成功すること");
    let session_state = EditorSessionState::new(outcome.target_path.clone());

    outcome.core_bridge.set_screen_size(30, 120);
    outcome
        .core_bridge
        .apply_ex_command(":split")
        .expect("split should succeed");
    outcome
        .core_bridge
        .apply_ex_command(":vsplit")
        .expect("vsplit should succeed");

    for command in ['H', 'J', 'K', 'L'] {
        dispatch_ctrl_w(&mut outcome, command);
        let snapshot = outcome.core_bridge.snapshot();
        assert_workspace_tracks_snapshot(&snapshot, &session_state);
        assert_active_window_matches_direction(&snapshot, command.to_ascii_lowercase());
    }
}

#[test]
fn ctrl_w_equal_and_resize_commands_follow_geometry_rules() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut outcome = prepare_launch(LaunchRequest::default()).expect("起動が成功すること");
    let session_state = EditorSessionState::new(outcome.target_path.clone());

    outcome.core_bridge.set_screen_size(30, 120);
    outcome
        .core_bridge
        .apply_ex_command(":split")
        .expect("split should succeed");

    dispatch_ctrl_w(&mut outcome, '+');
    let after_grow = outcome.core_bridge.snapshot();
    assert_workspace_tracks_snapshot(&after_grow, &session_state);
    let grown_active_height = active_window(&after_grow).height;

    dispatch_ctrl_w(&mut outcome, '-');
    let after_shrink = outcome.core_bridge.snapshot();
    assert_workspace_tracks_snapshot(&after_shrink, &session_state);
    let shrunk_active_height = active_window(&after_shrink).height;
    assert!(
        grown_active_height > shrunk_active_height,
        "Ctrl-w + should make the active pane taller than after Ctrl-w -"
    );

    dispatch_ctrl_w(&mut outcome, '=');
    let after_equalize = outcome.core_bridge.snapshot();
    assert_workspace_tracks_snapshot(&after_equalize, &session_state);
    let heights = after_equalize
        .windows
        .iter()
        .map(|window| window.height)
        .collect::<Vec<_>>();
    let min_height = *heights.iter().min().expect("split should keep heights");
    let max_height = *heights.iter().max().expect("split should keep heights");
    assert!(
        max_height.saturating_sub(min_height) <= 1,
        "Ctrl-w = should rebalance split heights to within one row: {:?}",
        heights
    );

    outcome
        .core_bridge
        .apply_ex_command(":only")
        .expect("only should reset the layout");
    outcome
        .core_bridge
        .apply_ex_command(":vsplit")
        .expect("vsplit should succeed");

    dispatch_ctrl_w(&mut outcome, '>');
    let after_widen = outcome.core_bridge.snapshot();
    assert_workspace_tracks_snapshot(&after_widen, &session_state);
    let widened_active_width = active_window(&after_widen).width;

    dispatch_ctrl_w(&mut outcome, '<');
    let after_narrow = outcome.core_bridge.snapshot();
    assert_workspace_tracks_snapshot(&after_narrow, &session_state);
    let narrowed_active_width = active_window(&after_narrow).width;
    assert!(
        widened_active_width > narrowed_active_width,
        "Ctrl-w > should make the active pane wider than after Ctrl-w <"
    );
}

#[test]
fn ctrl_w_close_on_last_window_keeps_layout_and_surfaces_message() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut outcome = prepare_launch(LaunchRequest::default()).expect("起動が成功すること");
    let session_state = EditorSessionState::new(outcome.target_path.clone());

    outcome.core_bridge.set_screen_size(24, 80);
    let before = outcome.core_bridge.snapshot();

    dispatch_ctrl_w(&mut outcome, 'c');

    let after = outcome.core_bridge.snapshot();
    assert_eq!(
        summarize_layout(&after),
        summarize_layout(&before),
        "Ctrl-w c on the last window must not mutate the layout or fake a fallback"
    );
    assert_workspace_tracks_snapshot(&after, &session_state);

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
        .last()
        .expect("closing the last window should surface a user-visible message");
    let mut viewport_store = WindowViewportStore::new();
    viewport_store.sync_from_windows(&after.windows);
    let search_states = BTreeMap::new();
    let workspace = project_workspace(&WorkspaceProjectionInput {
        snapshot: &after,
        session_state: &session_state,
        visual_selection: None,
        search_states: &search_states,
        command_preview: None,
        core_message: Some(latest_message.as_str()),
        system_warning: None,
        transient_info: None,
        viewport_store: &viewport_store,
        terminal_width: 80,
        terminal_height: 24,
    })
    .expect("workspace projection should succeed after failed close");
    assert!(
        latest_message.contains("close") || latest_message.contains("window"),
        "failure should mention that the last window cannot be closed: {latest_message}"
    );
    assert_eq!(
        workspace.global_message_line,
        Some(latest_message),
        "workspace projection should surface the close failure message"
    );
}

#[test]
fn split_focus_resize_keeps_inactive_pane_viewport_search_and_cursor_continuity() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("split-continuity");
    std::fs::write(
        &target_path,
        "alpha needle 01\nalpha 02\nalpha 03\nalpha 04\nalpha 05\nalpha 06\nalpha 07\nalpha needle 08\nalpha 09\nalpha 10\nalpha 11\nalpha needle 12\nalpha 13\nalpha 14\nalpha 15\nalpha 16\nalpha needle 17\nalpha 18\nalpha 19\nalpha 20\n",
    )
    .expect("continuity 用のテストファイルの作成");

    let mut outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::File(target_path.clone()),
        ..LaunchRequest::default()
    })
    .expect("起動が成功すること");
    let session_state = EditorSessionState::new(outcome.target_path.clone());
    let mut viewport_store = WindowViewportStore::new();

    outcome.core_bridge.set_screen_size(20, 80);
    outcome
        .core_bridge
        .apply_ex_command(":split")
        .expect("split should succeed");
    outcome
        .core_bridge
        .dispatch_key("/needle\r")
        .expect("search should succeed");
    dispatch_ctrl_w(&mut outcome, 'k');
    outcome
        .core_bridge
        .dispatch_key("G")
        .expect("top cursor move");
    dispatch_ctrl_w(&mut outcome, 'j');
    outcome
        .core_bridge
        .dispatch_key("g")
        .expect("bottom gg part 1");
    outcome
        .core_bridge
        .dispatch_key("g")
        .expect("bottom gg part 2");

    let before_resize = outcome.core_bridge.snapshot();
    viewport_store.sync_from_windows(&before_resize.windows);
    let before_search_states = collect_search_states_for_snapshot(&mut outcome, &before_resize);
    let before_model = project_workspace(&WorkspaceProjectionInput {
        snapshot: &before_resize,
        session_state: &session_state,
        visual_selection: None,
        search_states: &before_search_states,
        command_preview: None,
        core_message: None,
        system_warning: None,
        transient_info: None,
        viewport_store: &viewport_store,
        terminal_width: 80,
        terminal_height: 20,
    })
    .expect("workspace projection before resize should succeed");

    dispatch_ctrl_w(&mut outcome, 'k');
    dispatch_ctrl_w(&mut outcome, '+');

    let after_resize = outcome.core_bridge.snapshot();
    viewport_store.sync_from_windows(&after_resize.windows);
    let after_search_states = collect_search_states_for_snapshot(&mut outcome, &after_resize);
    let after_model = project_workspace(&WorkspaceProjectionInput {
        snapshot: &after_resize,
        session_state: &session_state,
        visual_selection: None,
        search_states: &after_search_states,
        command_preview: None,
        core_message: None,
        system_warning: None,
        transient_info: None,
        viewport_store: &viewport_store,
        terminal_width: 80,
        terminal_height: 20,
    })
    .expect("workspace projection after resize should succeed");

    for snapshot in [&before_resize, &after_resize] {
        assert_eq!(
            snapshot.windows.len(),
            2,
            "continuity scenario should keep both panes"
        );
    }

    for model in [&before_model, &after_model] {
        for pane in &model.panes {
            let window = if std::ptr::eq(model, &before_model) {
                before_resize
                    .window(pane.window_id)
                    .unwrap_or_else(|| panic!("missing before window: {}", pane.window_id))
            } else {
                after_resize
                    .window(pane.window_id)
                    .unwrap_or_else(|| panic!("missing after window: {}", pane.window_id))
            };
            let expected_first_line = if window.topline == 0 {
                ""
            } else {
                before_resize
                    .text
                    .lines()
                    .nth(window.topline - 1)
                    .unwrap_or("")
            };
            assert_eq!(
                pane.lines.first().map(String::as_str).unwrap_or(""),
                expected_first_line,
                "pane should preserve its own viewport top after split/focus/resize: window_id={}",
                pane.window_id
            );
            assert!(
                pane.search_overlays.iter().all(|overlay| {
                    let visible_height = usize::from(window.height.saturating_sub(1).max(1));
                    usize::from(overlay.row) < visible_height
                }),
                "search overlays should remain pane-local and in-bounds: window_id={}, overlays={:?}",
                pane.window_id,
                pane.search_overlays
            );
        }
    }

    let inactive_after_resize = after_model
        .panes
        .iter()
        .find(|pane| pane.window_id != after_model.active_window_id)
        .expect("inactive pane should exist after resize");
    let inactive_before_resize = before_model
        .panes
        .iter()
        .find(|pane| pane.window_id == inactive_after_resize.window_id)
        .expect("same inactive pane should exist before resize");
    let inactive_window = after_resize
        .window(inactive_after_resize.window_id)
        .expect("inactive window should exist after resize");
    assert_eq!(
        usize::from(inactive_after_resize.cursor_row),
        inactive_window
            .cursor_row
            .saturating_sub(inactive_window.topline.saturating_sub(1)),
        "inactive pane cursor should keep its own window-local continuity after focus+resize"
    );
    assert!(
        after_search_states
            .get(&inactive_after_resize.window_id)
            .expect("inactive search state")
            .window_id
            == inactive_after_resize.window_id,
        "inactive pane search state should stay bound to its own window_id after focus+resize"
    );
    assert_eq!(
        inactive_after_resize.lines.first(),
        inactive_before_resize.lines.first(),
        "inactive pane viewport top should not be replaced by the active pane after resize"
    );
    assert_eq!(
        inactive_after_resize.cursor_col, inactive_before_resize.cursor_col,
        "inactive pane cursor column should preserve continuity across focus+resize"
    );

    std::fs::remove_file(&target_path).expect("continuity 用のテストファイルの削除");
}
