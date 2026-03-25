use saya::bootstrap::{BootstrapError, prepare_launch};
use saya::cli::{CliParseError, StartupAction, parse_launch_request};
use saya::editor_session::{QuitDecision, SaveRequestError};
use saya::event_loop::{EventLoopCoordinator, LoopAction, ShutdownReason, UiEvent};
use saya::ex_command::apply_local_ex_command;
use saya::host_io::{SaveResult, write_to_path};
use saya::input_loop::{CrosstermEventSource, run_terminal_input_loop};
use saya::input_router::{EditorIntent, KeyInput, resolve_intent};
use saya::screen_model::{ProjectionInput, project};
use saya::terminal_lifecycle::TerminalLifecycle;
use saya::tui_renderer::{CrosstermBackendImpl, TuiRenderer};
use saya::viewport::ViewportState;
use vim_core_rs::{CoreMessageEvent, CoreMode};

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use vim_core_rs::CoreHostAction;

#[tokio::main]
async fn main() {
    let launch_request = match parse_launch_request(std::env::args_os().skip(1)) {
        Ok(request) => request,
        Err(error) => {
            log::debug!("{}", format_cli_error(error));
            std::process::exit(1);
        }
    };

    match &launch_request.startup_action {
        StartupAction::Edit => {}
        StartupAction::PrintHelp => {
            println!("{}", render_help_text());
            std::process::exit(0);
        }
        StartupAction::PrintVersion => {
            println!("{}", render_version_text());
            std::process::exit(0);
        }
    }

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
    let mut session_state = outcome.editor_session_state();
    let mut transient_msg: Option<String> = None;
    let mut viewport = ViewportState::new();
    let mut command_line_mode = false;
    let mut command_line_buffer = String::new();

    // イベントループ初期化
    let (mut coordinator, sender) = EventLoopCoordinator::new();

    // 入力監視タスク
    let input_sender = sender.clone();
    let input_stop_requested = Arc::new(AtomicBool::new(false));
    let input_stop_for_task = input_stop_requested.clone();
    let input_task = tokio::task::spawn_blocking(move || {
        let mut source = CrosstermEventSource;
        run_terminal_input_loop(&mut source, input_sender, input_stop_for_task);
    });

    // 初期描画
    let snapshot = outcome.core_bridge.snapshot();
    let visual_selection = outcome.core_bridge.current_visual_selection();
    let body_height = current_body_height();
    viewport.ensure_cursor_visible(
        snapshot.cursor_row,
        body_height,
        buffer_line_count(&snapshot.text),
    );
    let model = project(
        &ProjectionInput::new(
            &snapshot,
            &session_state,
            visible_message_line(
                command_line_mode,
                &command_line_buffer,
                transient_msg.as_deref(),
            )
            .as_deref(),
        )
        .with_visual_selection(visual_selection.as_ref())
        .with_viewport(viewport.top_line(), body_height),
    );
    let _ = renderer.draw(&model);

    // メインループ
    let shutdown_reason = 'main: loop {
        let action = coordinator.next_action().await;

        let events_to_process = coordinator.drain_pending();
        // action が NeedRedraw などで event 自体が drained に含まれないことは修正済みなので
        // drained に Input などのイベントが入っている。
        // ※ next_action が Exit なら終了処理
        if let LoopAction::Exit(reason) = action {
            log::debug!("[main] coordinator requested shutdown: reason={:?}", reason);
            break 'main reason;
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
                            }
                            KeyInput::Enter => {
                                let cmd = format!(":{}", command_line_buffer);
                                command_line_mode = false;
                                command_line_buffer.clear();
                                if let Some(message) =
                                    apply_local_ex_command(&mut session_state, &cmd)
                                {
                                    transient_msg = Some(message);
                                } else {
                                    let _ = outcome.core_bridge.apply_ex_command(&cmd);
                                    update_transient_message_from_core(
                                        &mut outcome.core_bridge,
                                        &mut transient_msg,
                                    );
                                }
                                session_state.update_dirty(outcome.core_bridge.snapshot().dirty);
                            }
                            KeyInput::Backspace => {
                                if command_line_buffer.pop().is_none() {
                                    command_line_mode = false;
                                }
                            }
                            KeyInput::Char(c) => {
                                command_line_buffer.push(c);
                            }
                            _ => {}
                        }
                        handled = true;
                        need_redraw = true;

                        if let Some(reason) = process_pending_host_actions(
                            &mut outcome,
                            &mut session_state,
                            &mut transient_msg,
                        ) {
                            break 'main reason;
                        }
                    } else if let KeyInput::Char(':') = key
                        && outcome.core_bridge.snapshot().mode == CoreMode::Normal
                    {
                        command_line_mode = true;
                        command_line_buffer.clear();
                        handled = true;
                        need_redraw = true;
                    }

                    if !handled {
                        let intent = resolve_intent(&key);
                        match intent {
                            EditorIntent::EditKey(k) => {
                                let _ = outcome.core_bridge.dispatch_key(&k);
                                update_transient_message_from_core(
                                    &mut outcome.core_bridge,
                                    &mut transient_msg,
                                );

                                if let Some(reason) = process_pending_host_actions(
                                    &mut outcome,
                                    &mut session_state,
                                    &mut transient_msg,
                                ) {
                                    break 'main reason;
                                }

                                session_state.update_dirty(outcome.core_bridge.snapshot().dirty);
                                need_redraw = true;
                            }
                            EditorIntent::Save => {
                                let snapshot = outcome.core_bridge.snapshot();
                                transient_msg = save_snapshot(&snapshot.text, &mut session_state);
                                need_redraw = true;
                            }
                            EditorIntent::Quit { force } => {
                                let decision = session_state.evaluate_quit(force);
                                if let Some(reason) = shutdown_reason_from_quit_decision(
                                    decision,
                                    force,
                                    &mut transient_msg,
                                ) {
                                    break 'main reason;
                                }
                                need_redraw = true;
                            }
                        }
                    }
                }
                UiEvent::Resize { .. } => {
                    need_redraw = true;
                }
                UiEvent::Shutdown(reason) => {
                    log::debug!(
                        "[main] explicit shutdown event received in drain: reason={:?}",
                        reason
                    );
                    break 'main reason;
                }
                _ => {}
            }
        }

        if need_redraw {
            let snapshot = outcome.core_bridge.snapshot();
            let visual_selection = outcome.core_bridge.current_visual_selection();
            let body_height = current_body_height();
            viewport.ensure_cursor_visible(
                snapshot.cursor_row,
                body_height,
                buffer_line_count(&snapshot.text),
            );
            let model = project(
                &ProjectionInput::new(
                    &snapshot,
                    &session_state,
                    visible_message_line(
                        command_line_mode,
                        &command_line_buffer,
                        transient_msg.as_deref(),
                    )
                    .as_deref(),
                )
                .with_visual_selection(visual_selection.as_ref())
                .with_viewport(viewport.top_line(), body_height),
            );
            let _ = renderer.draw(&model);
        }
    };

    log::debug!(
        "[main] beginning unified shutdown: reason={:?}",
        shutdown_reason
    );
    let mut shutdown_sequence = coordinator.begin_shutdown(shutdown_reason);
    shutdown_sequence.record_loop_stopped();

    log::debug!("[main] requesting input loop shutdown");
    input_stop_requested.store(true, Ordering::Relaxed);
    drop(sender);
    if let Err(error) = input_task.await {
        log::debug!("[main] input task join failed: {}", error);
    }

    drop(renderer);
    log::debug!("[main] dropping editor outcome for session cleanup");
    drop(outcome);
    shutdown_sequence.record_session_released();

    let restore_result = terminal_session
        .restore()
        .map_err(|error| format!("{error:?}"));
    shutdown_sequence.record_terminal_restored(restore_result);

    if let Some(error) = shutdown_sequence.restore_error() {
        log::debug!("[main] terminal restore error recorded during shutdown: {error}");
    }
    log::debug!(
        "[main] unified shutdown completed: steps={:?}, complete={}",
        shutdown_sequence.steps(),
        shutdown_sequence.is_complete()
    );
}

fn process_pending_host_actions(
    outcome: &mut saya::bootstrap::BootstrapOutcome,
    session_state: &mut saya::editor_session::EditorSessionState,
    transient_msg: &mut Option<String>,
) -> Option<ShutdownReason> {
    for action in outcome.core_bridge.take_pending_host_actions() {
        match action {
            CoreHostAction::Write { .. } => {
                handle_write_host_action(outcome, session_state, transient_msg);
            }
            CoreHostAction::Quit { force, .. } => {
                let decision = session_state.evaluate_quit(force);
                if let Some(reason) =
                    shutdown_reason_from_quit_decision(decision, force, transient_msg)
                {
                    return Some(reason);
                }
            }
            _ => {}
        }
    }

    None
}

fn handle_write_host_action(
    outcome: &mut saya::bootstrap::BootstrapOutcome,
    session_state: &mut saya::editor_session::EditorSessionState,
    transient_msg: &mut Option<String>,
) {
    let snapshot = outcome.core_bridge.snapshot();
    log::debug!(
        "[main] processing write host action: path_present={}, contents_len={}",
        session_state.target_path().is_some(),
        snapshot.text.len()
    );
    *transient_msg = save_snapshot(&snapshot.text, session_state);
}

fn save_snapshot(
    buffer_contents: &str,
    session_state: &mut saya::editor_session::EditorSessionState,
) -> Option<String> {
    match session_state.build_save_request(buffer_contents) {
        Ok(req) => match write_to_path(&req) {
            SaveResult::Saved => {
                session_state.record_save_success();
                Some("Saved successfully".to_string())
            }
            SaveResult::Failed { message } => {
                session_state.record_save_failure(message);
                Some(format!(
                    "Save failed: {}",
                    session_state.last_save_error().unwrap_or("")
                ))
            }
        },
        Err(error) => Some(save_error_message(&error)),
    }
}

fn save_error_message(error: &SaveRequestError) -> String {
    match error {
        SaveRequestError::NoTargetPath => "No file name to save".to_string(),
        SaveRequestError::ReadOnly => "Read-only option is set; add ! to override".to_string(),
    }
}

fn update_transient_message_from_core(
    core_bridge: &mut saya::core_bridge::CoreBridge,
    transient_msg: &mut Option<String>,
) {
    if let Some(message) = latest_non_empty_message(core_bridge.take_pending_messages()) {
        log::debug!(
            "[main] replacing transient message from core: {:?}",
            message
        );
        *transient_msg = Some(message);
    }
}

fn latest_non_empty_message(messages: Vec<CoreMessageEvent>) -> Option<String> {
    messages
        .into_iter()
        .filter_map(|event| {
            let trimmed = event.content.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .last()
}

fn visible_message_line(
    command_line_mode: bool,
    command_line_buffer: &str,
    transient_msg: Option<&str>,
) -> Option<String> {
    if command_line_mode {
        return Some(format!(":{}", command_line_buffer));
    }
    transient_msg.map(ToString::to_string)
}

fn shutdown_reason_from_quit_decision(
    decision: QuitDecision,
    force: bool,
    transient_msg: &mut Option<String>,
) -> Option<ShutdownReason> {
    log::debug!(
        "[main] evaluating quit decision for shutdown: force={}, decision={:?}",
        force,
        decision
    );
    match decision {
        QuitDecision::Allow => Some(ShutdownReason::UserQuit),
        QuitDecision::ForceQuit => Some(ShutdownReason::UserForceQuit),
        QuitDecision::WarnUnsaved => {
            *transient_msg = Some(if force {
                "No write since last change (add ! to override)".to_string()
            } else {
                "No write since last change (add force to override)".to_string()
            });
            None
        }
    }
}

fn current_body_height() -> usize {
    let rows = crossterm::terminal::size()
        .map(|(_, rows)| rows)
        .unwrap_or(3);
    // 本文 + status line + message line の 3 段構成を前提に本文高さを計算する。
    usize::from(rows.saturating_sub(2).max(1))
}

fn buffer_line_count(text: &str) -> usize {
    text.lines().count().max(1)
}

fn format_cli_error(error: CliParseError) -> String {
    match error {
        CliParseError::MissingConfigPath => "設定ファイルのパスが指定されていません".to_string(),
        CliParseError::MissingLineNumber => "開始行番号が指定されていません".to_string(),
        CliParseError::InvalidLineNumber(value) => {
            format!("開始行番号が不正です: {}", value.to_string_lossy())
        }
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
        BootstrapError::StdinReadFailed { message } => {
            format!("標準入力を読み込めませんでした: {}", message)
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

fn render_help_text() -> String {
    [
        "Usage: sy [arguments] [file]",
        "",
        "Arguments:",
        "  --               Only file names after this",
        "  -                Read text from stdin",
        "  -u <init.ts>     Use <init.ts> as startup config",
        "  --config <path>  Use <path> as startup config",
        "  +                Start at end of file",
        "  +<lnum>          Start at line <lnum>",
        "  -R               Read-only mode",
        "  -h, --help       Print help and exit",
        "  --version        Print version information and exit",
    ]
    .join("\n")
}

fn render_version_text() -> String {
    format!("sy {}", env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutdown_reason_maps_clean_quit_to_user_quit() {
        let mut transient_msg = None;

        let reason =
            shutdown_reason_from_quit_decision(QuitDecision::Allow, false, &mut transient_msg);

        assert_eq!(reason, Some(ShutdownReason::UserQuit));
        assert_eq!(transient_msg, None);
    }

    #[test]
    fn shutdown_reason_maps_forced_quit_to_force_quit() {
        let mut transient_msg = None;

        let reason =
            shutdown_reason_from_quit_decision(QuitDecision::ForceQuit, true, &mut transient_msg);

        assert_eq!(reason, Some(ShutdownReason::UserForceQuit));
        assert_eq!(transient_msg, None);
    }

    #[test]
    fn shutdown_reason_keeps_loop_running_when_quit_is_rejected() {
        let mut transient_msg = None;

        let reason = shutdown_reason_from_quit_decision(
            QuitDecision::WarnUnsaved,
            false,
            &mut transient_msg,
        );

        assert_eq!(reason, None);
        assert_eq!(
            transient_msg,
            Some("No write since last change (add force to override)".to_string())
        );
    }

    #[test]
    fn save_error_message_reports_read_only_mode() {
        let message = save_error_message(&SaveRequestError::ReadOnly);

        assert_eq!(message, "Read-only option is set; add ! to override");
    }

    #[test]
    fn visible_message_line_prefers_command_line_preview() {
        let visible = visible_message_line(true, "q!", Some("saved"));

        assert_eq!(visible, Some(":q!".to_string()));
    }

    #[test]
    fn visible_message_line_restores_transient_message_after_command_line() {
        let visible = visible_message_line(false, "", Some("vim core message"));

        assert_eq!(visible, Some("vim core message".to_string()));
    }

    #[test]
    fn latest_non_empty_message_returns_last_core_message() {
        let messages = vec![
            CoreMessageEvent {
                kind: vim_core_rs::CoreMessageKind::Normal,
                content: "first".to_string(),
            },
            CoreMessageEvent {
                kind: vim_core_rs::CoreMessageKind::Error,
                content: "second".to_string(),
            },
        ];

        assert_eq!(
            latest_non_empty_message(messages),
            Some("second".to_string())
        );
    }

    #[test]
    fn render_help_text_lists_vim_compatible_options() {
        let help = render_help_text();

        assert!(help.contains("Usage: sy [arguments] [file]"));
        assert!(help.contains("  --               Only file names after this"));
        assert!(help.contains("  -                Read text from stdin"));
        assert!(help.contains("  +<lnum>          Start at line <lnum>"));
        assert!(help.contains("  -R               Read-only mode"));
        assert!(help.contains("  --version        Print version information and exit"));
    }

    #[test]
    fn render_version_text_includes_package_version() {
        let version = render_version_text();

        assert_eq!(version, format!("sy {}", env!("CARGO_PKG_VERSION")));
    }
}
