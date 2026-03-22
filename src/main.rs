use saya::bootstrap::{BootstrapError, prepare_launch};
use saya::cli::{CliParseError, parse_launch_request};
use saya::editor_session::{EditorSessionState, QuitDecision};
use saya::event_loop::{EventLoopCoordinator, LoopAction, UiEvent};
use saya::host_io::{SaveResult, write_to_path};
use saya::input_router::{EditorIntent, KeyInput, resolve_intent};
use saya::screen_model::{ProjectionInput, project};
use saya::terminal_lifecycle::TerminalLifecycle;
use saya::tui_renderer::{CrosstermBackendImpl, TuiRenderer};
use vim_core_rs::CoreMode;

use crossterm::event::{Event, KeyCode, KeyModifiers};

#[tokio::main]
async fn main() {
    let launch_request = match parse_launch_request(std::env::args_os().skip(1)) {
        Ok(request) => request,
        Err(error) => {
            log::debug!("{}", format_cli_error(error));
            std::process::exit(1);
        }
    };

    let mut outcome = match prepare_launch(launch_request) {
        Ok(outcome) => outcome,
        Err(error) => {
            log::debug!("{}", format_bootstrap_error(error));
            std::process::exit(1);
        }
    };

    // UI 初期化
    let mut backend = CrosstermBackendImpl;
    let terminal_session = match TerminalLifecycle::start(&mut backend) {
        Ok(session) => session,
        Err(e) => {
            log::debug!("Terminal init failed: {:?}", e);
            std::process::exit(1);
        }
    };

    let mut renderer = TuiRenderer::new().expect("TUI Renderer init failed");
    let mut session_state = EditorSessionState::new_with_tab_size(
        outcome.target_path.clone(),
        outcome.initial_tab_size,
    );
    let mut transient_msg: Option<String> = None;

    // イベントループ初期化
    let (mut coordinator, sender) = EventLoopCoordinator::new();

    // 入力監視タスク
    let input_sender = sender.clone();
    tokio::task::spawn_blocking(move || {
        log::debug!("[main] input thread started");
        loop {
            log::debug!("[main] waiting for event");
            if let Ok(event) = crossterm::event::read() {
                log::debug!("[main] raw event: {:?}", event);
                match event {
                    Event::Key(ke) => {
                        let ki = match ke.code {
                            KeyCode::Char(c) => {
                                if ke.modifiers.contains(KeyModifiers::CONTROL) {
                                    Some(KeyInput::Ctrl(c))
                                } else {
                                    Some(KeyInput::Char(c))
                                }
                            }
                            KeyCode::Esc => Some(KeyInput::Escape),
                            KeyCode::Enter => Some(KeyInput::Enter),
                            KeyCode::Backspace => Some(KeyInput::Backspace),
                            _ => None,
                        };
                        if let Some(key) = ki {
                            log::debug!("[main] got key: {:?}", key);
                            if input_sender.blocking_send(UiEvent::Input(key)).is_err() {
                                break;
                            }
                        }
                    }
                    Event::Resize(cols, rows) => {
                        if input_sender
                            .blocking_send(UiEvent::Resize {
                                columns: cols,
                                rows,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    _ => {}
                }
            } else {
                break;
            }
        }
    });

    let mut command_line_mode = false;
    let mut command_line_buffer = String::new();

    // 初期描画
    let snapshot = outcome.core_bridge.snapshot();
    let model = project(&ProjectionInput {
        snapshot: &snapshot,
        session_state: &session_state,
        transient_message: transient_msg.as_deref(),
    });
    let _ = renderer.draw(&model);

    // メインループ
    'main: loop {
        let action = coordinator.next_action().await;

        let events_to_process = coordinator.drain_pending();
        // action が NeedRedraw などで event 自体が drained に含まれないことは修正済みなので
        // drained に Input などのイベントが入っている。
        // ※ next_action が Exit なら終了処理
        if let LoopAction::Exit(_) = action {
            break 'main;
        }

        let mut need_redraw = coordinator.take_redraw_pending();

        for event in events_to_process {
            match event {
                UiEvent::Input(key) => {
                    let mut handled = false;

                    if command_line_mode {
                        match key {
                            KeyInput::Escape => {
                                command_line_mode = false;
                                command_line_buffer.clear();
                                transient_msg = None;
                            }
                            KeyInput::Enter => {
                                let cmd = format!(":{}", command_line_buffer);
                                command_line_mode = false;
                                command_line_buffer.clear();
                                transient_msg = None;
                                let _ = outcome.core_bridge.apply_ex_command(&cmd);
                                session_state.update_dirty(outcome.core_bridge.snapshot().dirty);
                            }
                            KeyInput::Backspace => {
                                if command_line_buffer.pop().is_none() {
                                    command_line_mode = false;
                                    transient_msg = None;
                                } else {
                                    transient_msg = Some(format!(":{}", command_line_buffer));
                                }
                            }
                            KeyInput::Char(c) => {
                                command_line_buffer.push(c);
                                transient_msg = Some(format!(":{}", command_line_buffer));
                            }
                            _ => {}
                        }
                        handled = true;
                        need_redraw = true;

                        for action in outcome.core_bridge.take_pending_host_actions() {
                            match action {
                                vim_core_rs::CoreHostAction::Write { .. } => {
                                    let snapshot = outcome.core_bridge.snapshot();
                                    if let Ok(req) =
                                        session_state.build_save_request(&snapshot.text)
                                    {
                                        match write_to_path(&req) {
                                            SaveResult::Saved => {
                                                session_state.record_save_success();
                                                transient_msg =
                                                    Some("Saved successfully".to_string());
                                            }
                                            SaveResult::Failed { message } => {
                                                session_state.record_save_failure(message);
                                                transient_msg = Some(format!(
                                                    "Save failed: {}",
                                                    session_state.last_save_error().unwrap_or("")
                                                ));
                                            }
                                        }
                                    } else {
                                        transient_msg = Some("No file name to save".to_string());
                                    }
                                }
                                vim_core_rs::CoreHostAction::Quit { force, .. } => {
                                    match session_state.evaluate_quit(force) {
                                        QuitDecision::Allow | QuitDecision::ForceQuit => {
                                            drop(terminal_session);
                                            std::process::exit(0);
                                        }
                                        QuitDecision::WarnUnsaved => {
                                            transient_msg = Some(
                                                "No write since last change (add ! to override)"
                                                    .to_string(),
                                            );
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    } else if let KeyInput::Char(':') = key
                        && outcome.core_bridge.snapshot().mode == CoreMode::Normal
                    {
                        command_line_mode = true;
                        command_line_buffer.clear();
                        transient_msg = Some(":".to_string());
                        handled = true;
                        need_redraw = true;
                    }

                    if !handled {
                        let intent = resolve_intent(&key);
                        match intent {
                            EditorIntent::EditKey(k) => {
                                let _ = outcome.core_bridge.dispatch_key(&k);

                                for action in outcome.core_bridge.take_pending_host_actions() {
                                    match action {
                                        vim_core_rs::CoreHostAction::Write { .. } => {
                                            let snapshot = outcome.core_bridge.snapshot();
                                            if let Ok(req) =
                                                session_state.build_save_request(&snapshot.text)
                                            {
                                                match write_to_path(&req) {
                                                    SaveResult::Saved => {
                                                        session_state.record_save_success();
                                                        transient_msg =
                                                            Some("Saved successfully".to_string());
                                                    }
                                                    SaveResult::Failed { message } => {
                                                        session_state.record_save_failure(message);
                                                        transient_msg = Some(format!(
                                                            "Save failed: {}",
                                                            session_state
                                                                .last_save_error()
                                                                .unwrap_or("")
                                                        ));
                                                    }
                                                }
                                            } else {
                                                transient_msg =
                                                    Some("No file name to save".to_string());
                                            }
                                        }
                                        vim_core_rs::CoreHostAction::Quit { force, .. } => {
                                            match session_state.evaluate_quit(force) {
                                                QuitDecision::Allow | QuitDecision::ForceQuit => {
                                                    drop(terminal_session);
                                                    std::process::exit(0);
                                                }
                                                QuitDecision::WarnUnsaved => {
                                                    transient_msg = Some("No write since last change (add ! to override)".to_string());
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                }

                                session_state.update_dirty(outcome.core_bridge.snapshot().dirty);
                                need_redraw = true;
                            }
                            EditorIntent::Save => {
                                let snapshot = outcome.core_bridge.snapshot();
                                if let Ok(req) = session_state.build_save_request(&snapshot.text) {
                                    match write_to_path(&req) {
                                        SaveResult::Saved => {
                                            session_state.record_save_success();
                                            transient_msg = Some("Saved successfully".to_string());
                                        }
                                        SaveResult::Failed { message } => {
                                            session_state.record_save_failure(message);
                                            transient_msg = Some(format!(
                                                "Save failed: {}",
                                                session_state.last_save_error().unwrap_or("")
                                            ));
                                        }
                                    }
                                } else {
                                    transient_msg = Some("No file name to save".to_string());
                                }
                                need_redraw = true;
                            }
                            EditorIntent::Quit { force } => {
                                match session_state.evaluate_quit(force) {
                                    QuitDecision::Allow | QuitDecision::ForceQuit => {
                                        break 'main;
                                    }
                                    QuitDecision::WarnUnsaved => {
                                        transient_msg = Some(
                                            "No write since last change (add force to override)"
                                                .to_string(),
                                        );
                                        need_redraw = true;
                                    }
                                }
                            }
                        }
                    }
                }
                UiEvent::Resize { .. } => {
                    need_redraw = true;
                }
                UiEvent::Shutdown(_) => {
                    break 'main;
                }
                _ => {}
            }
        }

        if need_redraw {
            let snapshot = outcome.core_bridge.snapshot();
            let model = project(&ProjectionInput {
                snapshot: &snapshot,
                session_state: &session_state,
                transient_message: transient_msg.as_deref(),
            });
            let _ = renderer.draw(&model);
        }
    }

    drop(terminal_session);
    std::process::exit(0);
}

fn format_cli_error(error: CliParseError) -> String {
    match error {
        CliParseError::MissingConfigPath => "設定ファイルのパスが指定されていません".to_string(),
        CliParseError::MultipleTargetPaths => "対象ファイルは 1 つだけ指定できます".to_string(),
        CliParseError::UnknownFlag(flag) => {
            format!("未対応のオプションです: {}", flag.to_string_lossy())
        }
    }
}

fn format_bootstrap_error(error: BootstrapError) -> String {
    match error {
        BootstrapError::SessionAlreadyInitialized => {
            "エディタのセッションはすでに初期化されています".to_string()
        }
        BootstrapError::TargetReadFailed { path, message } => {
            format!(
                "対象ファイルを読み込めませんでした ({}): {}",
                path.display(),
                message
            )
        }
    }
}
