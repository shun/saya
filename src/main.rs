use saya::app_startup::{LaunchStartError, prepare_launch_and_start_terminal};
use saya::bootstrap::{BootstrapError, bootstrap_warning_message};
use saya::cli::{CliParseError, StartupAction, parse_launch_request};
use saya::editor_session::{QuitDecision, SaveRequestError};
use saya::event_loop::{EventLoopCoordinator, LoopAction, ShutdownReason, UiEvent};
use saya::ex_command::{LocalHostCommand, apply_local_ex_command, parse_local_host_command};
use saya::host_io::{SaveResult, write_to_path};
use saya::input_loop::{CrosstermEventSource, run_terminal_input_loop};
use saya::input_router::{EditorIntent, KeyInput, resolve_intent};
use saya::runtime_integration::{
    RuntimeCommandEffect, RuntimeDispatchOutcome, RuntimeEventMapper, RuntimeHostSession,
    RuntimeSessionOwner,
};
use saya::saya_live_runtime::{
    ReadonlyBufferSnapshot, ReadonlyEditorSnapshot, ReadonlyWindowSnapshot, RuntimeCommandError,
    RuntimeInitError, RuntimeMode,
};
use saya::screen_model::{ProjectionInput, project};
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

    if std::env::var_os("SAYA_BINARY_SMOKE").is_some() {
        if let Err(error) = run_binary_smoke(launch_request) {
            eprintln!("[main][smoke] {error}");
            std::process::exit(1);
        }
        std::process::exit(0);
    }

    // UI 初期化
    let mut backend = CrosstermBackendImpl;
    let (mut outcome, terminal_session) =
        match prepare_launch_and_start_terminal(launch_request, &mut backend) {
            Ok((outcome, terminal_session)) => (outcome, terminal_session),
            Err(error) => {
                log::debug!("{}", format_launch_start_error(error));
                std::process::exit(1);
            }
        };

    let mut renderer = TuiRenderer::new().expect("TUI Renderer init failed");
    let mut session_state = outcome.editor_session_state();
    let mut transient_msg: Option<String> = bootstrap_warning_message(&outcome.warnings);
    let mut viewport = ViewportState::new();
    let mut command_line_mode = false;
    let mut command_line_buffer = String::new();
    let mut runtime_session = match RuntimeSessionOwner::spawn(outcome.callback_registry.clone()) {
        Ok(runtime_session) => Some(runtime_session),
        Err(error) => {
            let message = format_runtime_init_error(&error);
            log::debug!(
                "[main] failed to initialize runtime session owner from startup registry: {:?}",
                error
            );
            transient_msg = Some(message);
            None
        }
    };

    let mut startup_runtime_redraw = false;
    dispatch_buffer_open_with_runtime(
        runtime_session.as_mut(),
        &mut outcome,
        &mut session_state,
        &mut transient_msg,
        &mut startup_runtime_redraw,
    )
    .await;

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
    trace_render_pipeline("initial", &snapshot.text, &model.lines, viewport.top_line());
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
                                } else if let Some(reason) =
                                    process_local_host_command_with_runtime(
                                        &cmd,
                                        &mut outcome,
                                        &mut session_state,
                                        &mut transient_msg,
                                        runtime_session.as_mut(),
                                        &mut need_redraw,
                                    )
                                    .await
                                {
                                    break 'main reason;
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

                        if let Some(reason) = process_pending_host_actions_with_runtime(
                            &mut outcome,
                            &mut session_state,
                            &mut transient_msg,
                            runtime_session.as_mut(),
                            &mut need_redraw,
                        )
                        .await
                        {
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

                                if let Some(reason) = process_pending_host_actions_with_runtime(
                                    &mut outcome,
                                    &mut session_state,
                                    &mut transient_msg,
                                    runtime_session.as_mut(),
                                    &mut need_redraw,
                                )
                                .await
                                {
                                    break 'main reason;
                                }

                                session_state.update_dirty(outcome.core_bridge.snapshot().dirty);
                                need_redraw = true;
                            }
                            EditorIntent::Save => {
                                let snapshot = outcome.core_bridge.snapshot();
                                let save_outcome =
                                    save_snapshot_result(&snapshot.text, &mut session_state);
                                transient_msg = save_outcome.transient_message;
                                if save_outcome.wrote {
                                    dispatch_buffer_write_post_with_runtime(
                                        runtime_session.as_mut(),
                                        &mut outcome,
                                        &mut session_state,
                                        &mut transient_msg,
                                        &mut need_redraw,
                                    )
                                    .await;
                                }
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
            trace_render_pipeline("redraw", &snapshot.text, &model.lines, viewport.top_line());
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

fn run_binary_smoke(launch_request: saya::cli::LaunchRequest) -> Result<(), String> {
    eprintln!("[main][smoke] preparing headless launch");
    let mut outcome =
        saya::bootstrap::prepare_launch(launch_request).map_err(format_bootstrap_error)?;
    let mut session_state = outcome.editor_session_state();
    let startup_model = project(&ProjectionInput::new(
        &outcome.initial_snapshot,
        &session_state,
        None,
    ));
    let mut transient_msg: Option<String> = None;

    eprintln!(
        "[main][smoke] projected startup ui: first_line={:?}, message_line={:?}, file_name={}, mode={}, dirty={}, line_numbers={}, number_width={}",
        startup_model.lines.first(),
        startup_model.message_line,
        startup_model.file_name,
        startup_model.mode_label,
        startup_model.dirty,
        session_state.line_numbers(),
        session_state.number_width()
    );

    eprintln!("[main][smoke] dispatching a single edit");
    outcome
        .core_bridge
        .dispatch_key("i")
        .map_err(|error| format!("insert mode failed: {:?}", error))?;
    outcome
        .core_bridge
        .dispatch_key("X")
        .map_err(|error| format!("typing failed: {:?}", error))?;
    outcome
        .core_bridge
        .dispatch_key("\x1b")
        .map_err(|error| format!("escape failed: {:?}", error))?;
    update_transient_message_from_core(&mut outcome.core_bridge, &mut transient_msg);
    session_state.update_dirty(outcome.core_bridge.snapshot().dirty);

    if session_state.target_path().is_none() {
        eprintln!("[main][smoke] stdin startup detected, verifying save-path restriction");
        let snapshot = outcome.core_bridge.snapshot();
        let save_message = save_snapshot(&snapshot.text, &mut session_state)
            .unwrap_or_else(|| "No file name to save".to_string());
        return Err(save_message);
    }

    eprintln!("[main][smoke] saving and quitting through host command");
    let reason =
        process_local_host_command(":wq", &mut outcome, &mut session_state, &mut transient_msg)
            .ok_or_else(|| {
                format!(
                    "smoke quit did not complete: dirty={}, last_save_error={:?}",
                    session_state.is_dirty(),
                    session_state.last_save_error()
                )
            })?;

    if reason != ShutdownReason::UserQuit {
        return Err(format!(
            "smoke quit returned unexpected shutdown reason: {:?}",
            reason
        ));
    }

    eprintln!("[main][smoke] completed with shutdown reason: {:?}", reason);
    Ok(())
}

async fn process_pending_host_actions_with_runtime(
    outcome: &mut saya::bootstrap::BootstrapOutcome,
    session_state: &mut saya::editor_session::EditorSessionState,
    transient_msg: &mut Option<String>,
    mut runtime_session: Option<&mut RuntimeSessionOwner>,
    need_redraw: &mut bool,
) -> Option<ShutdownReason> {
    for action in outcome.core_bridge.take_pending_host_actions() {
        match action {
            CoreHostAction::Write { .. } => {
                handle_write_host_action_with_runtime(
                    outcome,
                    session_state,
                    transient_msg,
                    runtime_session.as_deref_mut(),
                    need_redraw,
                )
                .await;
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

async fn handle_write_host_action_with_runtime(
    outcome: &mut saya::bootstrap::BootstrapOutcome,
    session_state: &mut saya::editor_session::EditorSessionState,
    transient_msg: &mut Option<String>,
    runtime_session: Option<&mut RuntimeSessionOwner>,
    need_redraw: &mut bool,
) {
    let snapshot = outcome.core_bridge.snapshot();
    log::debug!(
        "[main] processing write host action with runtime integration: path_present={}, contents_len={}",
        session_state.target_path().is_some(),
        snapshot.text.len()
    );
    let save_outcome = save_snapshot_result(&snapshot.text, session_state);
    *transient_msg = save_outcome.transient_message;
    if save_outcome.wrote {
        dispatch_buffer_write_post_with_runtime(
            runtime_session,
            outcome,
            session_state,
            transient_msg,
            need_redraw,
        )
        .await;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SaveSnapshotOutcome {
    transient_message: Option<String>,
    wrote: bool,
}

fn save_snapshot_result(
    buffer_contents: &str,
    session_state: &mut saya::editor_session::EditorSessionState,
) -> SaveSnapshotOutcome {
    match session_state.build_save_request(buffer_contents) {
        Ok(req) => match write_to_path(&req) {
            SaveResult::Saved => {
                session_state.record_save_success();
                SaveSnapshotOutcome {
                    transient_message: Some("Saved successfully".to_string()),
                    wrote: true,
                }
            }
            SaveResult::Failed { message } => {
                session_state.record_save_failure(message);
                SaveSnapshotOutcome {
                    transient_message: Some(format!(
                        "Save failed: {}",
                        session_state.last_save_error().unwrap_or("")
                    )),
                    wrote: false,
                }
            }
        },
        Err(error) => SaveSnapshotOutcome {
            transient_message: Some(save_error_message(&error)),
            wrote: false,
        },
    }
}

fn save_snapshot(
    buffer_contents: &str,
    session_state: &mut saya::editor_session::EditorSessionState,
) -> Option<String> {
    save_snapshot_result(buffer_contents, session_state).transient_message
}

fn process_local_host_command(
    command: &str,
    outcome: &mut saya::bootstrap::BootstrapOutcome,
    session_state: &mut saya::editor_session::EditorSessionState,
    transient_msg: &mut Option<String>,
) -> Option<ShutdownReason> {
    let host_command = parse_local_host_command(command)?;
    let snapshot = outcome.core_bridge.snapshot();
    log::debug!(
        "[main] processing local host command: command={}, host_command={:?}, dirty={}",
        command,
        host_command,
        snapshot.dirty
    );

    *transient_msg = save_snapshot(&snapshot.text, session_state);

    match host_command {
        LocalHostCommand::Save => None,
        LocalHostCommand::SaveThenQuit => (!session_state.is_dirty()
            && session_state.last_save_error().is_none())
        .then_some(ShutdownReason::UserQuit),
    }
}

async fn process_local_host_command_with_runtime(
    command: &str,
    outcome: &mut saya::bootstrap::BootstrapOutcome,
    session_state: &mut saya::editor_session::EditorSessionState,
    transient_msg: &mut Option<String>,
    runtime_session: Option<&mut RuntimeSessionOwner>,
    need_redraw: &mut bool,
) -> Option<ShutdownReason> {
    let host_command = parse_local_host_command(command)?;
    let snapshot = outcome.core_bridge.snapshot();
    log::debug!(
        "[main] processing local host command with runtime integration: command={}, host_command={:?}, dirty={}",
        command,
        host_command,
        snapshot.dirty
    );

    let save_outcome = save_snapshot_result(&snapshot.text, session_state);
    *transient_msg = save_outcome.transient_message;
    if save_outcome.wrote {
        dispatch_buffer_write_post_with_runtime(
            runtime_session,
            outcome,
            session_state,
            transient_msg,
            need_redraw,
        )
        .await;
    }

    match host_command {
        LocalHostCommand::Save => None,
        LocalHostCommand::SaveThenQuit => (!session_state.is_dirty()
            && session_state.last_save_error().is_none())
        .then_some(ShutdownReason::UserQuit),
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
    if let Some(message) = latest_user_visible_message(core_bridge.take_pending_messages()) {
        log::debug!(
            "[main] replacing transient message from core: {:?}",
            message
        );
        *transient_msg = Some(message);
    }
}

fn latest_user_visible_message(messages: Vec<CoreMessageEvent>) -> Option<String> {
    messages
        .into_iter()
        .filter_map(|event| {
            let trimmed = event.content.trim();
            if trimmed.is_empty() || !event.category.is_user_visible() {
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

async fn dispatch_buffer_open_with_runtime(
    runtime_session: Option<&mut RuntimeSessionOwner>,
    outcome: &mut saya::bootstrap::BootstrapOutcome,
    session_state: &mut saya::editor_session::EditorSessionState,
    transient_msg: &mut Option<String>,
    need_redraw: &mut bool,
) {
    let Some(runtime_session) = runtime_session else {
        return;
    };
    let mut host_session = MainRuntimeHostSession::new(outcome, session_state);
    let payload = RuntimeEventMapper::buffer_open(host_session.current_buffer_snapshot());
    let dispatch_outcome = runtime_session.dispatch(payload, &mut host_session).await;
    apply_runtime_dispatch_outcome(transient_msg, need_redraw, dispatch_outcome);
}

async fn dispatch_buffer_write_post_with_runtime(
    runtime_session: Option<&mut RuntimeSessionOwner>,
    outcome: &mut saya::bootstrap::BootstrapOutcome,
    session_state: &mut saya::editor_session::EditorSessionState,
    transient_msg: &mut Option<String>,
    need_redraw: &mut bool,
) {
    let Some(runtime_session) = runtime_session else {
        return;
    };
    let mut host_session = MainRuntimeHostSession::new(outcome, session_state);
    let payload = RuntimeEventMapper::buffer_write_post(host_session.current_buffer_snapshot());
    let dispatch_outcome = runtime_session.dispatch(payload, &mut host_session).await;
    apply_runtime_dispatch_outcome(transient_msg, need_redraw, dispatch_outcome);
}

fn apply_runtime_dispatch_outcome(
    transient_msg: &mut Option<String>,
    need_redraw: &mut bool,
    dispatch_outcome: RuntimeDispatchOutcome,
) {
    if let Some(message) = dispatch_outcome.transient_message {
        log::debug!(
            "[main] applying normalized runtime transient message to application state: {}",
            message
        );
        *transient_msg = Some(message);
    }
    if dispatch_outcome.requires_redraw {
        log::debug!("[main] applying normalized runtime redraw request to main loop");
        *need_redraw = true;
    }
}

struct MainRuntimeHostSession<'a> {
    outcome: &'a mut saya::bootstrap::BootstrapOutcome,
    session_state: &'a mut saya::editor_session::EditorSessionState,
}

impl<'a> MainRuntimeHostSession<'a> {
    fn new(
        outcome: &'a mut saya::bootstrap::BootstrapOutcome,
        session_state: &'a mut saya::editor_session::EditorSessionState,
    ) -> Self {
        Self {
            outcome,
            session_state,
        }
    }
}

impl RuntimeHostSession for MainRuntimeHostSession<'_> {
    fn current_buffer_snapshot(&mut self) -> ReadonlyBufferSnapshot {
        let snapshot = self.outcome.core_bridge.snapshot();
        let active_buffer_id = snapshot
            .buffers
            .iter()
            .find(|buffer| buffer.is_active)
            .map(|buffer| buffer.id as u64)
            .unwrap_or(1);
        ReadonlyBufferSnapshot {
            id: active_buffer_id,
            path: self.session_state.target_path().cloned(),
            line_count: buffer_line_count(&snapshot.text),
        }
    }

    fn current_window_snapshot(&mut self) -> ReadonlyWindowSnapshot {
        let snapshot = self.outcome.core_bridge.snapshot();
        let active_window_id = snapshot
            .windows
            .iter()
            .find(|window| window.is_active)
            .map(|window| window.id as u64)
            .unwrap_or(1);
        ReadonlyWindowSnapshot {
            id: active_window_id,
        }
    }

    fn current_editor_snapshot(&mut self) -> ReadonlyEditorSnapshot {
        let snapshot = self.outcome.core_bridge.snapshot();
        ReadonlyEditorSnapshot {
            mode: runtime_mode_from_core(snapshot.mode),
        }
    }

    fn execute_host_command(
        &mut self,
        name: &str,
    ) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
        log::debug!(
            "[main] executing runtime host command through application session owner: {}",
            name
        );
        match parse_local_host_command(name) {
            Some(LocalHostCommand::Save) => {
                let snapshot = self.outcome.core_bridge.snapshot();
                let save_outcome = save_snapshot_result(&snapshot.text, self.session_state);
                let follow_up_events = if save_outcome.wrote {
                    vec![RuntimeEventMapper::buffer_write_post(
                        self.current_buffer_snapshot(),
                    )]
                } else {
                    Vec::new()
                };
                Ok(RuntimeCommandEffect {
                    transient_message: save_outcome.transient_message,
                    follow_up_events,
                })
            }
            Some(LocalHostCommand::SaveThenQuit) => Err(RuntimeCommandError::CommandFailed {
                name: name.to_string(),
                message: "quit commands are not available from runtime callbacks in the first live integration pass".to_string(),
            }),
            None => Err(RuntimeCommandError::UnknownCommand {
                name: name.to_string(),
            }),
        }
    }
}

fn runtime_mode_from_core(mode: CoreMode) -> RuntimeMode {
    match mode {
        CoreMode::Insert => RuntimeMode::Insert,
        CoreMode::Visual | CoreMode::VisualLine | CoreMode::VisualBlock => RuntimeMode::Visual,
        _ => RuntimeMode::Normal,
    }
}

fn format_runtime_init_error(error: &RuntimeInitError) -> String {
    match error {
        RuntimeInitError::WorkerStartFailed { message } => {
            format!("Runtime initialization failed: {}", message)
        }
        RuntimeInitError::UnsupportedEvent { name } => {
            format!("Runtime initialization failed: unsupported event {}", name)
        }
        RuntimeInitError::BootstrapFailed { message } => {
            format!("Runtime initialization failed: {}", message)
        }
    }
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
            *transient_msg = Some(normal_quit_warning_message().to_string());
            None
        }
    }
}

fn normal_quit_warning_message() -> &'static str {
    "No write since last change (add ! to override)"
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

fn trace_render_pipeline(
    phase: &str,
    snapshot_text: &str,
    visible_lines: &[String],
    viewport_top: usize,
) {
    if std::env::var_os("SAYA_TRACE_RENDER").is_none() {
        return;
    }

    let absolute_row = 6usize;
    let snapshot_line = snapshot_text.lines().nth(absolute_row).unwrap_or("");
    let visible_row = absolute_row.checked_sub(viewport_top);
    let projected_line = visible_row
        .and_then(|row| visible_lines.get(row))
        .map(String::as_str)
        .unwrap_or("");

    eprintln!(
        "[saya-trace][main][{phase}] viewport_top={viewport_top} abs_row=7 snapshot={snapshot_line:?} projected={projected_line:?}"
    );
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

fn format_launch_start_error(error: LaunchStartError) -> String {
    match error {
        LaunchStartError::Bootstrap(error) => format_bootstrap_error(error),
        LaunchStartError::Terminal(error) => {
            format!("terminal lifecycle の初期化に失敗しました: {:?}", error)
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
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn unique_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        std::env::temp_dir().join(format!("saya-main-test-{name}-{nanos}"))
    }

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
            Some(normal_quit_warning_message().to_string())
        );
    }

    #[test]
    fn normal_and_force_quit_messages_remain_distinct() {
        let mut normal_transient_msg = None;
        let normal_reason = shutdown_reason_from_quit_decision(
            QuitDecision::WarnUnsaved,
            false,
            &mut normal_transient_msg,
        );

        let mut force_transient_msg = None;
        let force_reason = shutdown_reason_from_quit_decision(
            QuitDecision::ForceQuit,
            true,
            &mut force_transient_msg,
        );

        assert_eq!(normal_reason, None);
        assert_eq!(force_reason, Some(ShutdownReason::UserForceQuit));
        assert_eq!(
            normal_transient_msg,
            Some("No write since last change (add ! to override)".to_string())
        );
        assert_eq!(force_transient_msg, None);
        assert_ne!(normal_transient_msg, force_transient_msg);
    }

    #[test]
    fn save_error_message_reports_read_only_mode() {
        let message = save_error_message(&SaveRequestError::ReadOnly);

        assert_eq!(message, "Read-only option is set; add ! to override");
    }

    #[test]
    fn write_host_action_updates_transient_message_on_failure() {
        let _lock = saya::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let target_path = unique_path("write-failure");
        std::fs::write(&target_path, "initial\n").expect("test file");

        let mut outcome = saya::bootstrap::prepare_launch(saya::cli::LaunchRequest {
            input_source: saya::cli::InputSource::File(target_path.clone()),
            config_source: saya::cli::ConfigSource::Default,
            ..saya::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let bad_path = PathBuf::from("/nonexistent/dir/file.txt");
        let mut session_state = saya::editor_session::EditorSessionState::new(Some(bad_path));

        outcome.core_bridge.dispatch_key("i").unwrap();
        outcome.core_bridge.dispatch_key("X").unwrap();
        outcome.core_bridge.dispatch_key("\x1b").unwrap();
        session_state.update_dirty(outcome.core_bridge.snapshot().dirty);

        outcome
            .core_bridge
            .apply_ex_command(":w")
            .expect(":w command should succeed");

        let actions = outcome.core_bridge.take_pending_host_actions();
        assert!(
            matches!(
                actions.as_slice(),
                [vim_core_rs::CoreHostAction::Write { .. }]
            ),
            ":w 後に write host action が 1 件発行されること: {:?}",
            actions
        );

        let snapshot = outcome.core_bridge.snapshot();
        log::debug!(
            "[main::tests] processing write host action failure path: text_len={}, dirty={}",
            snapshot.text.len(),
            session_state.is_dirty()
        );
        let transient_msg = save_snapshot(&snapshot.text, &mut session_state);
        let expected_error = session_state
            .last_save_error()
            .expect("save failure should be recorded")
            .to_string();
        let expected_message = format!("Save failed: {}", expected_error);

        assert_eq!(transient_msg, Some(expected_message.clone()));
        assert_eq!(
            visible_message_line(false, "", transient_msg.as_deref()),
            Some(expected_message)
        );
        assert!(session_state.is_dirty());

        std::fs::remove_file(&target_path).expect("cleanup");
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
    fn latest_user_visible_message_returns_last_user_visible_message() {
        let messages = vec![
            CoreMessageEvent {
                severity: vim_core_rs::CoreMessageSeverity::Info,
                category: vim_core_rs::CoreMessageCategory::UserVisible,
                content: "first".to_string(),
            },
            CoreMessageEvent {
                severity: vim_core_rs::CoreMessageSeverity::Error,
                category: vim_core_rs::CoreMessageCategory::UserVisible,
                content: "second".to_string(),
            },
        ];

        assert_eq!(
            latest_user_visible_message(messages),
            Some("second".to_string())
        );
    }

    #[test]
    fn latest_user_visible_message_ignores_undo_command_feedback() {
        let messages = vec![
            CoreMessageEvent {
                severity: vim_core_rs::CoreMessageSeverity::Info,
                category: vim_core_rs::CoreMessageCategory::CommandFeedback,
                content: "2 fewer lines; before #2  4 seconds ago".to_string(),
            },
            CoreMessageEvent {
                severity: vim_core_rs::CoreMessageSeverity::Info,
                category: vim_core_rs::CoreMessageCategory::CommandFeedback,
                content: "1 change; after #3  1 second ago".to_string(),
            },
        ];

        assert_eq!(latest_user_visible_message(messages), None);
    }

    #[test]
    fn latest_user_visible_message_skips_command_feedback_and_keeps_visible_notice() {
        let messages = vec![
            CoreMessageEvent {
                severity: vim_core_rs::CoreMessageSeverity::Info,
                category: vim_core_rs::CoreMessageCategory::CommandFeedback,
                content: "2 fewer lines; before #2  4 seconds ago".to_string(),
            },
            CoreMessageEvent {
                severity: vim_core_rs::CoreMessageSeverity::Warning,
                category: vim_core_rs::CoreMessageCategory::UserVisible,
                content: "visible warning".to_string(),
            },
        ];

        assert_eq!(
            latest_user_visible_message(messages),
            Some("visible warning".to_string())
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
