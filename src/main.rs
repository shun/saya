use saya::app_startup::{LaunchStartError, prepare_launch_and_start_terminal};
use saya::bootstrap::{BootstrapError, bootstrap_warning_message};
use saya::cli::{CliParseError, StartupAction, parse_launch_request};
use saya::editor_session::{QuitDecision, SaveRequestError};
use saya::event_loop::{EventLoopCoordinator, LoopAction, ShutdownReason, UiEvent};
use saya::ex_command::{ExCommandRoute, apply_local_ex_command, route_ex_command};
use saya::host_io::{SaveRequest, SaveResult, write_to_path};
use saya::input_loop::{CrosstermEventSource, run_terminal_input_loop};
use saya::input_router::{EditorIntent, KeyInput, resolve_intent};
use saya::runtime_integration::{
    RuntimeCommandEffect, RuntimeDispatchOutcome, RuntimeEventMapper, RuntimeHostSession,
    RuntimeSessionOwner, RuntimeShutdownIntent,
};
use saya::saya_live_runtime::{
    ReadonlyBufferSnapshot, ReadonlyEditorSnapshot, ReadonlyWindowSnapshot, RuntimeCommandError,
    RuntimeInitError, RuntimeMode,
};
use saya::screen_model::{
    ProjectionInput, WorkspaceProjectionError, WorkspaceProjectionInput, WorkspaceScreenModel,
    project, project_workspace,
};
use saya::search_query::{SearchStateError, SearchVisibleState};
use saya::search_refresh::{SearchModeHint, SearchRefreshInput, WindowSearchRefreshStore};
use saya::tui_renderer::{CrosstermBackendImpl, TuiRenderer};
use saya::viewport::WindowViewportStore;
use vim_core_rs::{CoreMessageEvent, CoreMode};

use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};
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

    if std::env::var_os("SAYA_PTY_SMOKE").is_some() {
        if let Err(error) = run_binary_pty_smoke(launch_request) {
            eprintln!("[main][pty-smoke] {error}");
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
    let mut core_message: Option<String> = None;
    let mut system_warning: Option<String> = bootstrap_warning_message(&outcome.warnings);
    let mut transient_msg: Option<String> = None;
    let mut viewport_store = WindowViewportStore::new();
    let mut search_refresh_store = WindowSearchRefreshStore::new();
    let mut command_line_prompt: Option<char> = None;
    let mut command_line_buffer = String::new();
    let mut last_successful_workspace_model: Option<WorkspaceScreenModel> = None;
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
    let startup_shutdown_reason = dispatch_buffer_open_with_runtime(
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
    sync_core_screen_size(&mut outcome);
    let (terminal_width, terminal_height) = current_terminal_size();
    match apply_workspace_redraw_transaction(
        &mut last_successful_workspace_model,
        build_workspace_render_output(
            &mut outcome,
            &session_state,
            &mut viewport_store,
            &mut search_refresh_store,
            command_line_prompt,
            &command_line_buffer,
            core_message.as_deref(),
            system_warning.as_deref(),
            transient_msg.as_deref(),
            terminal_width,
            terminal_height,
        ),
    ) {
        Ok(render_output) => {
            if let Some(message) = render_output.failure_message {
                transient_msg = Some(message);
            }
            trace_workspace_render_pipeline(
                "initial",
                &outcome.core_bridge.snapshot().text,
                &render_output.model,
            );
            let _ = renderer.draw(&render_output.model);
        }
        Err(error) => {
            transient_msg = Some(error.to_string());
            log::debug!(
                "[main] initial workspace redraw failed without rollback: error={:?}",
                error
            );
        }
    }

    // メインループ
    let shutdown_reason = if let Some(reason) = startup_shutdown_reason {
        log::debug!(
            "[main] startup runtime dispatch requested shutdown before entering loop: reason={:?}",
            reason
        );
        reason
    } else {
        'main: loop {
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

                        if let Some(prompt) = command_line_prompt {
                            match key {
                                KeyInput::Escape => {
                                    if prompt == '/' {
                                        let _ = outcome.core_bridge.cancel_search_input();
                                        update_core_message_from_core(
                                            &mut outcome.core_bridge,
                                            &mut core_message,
                                        );
                                    }
                                    command_line_prompt = None;
                                    command_line_buffer.clear();
                                }
                                KeyInput::Enter => {
                                    if prompt == ':' {
                                        let cmd = format!("{}{}", prompt, command_line_buffer);
                                        command_line_prompt = None;
                                        command_line_buffer.clear();
                                        match route_ex_command(&cmd) {
                                            ExCommandRoute::PresentationLocal => {
                                                if let Some(message) =
                                                    apply_local_ex_command(&mut session_state, &cmd)
                                                {
                                                    transient_msg = Some(message);
                                                } else {
                                                    log::debug!(
                                                        "[main] presentation-local route fell through to core-owned handler: command={:?}",
                                                        cmd
                                                    );
                                                    let _ =
                                                        outcome.core_bridge.apply_ex_command(&cmd);
                                                    update_core_message_from_core(
                                                        &mut outcome.core_bridge,
                                                        &mut core_message,
                                                    );
                                                }
                                            }
                                            ExCommandRoute::SearchOption(search_option) => {
                                                log::debug!(
                                                    "[main] routing search option command to core-owned option update: command={:?}, search_option={:?}",
                                                    cmd,
                                                    search_option
                                                );
                                                let _ = outcome.core_bridge.apply_ex_command(&cmd);
                                                update_core_message_from_core(
                                                    &mut outcome.core_bridge,
                                                    &mut core_message,
                                                );
                                            }
                                            ExCommandRoute::CoreOwned => {
                                                let _ = outcome.core_bridge.apply_ex_command(&cmd);
                                                update_core_message_from_core(
                                                    &mut outcome.core_bridge,
                                                    &mut core_message,
                                                );
                                            }
                                        }
                                    } else if prompt == '/' {
                                        let _ = outcome
                                            .core_bridge
                                            .commit_search_input(&command_line_buffer);
                                        update_core_message_from_core(
                                            &mut outcome.core_bridge,
                                            &mut core_message,
                                        );
                                        command_line_prompt = None;
                                        command_line_buffer.clear();
                                    }
                                    session_state
                                        .update_dirty(outcome.core_bridge.snapshot().dirty);
                                }
                                KeyInput::Backspace => {
                                    if prompt == '/' {
                                        let _ = command_line_buffer.pop();
                                        let _ = outcome
                                            .core_bridge
                                            .sync_search_input(&command_line_buffer);
                                        update_core_message_from_core(
                                            &mut outcome.core_bridge,
                                            &mut core_message,
                                        );
                                    } else if command_line_buffer.pop().is_none() {
                                        command_line_prompt = None;
                                    }
                                }
                                KeyInput::Char(c) => {
                                    command_line_buffer.push(c);
                                    if prompt == '/' {
                                        let _ = outcome
                                            .core_bridge
                                            .sync_search_input(&command_line_buffer);
                                        update_core_message_from_core(
                                            &mut outcome.core_bridge,
                                            &mut core_message,
                                        );
                                    }
                                }
                                _ => {}
                            }
                            handled = true;
                            need_redraw = true;

                            if let Some(reason) = process_pending_host_actions_with_runtime(
                                &mut outcome,
                                &mut session_state,
                                &mut transient_msg,
                                &mut system_warning,
                                runtime_session.as_mut(),
                                &mut need_redraw,
                            )
                            .await
                            {
                                break 'main reason;
                            }
                        } else if (key == KeyInput::Char(':') || key == KeyInput::Char('/'))
                            && outcome.core_bridge.snapshot().mode == CoreMode::Normal
                        {
                            if let KeyInput::Char(c) = key {
                                command_line_prompt = Some(c);
                            }
                            command_line_buffer.clear();
                            handled = true;
                            need_redraw = true;
                        }

                        if !handled {
                            let intent = resolve_intent(&key);
                            match intent {
                                EditorIntent::EditKey(k) => {
                                    let _ = outcome.core_bridge.dispatch_key(&k);
                                    update_core_message_from_core(
                                        &mut outcome.core_bridge,
                                        &mut core_message,
                                    );

                                    if let Some(reason) = process_pending_host_actions_with_runtime(
                                        &mut outcome,
                                        &mut session_state,
                                        &mut transient_msg,
                                        &mut system_warning,
                                        runtime_session.as_mut(),
                                        &mut need_redraw,
                                    )
                                    .await
                                    {
                                        break 'main reason;
                                    }

                                    session_state
                                        .update_dirty(outcome.core_bridge.snapshot().dirty);
                                    need_redraw = true;
                                }
                                EditorIntent::Save => {
                                    let snapshot = outcome.core_bridge.snapshot();
                                    let save_outcome =
                                        save_snapshot_result(&snapshot.text, &mut session_state);
                                    transient_msg = save_outcome.transient_message;
                                    if save_outcome.wrote {
                                        if let Some(reason) =
                                            dispatch_buffer_write_post_with_runtime(
                                                runtime_session.as_mut(),
                                                &mut outcome,
                                                &mut session_state,
                                                &mut transient_msg,
                                                &mut need_redraw,
                                            )
                                            .await
                                        {
                                            break 'main reason;
                                        }
                                    }
                                    need_redraw = true;
                                }
                                EditorIntent::Quit { force } => {
                                    let decision = session_state.evaluate_quit(force);
                                    if let Some(reason) = shutdown_reason_from_quit_decision(
                                        decision,
                                        force,
                                        &mut system_warning,
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
                sync_core_screen_size(&mut outcome);
                let (terminal_width, terminal_height) = current_terminal_size();
                match apply_workspace_redraw_transaction(
                    &mut last_successful_workspace_model,
                    build_workspace_render_output(
                        &mut outcome,
                        &session_state,
                        &mut viewport_store,
                        &mut search_refresh_store,
                        command_line_prompt,
                        &command_line_buffer,
                        core_message.as_deref(),
                        system_warning.as_deref(),
                        transient_msg.as_deref(),
                        terminal_width,
                        terminal_height,
                    ),
                ) {
                    Ok(render_output) => {
                        if let Some(message) = render_output.failure_message {
                            transient_msg = Some(message);
                        }
                        trace_workspace_render_pipeline(
                            "redraw",
                            &outcome.core_bridge.snapshot().text,
                            &render_output.model,
                        );
                        let _ = renderer.draw(&render_output.model);
                    }
                    Err(error) => {
                        transient_msg = Some(error.to_string());
                        log::debug!(
                            "[main] redraw failed without rollback because no successful model exists yet: error={:?}",
                            error
                        );
                    }
                }
            }
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
    let mut system_warning: Option<String> = None;

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
    update_core_message_from_core(&mut outcome.core_bridge, &mut transient_msg);
    session_state.update_dirty(outcome.core_bridge.snapshot().dirty);

    if session_state.target_path().is_none() {
        eprintln!("[main][smoke] stdin startup detected, verifying save-path restriction");
        let snapshot = outcome.core_bridge.snapshot();
        let save_message = save_snapshot_result(&snapshot.text, &mut session_state)
            .transient_message
            .unwrap_or_else(|| "No file name to save".to_string());
        return Err(save_message);
    }

    eprintln!("[main][smoke] saving and quitting through host action coordination");
    outcome
        .core_bridge
        .apply_ex_command(":wq")
        .map_err(|error| format!("smoke :wq failed: {:?}", error))?;
    session_state.update_dirty(outcome.core_bridge.snapshot().dirty);

    let reason = process_pending_host_actions_without_runtime(
        &mut outcome,
        &mut session_state,
        &mut transient_msg,
        &mut system_warning,
    )
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

fn run_binary_pty_smoke(launch_request: saya::cli::LaunchRequest) -> Result<(), String> {
    eprintln!("[main][pty-smoke] preparing PTY launch");
    let mut backend = CrosstermBackendImpl;
    let (mut outcome, terminal_session) =
        prepare_launch_and_start_terminal(launch_request, &mut backend)
            .map_err(format_launch_start_error)?;

    let mut renderer = TuiRenderer::new().map_err(|error| format!("TUI init failed: {error}"))?;
    let session_state = outcome.editor_session_state();
    let mut viewport_store = WindowViewportStore::new();
    let mut search_refresh_store = WindowSearchRefreshStore::new();
    let mut last_successful_workspace_model: Option<WorkspaceScreenModel> = None;
    sync_core_screen_size(&mut outcome);

    let (terminal_width, terminal_height) = current_terminal_size();
    let initial_render = apply_workspace_redraw_transaction(
        &mut last_successful_workspace_model,
        build_workspace_render_output(
            &mut outcome,
            &session_state,
            &mut viewport_store,
            &mut search_refresh_store,
            None,
            "",
            None,
            None,
            None,
            terminal_width,
            terminal_height,
        ),
    )
    .map_err(|error| format!("initial PTY redraw failed: {error}"))?;

    let initial_active_pane = initial_render
        .model
        .panes
        .iter()
        .find(|pane| pane.window_id == initial_render.model.active_window_id)
        .ok_or_else(|| "initial PTY draw missing active pane".to_string())?;
    eprintln!(
        "[main][pty-smoke] initial draw: panes={}, active_window_id={}, cursor=({},{}), status={:?}, message={:?}",
        initial_render.model.panes.len(),
        initial_render.model.active_window_id,
        initial_active_pane.cursor_row,
        initial_active_pane.cursor_col,
        initial_render
            .model
            .panes
            .iter()
            .find(|pane| pane.window_id == initial_render.model.active_window_id)
            .map(|pane| format!("{} | {}", pane.file_name, pane.mode_label)),
        initial_render.model.global_message_line,
    );
    renderer
        .draw(&initial_render.model)
        .map_err(|error| format!("initial PTY draw failed: {error}"))?;

    std::thread::sleep(std::time::Duration::from_millis(25));

    outcome
        .core_bridge
        .apply_ex_command(":split")
        .map_err(|error| format!("PTY split command failed: {error:?}"))?;

    let split_render = apply_workspace_redraw_transaction(
        &mut last_successful_workspace_model,
        build_workspace_render_output(
            &mut outcome,
            &session_state,
            &mut viewport_store,
            &mut search_refresh_store,
            None,
            "",
            None,
            None,
            None,
            terminal_width,
            terminal_height,
        ),
    )
    .map_err(|error| format!("split PTY redraw failed: {error}"))?;

    let split_active_pane = split_render
        .model
        .panes
        .iter()
        .find(|pane| pane.window_id == split_render.model.active_window_id)
        .ok_or_else(|| "split PTY draw missing active pane".to_string())?;
    let split_status = split_render
        .model
        .panes
        .iter()
        .find(|pane| pane.window_id == split_render.model.active_window_id)
        .map(|pane| format!("{} | {}", pane.file_name, pane.mode_label));
    eprintln!(
        "[main][pty-smoke] split draw: panes={}, active_window_id={}, cursor=({},{}), status={:?}, message={:?}",
        split_render.model.panes.len(),
        split_render.model.active_window_id,
        split_active_pane.cursor_row,
        split_active_pane.cursor_col,
        split_status,
        split_render.model.global_message_line,
    );
    renderer
        .draw(&split_render.model)
        .map_err(|error| format!("split PTY draw failed: {error}"))?;

    std::thread::sleep(std::time::Duration::from_millis(25));

    let rollback_render = apply_workspace_redraw_transaction(
        &mut last_successful_workspace_model,
        Err(WorkspaceRedrawError::Projection(
            WorkspaceProjectionError::ActiveWindowMissing,
        )),
    )
    .map_err(|error| format!("rollback PTY redraw failed: {error}"))?;

    let rollback_active_pane = rollback_render
        .model
        .panes
        .iter()
        .find(|pane| pane.window_id == rollback_render.model.active_window_id);
    eprintln!(
        "[main][pty-smoke] rollback draw: panes={}, active_window_id={}, cursor=({}, {}), message={:?}",
        rollback_render.model.panes.len(),
        rollback_render.model.active_window_id,
        rollback_active_pane
            .map(|pane| pane.cursor_row)
            .unwrap_or_default(),
        rollback_active_pane
            .map(|pane| pane.cursor_col)
            .unwrap_or_default(),
        rollback_render.model.global_message_line,
    );
    renderer
        .draw(&rollback_render.model)
        .map_err(|error| format!("rollback PTY draw failed: {error}"))?;

    drop(renderer);
    drop(outcome);
    terminal_session
        .restore()
        .map_err(|error| format!("PTY smoke terminal restore failed: {error:?}"))?;

    eprintln!("[main][pty-smoke] completed successfully");
    Ok(())
}

async fn process_pending_host_actions_with_runtime(
    outcome: &mut saya::bootstrap::BootstrapOutcome,
    session_state: &mut saya::editor_session::EditorSessionState,
    transient_msg: &mut Option<String>,
    system_warning: &mut Option<String>,
    mut runtime_session: Option<&mut RuntimeSessionOwner>,
    need_redraw: &mut bool,
) -> Option<ShutdownReason> {
    let current_revision = outcome.core_bridge.snapshot().revision;
    let mut shutdown_reason = None;
    for action in prioritize_save_family_host_actions(
        outcome.core_bridge.take_pending_host_actions(),
        current_revision,
    ) {
        match action {
            CoreHostAction::Write { path, .. } => {
                if let Some(reason) = handle_write_host_action_with_runtime(
                    outcome,
                    session_state,
                    Some(path.as_str()),
                    transient_msg,
                    runtime_session.as_deref_mut(),
                    need_redraw,
                )
                .await
                {
                    merge_shutdown_reason(&mut shutdown_reason, Some(reason));
                }
            }
            CoreHostAction::Quit { force, .. } => {
                let decision = session_state.evaluate_quit(force);
                if let Some(reason) =
                    shutdown_reason_from_quit_decision(decision, force, system_warning)
                {
                    merge_shutdown_reason(&mut shutdown_reason, Some(reason));
                }
            }
            _ => {}
        }
    }

    shutdown_reason
}

fn prioritize_save_family_host_actions(
    actions: Vec<CoreHostAction>,
    current_revision: u64,
) -> Vec<CoreHostAction> {
    let mut writes = Vec::new();
    let mut quits = Vec::new();

    for action in actions {
        match action {
            CoreHostAction::Write {
                issued_after_revision,
                ..
            } if issued_after_revision == current_revision => {
                writes.push(action);
            }
            CoreHostAction::Quit {
                issued_after_revision,
                ..
            } if issued_after_revision == current_revision => {
                quits.push(action);
            }
            CoreHostAction::Write {
                issued_after_revision,
                ..
            } => {
                log::debug!(
                    "[main] skipping stale write host action: current_revision={}, issued_after_revision={}",
                    current_revision,
                    issued_after_revision
                );
            }
            CoreHostAction::Quit {
                issued_after_revision,
                ..
            } => {
                log::debug!(
                    "[main] skipping stale quit host action: current_revision={}, issued_after_revision={}",
                    current_revision,
                    issued_after_revision
                );
            }
            other => {
                log::debug!(
                    "[main] ignoring unsupported pending host action during save-family coordination: {:?}",
                    other
                );
            }
        }
    }

    writes.extend(quits);
    writes
}

fn process_pending_host_actions_without_runtime(
    outcome: &mut saya::bootstrap::BootstrapOutcome,
    session_state: &mut saya::editor_session::EditorSessionState,
    transient_msg: &mut Option<String>,
    system_warning: &mut Option<String>,
) -> Option<ShutdownReason> {
    let current_revision = outcome.core_bridge.snapshot().revision;
    for action in prioritize_save_family_host_actions(
        outcome.core_bridge.take_pending_host_actions(),
        current_revision,
    ) {
        match action {
            CoreHostAction::Write { path, .. } => {
                let snapshot = outcome.core_bridge.snapshot();
                let save_outcome = save_snapshot_result_with_path_override(
                    &snapshot.text,
                    session_state,
                    Some(path.as_str()),
                );
                *transient_msg = save_outcome.transient_message;
            }
            CoreHostAction::Quit { force, .. } => {
                let decision = session_state.evaluate_quit(force);
                if let Some(reason) =
                    shutdown_reason_from_quit_decision(decision, force, system_warning)
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
    path_override: Option<&str>,
    transient_msg: &mut Option<String>,
    runtime_session: Option<&mut RuntimeSessionOwner>,
    need_redraw: &mut bool,
) -> Option<ShutdownReason> {
    let snapshot = outcome.core_bridge.snapshot();
    log::debug!(
        "[main] processing write host action with runtime integration: path_present={}, contents_len={}",
        path_override.filter(|path| !path.is_empty()).is_some()
            || session_state.target_path().is_some(),
        snapshot.text.len()
    );
    let save_outcome =
        save_snapshot_result_with_path_override(&snapshot.text, session_state, path_override);
    *transient_msg = save_outcome.transient_message;
    if save_outcome.wrote {
        return dispatch_buffer_write_post_with_runtime(
            runtime_session,
            outcome,
            session_state,
            transient_msg,
            need_redraw,
        )
        .await;
    }

    None
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
    save_snapshot_result_with_path_override(buffer_contents, session_state, None)
}

fn save_snapshot_result_with_path_override(
    buffer_contents: &str,
    session_state: &mut saya::editor_session::EditorSessionState,
    path_override: Option<&str>,
) -> SaveSnapshotOutcome {
    match build_save_request_for_host_write(buffer_contents, session_state, path_override) {
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

fn build_save_request_for_host_write(
    buffer_contents: &str,
    session_state: &saya::editor_session::EditorSessionState,
    path_override: Option<&str>,
) -> Result<SaveRequest, SaveRequestError> {
    let Some(path_override) = path_override.filter(|path| !path.is_empty()) else {
        return session_state.build_save_request(buffer_contents);
    };

    if session_state.read_only() {
        return Err(SaveRequestError::ReadOnly);
    }

    Ok(SaveRequest {
        path: std::path::PathBuf::from(path_override),
        contents: buffer_contents.to_string(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MainHostCommand {
    Save,
    SaveThenQuit,
}

fn parse_main_host_command(command: &str) -> Option<MainHostCommand> {
    let normalized = normalize_main_host_command(command)?;
    match normalized.as_str() {
        "w" | "write" => Some(MainHostCommand::Save),
        "wq" | "x" | "xit" | "exit" => Some(MainHostCommand::SaveThenQuit),
        _ => None,
    }
}

fn runtime_save_then_quit_ex_command(command: &str) -> Option<&'static str> {
    let normalized = normalize_main_host_command(command)?;
    match normalized.as_str() {
        "wq" => Some(":wq"),
        "x" | "xit" | "exit" => Some(":x"),
        _ => None,
    }
}

fn normalize_main_host_command(command: &str) -> Option<String> {
    let trimmed = command.trim();
    let trimmed = trimmed.strip_prefix(':').unwrap_or(trimmed).trim();
    if trimmed.is_empty() {
        return None;
    }

    Some(trimmed.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn runtime_shutdown_intent_from_quit_decision(
    force: bool,
    decision: QuitDecision,
) -> Option<RuntimeShutdownIntent> {
    match decision {
        QuitDecision::Allow => Some(RuntimeShutdownIntent::UserQuit),
        QuitDecision::ForceQuit => Some(RuntimeShutdownIntent::UserForceQuit),
        QuitDecision::WarnUnsaved => {
            log::debug!(
                "[main] runtime host command quit intent was rejected by session policy: force={}, decision={:?}",
                force,
                decision
            );
            None
        }
    }
}

fn merge_runtime_shutdown_intent(
    current: &mut Option<RuntimeShutdownIntent>,
    next: Option<RuntimeShutdownIntent>,
) {
    match (*current, next) {
        (None, Some(intent)) => *current = Some(intent),
        (Some(RuntimeShutdownIntent::UserQuit), Some(RuntimeShutdownIntent::UserForceQuit)) => {
            *current = Some(RuntimeShutdownIntent::UserForceQuit);
        }
        _ => {}
    }
}

fn merge_shutdown_reason(current: &mut Option<ShutdownReason>, next: Option<ShutdownReason>) {
    match (current.clone(), next) {
        (None, Some(reason)) => *current = Some(reason),
        (Some(ShutdownReason::UserQuit), Some(ShutdownReason::UserForceQuit)) => {
            *current = Some(ShutdownReason::UserForceQuit);
        }
        _ => {}
    }
}

fn execute_runtime_host_command_through_core(
    ex_command: &str,
    outcome: &mut saya::bootstrap::BootstrapOutcome,
    session_state: &mut saya::editor_session::EditorSessionState,
) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
    outcome
        .core_bridge
        .apply_ex_command(ex_command)
        .map_err(|error| RuntimeCommandError::CommandFailed {
            name: ex_command.to_string(),
            message: format!("{error:?}"),
        })?;

    let mut effect = RuntimeCommandEffect::default();
    let current_revision = outcome.core_bridge.snapshot().revision;
    for action in prioritize_save_family_host_actions(
        outcome.core_bridge.take_pending_host_actions(),
        current_revision,
    ) {
        match action {
            CoreHostAction::Write { path, .. } => {
                let snapshot = outcome.core_bridge.snapshot();
                let save_outcome = save_snapshot_result_with_path_override(
                    &snapshot.text,
                    session_state,
                    Some(path.as_str()),
                );
                effect.transient_message = save_outcome.transient_message;
                if save_outcome.wrote {
                    let mut host_session = MainRuntimeHostSession::new(outcome, session_state);
                    effect
                        .follow_up_events
                        .push(RuntimeEventMapper::buffer_write_post(
                            host_session.current_buffer_snapshot(),
                        ));
                }
            }
            CoreHostAction::Quit { force, .. } => {
                let decision = session_state.evaluate_quit(force);
                merge_runtime_shutdown_intent(
                    &mut effect.shutdown_intent,
                    runtime_shutdown_intent_from_quit_decision(force, decision),
                );
            }
            _ => {}
        }
    }

    Ok(effect)
}

fn execute_runtime_host_command(
    command: &str,
    outcome: &mut saya::bootstrap::BootstrapOutcome,
    session_state: &mut saya::editor_session::EditorSessionState,
) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
    match parse_main_host_command(command) {
        Some(MainHostCommand::Save) => {
            execute_runtime_host_command_through_core(":w", outcome, session_state)
        }
        Some(MainHostCommand::SaveThenQuit) => {
            let core_command = runtime_save_then_quit_ex_command(command).ok_or_else(|| {
                RuntimeCommandError::UnknownCommand {
                    name: command.to_string(),
                }
            })?;
            log::debug!(
                "[main] routing runtime save/quit command through core coordinator: command={:?}, core_command={}",
                command,
                core_command
            );
            execute_runtime_host_command_through_core(core_command, outcome, session_state)
        }
        None => Err(RuntimeCommandError::UnknownCommand {
            name: command.to_string(),
        }),
    }
}

fn save_error_message(error: &SaveRequestError) -> String {
    match error {
        SaveRequestError::NoTargetPath => "No file name to save".to_string(),
        SaveRequestError::ReadOnly => "Read-only option is set; add ! to override".to_string(),
    }
}

fn update_core_message_from_core(
    core_bridge: &mut saya::core_bridge::CoreBridge,
    core_message: &mut Option<String>,
) {
    if let Some(message) = latest_user_visible_message(core_bridge.take_pending_messages()) {
        log::debug!(
            "[main] replacing core message from core bridge: {:?}",
            message
        );
        *core_message = Some(message);
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

async fn dispatch_buffer_open_with_runtime(
    runtime_session: Option<&mut RuntimeSessionOwner>,
    outcome: &mut saya::bootstrap::BootstrapOutcome,
    session_state: &mut saya::editor_session::EditorSessionState,
    transient_msg: &mut Option<String>,
    need_redraw: &mut bool,
) -> Option<ShutdownReason> {
    let Some(runtime_session) = runtime_session else {
        return None;
    };
    let mut host_session = MainRuntimeHostSession::new(outcome, session_state);
    let payload = RuntimeEventMapper::buffer_open(host_session.current_buffer_snapshot());
    let dispatch_outcome = runtime_session.dispatch(payload, &mut host_session).await;
    apply_runtime_dispatch_outcome(transient_msg, need_redraw, dispatch_outcome)
}

async fn dispatch_buffer_write_post_with_runtime(
    runtime_session: Option<&mut RuntimeSessionOwner>,
    outcome: &mut saya::bootstrap::BootstrapOutcome,
    session_state: &mut saya::editor_session::EditorSessionState,
    transient_msg: &mut Option<String>,
    need_redraw: &mut bool,
) -> Option<ShutdownReason> {
    let Some(runtime_session) = runtime_session else {
        return None;
    };
    let mut host_session = MainRuntimeHostSession::new(outcome, session_state);
    let payload = RuntimeEventMapper::buffer_write_post(host_session.current_buffer_snapshot());
    let dispatch_outcome = runtime_session.dispatch(payload, &mut host_session).await;
    apply_runtime_dispatch_outcome(transient_msg, need_redraw, dispatch_outcome)
}

fn apply_runtime_dispatch_outcome(
    transient_msg: &mut Option<String>,
    need_redraw: &mut bool,
    dispatch_outcome: RuntimeDispatchOutcome,
) -> Option<ShutdownReason> {
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
    dispatch_outcome.shutdown_intent.map(|intent| match intent {
        RuntimeShutdownIntent::UserQuit => ShutdownReason::UserQuit,
        RuntimeShutdownIntent::UserForceQuit => ShutdownReason::UserForceQuit,
    })
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
        let active_window_id = resolve_runtime_current_window_id(&snapshot)
            .expect("runtime current window should resolve from active window id");
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
        execute_runtime_host_command(name, self.outcome, self.session_state)
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
    system_warning: &mut Option<String>,
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
            *system_warning = Some(normal_quit_warning_message().to_string());
            None
        }
    }
}

fn normal_quit_warning_message() -> &'static str {
    "No write since last change (add ! to override)"
}

fn current_terminal_size() -> (u16, u16) {
    crossterm::terminal::size().unwrap_or((80, 24))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkspaceRenderOutput {
    model: WorkspaceScreenModel,
    failure_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WorkspaceRedrawError {
    Projection(WorkspaceProjectionError),
    Search {
        window_id: i32,
        error: SearchStateError,
    },
}

impl fmt::Display for WorkspaceRedrawError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorkspaceRedrawError::Projection(error) => {
                write!(f, "workspace projection failed: {error}")
            }
            WorkspaceRedrawError::Search { window_id, error } => {
                write!(
                    f,
                    "workspace search refresh failed: window_id={window_id}, {error}"
                )
            }
        }
    }
}

impl From<WorkspaceProjectionError> for WorkspaceRedrawError {
    fn from(error: WorkspaceProjectionError) -> Self {
        WorkspaceRedrawError::Projection(error)
    }
}

fn apply_workspace_redraw_transaction(
    last_successful_workspace_model: &mut Option<WorkspaceScreenModel>,
    render_result: Result<WorkspaceScreenModel, WorkspaceRedrawError>,
) -> Result<WorkspaceRenderOutput, WorkspaceRedrawError> {
    match render_result {
        Ok(model) => {
            *last_successful_workspace_model = Some(model.clone());
            Ok(WorkspaceRenderOutput {
                model,
                failure_message: None,
            })
        }
        Err(error) => {
            log::debug!(
                "[main] workspace redraw failed; attempting rollback to last successful model: error={:?}",
                error
            );
            if let Some(last_successful) = last_successful_workspace_model.as_ref() {
                let mut rollback_model = last_successful.clone();
                let failure_message = error.to_string();
                rollback_model.global_message_line = Some(failure_message.clone());
                Ok(WorkspaceRenderOutput {
                    model: rollback_model,
                    failure_message: Some(failure_message),
                })
            } else {
                Err(error)
            }
        }
    }
}

fn build_workspace_render_output(
    outcome: &mut saya::bootstrap::BootstrapOutcome,
    session_state: &saya::editor_session::EditorSessionState,
    viewport_store: &mut WindowViewportStore,
    search_refresh_store: &mut WindowSearchRefreshStore,
    command_line_prompt: Option<char>,
    command_line_buffer: &str,
    core_message: Option<&str>,
    system_warning: Option<&str>,
    transient_msg: Option<&str>,
    terminal_width: u16,
    terminal_height: u16,
) -> Result<WorkspaceScreenModel, WorkspaceRedrawError> {
    let snapshot = outcome.core_bridge.snapshot();
    let visual_selection = outcome.core_bridge.current_visual_selection();
    viewport_store.sync_from_windows(&snapshot.windows);
    search_refresh_store.retain_windows(
        &snapshot
            .windows
            .iter()
            .map(|window| window.id)
            .collect::<Vec<_>>(),
    );
    let search_states = collect_workspace_search_states(
        &mut outcome.core_bridge,
        &snapshot,
        viewport_store,
        search_refresh_store,
        resolve_prompt_revision(command_line_prompt, command_line_buffer),
        resolve_search_mode_hint(command_line_prompt, command_line_buffer),
    )?;
    let command_preview =
        command_line_prompt.map(|prompt| format!("{}{}", prompt, command_line_buffer));

    project_workspace(&WorkspaceProjectionInput {
        snapshot: &snapshot,
        session_state,
        visual_selection: visual_selection.as_ref(),
        search_states: &search_states,
        command_preview: command_preview.as_deref(),
        core_message,
        system_warning,
        transient_info: transient_msg,
        viewport_store,
        terminal_width,
        terminal_height,
    })
    .map_err(WorkspaceRedrawError::from)
}

fn sync_core_screen_size(outcome: &mut saya::bootstrap::BootstrapOutcome) {
    if let Ok((cols, rows)) = crossterm::terminal::size() {
        outcome
            .core_bridge
            .set_screen_size(i32::from(rows), i32::from(cols));
    }
}

fn buffer_line_count(text: &str) -> usize {
    text.lines().count().max(1)
}

fn resolve_runtime_current_window_id(snapshot: &vim_core_rs::CoreSnapshot) -> Option<u64> {
    let active_window_id = snapshot
        .active_window_id()
        .map(|window_id| window_id as u64);
    log::debug!(
        "[main] resolve runtime current window id: snapshot_active_window_id={:?}, chosen_window_id={:?}",
        snapshot.active_window_id(),
        active_window_id,
    );
    active_window_id
}

fn trace_workspace_render_pipeline(
    phase: &str,
    snapshot_text: &str,
    workspace_model: &saya::screen_model::WorkspaceScreenModel,
) {
    if std::env::var_os("SAYA_TRACE_RENDER").is_none() {
        return;
    }

    let Some(active_pane) = workspace_model
        .panes
        .iter()
        .find(|pane| pane.window_id == workspace_model.active_window_id)
    else {
        return;
    };
    let absolute_row = 6usize;
    let snapshot_line = snapshot_text.lines().nth(absolute_row).unwrap_or("");
    let viewport_top = usize::from(active_pane.rect.y);
    let visible_row = absolute_row.checked_sub(viewport_top);
    let projected_line = visible_row
        .and_then(|row| active_pane.lines.get(row))
        .map(String::as_str)
        .unwrap_or("");

    eprintln!(
        "[saya-trace][main][{phase}] viewport_top={viewport_top} abs_row=7 snapshot={snapshot_line:?} projected={projected_line:?}"
    );
}

fn collect_workspace_search_states(
    core_bridge: &mut saya::core_bridge::CoreBridge,
    snapshot: &vim_core_rs::CoreSnapshot,
    viewport_store: &WindowViewportStore,
    search_refresh_store: &mut WindowSearchRefreshStore,
    prompt_revision: Option<u64>,
    search_mode_hint: SearchModeHint,
) -> Result<BTreeMap<i32, SearchVisibleState>, WorkspaceRedrawError> {
    let mut search_states = BTreeMap::new();
    for window in &snapshot.windows {
        let body_height = usize::try_from(window.height.saturating_sub(1))
            .unwrap_or(1)
            .max(1);
        let viewport_top = viewport_store
            .get(window.id)
            .map(|viewport| viewport.top_line())
            .unwrap_or_else(|| window.topline.saturating_sub(1));
        let outcome = search_refresh_store.update_window(
            core_bridge,
            SearchRefreshInput {
                window_id: window.id,
                revision: snapshot.revision as u64,
                viewport_top,
                viewport_height: body_height,
                cursor_row: window.cursor_row,
                cursor_col: window.cursor_col,
                prompt_revision,
                search_mode_hint,
            },
        );
        if let Some(error) = outcome.query_error {
            log::debug!(
                "[main] workspace search refresh failed: window_id={}, error={:?}",
                window.id,
                error
            );
            return Err(WorkspaceRedrawError::Search {
                window_id: window.id,
                error,
            });
        }
        if let Some(render_state) = outcome.render_state {
            search_states.insert(window.id, render_state);
        }
    }
    Ok(search_states)
}

fn resolve_search_mode_hint(
    command_line_prompt: Option<char>,
    command_line_buffer: &str,
) -> SearchModeHint {
    if command_line_prompt == Some('/') && !command_line_buffer.is_empty() {
        SearchModeHint::Incsearch
    } else {
        SearchModeHint::Hlsearch
    }
}

fn resolve_prompt_revision(
    command_line_prompt: Option<char>,
    command_line_buffer: &str,
) -> Option<u64> {
    let prompt = command_line_prompt?;
    let mut hasher = DefaultHasher::new();
    prompt.hash(&mut hasher);
    command_line_buffer.hash(&mut hasher);
    Some(hasher.finish())
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
        "                    Default: $XDG_CONFIG_HOME/saya/init.ts",
        "                    Fallback: $HOME/.config/saya/init.ts",
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
    fn merge_shutdown_reason_prefers_force_quit_over_clean_quit() {
        let mut shutdown_reason = Some(ShutdownReason::UserQuit);

        merge_shutdown_reason(&mut shutdown_reason, Some(ShutdownReason::UserForceQuit));

        assert_eq!(shutdown_reason, Some(ShutdownReason::UserForceQuit));
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
        let save_outcome = save_snapshot_result(&snapshot.text, &mut session_state);
        let transient_msg = save_outcome.transient_message;
        let expected_error = session_state
            .last_save_error()
            .expect("save failure should be recorded")
            .to_string();
        let expected_message = format!("Save failed: {}", expected_error);

        assert_eq!(transient_msg, Some(expected_message.clone()));
        assert_eq!(transient_msg.as_deref(), Some(expected_message.as_str()));
        assert!(session_state.is_dirty());

        std::fs::remove_file(&target_path).expect("cleanup");
    }

    #[test]
    fn parse_main_host_command_recognizes_save_and_quit_family_commands() {
        assert_eq!(parse_main_host_command(":w"), Some(MainHostCommand::Save));
        assert_eq!(
            parse_main_host_command("write"),
            Some(MainHostCommand::Save)
        );
        assert_eq!(
            parse_main_host_command(":wq"),
            Some(MainHostCommand::SaveThenQuit)
        );
        assert_eq!(
            parse_main_host_command("wq"),
            Some(MainHostCommand::SaveThenQuit)
        );
        assert_eq!(
            parse_main_host_command("exit"),
            Some(MainHostCommand::SaveThenQuit)
        );
        assert_eq!(parse_main_host_command("set number"), None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runtime_host_command_executor_routes_quit_family_through_coordinator() {
        let _lock = saya::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let target_path = unique_path("runtime-host-command");
        std::fs::write(&target_path, "initial\n").expect("test file");

        let mut outcome = saya::bootstrap::prepare_launch(saya::cli::LaunchRequest {
            input_source: saya::cli::InputSource::File(target_path.clone()),
            config_source: saya::cli::ConfigSource::Default,
            ..saya::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();

        outcome.core_bridge.dispatch_key("i").unwrap();
        outcome.core_bridge.dispatch_key("X").unwrap();
        outcome.core_bridge.dispatch_key("\x1b").unwrap();
        session_state.update_dirty(outcome.core_bridge.snapshot().dirty);

        let effect = execute_runtime_host_command("exit", &mut outcome, &mut session_state)
            .expect("runtime quit-family command should succeed");

        assert_eq!(
            effect.transient_message,
            Some("Saved successfully".to_string())
        );
        assert_eq!(
            effect.shutdown_intent,
            Some(RuntimeShutdownIntent::UserQuit)
        );
        assert!(matches!(
            effect.follow_up_events.as_slice(),
            [saya::saya_live_runtime::RuntimeEventPayload::BufferWritePost(_)]
        ));
        assert_eq!(
            std::fs::read_to_string(&target_path).expect("saved file should exist"),
            "Xinitial\n"
        );

        std::fs::remove_file(&target_path).expect("cleanup");
    }

    #[test]
    fn apply_runtime_dispatch_outcome_returns_shutdown_reason_from_runtime_intent() {
        let mut transient_msg = None;
        let mut need_redraw = false;

        let shutdown_reason = apply_runtime_dispatch_outcome(
            &mut transient_msg,
            &mut need_redraw,
            RuntimeDispatchOutcome {
                transient_message: Some("Saved successfully".to_string()),
                requires_redraw: true,
                shutdown_intent: Some(RuntimeShutdownIntent::UserQuit),
            },
        );

        assert_eq!(shutdown_reason, Some(ShutdownReason::UserQuit));
        assert_eq!(transient_msg, Some("Saved successfully".to_string()));
        assert!(need_redraw);
    }

    #[test]
    fn save_snapshot_result_with_path_override_writes_to_explicit_host_path() {
        let original_path = unique_path("write-override-original");
        let alternate_path = unique_path("write-override-alternate");
        std::fs::write(&original_path, "original\n").expect("original file");
        let mut session_state =
            saya::editor_session::EditorSessionState::new(Some(original_path.clone()));
        session_state.update_dirty(true);
        let alternate_path_string = alternate_path.display().to_string();

        let save_outcome = save_snapshot_result_with_path_override(
            "alternate\n",
            &mut session_state,
            Some(&alternate_path_string),
        );

        assert_eq!(
            save_outcome,
            SaveSnapshotOutcome {
                transient_message: Some("Saved successfully".to_string()),
                wrote: true,
            }
        );
        assert_eq!(
            std::fs::read_to_string(&alternate_path).expect("alternate file should exist"),
            "alternate\n",
            "explicit host action path should receive the save contents"
        );
        assert_eq!(
            std::fs::read_to_string(&original_path).expect("original file should remain"),
            "original\n",
            "session target path should stay untouched when host action provides an explicit path"
        );
        assert!(
            !session_state.is_dirty(),
            "successful save should clear dirty"
        );

        std::fs::remove_file(&original_path).expect("cleanup original");
        std::fs::remove_file(&alternate_path).expect("cleanup alternate");
    }

    #[test]
    fn save_family_host_actions_are_prioritized_by_revision_and_kind() {
        let actions = vec![
            CoreHostAction::Quit {
                force: false,
                issued_after_revision: 9,
            },
            CoreHostAction::Write {
                path: "stale.txt".to_string(),
                force: false,
                issued_after_revision: 8,
            },
            CoreHostAction::Quit {
                force: false,
                issued_after_revision: 8,
            },
            CoreHostAction::Write {
                path: "fresh.txt".to_string(),
                force: false,
                issued_after_revision: 9,
            },
        ];

        let prioritized = prioritize_save_family_host_actions(actions, 9);

        assert_eq!(
            prioritized,
            vec![
                CoreHostAction::Write {
                    path: "fresh.txt".to_string(),
                    force: false,
                    issued_after_revision: 9,
                },
                CoreHostAction::Quit {
                    force: false,
                    issued_after_revision: 9,
                },
            ]
        );
    }

    #[test]
    fn prompt_revision_changes_when_search_buffer_text_changes_with_same_length() {
        let alpha = resolve_prompt_revision(Some('/'), "ab");
        let omega = resolve_prompt_revision(Some('/'), "cd");

        assert_ne!(alpha, omega);
    }

    #[test]
    fn runtime_current_window_id_keeps_explicit_failure_when_snapshot_has_no_active_window() {
        let _lock = saya::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let bridge = saya::core_bridge::CoreBridge::new("alpha\nbeta\n").expect("core bridge");
        let mut snapshot = bridge.snapshot();
        snapshot.windows[0].id = 42;
        snapshot.windows[0].is_active = false;

        assert_eq!(
            resolve_runtime_current_window_id(&snapshot),
            None,
            "runtime current window は固定 fallback を返さず explicit failure を保つこと"
        );
    }

    #[test]
    fn workspace_redraw_transaction_rolls_back_to_last_successful_model_with_failure_message() {
        let mut last_successful_workspace_model = Some(WorkspaceScreenModel {
            panes: vec![saya::screen_model::ScreenModel {
                window_id: 1,
                buffer_id: 1,
                rect: saya::screen_model::PaneRect {
                    x: 0,
                    y: 0,
                    width: 20,
                    height: 3,
                },
                file_name: "alpha.txt".to_string(),
                mode_label: "NORMAL".to_string(),
                dirty: false,
                lines: vec!["alpha".to_string()],
                cursor_row: 0,
                cursor_col: 0,
                visual_selection: None,
                search_overlays: vec![],
                message_line: None,
                command_cursor_col: None,
                is_active: true,
            }],
            active_window_id: 1,
            global_message_line: None,
            command_line: None,
        });

        let output = apply_workspace_redraw_transaction(
            &mut last_successful_workspace_model,
            Err(WorkspaceRedrawError::Projection(
                WorkspaceProjectionError::ActiveWindowMissing,
            )),
        )
        .expect("rollback should return the previous successful model");

        assert_eq!(output.model.active_window_id, 1);
        assert_eq!(output.model.panes.len(), 1);
        assert_eq!(
            output.model.global_message_line,
            Some("workspace projection failed: active window could not be resolved".to_string())
        );
        assert_eq!(output.failure_message, output.model.global_message_line);
        assert_eq!(
            last_successful_workspace_model
                .as_ref()
                .expect("last successful model should be retained")
                .global_message_line,
            None
        );
    }

    #[test]
    fn prompt_revision_distinguishes_search_and_command_prompts() {
        let search = resolve_prompt_revision(Some('/'), "word");
        let command = resolve_prompt_revision(Some(':'), "word");

        assert_ne!(search, command);
    }

    #[test]
    fn prompt_revision_is_none_when_prompt_is_inactive() {
        assert_eq!(resolve_prompt_revision(None, "word"), None);
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
        assert!(help.contains("Default: $XDG_CONFIG_HOME/saya/init.ts"));
        assert!(help.contains("Fallback: $HOME/.config/saya/init.ts"));
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
