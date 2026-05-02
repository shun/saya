use saya::app_startup::{
    LaunchStartError, PreparedTuiStartup, TuiStartupContextError, prepare_tui_startup_context,
};
use saya::bootstrap::{BootstrapError, bootstrap_warning_message};
use saya::cli::{CliParseError, StartupAction, parse_launch_request};
use saya::core_host_actions::HostActionRuntime;
use saya::core_notification_prompt::{
    NotificationPromptProjectionState, ProjectionFrame, PromptInputAction, handle_prompt_key,
    record_prompt_response_error,
};
use saya::core_outcome::{
    ApplicationDispatchEffects, ApplicationOutcomeState, NormalizedHostDirective,
    NormalizedOutcomeBatch, fold_normalized_outcomes,
};
use saya::core_prompt::PromptResponseCommand;
use saya::editor_session::{QuitDecision, SaveRequestError};
use saya::event_loop::{EventLoopCoordinator, LoopAction, ShutdownReason, UiEvent};
use saya::ex_command::{ExCommandRoute, apply_local_ex_command, route_ex_command};
use saya::host_io::{SaveRequest, SaveResult, write_to_path};
use saya::input_loop::CrosstermEventSource;
use saya::input_router::{EditorIntent, KeyInput, resolve_intent};
use saya::optional_graphics::OptionalGraphicsAdapter;
use saya::overlay_asset_store::OverlayAssetStore;
use saya::presentation_effect::RuntimePresentationIntent;
use saya::runtime_integration::{
    RuntimeCommandEffect, RuntimeDispatchOutcome, RuntimeEventMapper, RuntimeHostSession,
    RuntimeSessionOwner, RuntimeShutdownIntent,
};
use saya::saya_live_runtime::{
    ReadonlyBufferSnapshot, ReadonlyEditorSnapshot, ReadonlyWindowSnapshot, RuntimeCommandError,
    RuntimeMode,
};
use saya::screen_model::{
    ProjectionInput, WorkspaceProjectionError, WorkspaceProjectionInput, WorkspaceScreenModel,
    project, project_workspace,
};
use saya::search_query::{SearchStateError, SearchVisibleState};
use saya::search_refresh::{SearchModeHint, SearchRefreshInput, WindowSearchRefreshStore};
use saya::structural_refresh::{StructuralRefresh, StructuralRefreshOutcome};
use saya::terminal_capability::TerminalCapabilityProbe;
use saya::terminal_lifecycle::TerminalSize;
use saya::tui_render_coordinator::TuiRenderCoordinator;
use saya::tui_renderer::{CrosstermBackendImpl, TuiRenderer};
use saya::viewport::WindowViewportStore;
#[cfg(test)]
use vim_core_rs::CoreMessageEvent;
use vim_core_rs::CoreMode;

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::hash::{Hash, Hasher};

#[derive(Debug, Default)]
struct MainOutcomeAccumulator {
    state: ApplicationOutcomeState,
    host_directives: Vec<NormalizedHostDirective>,
    projection: NotificationPromptProjectionState,
    last_projection_frame: Option<ProjectionFrame>,
    last_structural_refresh: Option<StructuralRefreshOutcome>,
}

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
        if let Err(error) = run_binary_pty_smoke(launch_request).await {
            eprintln!("[main][pty-smoke] {error}");
            std::process::exit(1);
        }
        std::process::exit(0);
    }

    let mut backend = CrosstermBackendImpl;
    let mut capability_probe = TerminalCapabilityProbe::from_env();
    let PreparedTuiStartup {
        mut outcome,
        mut terminal_broker,
        capability_profile,
        mut runtime_session,
        runtime_init_message,
    } = match prepare_tui_startup_context(launch_request, &mut backend, &mut capability_probe) {
        Ok(startup) => startup,
        Err(error) => {
            log::debug!("{}", format_tui_startup_context_error(error));
            std::process::exit(1);
        }
    };

    let renderer = TuiRenderer::new().expect("TUI Renderer init failed");
    let mut render_coordinator = TuiRenderCoordinator::new(
        renderer,
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    );
    let mut session_state = outcome.editor_session_state();
    let mut system_warning: Option<String> = bootstrap_warning_message(&outcome.warnings);
    let mut transient_msg: Option<String> = runtime_init_message;
    let mut outcome_accumulator = MainOutcomeAccumulator::default();
    let mut host_action_runtime = HostActionRuntime::default();
    let mut viewport_store = WindowViewportStore::new();
    let mut search_refresh_store = WindowSearchRefreshStore::new();
    let mut command_line_prompt: Option<char> = None;
    let mut command_line_buffer = String::new();
    let mut runtime_presentation_intents: Vec<RuntimePresentationIntent> = Vec::new();
    let mut last_workspace_model: Option<WorkspaceScreenModel> = None;

    let mut startup_runtime_redraw = false;
    let startup_shutdown_reason = dispatch_buffer_open_with_runtime(
        runtime_session.as_mut(),
        &mut outcome,
        &mut session_state,
        &mut transient_msg,
        &mut startup_runtime_redraw,
        &mut runtime_presentation_intents,
    )
    .await;

    // イベントループ初期化
    let (mut coordinator, sender) = EventLoopCoordinator::new();

    log::debug!(
        "[main] terminal capability profile resolved before interactive input: {:?}",
        capability_profile
    );
    terminal_broker
        .start_interactive_input(sender.clone(), CrosstermEventSource)
        .expect("interactive input should start after the capability probe");

    // 初期描画
    sync_core_screen_size(&mut outcome);
    let (terminal_width, terminal_height) = current_terminal_size();
    let initial_render = build_workspace_render_output(
        &mut outcome,
        &session_state,
        &mut viewport_store,
        &mut search_refresh_store,
        command_line_prompt,
        &command_line_buffer,
        outcome_accumulator.last_projection_frame.as_ref(),
        outcome_accumulator.last_structural_refresh.as_mut(),
        system_warning.as_deref(),
        transient_msg.as_deref(),
        terminal_width,
        terminal_height,
    );
    let initial_render_failure = initial_render.as_ref().err().map(ToString::to_string);
    match render_coordinator.render_workspace_result_with_structural_refresh(
        initial_render,
        &capability_profile,
        &runtime_presentation_intents,
        Some(&mut terminal_broker),
        outcome_accumulator.last_structural_refresh.as_ref(),
    ) {
        Ok(render_output) => {
            if let Some(message) = initial_render_failure {
                transient_msg = Some(message);
            }
            last_workspace_model = Some(render_output.rendered_workspace.clone());
            trace_workspace_render_pipeline(
                "initial",
                &outcome.core_bridge.snapshot().text,
                &render_output.rendered_workspace,
            );
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

                        match handle_prompt_key(&mut outcome_accumulator.projection, &key) {
                            PromptInputAction::Consumed | PromptInputAction::AwaitingCore => {
                                handled = true;
                                need_redraw = true;
                            }
                            PromptInputAction::Submit(command)
                            | PromptInputAction::Cancel(command) => {
                                handled = true;
                                dispatch_prompt_response_command(
                                    &mut outcome.core_bridge,
                                    &mut outcome_accumulator,
                                    command,
                                    &mut need_redraw,
                                );

                                if let Some(reason) = process_pending_host_actions_with_runtime(
                                    &mut outcome,
                                    &mut outcome_accumulator,
                                    &mut session_state,
                                    &mut transient_msg,
                                    &mut system_warning,
                                    &mut host_action_runtime,
                                    runtime_session.as_mut(),
                                    &mut need_redraw,
                                    &mut runtime_presentation_intents,
                                )
                                .await
                                {
                                    break 'main reason;
                                }
                            }
                            PromptInputAction::NotPromptInput => {}
                        }

                        if !handled && let Some(prompt) = command_line_prompt {
                            match key {
                                KeyInput::Escape => {
                                    if prompt == '/' {
                                        let _ = outcome.core_bridge.cancel_search_input();
                                        consume_core_outcomes_from_core(
                                            &mut outcome.core_bridge,
                                            &mut outcome_accumulator,
                                            &mut need_redraw,
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
                                                    consume_core_outcomes_from_core(
                                                        &mut outcome.core_bridge,
                                                        &mut outcome_accumulator,
                                                        &mut need_redraw,
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
                                                consume_core_outcomes_from_core(
                                                    &mut outcome.core_bridge,
                                                    &mut outcome_accumulator,
                                                    &mut need_redraw,
                                                );
                                            }
                                            ExCommandRoute::CoreOwned => {
                                                let _ = outcome.core_bridge.apply_ex_command(&cmd);
                                                consume_core_outcomes_from_core(
                                                    &mut outcome.core_bridge,
                                                    &mut outcome_accumulator,
                                                    &mut need_redraw,
                                                );
                                            }
                                        }
                                    } else if prompt == '/' {
                                        let _ = outcome
                                            .core_bridge
                                            .commit_search_input(&command_line_buffer);
                                        consume_core_outcomes_from_core(
                                            &mut outcome.core_bridge,
                                            &mut outcome_accumulator,
                                            &mut need_redraw,
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
                                        consume_core_outcomes_from_core(
                                            &mut outcome.core_bridge,
                                            &mut outcome_accumulator,
                                            &mut need_redraw,
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
                                        consume_core_outcomes_from_core(
                                            &mut outcome.core_bridge,
                                            &mut outcome_accumulator,
                                            &mut need_redraw,
                                        );
                                    }
                                }
                                _ => {}
                            }
                            handled = true;
                            need_redraw = true;

                            if let Some(reason) = process_pending_host_actions_with_runtime(
                                &mut outcome,
                                &mut outcome_accumulator,
                                &mut session_state,
                                &mut transient_msg,
                                &mut system_warning,
                                &mut host_action_runtime,
                                runtime_session.as_mut(),
                                &mut need_redraw,
                                &mut runtime_presentation_intents,
                            )
                            .await
                            {
                                break 'main reason;
                            }
                        } else if !handled
                            && (key == KeyInput::Char(':') || key == KeyInput::Char('/'))
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
                                    consume_core_outcomes_from_core(
                                        &mut outcome.core_bridge,
                                        &mut outcome_accumulator,
                                        &mut need_redraw,
                                    );

                                    if let Some(reason) = process_pending_host_actions_with_runtime(
                                        &mut outcome,
                                        &mut outcome_accumulator,
                                        &mut session_state,
                                        &mut transient_msg,
                                        &mut system_warning,
                                        &mut host_action_runtime,
                                        runtime_session.as_mut(),
                                        &mut need_redraw,
                                        &mut runtime_presentation_intents,
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
                                                &mut runtime_presentation_intents,
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
                    UiEvent::Resize { columns, rows } => {
                        terminal_broker.record_resize(TerminalSize { columns, rows });
                        need_redraw = true;
                    }
                    UiEvent::Redraw => {
                        log::debug!("[main] explicit redraw event received in drain");
                        need_redraw = true;
                    }
                    UiEvent::MouseClick { column, row } => {
                        log::debug!(
                            "[main] processing mouse click event at terminal coordinates: column={}, row={}",
                            column,
                            row
                        );
                        if let Some(sequence) =
                            mouse_click_to_sgr_sequence(last_workspace_model.as_ref(), column, row)
                        {
                            log::debug!(
                                "[main] dispatching mouse click as SGR sequence: column={}, row={}, sequence={:?}",
                                column,
                                row,
                                sequence
                            );
                            let _ = outcome.core_bridge.dispatch_key(&sequence);
                            consume_core_outcomes_from_core(
                                &mut outcome.core_bridge,
                                &mut outcome_accumulator,
                                &mut need_redraw,
                            );

                            if let Some(reason) = process_pending_host_actions_with_runtime(
                                &mut outcome,
                                &mut outcome_accumulator,
                                &mut session_state,
                                &mut transient_msg,
                                &mut system_warning,
                                &mut host_action_runtime,
                                runtime_session.as_mut(),
                                &mut need_redraw,
                                &mut runtime_presentation_intents,
                            )
                            .await
                            {
                                break 'main reason;
                            }

                            session_state.update_dirty(outcome.core_bridge.snapshot().dirty);
                        } else {
                            log::debug!(
                                "[main] ignoring mouse click outside editor body: column={}, row={}",
                                column,
                                row
                            );
                        }
                        need_redraw = true;
                    }
                    UiEvent::PastedText(text) => {
                        log::debug!(
                            "[main] dispatching pasted text to core bridge: chars={}",
                            text.chars().count()
                        );
                        for unit in pasted_text_to_dispatch_units(&text) {
                            let _ = outcome.core_bridge.dispatch_key(&unit);
                        }
                        consume_core_outcomes_from_core(
                            &mut outcome.core_bridge,
                            &mut outcome_accumulator,
                            &mut need_redraw,
                        );

                        if let Some(reason) = process_pending_host_actions_with_runtime(
                            &mut outcome,
                            &mut outcome_accumulator,
                            &mut session_state,
                            &mut transient_msg,
                            &mut system_warning,
                            &mut host_action_runtime,
                            runtime_session.as_mut(),
                            &mut need_redraw,
                            &mut runtime_presentation_intents,
                        )
                        .await
                        {
                            break 'main reason;
                        }

                        session_state.update_dirty(outcome.core_bridge.snapshot().dirty);
                        need_redraw = true;
                    }
                    UiEvent::Shutdown(reason) => {
                        log::debug!(
                            "[main] explicit shutdown event received in drain: reason={:?}",
                            reason
                        );
                        break 'main reason;
                    }
                }
            }

            if outcome_accumulator
                .projection
                .prompt()
                .active_input()
                .is_some()
                && command_line_prompt.is_some()
            {
                log::debug!(
                    "[main] clearing local command/search preview because core-owned prompt is active: prompt={:?}, buffer_len={}",
                    command_line_prompt,
                    command_line_buffer.len()
                );
                command_line_prompt = None;
                command_line_buffer.clear();
                need_redraw = true;
            }

            if need_redraw {
                sync_core_screen_size(&mut outcome);
                let (terminal_width, terminal_height) = current_terminal_size();
                let redraw_result = build_workspace_render_output(
                    &mut outcome,
                    &session_state,
                    &mut viewport_store,
                    &mut search_refresh_store,
                    command_line_prompt,
                    &command_line_buffer,
                    outcome_accumulator.last_projection_frame.as_ref(),
                    outcome_accumulator.last_structural_refresh.as_mut(),
                    system_warning.as_deref(),
                    transient_msg.as_deref(),
                    terminal_width,
                    terminal_height,
                );
                let redraw_failure = redraw_result.as_ref().err().map(ToString::to_string);
                match render_coordinator.render_workspace_result_with_structural_refresh(
                    redraw_result,
                    &capability_profile,
                    &runtime_presentation_intents,
                    Some(&mut terminal_broker),
                    outcome_accumulator.last_structural_refresh.as_ref(),
                ) {
                    Ok(render_output) => {
                        if let Some(message) = redraw_failure {
                            transient_msg = Some(message);
                        }
                        last_workspace_model = Some(render_output.rendered_workspace.clone());
                        trace_workspace_render_pipeline(
                            "redraw",
                            &outcome.core_bridge.snapshot().text,
                            &render_output.rendered_workspace,
                        );
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

    log::debug!("[main] requesting terminal broker shutdown");
    terminal_broker.request_shutdown();
    drop(sender);

    drop(render_coordinator);
    log::debug!("[main] dropping editor outcome for session cleanup");
    drop(outcome);
    shutdown_sequence.record_session_released();

    let restore_result = terminal_broker
        .shutdown()
        .await
        .map_err(|error| error.to_string());
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
    let mut need_redraw = false;
    let mut outcome_accumulator = MainOutcomeAccumulator::default();
    let mut host_action_runtime = HostActionRuntime::default();

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
    consume_core_outcomes_from_core(
        &mut outcome.core_bridge,
        &mut outcome_accumulator,
        &mut need_redraw,
    );
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
    consume_core_outcomes_from_core(
        &mut outcome.core_bridge,
        &mut outcome_accumulator,
        &mut need_redraw,
    );
    session_state.update_dirty(outcome.core_bridge.snapshot().dirty);

    let reason = process_pending_host_actions_without_runtime(
        &mut outcome,
        &mut outcome_accumulator,
        &mut session_state,
        &mut transient_msg,
        &mut system_warning,
        &mut host_action_runtime,
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

async fn run_binary_pty_smoke(launch_request: saya::cli::LaunchRequest) -> Result<(), String> {
    eprintln!("[main][pty-smoke] preparing PTY launch");
    let mut backend = CrosstermBackendImpl;
    let mut capability_probe = TerminalCapabilityProbe::from_env();
    let PreparedTuiStartup {
        mut outcome,
        mut terminal_broker,
        capability_profile,
        runtime_session: _runtime_session,
        runtime_init_message,
    } = prepare_tui_startup_context(launch_request, &mut backend, &mut capability_probe)
        .map_err(format_tui_startup_context_error)?;
    eprintln!(
        "[main][pty-smoke] capability profile: {:?}",
        capability_profile
    );
    if let Some(message) = runtime_init_message.as_deref() {
        eprintln!("[main][pty-smoke] runtime init degraded: {message}");
    }

    let renderer = TuiRenderer::new().map_err(|error| format!("TUI init failed: {error}"))?;
    let mut render_coordinator = TuiRenderCoordinator::new(
        renderer,
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    );
    let mut session_state = outcome.editor_session_state();
    let mut transient_msg = runtime_init_message;
    let mut system_warning = bootstrap_warning_message(&outcome.warnings);
    let mut viewport_store = WindowViewportStore::new();
    let mut search_refresh_store = WindowSearchRefreshStore::new();
    let mut runtime_presentation_intents: Vec<RuntimePresentationIntent> = Vec::new();
    let mut outcome_accumulator = MainOutcomeAccumulator::default();
    let mut host_action_runtime = HostActionRuntime::default();
    sync_core_screen_size(&mut outcome);

    let (terminal_width, terminal_height) = current_terminal_size();
    let initial_render = render_coordinator
        .render_workspace_result(
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
                None,
                terminal_width,
                terminal_height,
            ),
            &capability_profile,
            &runtime_presentation_intents,
            Some(&mut terminal_broker),
        )
        .map_err(|error| format!("initial PTY redraw failed: {error}"))?;

    let initial_active_pane = initial_render
        .rendered_workspace
        .panes
        .iter()
        .find(|pane| pane.window_id == initial_render.rendered_workspace.active_window_id)
        .ok_or_else(|| "initial PTY draw missing active pane".to_string())?;
    eprintln!(
        "[main][pty-smoke] initial draw: panes={}, active_window_id={}, cursor=({},{}), status={:?}, message={:?}",
        initial_render.rendered_workspace.panes.len(),
        initial_render.rendered_workspace.active_window_id,
        initial_active_pane.cursor_row,
        initial_active_pane.cursor_col,
        initial_render
            .rendered_workspace
            .panes
            .iter()
            .find(|pane| pane.window_id == initial_render.rendered_workspace.active_window_id)
            .map(|pane| format!("{} | {}", pane.file_name, pane.mode_label)),
        initial_render.rendered_workspace.visible_message_text(),
    );

    std::thread::sleep(std::time::Duration::from_millis(25));

    outcome
        .core_bridge
        .apply_ex_command(":split")
        .map_err(|error| format!("PTY split command failed: {error:?}"))?;

    let split_render = render_coordinator
        .render_workspace_result(
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
                None,
                terminal_width,
                terminal_height,
            ),
            &capability_profile,
            &runtime_presentation_intents,
            Some(&mut terminal_broker),
        )
        .map_err(|error| format!("split PTY redraw failed: {error}"))?;

    let split_active_pane = split_render
        .rendered_workspace
        .panes
        .iter()
        .find(|pane| pane.window_id == split_render.rendered_workspace.active_window_id)
        .ok_or_else(|| "split PTY draw missing active pane".to_string())?;
    let split_status = split_render
        .rendered_workspace
        .panes
        .iter()
        .find(|pane| pane.window_id == split_render.rendered_workspace.active_window_id)
        .map(|pane| format!("{} | {}", pane.file_name, pane.mode_label));
    eprintln!(
        "[main][pty-smoke] split draw: panes={}, active_window_id={}, cursor=({},{}), status={:?}, message={:?}",
        split_render.rendered_workspace.panes.len(),
        split_render.rendered_workspace.active_window_id,
        split_active_pane.cursor_row,
        split_active_pane.cursor_col,
        split_status,
        split_render.rendered_workspace.visible_message_text(),
    );

    std::thread::sleep(std::time::Duration::from_millis(25));

    let resized_width = 72;
    let resized_height = 18;
    terminal_broker.record_resize(TerminalSize {
        columns: resized_width,
        rows: resized_height,
    });
    let resize_render = render_coordinator
        .render_workspace_result(
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
                None,
                resized_width,
                resized_height,
            ),
            &capability_profile,
            &runtime_presentation_intents,
            Some(&mut terminal_broker),
        )
        .map_err(|error| format!("resize PTY redraw failed: {error}"))?;
    eprintln!(
        "[main][pty-smoke] resize draw: panes={}, active_window_id={}, terminal=({},{}), latest_size={:?}, message={:?}",
        resize_render.rendered_workspace.panes.len(),
        resize_render.rendered_workspace.active_window_id,
        resized_width,
        resized_height,
        terminal_broker.latest_size(),
        resize_render.rendered_workspace.visible_message_text(),
    );

    let rollback_render = render_coordinator
        .render_workspace_result(
            Err(WorkspaceRedrawError::Projection(
                WorkspaceProjectionError::ActiveWindowMissing,
            )),
            &capability_profile,
            &runtime_presentation_intents,
            Some(&mut terminal_broker),
        )
        .map_err(|error| format!("rollback PTY redraw failed: {error}"))?;

    let rollback_active_pane = rollback_render
        .rendered_workspace
        .panes
        .iter()
        .find(|pane| pane.window_id == rollback_render.rendered_workspace.active_window_id);
    eprintln!(
        "[main][pty-smoke] rollback draw: panes={}, active_window_id={}, cursor=({}, {}), message={:?}",
        rollback_render.rendered_workspace.panes.len(),
        rollback_render.rendered_workspace.active_window_id,
        rollback_active_pane
            .map(|pane| pane.cursor_row)
            .unwrap_or_default(),
        rollback_active_pane
            .map(|pane| pane.cursor_col)
            .unwrap_or_default(),
        rollback_render.rendered_workspace.visible_message_text(),
    );

    outcome
        .core_bridge
        .apply_ex_command(":write")
        .map_err(|error| format!("PTY save command failed: {error:?}"))?;
    let mut save_redraw = false;
    consume_core_outcomes_from_core(
        &mut outcome.core_bridge,
        &mut outcome_accumulator,
        &mut save_redraw,
    );
    let save_shutdown = process_pending_host_actions_with_runtime(
        &mut outcome,
        &mut outcome_accumulator,
        &mut session_state,
        &mut transient_msg,
        &mut system_warning,
        &mut host_action_runtime,
        None,
        &mut save_redraw,
        &mut runtime_presentation_intents,
    )
    .await;
    if save_shutdown.is_some() {
        return Err(format!(
            "PTY save should not request shutdown, got: {:?}",
            save_shutdown
        ));
    }
    let saved_path = session_state
        .target_path()
        .cloned()
        .ok_or_else(|| "PTY save missing target path".to_string())?;
    let saved_contents = std::fs::read_to_string(&saved_path)
        .map_err(|error| format!("failed to read saved PTY target: {error}"))?;
    eprintln!(
        "[main][pty-smoke] save result: need_redraw={}, transient={:?}, bytes={}",
        save_redraw,
        transient_msg,
        saved_contents.len(),
    );

    outcome
        .core_bridge
        .apply_ex_command(":quit")
        .map_err(|error| format!("PTY quit command failed: {error:?}"))?;
    let mut quit_redraw = false;
    consume_core_outcomes_from_core(
        &mut outcome.core_bridge,
        &mut outcome_accumulator,
        &mut quit_redraw,
    );
    let quit_reason = process_pending_host_actions_with_runtime(
        &mut outcome,
        &mut outcome_accumulator,
        &mut session_state,
        &mut transient_msg,
        &mut system_warning,
        &mut host_action_runtime,
        None,
        &mut quit_redraw,
        &mut runtime_presentation_intents,
    )
    .await
    .ok_or_else(|| "PTY quit should produce a shutdown reason".to_string())?;
    if quit_reason != ShutdownReason::UserQuit {
        return Err(format!(
            "PTY quit returned unexpected shutdown reason: {:?}",
            quit_reason
        ));
    }
    eprintln!("[main][pty-smoke] quit reason: {:?}", quit_reason);

    outcome
        .core_bridge
        .apply_ex_command(":quit!")
        .map_err(|error| format!("PTY force quit command failed: {error:?}"))?;
    let mut force_quit_redraw = false;
    consume_core_outcomes_from_core(
        &mut outcome.core_bridge,
        &mut outcome_accumulator,
        &mut force_quit_redraw,
    );
    let force_quit_reason = process_pending_host_actions_with_runtime(
        &mut outcome,
        &mut outcome_accumulator,
        &mut session_state,
        &mut transient_msg,
        &mut system_warning,
        &mut host_action_runtime,
        None,
        &mut force_quit_redraw,
        &mut runtime_presentation_intents,
    )
    .await
    .ok_or_else(|| "PTY force quit should produce a shutdown reason".to_string())?;
    if force_quit_reason != ShutdownReason::UserForceQuit {
        return Err(format!(
            "PTY force quit returned unexpected shutdown reason: {:?}",
            force_quit_reason
        ));
    }
    eprintln!(
        "[main][pty-smoke] force quit reason: {:?}",
        force_quit_reason
    );

    drop(render_coordinator);
    drop(outcome);
    terminal_broker
        .shutdown()
        .await
        .map_err(|error| format!("PTY smoke terminal restore failed: {error}"))?;

    eprintln!("[main][pty-smoke] completed successfully");
    Ok(())
}

async fn process_pending_host_actions_with_runtime(
    outcome: &mut saya::bootstrap::BootstrapOutcome,
    outcome_accumulator: &mut MainOutcomeAccumulator,
    session_state: &mut saya::editor_session::EditorSessionState,
    transient_msg: &mut Option<String>,
    system_warning: &mut Option<String>,
    host_action_runtime: &mut HostActionRuntime,
    mut runtime_session: Option<&mut RuntimeSessionOwner>,
    need_redraw: &mut bool,
    runtime_presentation_intents: &mut Vec<RuntimePresentationIntent>,
) -> Option<ShutdownReason> {
    let mut shutdown_reason = None;
    loop {
        if let Err(error) = host_action_runtime.drain_job_events(&mut outcome.core_bridge) {
            log::debug!("[main] failed to drain job events: {:?}", error);
        }
        consume_core_outcomes_from_core(&mut outcome.core_bridge, outcome_accumulator, need_redraw);

        let current_revision = outcome.core_bridge.snapshot().revision;
        let directives = std::mem::take(&mut outcome_accumulator.host_directives);
        if directives.is_empty() {
            break;
        }

        for directive in prioritize_save_family_host_directives(directives, current_revision) {
            match directive {
                NormalizedHostDirective::Write { path, .. } => {
                    if let Some(reason) = handle_write_host_action_with_runtime(
                        outcome,
                        session_state,
                        Some(path.as_str()),
                        transient_msg,
                        runtime_session.as_deref_mut(),
                        need_redraw,
                        runtime_presentation_intents,
                    )
                    .await
                    {
                        merge_shutdown_reason(&mut shutdown_reason, Some(reason));
                    }
                }
                NormalizedHostDirective::Quit { force, .. } => {
                    let decision = session_state.evaluate_quit(force);
                    if let Some(reason) =
                        shutdown_reason_from_quit_decision(decision, force, system_warning)
                    {
                        merge_shutdown_reason(&mut shutdown_reason, Some(reason));
                    }
                }
                NormalizedHostDirective::VfsRequest { request, trace } => {
                    log::debug!(
                        "[main] processing normalized VFS directive: sequence={}, request={:?}",
                        trace.sequence,
                        request
                    );
                    if let Err(error) =
                        host_action_runtime.handle_vfs_request(&mut outcome.core_bridge, request)
                    {
                        log::debug!("[main] VFS directive failed: {:?}", error);
                    }
                }
                NormalizedHostDirective::JobStart { request, trace } => {
                    log::debug!(
                        "[main] processing normalized job start directive: sequence={}, job_id={}, argv={:?}",
                        trace.sequence,
                        request.job_id,
                        request.argv
                    );
                    if let Err(error) =
                        host_action_runtime.start_job(&mut outcome.core_bridge, request)
                    {
                        log::debug!("[main] job start directive failed: {:?}", error);
                    }
                }
                NormalizedHostDirective::JobWrite { vfd, data, trace } => {
                    log::debug!(
                        "[main] processing normalized job write directive: sequence={}, vfd={}, bytes={}",
                        trace.sequence,
                        vfd,
                        data.len()
                    );
                    host_action_runtime.write_job(vfd, data);
                }
                NormalizedHostDirective::JobStop { job_id, trace } => {
                    log::debug!(
                        "[main] processing normalized job stop directive: sequence={}, job_id={}",
                        trace.sequence,
                        job_id
                    );
                    if let Err(error) =
                        host_action_runtime.stop_job(&mut outcome.core_bridge, job_id)
                    {
                        log::debug!("[main] job stop directive failed: {:?}", error);
                    }
                }
            }
        }
    }

    shutdown_reason
}

fn prioritize_save_family_host_directives(
    directives: Vec<NormalizedHostDirective>,
    current_revision: u64,
) -> Vec<NormalizedHostDirective> {
    let mut writes = Vec::new();
    let mut quits = Vec::new();
    let mut other_directives = Vec::new();

    for directive in directives {
        match directive {
            NormalizedHostDirective::Write {
                issued_after_revision,
                ..
            } if issued_after_revision == current_revision => {
                writes.push(directive);
            }
            NormalizedHostDirective::Quit {
                issued_after_revision,
                ..
            } if issued_after_revision == current_revision => {
                quits.push(directive);
            }
            NormalizedHostDirective::Write {
                issued_after_revision,
                ..
            } => {
                log::debug!(
                    "[main] skipping stale write host action: current_revision={}, issued_after_revision={}",
                    current_revision,
                    issued_after_revision
                );
            }
            NormalizedHostDirective::Quit {
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
                    "[main] preserving unsupported normalized host directive during save-family coordination: {:?}",
                    other
                );
                other_directives.push(other);
            }
        }
    }

    writes.extend(quits);
    writes.extend(other_directives);
    writes
}

fn process_pending_host_actions_without_runtime(
    outcome: &mut saya::bootstrap::BootstrapOutcome,
    outcome_accumulator: &mut MainOutcomeAccumulator,
    session_state: &mut saya::editor_session::EditorSessionState,
    transient_msg: &mut Option<String>,
    system_warning: &mut Option<String>,
    host_action_runtime: &mut HostActionRuntime,
) -> Option<ShutdownReason> {
    loop {
        if let Err(error) = host_action_runtime.drain_job_events(&mut outcome.core_bridge) {
            log::debug!(
                "[main] failed to drain job events without runtime: {:?}",
                error
            );
        }
        let mut need_redraw = false;
        consume_core_outcomes_from_core(
            &mut outcome.core_bridge,
            outcome_accumulator,
            &mut need_redraw,
        );

        let current_revision = outcome.core_bridge.snapshot().revision;
        let directives = std::mem::take(&mut outcome_accumulator.host_directives);
        if directives.is_empty() {
            break;
        }

        for directive in prioritize_save_family_host_directives(directives, current_revision) {
            match directive {
                NormalizedHostDirective::Write { path, .. } => {
                    let snapshot = outcome.core_bridge.snapshot();
                    let save_outcome = save_snapshot_result_with_path_override(
                        &snapshot.text,
                        session_state,
                        Some(path.as_str()),
                    );
                    *transient_msg = save_outcome.transient_message;
                }
                NormalizedHostDirective::Quit { force, .. } => {
                    let decision = session_state.evaluate_quit(force);
                    if let Some(reason) =
                        shutdown_reason_from_quit_decision(decision, force, system_warning)
                    {
                        return Some(reason);
                    }
                }
                NormalizedHostDirective::VfsRequest { request, trace } => {
                    log::debug!(
                        "[main] processing normalized VFS directive without runtime: sequence={}, request={:?}",
                        trace.sequence,
                        request
                    );
                    if let Err(error) =
                        host_action_runtime.handle_vfs_request(&mut outcome.core_bridge, request)
                    {
                        log::debug!("[main] VFS directive failed without runtime: {:?}", error);
                    }
                }
                NormalizedHostDirective::JobStart { request, trace } => {
                    log::debug!(
                        "[main] processing normalized job start directive without runtime: sequence={}, job_id={}, argv={:?}",
                        trace.sequence,
                        request.job_id,
                        request.argv
                    );
                    if let Err(error) =
                        host_action_runtime.start_job(&mut outcome.core_bridge, request)
                    {
                        log::debug!(
                            "[main] job start directive failed without runtime: {:?}",
                            error
                        );
                    }
                }
                NormalizedHostDirective::JobWrite { vfd, data, trace } => {
                    log::debug!(
                        "[main] processing normalized job write directive without runtime: sequence={}, vfd={}, bytes={}",
                        trace.sequence,
                        vfd,
                        data.len()
                    );
                    host_action_runtime.write_job(vfd, data);
                }
                NormalizedHostDirective::JobStop { job_id, trace } => {
                    log::debug!(
                        "[main] processing normalized job stop directive without runtime: sequence={}, job_id={}",
                        trace.sequence,
                        job_id
                    );
                    if let Err(error) =
                        host_action_runtime.stop_job(&mut outcome.core_bridge, job_id)
                    {
                        log::debug!(
                            "[main] job stop directive failed without runtime: {:?}",
                            error
                        );
                    }
                }
            }
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
    runtime_presentation_intents: &mut Vec<RuntimePresentationIntent>,
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
            runtime_presentation_intents,
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
    let mut host_action_runtime = HostActionRuntime::default();
    let folded = fold_normalized_outcomes(
        outcome.core_bridge.take_normalized_outcomes(),
        ApplicationOutcomeState::default(),
    );
    if let Some(message) = folded.effects.notification.latest_user_visible_message {
        log::debug!(
            "[main] runtime host command consumed core message effect: {:?}",
            message
        );
        effect.transient_message = Some(message.content);
    }
    if let Some(redraw) = folded.effects.structural.redraw {
        log::debug!(
            "[main] runtime host command consumed structural redraw effect: full={}, clear_before_draw={}, required_by_structure_change={}",
            redraw.full,
            redraw.clear_before_draw,
            redraw.required_by_structure_change
        );
    }
    let current_revision = outcome.core_bridge.snapshot().revision;
    for directive in
        prioritize_save_family_host_directives(folded.effects.host_directives, current_revision)
    {
        match directive {
            NormalizedHostDirective::Write { path, .. } => {
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
            NormalizedHostDirective::Quit { force, .. } => {
                let decision = session_state.evaluate_quit(force);
                merge_runtime_shutdown_intent(
                    &mut effect.shutdown_intent,
                    runtime_shutdown_intent_from_quit_decision(force, decision),
                );
            }
            NormalizedHostDirective::VfsRequest { request, trace } => {
                log::debug!(
                    "[main] runtime host command processing normalized VFS directive: sequence={}, request={:?}",
                    trace.sequence,
                    request
                );
                if let Err(error) =
                    host_action_runtime.handle_vfs_request(&mut outcome.core_bridge, request)
                {
                    log::debug!(
                        "[main] runtime host command VFS directive failed: {:?}",
                        error
                    );
                }
            }
            NormalizedHostDirective::JobStart { request, trace } => {
                log::debug!(
                    "[main] runtime host command processing job start directive: sequence={}, job_id={}, argv={:?}",
                    trace.sequence,
                    request.job_id,
                    request.argv
                );
                if let Err(error) = host_action_runtime.start_job(&mut outcome.core_bridge, request)
                {
                    log::debug!("[main] runtime host command job start failed: {:?}", error);
                }
            }
            NormalizedHostDirective::JobWrite { vfd, data, trace } => {
                log::debug!(
                    "[main] runtime host command processing job write directive: sequence={}, vfd={}, bytes={}",
                    trace.sequence,
                    vfd,
                    data.len()
                );
                host_action_runtime.write_job(vfd, data);
            }
            NormalizedHostDirective::JobStop { job_id, trace } => {
                log::debug!(
                    "[main] runtime host command processing job stop directive: sequence={}, job_id={}",
                    trace.sequence,
                    job_id
                );
                if let Err(error) = host_action_runtime.stop_job(&mut outcome.core_bridge, job_id) {
                    log::debug!("[main] runtime host command job stop failed: {:?}", error);
                }
            }
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

fn consume_core_outcomes_from_core(
    core_bridge: &mut saya::core_bridge::CoreBridge,
    accumulator: &mut MainOutcomeAccumulator,
    need_redraw: &mut bool,
) {
    let batch = core_bridge.take_normalized_outcomes();
    if batch.is_empty() {
        return;
    }

    consume_normalized_batch(batch, accumulator, need_redraw);
}

fn consume_normalized_batch(
    batch: NormalizedOutcomeBatch,
    accumulator: &mut MainOutcomeAccumulator,
    need_redraw: &mut bool,
) {
    let current = std::mem::take(&mut accumulator.state);
    let folded = fold_normalized_outcomes(batch, current);
    let projection_frame = accumulator
        .projection
        .apply_seam(folded.downstream_consume_seam());
    let effects = folded.effects;
    accumulator.state = folded.state;
    accumulator.last_projection_frame = Some(projection_frame.clone());
    let structural_refresh = StructuralRefresh::from_folded_effects(&effects.structural);
    if structural_refresh.redraw_plan.requested {
        log::debug!(
            "[main] deriving redraw scheduling hint from structural RedrawPlan: full={}, clear_before_draw={}, source={:?}, coalesced_count={}",
            structural_refresh.redraw_plan.full,
            structural_refresh.redraw_plan.clear_before_draw,
            structural_refresh.redraw_plan.source,
            structural_refresh.redraw_plan.coalesced_count
        );
        *need_redraw = true;
    }
    accumulator.last_structural_refresh = Some(structural_refresh);
    apply_core_dispatch_effects(effects, &projection_frame, accumulator, need_redraw);
}

fn apply_core_dispatch_effects(
    effects: ApplicationDispatchEffects,
    projection_frame: &ProjectionFrame,
    accumulator: &mut MainOutcomeAccumulator,
    need_redraw: &mut bool,
) {
    if let Some(message) = effects.notification.latest_user_visible_message {
        log::debug!(
            "[main] replacing core message from normalized notification effect: {:?}",
            message
        );
    }
    if let Some(message) = effects.notification.latest_non_user_message {
        log::debug!(
            "[main] observed non-user core message from normalized notification effect: {:?}",
            message
        );
    }
    if effects.notification.bell_count > 0 {
        log::debug!(
            "[main] observed bell notification effect: count={}",
            effects.notification.bell_count
        );
    }
    if let Some(prompt) = effects.prompt.pager_prompt {
        log::debug!("[main] observed pager prompt effect: kind={:?}", prompt);
    }
    if let Some(transition) = effects.prompt.input_transition {
        log::debug!(
            "[main] observed input prompt transition effect: {:?}",
            transition
        );
    }
    if let Some(prompt) = projection_frame.input_prompt.as_ref() {
        log::debug!(
            "[main] retained prompt projection is active: correlation_id={}, input_kind={:?}, buffer_len={}, status={:?}",
            prompt.correlation_id,
            prompt.input_kind,
            prompt.input.len(),
            prompt.status
        );
    }
    if let Some(error) = projection_frame.response_error.as_ref() {
        log::debug!(
            "[main] prompt projection recorded response error: sequence={}, error={}",
            projection_frame.sequence,
            error
        );
    }
    if let Some(redraw) = effects.structural.redraw {
        log::debug!(
            "[main] applying structural redraw effect: full={}, clear_before_draw={}, required_by_structure_change={}",
            redraw.full,
            redraw.clear_before_draw,
            redraw.required_by_structure_change
        );
        *need_redraw = true;
    }
    if !effects.structural.invalidate_buffers.is_empty()
        || !effects.structural.invalidate_windows.is_empty()
        || effects.structural.layout_dirty
    {
        log::debug!(
            "[main] observed structural invalidation effect: buffers={:?}, windows={:?}, layout_dirty={}",
            effects.structural.invalidate_buffers,
            effects.structural.invalidate_windows,
            effects.structural.layout_dirty
        );
    }
    for diagnostic in &effects.diagnostics {
        log::debug!(
            "[main] observed normalized diagnostic effect: {:?}",
            diagnostic
        );
    }
    accumulator.host_directives.extend(effects.host_directives);
}

fn dispatch_prompt_response_command(
    core_bridge: &mut saya::core_bridge::CoreBridge,
    accumulator: &mut MainOutcomeAccumulator,
    command: PromptResponseCommand,
    need_redraw: &mut bool,
) {
    log::debug!(
        "[main] routing prompt response through core bridge: correlation_id={}",
        command.correlation_id()
    );
    match core_bridge.respond_to_prompt(command) {
        Ok(batch) => {
            consume_normalized_batch(batch, accumulator, need_redraw);
        }
        Err(error) => {
            log::debug!("[main] prompt response rejected: {}", error);
            record_prompt_response_error(&mut accumulator.projection, error);
        }
    }
    *need_redraw = true;
}

#[cfg(test)]
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
    runtime_presentation_intents: &mut Vec<RuntimePresentationIntent>,
) -> Option<ShutdownReason> {
    let Some(runtime_session) = runtime_session else {
        return None;
    };
    let mut host_session = MainRuntimeHostSession::new(outcome, session_state);
    let payload = RuntimeEventMapper::buffer_open(host_session.current_buffer_snapshot());
    let dispatch_outcome = runtime_session.dispatch(payload, &mut host_session).await;
    apply_runtime_dispatch_outcome(
        transient_msg,
        need_redraw,
        runtime_presentation_intents,
        dispatch_outcome,
    )
}

async fn dispatch_buffer_write_post_with_runtime(
    runtime_session: Option<&mut RuntimeSessionOwner>,
    outcome: &mut saya::bootstrap::BootstrapOutcome,
    session_state: &mut saya::editor_session::EditorSessionState,
    transient_msg: &mut Option<String>,
    need_redraw: &mut bool,
    runtime_presentation_intents: &mut Vec<RuntimePresentationIntent>,
) -> Option<ShutdownReason> {
    let Some(runtime_session) = runtime_session else {
        return None;
    };
    let mut host_session = MainRuntimeHostSession::new(outcome, session_state);
    let payload = RuntimeEventMapper::buffer_write_post(host_session.current_buffer_snapshot());
    let dispatch_outcome = runtime_session.dispatch(payload, &mut host_session).await;
    apply_runtime_dispatch_outcome(
        transient_msg,
        need_redraw,
        runtime_presentation_intents,
        dispatch_outcome,
    )
}

fn apply_runtime_dispatch_outcome(
    transient_msg: &mut Option<String>,
    need_redraw: &mut bool,
    runtime_presentation_intents: &mut Vec<RuntimePresentationIntent>,
    dispatch_outcome: RuntimeDispatchOutcome,
) -> Option<ShutdownReason> {
    let RuntimeDispatchOutcome {
        transient_message,
        requires_redraw,
        shutdown_intent,
        presentation_intents,
    } = dispatch_outcome;
    if let Some(message) = transient_message {
        log::debug!(
            "[main] applying normalized runtime transient message to application state: {}",
            message
        );
        *transient_msg = Some(message);
    }
    if !presentation_intents.is_empty() || !runtime_presentation_intents.is_empty() {
        log::debug!(
            "[main] updating runtime presentation intents in application state: count={}",
            presentation_intents.len()
        );
    }
    *runtime_presentation_intents = presentation_intents;
    if requires_redraw {
        log::debug!("[main] applying normalized runtime redraw request to main loop");
        *need_redraw = true;
    }
    shutdown_intent.map(|intent| match intent {
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

#[cfg_attr(not(test), allow(dead_code))]
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

#[cfg_attr(not(test), allow(dead_code))]
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
                let failure_message = error.to_string();
                let rollback_model = WorkspaceScreenModel {
                    message_line: saya::presentation_effect::merge_presentation_message_line(
                        &last_successful.message_line,
                        [saya::core_notification_prompt::MessageLineCandidate::legacy(
                            saya::core_notification_prompt::MessageLineSource::RenderProjectionError,
                            failure_message.as_str(),
                        )],
                    ),
                    ..last_successful.clone()
                };
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
    projection_frame: Option<&ProjectionFrame>,
    mut structural_refresh: Option<&mut StructuralRefreshOutcome>,
    system_warning: Option<&str>,
    transient_msg: Option<&str>,
    terminal_width: u16,
    terminal_height: u16,
) -> Result<WorkspaceScreenModel, WorkspaceRedrawError> {
    let snapshot = outcome.core_bridge.snapshot();
    let visual_selection = outcome.core_bridge.current_visual_selection();
    let invalidated_windows = structural_refresh
        .as_deref()
        .map(|refresh| {
            refresh
                .invalidation
                .window_ids
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let viewport_summary = viewport_store
        .sync_from_windows_with_invalidations(&snapshot.windows, &invalidated_windows);
    if let Some(refresh) = structural_refresh.as_deref_mut() {
        *refresh = refresh
            .clone()
            .with_viewport_sync_summary(&viewport_summary);
    }
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
    let notification_prompt = projection_frame.map(ProjectionFrame::workspace_view);

    let projection_result = project_workspace(&WorkspaceProjectionInput {
        snapshot: &snapshot,
        session_state,
        visual_selection: visual_selection.as_ref(),
        search_states: &search_states,
        command_preview: command_preview.as_deref(),
        core_message: None,
        notification_prompt: notification_prompt.as_ref(),
        system_warning,
        transient_info: transient_msg,
        viewport_store,
        terminal_width,
        terminal_height,
    });

    match projection_result {
        Ok(workspace) => {
            let projection_summary = workspace.projection_summary();
            if let Some(refresh) = structural_refresh.as_deref_mut() {
                *refresh = refresh.clone().with_projection_summary(projection_summary);
                log::debug!(
                    "[main] structural refresh diagnostics ready before render coordination: redraw_requested={}, projection_status={:?}, viewport_status={:?}",
                    refresh.redraw_plan.requested,
                    refresh.projection.status,
                    refresh.viewport_status
                );
            }
            Ok(workspace)
        }
        Err(error) => {
            if let Some(refresh) = structural_refresh.as_deref() {
                let diagnostic =
                    refresh.projection_failure(error.to_string(), refresh.viewport_status);
                log::debug!(
                    "[main] structural projection failure diagnostic prepared before render coordination: {:?}",
                    diagnostic
                );
            }
            Err(WorkspaceRedrawError::from(error))
        }
    }
}

fn sync_core_screen_size(outcome: &mut saya::bootstrap::BootstrapOutcome) {
    if let Ok((cols, rows)) = crossterm::terminal::size() {
        outcome
            .core_bridge
            .set_screen_size(i32::from(rows), i32::from(cols));
    }
}

fn mouse_click_to_sgr_sequence(
    workspace_model: Option<&WorkspaceScreenModel>,
    column: u16,
    row: u16,
) -> Option<String> {
    let workspace_model = workspace_model?;
    let inside_editor_body = workspace_model.panes.iter().any(|pane| {
        let body_height = pane.rect.height.saturating_sub(1).max(1);
        let column_offset = column.saturating_sub(pane.rect.x);
        let row_offset = row.saturating_sub(pane.rect.y);
        column >= pane.rect.x
            && row >= pane.rect.y
            && column_offset < pane.rect.width
            && row_offset < body_height
    });

    if inside_editor_body {
        let sgr_column = column.saturating_add(1);
        let sgr_row = row.saturating_add(1);
        Some(format!("\x1b[<0;{sgr_column};{sgr_row}M"))
    } else {
        None
    }
}

fn pasted_text_to_dispatch_units(text: &str) -> Vec<String> {
    text.chars().map(|ch| ch.to_string()).collect()
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
        LaunchStartError::Policy(error) => {
            format!("TUI-only policy に違反する起動要求です: {error}")
        }
    }
}

fn format_tui_startup_context_error(error: TuiStartupContextError) -> String {
    match error {
        TuiStartupContextError::Launch(error) => format_launch_start_error(error),
        TuiStartupContextError::CapabilityProbe(error) => {
            format!("terminal capability probe failed during startup composition: {error}")
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

    fn main_test_workspace() -> WorkspaceScreenModel {
        WorkspaceScreenModel {
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
            message_line: saya::core_notification_prompt::resolve_workspace_message_line(Vec::<
                saya::core_notification_prompt::MessageLineCandidate,
            >::new(
            )),
            prompt_line: None,
            pager_prompt: None,
            suppressed_prompt_hints: vec![],
            bell: None,
            command_line: None,
        }
    }

    #[test]
    fn editor_area_mouse_click_builds_one_based_sgr_sequence() {
        let workspace = main_test_workspace();

        let sequence = mouse_click_to_sgr_sequence(Some(&workspace), 0, 0);

        assert_eq!(sequence.as_deref(), Some("\x1b[<0;1;1M"));
    }

    #[test]
    fn mouse_click_outside_editor_body_does_not_dispatch() {
        let workspace = main_test_workspace();

        let status_row = mouse_click_to_sgr_sequence(Some(&workspace), 0, 2);
        let command_row = mouse_click_to_sgr_sequence(Some(&workspace), 0, 3);
        let no_workspace = mouse_click_to_sgr_sequence(None, 0, 0);

        assert_eq!(status_row, None);
        assert_eq!(command_row, None);
        assert_eq!(no_workspace, None);
    }

    #[test]
    fn mouse_click_sgr_coordinates_saturate_at_u16_max() {
        let workspace = WorkspaceScreenModel {
            panes: vec![saya::screen_model::ScreenModel {
                rect: saya::screen_model::PaneRect {
                    x: u16::MAX,
                    y: u16::MAX,
                    width: 1,
                    height: 1,
                },
                ..main_test_workspace().panes.remove(0)
            }],
            ..main_test_workspace()
        };

        let sequence = mouse_click_to_sgr_sequence(Some(&workspace), u16::MAX, u16::MAX);

        assert_eq!(sequence.as_deref(), Some("\x1b[<0;65535;65535M"));
    }

    #[test]
    fn pasted_text_dispatch_units_preserve_character_order_without_reinterpretation() {
        let units = pasted_text_to_dispatch_units("ab\n\r\nあ\x1b");

        assert_eq!(
            units,
            vec!["a", "b", "\n", "\r", "\n", "あ", "\x1b"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        );
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

        let mut outcome_accumulator = MainOutcomeAccumulator::default();
        let mut transient_msg = None;
        let mut system_warning = None;
        let mut need_redraw = false;
        let mut host_action_runtime = HostActionRuntime::default();
        consume_core_outcomes_from_core(
            &mut outcome.core_bridge,
            &mut outcome_accumulator,
            &mut need_redraw,
        );
        assert!(
            matches!(
                outcome_accumulator.host_directives.as_slice(),
                [NormalizedHostDirective::Write { .. }]
            ),
            ":w 後に normalized write directive が 1 件発行されること: {:?}",
            outcome_accumulator.host_directives
        );
        let shutdown = process_pending_host_actions_without_runtime(
            &mut outcome,
            &mut outcome_accumulator,
            &mut session_state,
            &mut transient_msg,
            &mut system_warning,
            &mut host_action_runtime,
        );
        let expected_error = session_state
            .last_save_error()
            .expect("save failure should be recorded")
            .to_string();
        let expected_message = format!("Save failed: {}", expected_error);

        assert_eq!(shutdown, None);
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
        let mut runtime_presentation_intents = Vec::new();

        let shutdown_reason = apply_runtime_dispatch_outcome(
            &mut transient_msg,
            &mut need_redraw,
            &mut runtime_presentation_intents,
            RuntimeDispatchOutcome {
                transient_message: Some("Saved successfully".to_string()),
                requires_redraw: true,
                shutdown_intent: Some(RuntimeShutdownIntent::UserQuit),
                presentation_intents: Vec::new(),
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
        let trace = |sequence| saya::core_outcome::OutcomeTrace {
            sequence,
            origin: saya::core_outcome::OutcomeOrigin::TransactionHostAction,
            raw_kind: "test",
        };
        let directives = vec![
            NormalizedHostDirective::Quit {
                force: false,
                issued_after_revision: 9,
                trace: trace(1),
            },
            NormalizedHostDirective::Write {
                path: "stale.txt".to_string(),
                force: false,
                issued_after_revision: 8,
                trace: trace(2),
            },
            NormalizedHostDirective::Quit {
                force: false,
                issued_after_revision: 8,
                trace: trace(3),
            },
            NormalizedHostDirective::Write {
                path: "fresh.txt".to_string(),
                force: false,
                issued_after_revision: 9,
                trace: trace(4),
            },
        ];

        let prioritized = prioritize_save_family_host_directives(directives, 9);

        assert_eq!(
            prioritized,
            vec![
                NormalizedHostDirective::Write {
                    path: "fresh.txt".to_string(),
                    force: false,
                    issued_after_revision: 9,
                    trace: trace(4),
                },
                NormalizedHostDirective::Quit {
                    force: false,
                    issued_after_revision: 9,
                    trace: trace(1),
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
            message_line: saya::core_notification_prompt::resolve_workspace_message_line(Vec::<
                saya::core_notification_prompt::MessageLineCandidate,
            >::new(
            )),
            prompt_line: None,
            pager_prompt: None,
            suppressed_prompt_hints: vec![],
            bell: None,
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
            output.model.visible_message_text(),
            Some("workspace projection failed: active window could not be resolved")
        );
        assert_eq!(
            output.failure_message.as_deref(),
            output.model.visible_message_text()
        );
        assert_eq!(
            last_successful_workspace_model
                .as_ref()
                .expect("last successful model should be retained")
                .visible_message_text(),
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
    fn consume_core_outcomes_marks_need_redraw_when_bridge_has_pending_redraw() {
        let _lock = saya::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut bridge = saya::core_bridge::CoreBridge::new("hello\n").expect("core bridge");
        let mut accumulator = MainOutcomeAccumulator::default();
        let mut need_redraw = false;

        bridge
            .apply_ex_command(":redraw")
            .expect(":redraw should succeed");
        consume_core_outcomes_from_core(&mut bridge, &mut accumulator, &mut need_redraw);

        assert!(
            need_redraw,
            "pending redraw from core should mark need_redraw"
        );
        assert!(
            bridge.take_normalized_outcomes().is_empty(),
            "normalized outcomes should be drained after helper runs"
        );
    }

    #[test]
    fn consume_core_outcomes_tracks_active_prompt_in_projection_state() {
        let _lock = saya::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut bridge = saya::core_bridge::CoreBridge::new("hello\n").expect("core bridge");
        let mut accumulator = MainOutcomeAccumulator::default();
        let mut need_redraw = false;

        bridge
            .apply_ex_command(":input Name")
            .expect("input request should succeed");
        consume_core_outcomes_from_core(&mut bridge, &mut accumulator, &mut need_redraw);

        assert_eq!(
            accumulator
                .projection
                .prompt()
                .active_input()
                .map(|view| view.correlation_id),
            Some(1)
        );
        assert_eq!(
            accumulator
                .last_projection_frame
                .as_ref()
                .and_then(|frame| frame.input_prompt.as_ref())
                .map(|view| view.prompt.as_str()),
            Some("Name")
        );
    }

    #[test]
    fn prompt_response_success_is_routed_through_bridge_and_closes_only_after_folded_batch() {
        let _lock = saya::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut bridge = saya::core_bridge::CoreBridge::new("hello\n").expect("core bridge");
        let mut accumulator = MainOutcomeAccumulator::default();
        let mut need_redraw = false;

        bridge
            .apply_ex_command(":input Name")
            .expect("input request should succeed");
        consume_core_outcomes_from_core(&mut bridge, &mut accumulator, &mut need_redraw);
        assert_eq!(
            accumulator
                .projection
                .prompt()
                .active_input()
                .map(|view| view.correlation_id),
            Some(1)
        );

        let action = saya::core_notification_prompt::handle_prompt_key(
            &mut accumulator.projection,
            &KeyInput::Enter,
        );
        let command = match action {
            saya::core_notification_prompt::PromptInputAction::Submit(command) => command,
            other => panic!("expected submit action, got {other:?}"),
        };

        dispatch_prompt_response_command(&mut bridge, &mut accumulator, command, &mut need_redraw);

        assert!(need_redraw);
        assert!(accumulator.projection.prompt().active_input().is_none());
        assert_eq!(
            accumulator
                .projection
                .prompt()
                .last_transition()
                .map(|transition| transition.kind),
            Some(saya::core_notification_prompt::PromptTransitionKind::Submitted)
        );
    }

    #[test]
    fn prompt_response_end_to_end_preserves_typed_input_value() {
        let _lock = saya::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut bridge = saya::core_bridge::CoreBridge::new("hello\n").expect("core bridge");
        let mut accumulator = MainOutcomeAccumulator::default();
        let mut need_redraw = false;

        bridge
            .apply_ex_command(":input Name")
            .expect("input request should succeed");
        consume_core_outcomes_from_core(&mut bridge, &mut accumulator, &mut need_redraw);

        assert!(matches!(
            saya::core_notification_prompt::handle_prompt_key(
                &mut accumulator.projection,
                &KeyInput::Char('a'),
            ),
            saya::core_notification_prompt::PromptInputAction::Consumed
        ));
        assert!(matches!(
            saya::core_notification_prompt::handle_prompt_key(
                &mut accumulator.projection,
                &KeyInput::Char('b'),
            ),
            saya::core_notification_prompt::PromptInputAction::Consumed
        ));
        let action = saya::core_notification_prompt::handle_prompt_key(
            &mut accumulator.projection,
            &KeyInput::Enter,
        );
        let command = match action {
            saya::core_notification_prompt::PromptInputAction::Submit(command) => command,
            other => panic!("expected submit action, got {other:?}"),
        };

        dispatch_prompt_response_command(&mut bridge, &mut accumulator, command, &mut need_redraw);

        assert!(accumulator.projection.prompt().active_input().is_none());
        assert_eq!(
            accumulator
                .projection
                .prompt()
                .last_transition()
                .map(|transition| (transition.kind, transition.input_len)),
            Some((
                saya::core_notification_prompt::PromptTransitionKind::Submitted,
                2
            ))
        );
    }

    #[test]
    fn structural_redraw_does_not_close_active_prompt() {
        let _lock = saya::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut bridge = saya::core_bridge::CoreBridge::new("hello\n").expect("core bridge");
        let mut accumulator = MainOutcomeAccumulator::default();
        let mut need_redraw = false;

        bridge
            .apply_ex_command(":input Name")
            .expect("input request should succeed");
        consume_core_outcomes_from_core(&mut bridge, &mut accumulator, &mut need_redraw);
        assert_eq!(
            accumulator
                .projection
                .prompt()
                .active_input()
                .map(|view| view.correlation_id),
            Some(1)
        );

        need_redraw = false;
        bridge
            .apply_ex_command(":redraw")
            .expect(":redraw should succeed");
        consume_core_outcomes_from_core(&mut bridge, &mut accumulator, &mut need_redraw);

        assert!(
            need_redraw,
            "structural redraw should still request a redraw"
        );
        assert_eq!(
            accumulator
                .projection
                .prompt()
                .active_input()
                .map(|view| view.correlation_id),
            Some(1),
            "redraw-only dispatch must not close the active prompt"
        );
    }

    #[test]
    fn prompt_response_error_restores_active_prompt_and_preserves_buffer() {
        let _lock = saya::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut bridge = saya::core_bridge::CoreBridge::new("hello\n").expect("core bridge");
        let mut accumulator = MainOutcomeAccumulator::default();
        let mut need_redraw = false;

        bridge
            .apply_ex_command(":input Name")
            .expect("input request should succeed");
        consume_core_outcomes_from_core(&mut bridge, &mut accumulator, &mut need_redraw);
        assert!(matches!(
            saya::core_notification_prompt::handle_prompt_key(
                &mut accumulator.projection,
                &KeyInput::Char('x'),
            ),
            saya::core_notification_prompt::PromptInputAction::Consumed
        ));
        let action = saya::core_notification_prompt::handle_prompt_key(
            &mut accumulator.projection,
            &KeyInput::Enter,
        );
        let command = match action {
            saya::core_notification_prompt::PromptInputAction::Submit(command) => command,
            other => panic!("expected submit action, got {other:?}"),
        };

        dispatch_prompt_response_command(
            &mut bridge,
            &mut accumulator,
            saya::core_prompt::PromptResponseCommand::Submit {
                correlation_id: command.correlation_id() + 1,
                value: "ignored".to_string(),
            },
            &mut need_redraw,
        );

        assert!(matches!(
            accumulator
                .projection
                .prompt()
                .active_input()
                .map(|view| view.status),
            Some(saya::core_notification_prompt::InputPromptStatus::Active)
        ));
        assert_eq!(
            accumulator
                .projection
                .prompt()
                .active_input()
                .map(|view| view.input.as_str()),
            Some("x")
        );
        assert!(
            accumulator
                .projection
                .prompt()
                .last_response_error()
                .is_some_and(|message| message.contains("expected=1"))
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
