#[cfg(test)]
use saya::app::bootstrap::StartupKeymapMode;
use saya::app::bootstrap::{StartupKeymapAction, bootstrap_warning_message};
use saya::app::cli::{StartupAction, parse_launch_request};
use saya::app::cli_output::{
    format_bootstrap_error, format_cli_error, format_tui_startup_context_error, render_help_text,
    render_version_text,
};
use saya::app::event_loop::{EventLoopCoordinator, LoopAction, ShutdownReason, UiEvent};
#[cfg(test)]
use saya::app::host_command::startup_registered_command_name_for_ex_command;
#[cfg(test)]
use saya::app::host_command::{MainHostCommand, parse_main_host_command};
use saya::app::outcome_consume::{
    MainOutcomeAccumulator, consume_core_outcomes_from_core, mark_structural_refresh_rendered,
};
use saya::app::runtime_dispatch::dispatch_command_line_key;
use saya::app::runtime_dispatch::dispatch_floating_ui_key;
use saya::app::runtime_dispatch::dispatch_selector_key_route;
#[cfg(test)]
use saya::app::runtime_dispatch::execute_runtime_host_command_with_floats;
#[cfg(test)]
use saya::app::runtime_dispatch::handle_selector_accept_action;
#[cfg(test)]
use saya::app::runtime_dispatch::shutdown_reason_from_quit_decision;
#[cfg(test)]
use saya::app::runtime_dispatch::{
    DirectoryOperationConfirmationKeyAction, SaveSnapshotOutcome,
    directory_operation_cancel_message, directory_operation_confirmation_key_action,
    execute_runtime_host_command, save_error_message, save_snapshot_result_with_path_override,
    startup_keymap_action_for_input, take_pending_directory_save_then_quit_shutdown,
};
use saya::app::runtime_dispatch::{
    LsifBridgeHandle, dispatch_buffer_changed_with_runtime, dispatch_buffer_open_with_runtime,
    execute_startup_keymap_registered_command,
    handle_directory_operation_confirmation_key_with_runtime, save_snapshot_result,
    startup_keymap_action_for_snapshot_input,
};
use saya::app::runtime_dispatch::{
    dispatch_completion_float_key, dispatch_floating_window_key, dispatch_resolved_intent_key,
    resolve_input_active_window_id,
};
use saya::app::runtime_dispatch::{
    dispatch_mouse_click, dispatch_mouse_wheel, dispatch_pasted_text,
};
use saya::app::runtime_dispatch::{
    dispatch_notification_prompt_key, dispatch_runtime_input_prompt_key,
};
#[cfg(test)]
use saya::app::runtime_dispatch::{mouse_click_to_sgr_sequence, pasted_text_to_dispatch_units};
use saya::app::runtime_dispatch::{
    process_pending_host_actions_with_runtime, process_pending_host_actions_without_runtime,
    sync_session_dirty_from_core,
};
#[cfg(test)]
use saya::app::session::SaveRequestError;
use saya::app::startup::{PreparedTuiStartup, prepare_tui_startup_context};
use saya::core::host_actions::HostActionRuntime;
use saya::features::completion::float::CompletionFloatManager;
#[cfg(test)]
use saya::features::completion::session::CompletionShowRequest;
use saya::features::dired::RuntimeInputPromptUiState;
#[cfg(test)]
use saya::features::dired::{
    apply_directory_buffer_operation_plan, execute_runtime_filer_operation,
};
use saya::features::lsp::float::LspDiagnosticStore;
use saya::features::search::refresh::WindowSearchRefreshStore;
#[cfg(test)]
use saya::features::selector::runtime::RuntimeSelectorControllerCommand;
use saya::input::command_line_editor::CommandLineEdit;
use saya::input::command_line_history::{
    load_histories_from_default_cache, save_histories_to_default_cache,
};
use saya::input::router::KeyInput;
#[cfg(test)]
use saya::input::router::NavigationKey;
use saya::presentation::floating_input::{FloatingWindowKeyHandling, handle_completion_float_key};
#[cfg(test)]
use saya::presentation::floating_input::{
    begin_command_line_from_focused_panel, handle_core_window_float_key,
    handle_floating_window_key, handle_mermaid_preview_key, handle_terminal_float_key,
    handle_terminal_panel_key,
};
#[cfg(test)]
use saya::presentation::floating_input::{
    focus_floating_window_from_mouse_click, focus_mermaid_preview_from_mouse_click,
    handle_mermaid_preview_mouse_wheel,
};
use saya::presentation::floating_window::{FloatingContentRef, FloatingWindowManager};
#[cfg(test)]
use saya::presentation::floating_window::{FloatingMouseOutcome, FloatingWindowId};
use saya::presentation::markdown::structure::{MarkdownDocumentMap, MarkdownMetadataCache};
use saya::presentation::overlay::asset_store::OverlayAssetStore;
use saya::presentation::overlay::effect::RuntimePresentationIntent;
use saya::presentation::overlay::optional_graphics::{
    OptionalGraphicsAdapter, OverlayTerminalWriter,
};
use saya::presentation::panel::PanelManager;
#[cfg(test)]
use saya::presentation::render::command_line_redraw::build_command_line_only_workspace;
use saya::presentation::render::command_line_redraw::{
    CommandLineOnlyRedraw, render_command_line_only_redraw_if_possible,
    sync_core_screen_size_if_changed,
};
use saya::presentation::render::coordinator::TuiRenderCoordinator;
#[cfg(test)]
use saya::presentation::render::redraw_trace::{
    RedrawTraceCounts, redraw_trace_counts as test_redraw_trace_counts,
    reset_redraw_trace_counts as reset_test_redraw_trace_counts,
};
use saya::presentation::render::renderer::{CrosstermBackendImpl, TuiRenderer};
#[cfg(test)]
use saya::presentation::render::workspace_output::structural_refresh_is_idle;
use saya::presentation::render::workspace_output::{
    WorkspaceRedrawError, build_workspace_render_output, effective_workspace_redraw_plan,
    terminal_display_invalidated_redraw_plan,
};
#[cfg(test)]
use saya::presentation::render::workspace_output::{
    apply_workspace_redraw_transaction, resolve_prompt_revision,
};
use saya::presentation::render::workspace_projection::trace_workspace_render_pipeline;
#[cfg(test)]
use saya::presentation::runtime_commands::{
    execute_runtime_panel_open, execute_runtime_window_close_float,
    execute_runtime_window_open_float,
};
use saya::presentation::screen_model::{
    ProjectionInput, WorkspaceProjectionError, WorkspaceScreenModel, project,
};
use saya::presentation::structural_refresh::RedrawPlan;
use saya::presentation::viewport::{ViewportSyncMode, WindowViewportStore};
#[cfg(test)]
use saya::runtime::integration::RuntimeDispatchOutcome;
#[cfg(test)]
use saya::runtime::integration::RuntimeHostSession;
use saya::runtime::integration::RuntimeSessionOwner;
#[cfg(test)]
use saya::runtime::integration::RuntimeShutdownIntent;
#[cfg(test)]
use saya::runtime::live::RuntimeCommandError;
#[cfg(test)]
use saya::runtime::live::{RuntimeFilerError, RuntimeFilerOperation};
#[cfg(test)]
use saya::runtime::live::{RuntimeFloatOpenRequest, RuntimePanelOpenRequest};
use saya::runtime::plugin::{PluginHost, render_plugin_report};
use saya::support::diagnostic_log::{
    configure_from_startup as configure_diagnostic_log_from_startup,
    init_from_env as init_diagnostic_log_from_env,
};
use saya::terminal::capability::TerminalCapabilityProbe;
use saya::terminal::float::TerminalFloatManager;
use saya::terminal::input_loop::CrosstermEventSource;
use saya::terminal::job_control::{
    start_job_control_signal_watcher, suspend_current_process_for_job_control,
    trace_job_control_diagnostic,
};
use saya::terminal::lifecycle::TerminalBackend;
use saya::terminal::lifecycle::TerminalSize;
use saya::terminal::lifecycle::current_terminal_size;
#[cfg(test)]
use vim_core_rs::CoreMessageEvent;
use vim_core_rs::CoreMode;

#[cfg(test)]
use std::sync::{Mutex, OnceLock};

#[cfg(test)]
use saya::app::outcome_consume::dispatch_prompt_response_command;
#[cfg(test)]
use saya::app::runtime_dispatch::{MainRuntimeHostSession, apply_runtime_dispatch_outcome};
#[cfg(test)]
use saya::app::runtime_dispatch::{
    merge_shutdown_reason, normal_quit_warning_message, prioritize_save_family_host_directives,
    refresh_directory_buffer_after_confirmed_save, save_snapshot_result_with_confirmation,
};
#[cfg(test)]
use saya::app::session::EditorSessionState;
#[cfg(test)]
use saya::app::session::QuitDecision;
#[cfg(test)]
use saya::core::outcome::NormalizedHostDirective;
#[cfg(test)]
use saya::presentation::floating_models::{
    append_active_mermaid_preview_float, refresh_buffer_backed_float_lines,
    refresh_terminal_float_lines,
};
#[cfg(test)]
use saya::presentation::render::workspace_output::wrap_message_for_pager;
#[cfg(test)]
use saya::presentation::render::workspace_projection::{
    collect_workspace_line_ranges, collect_workspace_markdown_document_maps,
    collect_workspace_tree_sitter_syntax,
};
#[cfg(test)]
use saya::presentation::structural_refresh::StructuralRefresh;
#[cfg(test)]
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug)]
struct MainInputPerfTrace {
    id: u64,
    key: String,
    command: Option<String>,
    started_at: std::time::Instant,
    command_elapsed_ms: Option<u128>,
}

#[tokio::main]
async fn main() {
    if let Err(error) = init_diagnostic_log_from_env() {
        eprintln!("[main] diagnostic log initialization failed: {error}");
    }

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
        StartupAction::Plugin(command) => {
            let host = PluginHost::default_from_env();
            let result = match command {
                saya::runtime::plugin::PluginCommand::Sync => {
                    match saya::app::bootstrap::collect_startup_registry_for_plugin_operation(
                        launch_request.config_source.clone(),
                    ) {
                        Ok(Some((registry, source_hash))) => {
                            host.sync_startup_plugin_declarations(&registry, source_hash)
                        }
                        Ok(None) => host.run_operation(*command),
                        Err(message) => {
                            Err(saya::runtime::plugin::PluginHostError::Operation { message })
                        }
                    }
                }
                _ => host.run_operation(*command),
            };
            match result {
                Ok(report) => {
                    println!("{}", render_plugin_report(&report));
                    std::process::exit(0);
                }
                Err(error) => {
                    eprintln!("[saya-plugin-manager] {error}");
                    std::process::exit(1);
                }
            }
        }
    }

    if std::env::var_os("SAYA_BINARY_SMOKE").is_some() {
        if let Err(error) = run_binary_smoke(launch_request).await {
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
    let mut viewport_sync_mode = ViewportSyncMode::Core;
    let mut search_refresh_store = WindowSearchRefreshStore::new();
    let mut markdown_metadata_cache = MarkdownMetadataCache::new();
    let mut command_line_prompt: Option<char> = None;
    let mut command_line_edit = CommandLineEdit::default();
    let mut runtime_input_prompt: Option<RuntimeInputPromptUiState> = None;
    let mut startup_keymap_pending_lhs: Option<String> = None;
    let mut command_line_histories = load_histories_from_default_cache();
    let mut runtime_presentation_intents: Vec<RuntimePresentationIntent> = Vec::new();
    let mut last_workspace_model: Option<WorkspaceScreenModel> = None;
    let mut floating_window_manager = FloatingWindowManager::default();
    let mut panel_manager = PanelManager::default();
    let mut completion_float_manager = CompletionFloatManager::default();
    let mut lsp_diagnostic_store = LspDiagnosticStore::default();
    let lsif_bridge = LsifBridgeHandle::default();
    let mut terminal_float_manager = TerminalFloatManager::default();
    let (mut coordinator, sender) = EventLoopCoordinator::new();
    terminal_float_manager.set_redraw_sender(sender.clone());
    let mermaid_redraw_sender = sender.clone();
    render_coordinator.set_mermaid_redraw_callback(move || {
        if let Err(error) = mermaid_redraw_sender.try_send(UiEvent::Redraw) {
            log::debug!(
                "[main][markdown_preview] failed to queue redraw after async Mermaid render: error={error}"
            );
        } else {
            log::debug!("[main][markdown_preview] queued redraw after async Mermaid render");
        }
    });
    let mut last_synced_terminal_size: Option<TerminalSize> = None;
    let mut terminal_display_redraw_plan: Option<RedrawPlan> = None;
    let mut workspace_projection_dirty = false;
    let mut next_input_perf_trace_id = 1_u64;
    let mut pending_input_perf_trace: Option<MainInputPerfTrace> = None;

    let mut startup_runtime_redraw = false;
    let startup_shutdown_reason = dispatch_buffer_open_with_runtime(
        runtime_session.as_mut(),
        &mut outcome,
        &mut session_state,
        &mut transient_msg,
        &mut startup_runtime_redraw,
        &mut runtime_presentation_intents,
        &mut panel_manager,
        &mut terminal_float_manager,
        Some(&lsif_bridge),
    )
    .await;

    // イベントループ初期化
    let job_control_watcher = match start_job_control_signal_watcher(sender.clone()) {
        Ok(watcher) => watcher,
        Err(error) => {
            log::debug!("[main] job-control signal watcher unavailable: {}", error);
            None
        }
    };

    log::debug!(
        "[main] terminal capability profile resolved before interactive input: {:?}",
        capability_profile
    );
    terminal_broker
        .start_interactive_input(sender.clone(), CrosstermEventSource)
        .expect("interactive input should start after the capability probe");

    // 初期描画
    let (terminal_width, terminal_height) = current_terminal_size();
    if sync_core_screen_size_if_changed(
        &mut outcome,
        &mut last_synced_terminal_size,
        TerminalSize {
            columns: terminal_width,
            rows: terminal_height,
        },
    ) {
        consume_core_outcomes_from_core(
            &mut outcome.core_bridge,
            &mut outcome_accumulator,
            &mut startup_runtime_redraw,
        );
    }
    let initial_render = build_workspace_render_output(
        &mut outcome,
        &mut session_state,
        &mut viewport_store,
        ViewportSyncMode::Core,
        &mut search_refresh_store,
        &mut markdown_metadata_cache,
        command_line_prompt,
        command_line_edit.buffer(),
        command_line_edit.cursor_byte_index(),
        outcome_accumulator.last_projection_frame.as_ref(),
        runtime_input_prompt.as_ref(),
        outcome_accumulator.last_structural_refresh.as_mut(),
        system_warning.as_deref(),
        transient_msg.as_deref(),
        terminal_width,
        terminal_height,
        Some(&mut floating_window_manager),
        Some(&mut panel_manager),
        Some(&mut terminal_float_manager),
        runtime_session
            .as_ref()
            .map(RuntimeSessionOwner::selector_tui_projection_sink),
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
            mark_structural_refresh_rendered(&mut outcome_accumulator);
            if std::env::var_os("SAYA_TRACE_RENDER").is_some() {
                trace_workspace_render_pipeline(
                    "initial",
                    &outcome.core_bridge.snapshot().text,
                    &render_output.rendered_workspace,
                );
            }
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
                        let input_snapshot = outcome.core_bridge.light_snapshot();
                        let input_perf_trace_id = next_input_perf_trace_id;
                        next_input_perf_trace_id = next_input_perf_trace_id.saturating_add(1);
                        pending_input_perf_trace = Some(MainInputPerfTrace {
                            id: input_perf_trace_id,
                            key: format!("{key:?}"),
                            command: None,
                            started_at: std::time::Instant::now(),
                            command_elapsed_ms: None,
                        });
                        log::info!(
                            "[main][input] key={:?}, mode={:?}, prompt={:?}, cursor=({},{}), revision={}, keymaps={}",
                            key,
                            input_snapshot.mode,
                            command_line_prompt,
                            input_snapshot.cursor_row,
                            input_snapshot.cursor_col,
                            input_snapshot.revision,
                            outcome.startup_registry.keymaps.len()
                        );
                        log::info!(
                            "[PERF][main][input_trace] start trace_id={} key={:?} mode={:?} revision={}",
                            input_perf_trace_id,
                            key,
                            input_snapshot.mode,
                            input_snapshot.revision
                        );

                        if let Some(reason) =
                            handle_directory_operation_confirmation_key_with_runtime(
                                &key,
                                &mut outcome,
                                &mut session_state,
                                &mut transient_msg,
                                &mut need_redraw,
                                runtime_session.as_mut(),
                                &mut runtime_presentation_intents,
                                Some(&lsif_bridge),
                            )
                            .await
                        {
                            handled = true;
                            if let Some(reason) = reason {
                                break 'main reason;
                            }
                        }

                        if !handled {
                            dispatch_runtime_input_prompt_key(
                                &key,
                                &mut runtime_input_prompt,
                                &mut outcome,
                                &mut session_state,
                                &mut runtime_session,
                                &mut transient_msg,
                                &mut need_redraw,
                                &mut runtime_presentation_intents,
                                &mut handled,
                            )
                            .await;
                        }

                        if !handled {
                            handled = dispatch_selector_key_route(
                                &key,
                                &mut runtime_session,
                                &mut outcome,
                                &mut session_state,
                                &mut transient_msg,
                                &mut need_redraw,
                                &mut runtime_presentation_intents,
                            )
                            .await;
                        }

                        if !handled {
                            if let Some(reason) = dispatch_notification_prompt_key(
                                &key,
                                &mut outcome,
                                &mut outcome_accumulator,
                                &mut session_state,
                                &mut transient_msg,
                                &mut system_warning,
                                &mut host_action_runtime,
                                &mut runtime_session,
                                &mut need_redraw,
                                &mut runtime_presentation_intents,
                                &lsif_bridge,
                                &mut handled,
                            )
                            .await
                            {
                                break 'main reason;
                            }
                        }

                        if !handled && let Some(prompt) = command_line_prompt {
                            handled = true;
                            if let Some(reason) = dispatch_command_line_key(
                                &key,
                                prompt,
                                &mut command_line_prompt,
                                &mut command_line_edit,
                                &mut command_line_histories,
                                &mut outcome,
                                &mut outcome_accumulator,
                                &mut session_state,
                                &mut transient_msg,
                                &mut system_warning,
                                &mut host_action_runtime,
                                &mut runtime_session,
                                &mut need_redraw,
                                &mut runtime_presentation_intents,
                                &lsif_bridge,
                                &mut floating_window_manager,
                                &mut completion_float_manager,
                                &mut lsp_diagnostic_store,
                                &mut terminal_float_manager,
                                &mut panel_manager,
                                &mut runtime_input_prompt,
                            )
                            .await
                            {
                                break 'main reason;
                            }
                        }

                        if !handled {
                            if let Some(reason) = dispatch_floating_ui_key(
                                &key,
                                &mut outcome,
                                &mut outcome_accumulator,
                                &mut session_state,
                                &mut transient_msg,
                                &mut system_warning,
                                &mut host_action_runtime,
                                &mut runtime_session,
                                &mut need_redraw,
                                &mut runtime_presentation_intents,
                                &lsif_bridge,
                                &mut floating_window_manager,
                                &mut terminal_float_manager,
                                &mut panel_manager,
                                &mut command_line_prompt,
                                &mut command_line_edit,
                                &mut command_line_histories,
                                &mut handled,
                                &mut workspace_projection_dirty,
                            )
                            .await
                            {
                                break 'main reason;
                            }
                        }

                        if !handled {
                            let active_window_id = resolve_input_active_window_id(
                                &input_snapshot,
                                last_workspace_model.as_ref(),
                                &mut outcome.core_bridge,
                            );
                            if let Some(reason) = dispatch_completion_float_key(
                                &key,
                                active_window_id,
                                &mut outcome,
                                &mut outcome_accumulator,
                                &mut session_state,
                                &mut transient_msg,
                                &mut system_warning,
                                &mut host_action_runtime,
                                &mut runtime_session,
                                &mut need_redraw,
                                &mut runtime_presentation_intents,
                                &lsif_bridge,
                                &mut floating_window_manager,
                                &mut completion_float_manager,
                                &mut lsp_diagnostic_store,
                                &mut terminal_float_manager,
                                &mut panel_manager,
                                &mut handled,
                                &mut workspace_projection_dirty,
                            )
                            .await
                            {
                                break 'main reason;
                            }
                        }

                        if !handled {
                            let active_window_id = resolve_input_active_window_id(
                                &input_snapshot,
                                last_workspace_model.as_ref(),
                                &mut outcome.core_bridge,
                            );
                            dispatch_floating_window_key(
                                &key,
                                active_window_id,
                                &mut floating_window_manager,
                                &mut handled,
                                &mut need_redraw,
                                &mut workspace_projection_dirty,
                            );
                        }

                        if !handled
                            && session_state.message_pager_active()
                            && session_state.handle_message_pager_key(&key)
                        {
                            handled = true;
                            need_redraw = true;
                            workspace_projection_dirty = true;
                            log::debug!(
                                "[main] message pager consumed input: key={:?}, offset={}, active={}, workspace_projection_dirty={}",
                                key,
                                session_state.message_scroll_offset(),
                                session_state.message_pager_active(),
                                workspace_projection_dirty
                            );
                        } else if !handled {
                            let pending_lhs_before = startup_keymap_pending_lhs.clone();
                            if let Some(action) = startup_keymap_action_for_snapshot_input(
                                &outcome.startup_registry.keymaps,
                                &outcome.core_bridge.light_snapshot(),
                                &key,
                                &mut startup_keymap_pending_lhs,
                            ) {
                                handled = true;
                                log::debug!(
                                    "[main] applying startup keymap before core dispatch: key={:?}, action={:?}",
                                    key,
                                    action
                                );
                                if outcome.core_bridge.pending_input_is_pending() {
                                    let _ = outcome.core_bridge.dispatch_key("\x1b");
                                    consume_core_outcomes_from_core(
                                        &mut outcome.core_bridge,
                                        &mut outcome_accumulator,
                                        &mut need_redraw,
                                    );
                                }
                                match action {
                                    StartupKeymapAction::Literal(rhs) => {
                                        let _ = outcome.core_bridge.dispatch_key(&rhs);
                                        consume_core_outcomes_from_core(
                                            &mut outcome.core_bridge,
                                            &mut outcome_accumulator,
                                            &mut need_redraw,
                                        );

                                        if let Some(reason) =
                                            process_pending_host_actions_with_runtime(
                                                &mut outcome,
                                                &mut outcome_accumulator,
                                                &mut session_state,
                                                &mut transient_msg,
                                                &mut system_warning,
                                                &mut host_action_runtime,
                                                runtime_session.as_mut(),
                                                &mut need_redraw,
                                                &mut runtime_presentation_intents,
                                                Some(&lsif_bridge),
                                            )
                                            .await
                                        {
                                            break 'main reason;
                                        }
                                        sync_session_dirty_from_core(
                                            &mut session_state,
                                            &outcome.core_bridge,
                                        );
                                    }
                                    StartupKeymapAction::RegisteredCommand(command_name) => {
                                        log::info!(
                                            "[main][keymap] executing registered command from keymap: key={:?}, command={}",
                                            key,
                                            command_name
                                        );
                                        if let Some(trace) = pending_input_perf_trace.as_mut() {
                                            trace.command = Some(command_name.clone());
                                            log::info!(
                                                "[PERF][main][input_trace] registered_command_start trace_id={} key={} command={} elapsed_ms={}",
                                                trace.id,
                                                trace.key,
                                                command_name,
                                                trace.started_at.elapsed().as_millis()
                                            );
                                        }
                                        let command_started_at = std::time::Instant::now();
                                        if let Some(reason) =
                                            execute_startup_keymap_registered_command(
                                                runtime_session.as_mut(),
                                                &command_name,
                                                &mut outcome,
                                                &mut session_state,
                                                &mut floating_window_manager,
                                                &mut completion_float_manager,
                                                &mut lsp_diagnostic_store,
                                                &mut terminal_float_manager,
                                                &mut panel_manager,
                                                Some(&mut runtime_input_prompt),
                                                &mut transient_msg,
                                                &mut need_redraw,
                                                &mut runtime_presentation_intents,
                                                Some(&lsif_bridge),
                                            )
                                            .await
                                        {
                                            break 'main reason;
                                        }
                                        if let Some(trace) = pending_input_perf_trace.as_mut() {
                                            let command_elapsed_ms =
                                                command_started_at.elapsed().as_millis();
                                            trace.command_elapsed_ms = Some(command_elapsed_ms);
                                            log::info!(
                                                "[PERF][main][input_trace] registered_command_done trace_id={} key={} command={} command_ms={} total_ms={}",
                                                trace.id,
                                                trace.key,
                                                command_name,
                                                command_elapsed_ms,
                                                trace.started_at.elapsed().as_millis()
                                            );
                                        }
                                    }
                                }
                                need_redraw = true;
                            } else if startup_keymap_pending_lhs.is_some()
                                && startup_keymap_pending_lhs != pending_lhs_before
                            {
                                handled = true;
                                log::debug!(
                                    "[main] startup keymap prefix pending: key={:?}, pending_lhs={:?}",
                                    key,
                                    startup_keymap_pending_lhs
                                );
                            }
                        }

                        if !handled
                            && (key == KeyInput::Char(':') || key == KeyInput::Char('/'))
                            && outcome.core_bridge.mode() == CoreMode::Normal
                        {
                            if let KeyInput::Char(c) = key {
                                command_line_prompt = Some(c);
                            }
                            command_line_edit.clear();
                            command_line_histories.reset_navigation();
                            handled = true;
                            need_redraw = true;
                        }

                        if !handled {
                            if let Some(reason) = dispatch_resolved_intent_key(
                                &key,
                                &mut outcome,
                                &mut outcome_accumulator,
                                &mut session_state,
                                &mut transient_msg,
                                &mut system_warning,
                                &mut host_action_runtime,
                                &mut runtime_session,
                                &mut need_redraw,
                                &mut runtime_presentation_intents,
                                &lsif_bridge,
                                &mut floating_window_manager,
                                &mut completion_float_manager,
                                &mut lsp_diagnostic_store,
                                &mut terminal_float_manager,
                                &mut panel_manager,
                                &mut startup_keymap_pending_lhs,
                                &mut viewport_sync_mode,
                                &mut workspace_projection_dirty,
                            )
                            .await
                            {
                                break 'main reason;
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
                    UiEvent::TerminalSuspendRequested => {
                        log::debug!("[main] terminal suspend event received in drain");
                        perform_job_control_suspend_cycle(
                            &mut terminal_broker,
                            &mut need_redraw,
                            &mut terminal_display_redraw_plan,
                        );
                    }
                    UiEvent::TerminalResumed { columns, rows } => {
                        log::debug!(
                            "[main] terminal resume event received in drain: columns={}, rows={}",
                            columns,
                            rows
                        );
                        if let Err(error) = terminal_broker.resume_after_job_control() {
                            log::debug!("[main] terminal resume reclaim failed: {error}");
                            transient_msg = Some(format!("Terminal resume failed: {error}"));
                        }
                        terminal_broker.record_resize(TerminalSize { columns, rows });
                        terminal_display_redraw_plan =
                            Some(terminal_display_invalidated_redraw_plan());
                        trace_job_control_diagnostic(format_args!(
                            "SIGCONT resume requested full terminal redraw: columns={}, rows={}",
                            columns, rows
                        ));
                        need_redraw = true;
                    }
                    UiEvent::MouseClick { column, row } => {
                        if let Some(reason) = dispatch_mouse_click(
                            column,
                            row,
                            &mut outcome,
                            &mut outcome_accumulator,
                            &mut session_state,
                            last_workspace_model.as_ref(),
                            &mut floating_window_manager,
                            &mut transient_msg,
                            &mut system_warning,
                            &mut host_action_runtime,
                            &mut runtime_session,
                            &mut need_redraw,
                            &mut runtime_presentation_intents,
                            &mut workspace_projection_dirty,
                            &lsif_bridge,
                        )
                        .await
                        {
                            break 'main reason;
                        }
                    }
                    UiEvent::MouseWheel {
                        column,
                        row,
                        delta_x,
                        delta_y,
                    } => {
                        dispatch_mouse_wheel(
                            column,
                            row,
                            delta_x,
                            delta_y,
                            &mut session_state,
                            last_workspace_model.as_ref(),
                            &mut need_redraw,
                            &mut workspace_projection_dirty,
                        );
                    }
                    UiEvent::PastedText(text) => {
                        if let Some(reason) = dispatch_pasted_text(
                            &text,
                            &mut outcome,
                            &mut outcome_accumulator,
                            &mut session_state,
                            &mut transient_msg,
                            &mut system_warning,
                            &mut host_action_runtime,
                            &mut runtime_session,
                            &mut need_redraw,
                            &mut runtime_presentation_intents,
                            &lsif_bridge,
                        )
                        .await
                        {
                            break 'main reason;
                        }
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
                    command_line_edit.buffer().len()
                );
                command_line_prompt = None;
                command_line_edit.clear();
                need_redraw = true;
            }

            if outcome_accumulator.suspend_requested {
                log::debug!("[main] processing core-requested job-control suspend");
                outcome_accumulator.suspend_requested = false;
                perform_job_control_suspend_cycle(
                    &mut terminal_broker,
                    &mut need_redraw,
                    &mut terminal_display_redraw_plan,
                );
            }

            render_workspace_if_needed(
                need_redraw,
                &mut outcome,
                &mut outcome_accumulator,
                &mut session_state,
                &mut render_coordinator,
                &mut terminal_broker,
                &mut last_workspace_model,
                &mut last_synced_terminal_size,
                &mut terminal_display_redraw_plan,
                &mut viewport_store,
                &mut viewport_sync_mode,
                &mut search_refresh_store,
                &mut markdown_metadata_cache,
                &mut workspace_projection_dirty,
                &mut transient_msg,
                &mut pending_input_perf_trace,
                &mut floating_window_manager,
                &mut panel_manager,
                &mut terminal_float_manager,
                command_line_prompt,
                &command_line_edit,
                &system_warning,
                &runtime_input_prompt,
                &runtime_session,
                &runtime_presentation_intents,
                &capability_profile,
            );
        }
    };

    log::debug!(
        "[main] beginning unified shutdown: reason={:?}",
        shutdown_reason
    );
    save_histories_to_default_cache(&command_line_histories);
    let mut shutdown_sequence = coordinator.begin_shutdown(shutdown_reason);
    shutdown_sequence.record_loop_stopped();

    log::debug!("[main] requesting terminal broker shutdown");
    terminal_broker.request_shutdown();
    drop(sender);
    drop(job_control_watcher);

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

#[cfg(test)]
fn viewport_sync_mode_for_input(key: &KeyInput) -> ViewportSyncMode {
    match key {
        KeyInput::Char('j')
        | KeyInput::Char('k')
        | KeyInput::Down
        | KeyInput::Up
        | KeyInput::ShiftedNav(NavigationKey::Down)
        | KeyInput::ShiftedNav(NavigationKey::Up)
        | KeyInput::CtrlNav(NavigationKey::Down)
        | KeyInput::CtrlNav(NavigationKey::Up) => ViewportSyncMode::SmoothLineMotion,
        _ => ViewportSyncMode::Core,
    }
}

fn emit_binary_smoke_state(label: &str, state: serde_json::Value) {
    eprintln!("[main][smoke][state] {label} {state}");
}

async fn run_binary_smoke(launch_request: saya::app::cli::LaunchRequest) -> Result<(), String> {
    if std::env::var_os("SAYA_COMPLETION_SMOKE").is_some() {
        return run_binary_completion_smoke(launch_request).await;
    }
    eprintln!("[main][smoke] preparing headless launch");
    let mut outcome =
        saya::app::bootstrap::prepare_launch(launch_request).map_err(format_bootstrap_error)?;
    configure_diagnostic_log_from_startup(
        outcome.startup_registry.log.log_file.as_deref(),
        outcome.startup_registry.log.log_level,
    )
    .map_err(|error| error.to_string())?;
    let mut session_state = outcome.editor_session_state();
    let startup_markdown_map = session_state
        .markdown_render()
        .then(|| MarkdownDocumentMap::parse(&outcome.initial_snapshot.text));
    let startup_model = project(
        &ProjectionInput::new(&outcome.initial_snapshot, &session_state, None)
            .with_markdown_document_map(startup_markdown_map.as_ref()),
    );
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
    emit_binary_smoke_state(
        "startup",
        serde_json::json!({
            "firstLine": startup_model.lines.first(),
            "lines": startup_model.lines,
            "messageLine": startup_model.message_line,
            "fileName": startup_model.file_name,
            "mode": startup_model.mode_label,
            "dirty": startup_model.dirty,
            "lineNumbers": session_state.line_numbers(),
            "numberWidth": session_state.number_width(),
        }),
    );

    if session_state.directory_buffer().is_some() {
        eprintln!("[main][smoke] directory buffer startup detected, quitting without edit");
        outcome
            .core_bridge
            .apply_ex_command(":q")
            .map_err(|error| format!("directory smoke :q failed: {:?}", error))?;
        consume_core_outcomes_from_core(
            &mut outcome.core_bridge,
            &mut outcome_accumulator,
            &mut need_redraw,
        );
        sync_session_dirty_from_core(&mut session_state, &outcome.core_bridge);
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
                "directory smoke quit did not complete: dirty={}, last_save_error={:?}",
                session_state.is_dirty(),
                session_state.last_save_error()
            )
        })?;
        if reason != ShutdownReason::UserQuit {
            return Err(format!(
                "directory smoke quit returned unexpected shutdown reason: {:?}",
                reason
            ));
        }
        eprintln!("[main][smoke] completed with shutdown reason: {:?}", reason);
        return Ok(());
    }

    if std::env::var_os("SAYA_BINARY_SMOKE_QUIT_WITHOUT_EDIT").is_some() {
        eprintln!("[main][smoke] quitting without edit or write as requested");
        outcome
            .core_bridge
            .apply_ex_command(":q")
            .map_err(|error| format!("quit-without-edit smoke :q failed: {:?}", error))?;
        consume_core_outcomes_from_core(
            &mut outcome.core_bridge,
            &mut outcome_accumulator,
            &mut need_redraw,
        );
        sync_session_dirty_from_core(&mut session_state, &outcome.core_bridge);
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
                "quit-without-edit smoke did not complete: dirty={}, last_save_error={:?}",
                session_state.is_dirty(),
                session_state.last_save_error()
            )
        })?;
        if reason != ShutdownReason::UserQuit {
            return Err(format!(
                "quit-without-edit smoke returned unexpected shutdown reason: {:?}",
                reason
            ));
        }
        eprintln!("[main][smoke] completed with shutdown reason: {:?}", reason);
        return Ok(());
    }

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
    sync_session_dirty_from_core(&mut session_state, &outcome.core_bridge);

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
    sync_session_dirty_from_core(&mut session_state, &outcome.core_bridge);

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

async fn run_binary_completion_smoke(
    launch_request: saya::app::cli::LaunchRequest,
) -> Result<(), String> {
    eprintln!("[main][smoke][completion] preparing headless launch");
    let mut outcome =
        saya::app::bootstrap::prepare_launch(launch_request).map_err(format_bootstrap_error)?;
    configure_diagnostic_log_from_startup(
        outcome.startup_registry.log.log_file.as_deref(),
        outcome.startup_registry.log.log_level,
    )
    .map_err(|error| error.to_string())?;
    let mut session_state = outcome.editor_session_state();
    let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
        .map_err(|error| format!("runtime session initialization failed: {error:?}"))?;
    let mut floating_window_manager = FloatingWindowManager::default();
    let mut completion_float_manager = CompletionFloatManager::default();
    let mut lsp_diagnostic_store = LspDiagnosticStore::default();
    let mut terminal_float_manager = TerminalFloatManager::default();
    let mut panel_manager = PanelManager::default();
    let mut transient_msg = None;
    let mut need_redraw = false;
    let mut runtime_presentation_intents = Vec::new();
    let mut outcome_accumulator = MainOutcomeAccumulator::default();

    outcome
        .core_bridge
        .dispatch_key("A")
        .map_err(|error| format!("completion smoke insert mode failed: {error:?}"))?;
    let mut startup_keymap_pending_lhs = None;
    let action = startup_keymap_action_for_snapshot_input(
        &outcome.startup_registry.keymaps,
        &outcome.core_bridge.light_snapshot(),
        &KeyInput::Ctrl('x'),
        &mut startup_keymap_pending_lhs,
    )
    .ok_or_else(|| {
        format!(
            "completion smoke keymap did not resolve: mode={:?}, keymaps={:?}, warnings={:?}",
            outcome.core_bridge.mode(),
            outcome.startup_registry.keymaps,
            outcome.warnings
        )
    })?;
    let StartupKeymapAction::RegisteredCommand(command_name) = action else {
        return Err(format!(
            "completion smoke keymap resolved to non-command action: {action:?}"
        ));
    };
    execute_startup_keymap_registered_command(
        Some(&mut runtime_session),
        &command_name,
        &mut outcome,
        &mut session_state,
        &mut floating_window_manager,
        &mut completion_float_manager,
        &mut lsp_diagnostic_store,
        &mut terminal_float_manager,
        &mut panel_manager,
        None,
        &mut transient_msg,
        &mut need_redraw,
        &mut runtime_presentation_intents,
        None,
    )
    .await;
    if let Some(message) = transient_msg {
        return Err(format!("completion smoke command failed: {message}"));
    }
    let menu_window = floating_window_manager
        .windows()
        .iter()
        .find(|window| matches!(window.content, FloatingContentRef::CompletionMenu { .. }));
    let Some(menu_window) = menu_window else {
        return Err("completion smoke did not open a completion menu".to_string());
    };
    eprintln!(
        "[main][smoke][completion] menu opened: lines={:?}",
        menu_window.lines
    );
    emit_binary_smoke_state(
        "completion-menu-opened",
        serde_json::json!({
            "lines": menu_window.lines,
        }),
    );
    if std::env::var_os("SAYA_COMPLETION_SMOKE_EXPECT_MULTIPLE").is_some()
        && menu_window.lines.len() < 2
    {
        return Err(format!(
            "completion smoke expected multiple candidates, got lines={:?}",
            menu_window.lines
        ));
    }

    let active_window_id = outcome
        .core_bridge
        .light_snapshot()
        .active_window_id()
        .unwrap_or(1);
    if std::env::var_os("SAYA_COMPLETION_SMOKE_SELECT_NEXT").is_some() {
        match handle_completion_float_key(
            &mut completion_float_manager,
            &mut floating_window_manager,
            &mut outcome.core_bridge,
            &KeyInput::Down,
            active_window_id,
        ) {
            Some(FloatingWindowKeyHandling::Consumed) => {}
            other => {
                return Err(format!(
                    "completion smoke Down did not move the active menu selection: {other:?}"
                ));
            }
        }
        let selected_lines = floating_window_manager
            .windows()
            .iter()
            .find(|window| matches!(window.content, FloatingContentRef::CompletionMenu { .. }))
            .map(|window| window.lines.clone())
            .unwrap_or_default();
        eprintln!(
            "[main][smoke][completion] menu after Down: lines={:?}",
            selected_lines
        );
        emit_binary_smoke_state(
            "completion-menu-after-down",
            serde_json::json!({
                "lines": selected_lines,
            }),
        );
    }
    let before_confirm_snapshot = outcome.core_bridge.light_snapshot();
    let confirm_key = if std::env::var_os("SAYA_COMPLETION_SMOKE_CONFIRM_TAB").is_some() {
        KeyInput::Tab
    } else {
        KeyInput::Enter
    };
    match handle_completion_float_key(
        &mut completion_float_manager,
        &mut floating_window_manager,
        &mut outcome.core_bridge,
        &confirm_key,
        active_window_id,
    ) {
        Some(FloatingWindowKeyHandling::Closed { .. }) => {}
        other => {
            return Err(format!(
                "completion smoke confirm key did not accept the active menu: key={confirm_key:?}, outcome={other:?}"
            ));
        }
    }
    consume_core_outcomes_from_core(
        &mut outcome.core_bridge,
        &mut outcome_accumulator,
        &mut need_redraw,
    );
    let after_confirm_snapshot = outcome.core_bridge.light_snapshot();
    if after_confirm_snapshot.revision != before_confirm_snapshot.revision {
        if let Some(reason) = dispatch_buffer_changed_with_runtime(
            Some(&mut runtime_session),
            &mut outcome,
            &mut session_state,
            &mut transient_msg,
            &mut need_redraw,
            &mut runtime_presentation_intents,
            &mut floating_window_manager,
            &mut completion_float_manager,
            &mut lsp_diagnostic_store,
            &mut terminal_float_manager,
            &mut panel_manager,
            None,
        )
        .await
        {
            return Err(format!(
                "completion smoke bufferChanged requested shutdown: {reason:?}"
            ));
        }
        if let Some(message) = transient_msg.take() {
            return Err(format!(
                "completion smoke bufferChanged failed after confirm: {message}"
            ));
        }
    }

    let snapshot = outcome.core_bridge.snapshot();
    eprintln!(
        "[main][smoke][completion] after confirm: cursor=({},{}), mode={:?}",
        snapshot.cursor_row, snapshot.cursor_col, snapshot.mode
    );
    emit_binary_smoke_state(
        "completion-after-confirm",
        serde_json::json!({
            "cursorRow": snapshot.cursor_row,
            "cursorCol": snapshot.cursor_col,
            "mode": format!("{:?}", snapshot.mode),
        }),
    );
    if std::env::var_os("SAYA_COMPLETION_SMOKE_EXPECT_REOPEN_AFTER_CONFIRM").is_some() {
        let reopened_lines = floating_window_manager
            .windows()
            .iter()
            .find(|window| matches!(window.content, FloatingContentRef::CompletionMenu { .. }))
            .map(|window| window.lines.clone())
            .unwrap_or_default();
        if reopened_lines.is_empty() {
            return Err(
                "completion smoke expected completion menu to reopen after confirm".to_string(),
            );
        }
        eprintln!(
            "[main][smoke][completion] menu reopened after confirm: lines={:?}",
            reopened_lines
        );
        emit_binary_smoke_state(
            "completion-menu-reopened-after-confirm",
            serde_json::json!({
                "lines": reopened_lines,
            }),
        );
    }
    let save = save_snapshot_result(&snapshot.text, &mut session_state);
    if !save.wrote {
        return Err(format!(
            "completion smoke save failed: {:?}",
            save.transient_message
        ));
    }
    eprintln!(
        "[main][smoke][completion] completed: text_len={}, transient={:?}",
        snapshot.text.len(),
        save.transient_message
    );
    emit_binary_smoke_state(
        "completion-completed",
        serde_json::json!({
            "textLen": snapshot.text.len(),
            "transient": save.transient_message,
        }),
    );
    Ok(())
}

async fn run_binary_pty_smoke(launch_request: saya::app::cli::LaunchRequest) -> Result<(), String> {
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
    let mut markdown_metadata_cache = MarkdownMetadataCache::new();
    let mut runtime_presentation_intents: Vec<RuntimePresentationIntent> = Vec::new();
    let mut outcome_accumulator = MainOutcomeAccumulator::default();
    let mut host_action_runtime = HostActionRuntime::default();

    let (terminal_width, terminal_height) = current_terminal_size();
    let mut last_synced_terminal_size = None;
    if sync_core_screen_size_if_changed(
        &mut outcome,
        &mut last_synced_terminal_size,
        TerminalSize {
            columns: terminal_width,
            rows: terminal_height,
        },
    ) {
        let mut initial_need_redraw = false;
        consume_core_outcomes_from_core(
            &mut outcome.core_bridge,
            &mut outcome_accumulator,
            &mut initial_need_redraw,
        );
    }
    let initial_render = render_coordinator
        .render_workspace_result(
            build_workspace_render_output(
                &mut outcome,
                &mut session_state,
                &mut viewport_store,
                ViewportSyncMode::Core,
                &mut search_refresh_store,
                &mut markdown_metadata_cache,
                None,
                "",
                0,
                None,
                None,
                None,
                None,
                None,
                terminal_width,
                terminal_height,
                None,
                None,
                None,
                None,
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
                &mut session_state,
                &mut viewport_store,
                ViewportSyncMode::Core,
                &mut search_refresh_store,
                &mut markdown_metadata_cache,
                None,
                "",
                0,
                None,
                None,
                None,
                None,
                None,
                terminal_width,
                terminal_height,
                None,
                None,
                None,
                None,
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
                &mut session_state,
                &mut viewport_store,
                ViewportSyncMode::Core,
                &mut search_refresh_store,
                &mut markdown_metadata_cache,
                None,
                "",
                0,
                None,
                None,
                None,
                None,
                None,
                resized_width,
                resized_height,
                None,
                None,
                None,
                None,
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
        None,
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
        None,
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
        None,
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

#[cfg(test)]
fn handle_directory_operation_confirmation_key_without_runtime(
    key: &KeyInput,
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    session_state: &mut saya::app::session::EditorSessionState,
    transient_msg: &mut Option<String>,
    need_redraw: &mut bool,
) -> Option<Option<ShutdownReason>> {
    let Some(action) = directory_operation_confirmation_key_action(key, session_state) else {
        return None;
    };
    *need_redraw = true;
    let mut shutdown_reason = None;
    match action {
        DirectoryOperationConfirmationKeyAction::Confirm => {
            let snapshot = outcome.core_bridge.snapshot();
            let save_outcome = save_snapshot_result_with_confirmation(
                &snapshot.text,
                session_state,
                None,
                true,
                Some(outcome.core_bridge.revision()),
            );
            *transient_msg = save_outcome.transient_message;
            if save_outcome.wrote {
                refresh_directory_buffer_after_confirmed_save(
                    outcome,
                    session_state,
                    transient_msg,
                );
                shutdown_reason = take_pending_directory_save_then_quit_shutdown(session_state);
            }
        }
        DirectoryOperationConfirmationKeyAction::Cancel => {
            *transient_msg = Some(directory_operation_cancel_message(session_state));
        }
        DirectoryOperationConfirmationKeyAction::KeepWaiting => {
            *transient_msg = Some(
                "Apply directory operations? Press y or Enter for OK, n or Esc to cancel"
                    .to_string(),
            );
        }
    }
    Some(shutdown_reason)
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

fn perform_job_control_suspend_cycle<B: TerminalBackend>(
    terminal_broker: &mut saya::terminal::io_broker::TerminalIoBroker<'_, B>,
    need_redraw: &mut bool,
    terminal_display_redraw_plan: &mut Option<RedrawPlan>,
) {
    log::debug!("[main] starting job-control suspend cycle");
    trace_job_control_diagnostic(format_args!("starting job-control suspend cycle"));
    if let Err(error) = terminal_broker.suspend_for_job_control() {
        log::debug!("[main] terminal release before suspend failed: {error}");
        trace_job_control_diagnostic(format_args!(
            "terminal release before suspend failed: {error}"
        ));
        *need_redraw = true;
        return;
    }

    if let Err(error) = suspend_current_process_for_job_control() {
        log::debug!("[main] process suspend failed: {error}");
        trace_job_control_diagnostic(format_args!("process suspend failed: {error}"));
    }

    if let Err(error) = terminal_broker.resume_after_job_control() {
        log::debug!("[main] terminal reclaim after suspend failed: {error}");
        trace_job_control_diagnostic(format_args!(
            "terminal reclaim after suspend failed: {error}"
        ));
        *need_redraw = true;
        return;
    }

    let (columns, rows) = current_terminal_size();
    terminal_broker.record_resize(TerminalSize { columns, rows });
    log::debug!(
        "[main] completed job-control suspend cycle: columns={}, rows={}",
        columns,
        rows
    );
    *terminal_display_redraw_plan = Some(terminal_display_invalidated_redraw_plan());
    trace_job_control_diagnostic(format_args!(
        "completed job-control suspend cycle; full terminal redraw required: columns={}, rows={}",
        columns, rows
    ));
    *need_redraw = true;
}

/// `need_redraw` が立っているとき、コア画面サイズ同期・コマンドライン専用
/// リドローの早期復帰判定・ワークスペース全体描画までを一括で行う。
/// イベントループの最終段（リドロー段）を 1 関数に閉じ込めてループ本体を薄く保つ。
#[allow(clippy::too_many_arguments)]
fn render_workspace_if_needed(
    mut need_redraw: bool,
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    outcome_accumulator: &mut MainOutcomeAccumulator,
    session_state: &mut saya::app::session::EditorSessionState,
    render_coordinator: &mut TuiRenderCoordinator,
    terminal_broker: &mut dyn OverlayTerminalWriter,
    last_workspace_model: &mut Option<WorkspaceScreenModel>,
    last_synced_terminal_size: &mut Option<TerminalSize>,
    terminal_display_redraw_plan: &mut Option<RedrawPlan>,
    viewport_store: &mut WindowViewportStore,
    viewport_sync_mode: &mut ViewportSyncMode,
    search_refresh_store: &mut WindowSearchRefreshStore,
    markdown_metadata_cache: &mut MarkdownMetadataCache,
    workspace_projection_dirty: &mut bool,
    transient_msg: &mut Option<String>,
    pending_input_perf_trace: &mut Option<MainInputPerfTrace>,
    floating_window_manager: &mut FloatingWindowManager,
    panel_manager: &mut PanelManager,
    terminal_float_manager: &mut TerminalFloatManager,
    command_line_prompt: Option<char>,
    command_line_edit: &CommandLineEdit,
    system_warning: &Option<String>,
    runtime_input_prompt: &Option<RuntimeInputPromptUiState>,
    runtime_session: &Option<RuntimeSessionOwner>,
    runtime_presentation_intents: &[RuntimePresentationIntent],
    capability_profile: &saya::terminal::capability::TerminalCapabilityProfile,
) {
    if need_redraw {
        let (terminal_width, terminal_height) = current_terminal_size();
        if sync_core_screen_size_if_changed(
            outcome,
            last_synced_terminal_size,
            TerminalSize {
                columns: terminal_width,
                rows: terminal_height,
            },
        ) {
            consume_core_outcomes_from_core(
                &mut outcome.core_bridge,
                outcome_accumulator,
                &mut need_redraw,
            );
        }
        if terminal_display_redraw_plan.is_none() {
            match render_command_line_only_redraw_if_possible(
                render_coordinator,
                Some(&mut *terminal_broker),
                last_workspace_model,
                outcome_accumulator.last_structural_refresh.as_ref(),
                *workspace_projection_dirty,
                command_line_prompt,
                command_line_edit.buffer(),
                command_line_edit.cursor_byte_index(),
                session_state.tab_size(),
            ) {
                CommandLineOnlyRedraw::Rendered => return,
                CommandLineOnlyRedraw::NotApplicable | CommandLineOnlyRedraw::Fallback => {}
            }
        } else {
            trace_job_control_diagnostic(format_args!(
                "command-line-only redraw bypassed because terminal display was invalidated"
            ));
        }
        let redraw_started_at = std::time::Instant::now();
        let redraw_result = build_workspace_render_output(
            outcome,
            session_state,
            viewport_store,
            *viewport_sync_mode,
            search_refresh_store,
            markdown_metadata_cache,
            command_line_prompt,
            command_line_edit.buffer(),
            command_line_edit.cursor_byte_index(),
            outcome_accumulator.last_projection_frame.as_ref(),
            runtime_input_prompt.as_ref(),
            outcome_accumulator.last_structural_refresh.as_mut(),
            system_warning.as_deref(),
            transient_msg.as_deref(),
            terminal_width,
            terminal_height,
            Some(&mut *floating_window_manager),
            Some(&mut *panel_manager),
            Some(&mut *terminal_float_manager),
            runtime_session
                .as_ref()
                .map(RuntimeSessionOwner::selector_tui_projection_sink),
        );
        *viewport_sync_mode = ViewportSyncMode::Core;
        let redraw_failure = redraw_result.as_ref().err().map(ToString::to_string);
        let redraw_plan = effective_workspace_redraw_plan(
            outcome_accumulator.last_structural_refresh.as_ref(),
            terminal_display_redraw_plan.as_ref(),
        );
        trace_job_control_diagnostic(format_args!(
            "rendering resumed terminal with redraw plan: requested={}, full={}, clear_before_draw={}, source={:?}",
            redraw_plan.requested,
            redraw_plan.full,
            redraw_plan.clear_before_draw,
            redraw_plan.source
        ));
        match render_coordinator.render_workspace_result_with_structural_refresh_and_redraw_plan(
            redraw_result,
            capability_profile,
            runtime_presentation_intents,
            Some(&mut *terminal_broker),
            outcome_accumulator.last_structural_refresh.as_ref(),
            redraw_plan,
        ) {
            Ok(render_output) => {
                if let Some(message) = redraw_failure {
                    *transient_msg = Some(message);
                }
                *last_workspace_model = Some(render_output.rendered_workspace.clone());
                mark_structural_refresh_rendered(outcome_accumulator);
                if let Some(trace) = pending_input_perf_trace.take() {
                    let first_line = render_output
                        .rendered_workspace
                        .panes
                        .iter()
                        .find(|pane| pane.is_active)
                        .or_else(|| render_output.rendered_workspace.panes.first())
                        .and_then(|pane| pane.lines.first())
                        .cloned()
                        .unwrap_or_default();
                    log::info!(
                        "[PERF][main][input_trace] redraw_complete trace_id={} key={} command={} command_ms={:?} redraw_ms={} total_to_draw_ms={} active_window_id={} panes={} first_line={:?}",
                        trace.id,
                        trace.key,
                        trace.command.as_deref().unwrap_or("<none>"),
                        trace.command_elapsed_ms,
                        redraw_started_at.elapsed().as_millis(),
                        trace.started_at.elapsed().as_millis(),
                        render_output.rendered_workspace.active_window_id,
                        render_output.rendered_workspace.panes.len(),
                        first_line
                    );
                }
                if std::env::var_os("SAYA_TRACE_RENDER").is_some() {
                    trace_workspace_render_pipeline(
                        "redraw",
                        &outcome.core_bridge.snapshot().text,
                        &render_output.rendered_workspace,
                    );
                }
                *terminal_display_redraw_plan = None;
                *workspace_projection_dirty = false;
            }
            Err(error) => {
                *transient_msg = Some(error.to_string());
                log::debug!(
                    "[main] redraw failed without rollback because no successful model exists yet: error={:?}",
                    error
                );
            }
        }
    }
}

#[cfg(test)]
fn is_markdown_target_path(path: Option<&std::path::PathBuf>) -> bool {
    path.and_then(|path| path.extension())
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "md" | "markdown" | "mdown"
            )
        })
        .unwrap_or(false)
}

#[cfg(test)]
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

#[cfg(test)]
fn redraw_trace_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::*;
    use saya::app::session::{DirectoryBufferPlannedOperation, MermaidPreviewZoom};
    use saya::features::lsp::float::{
        PopupSizeBasis, PopupSizeSpec, PopupSizeValue, ResolvedPopupSizeLimit,
    };
    use saya::features::lsp::host_commands::{
        LspHoverPopupKind, LspPopupKind, PopupSizingContext, default_lsp_popup_basis,
        parse_lsp_hover_popup_kind, parse_lsp_popup_size_spec, resolve_lsp_popup_size_limit,
    };
    use saya::presentation::floating_window::{
        FloatingBorder, FloatingChrome, FloatingImageSource, FloatingPlacement, FloatingRelativeTo,
        FloatingSize, FloatingZIndex,
    };
    use saya::presentation::overlay::optional_graphics::RecordingOverlayWriter;
    use saya::presentation::panel::{
        PanelCloseBehavior, PanelContent, PanelNode, PanelOpenRequest, PanelPosition, PanelSize,
    };
    use saya::presentation::screen_model::ScreenCursorStyle;
    use saya::runtime::live::{
        RuntimeFilerErrorKind, RuntimeFilerOperationKind, RuntimeFloatContentRequest,
        RuntimeFloatRelativeToRequest, RuntimeFloatZIndexRequest, RuntimePanelContentRequest,
        RuntimePanelNodeRequest,
    };
    use saya::terminal::float::{TerminalFloatCloseBehavior, TerminalFloatSpawnRequest};

    fn unique_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        std::env::temp_dir().join(format!("saya-main-test-{name}-{nanos}"))
    }

    fn unique_repo_relative_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        PathBuf::from("tmp").join(format!("saya-main-test-{name}-{nanos}"))
    }

    fn wait_for_test_condition(mut predicate: impl FnMut() -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if predicate() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(predicate(), "condition did not become true before timeout");
    }

    #[test]
    fn message_pager_wraps_long_single_line_before_counting_visible_rows() {
        let wrapped =
            wrap_message_for_pager("unsupported startup option: saya.options.lineNumbers", 20);

        assert_eq!(
            wrapped.lines().collect::<Vec<_>>(),
            vec![
                "unsupported startup ",
                "option: saya.options",
                ".lineNumbers"
            ]
        );
    }

    fn main_test_workspace() -> WorkspaceScreenModel {
        WorkspaceScreenModel {
            panes: vec![saya::presentation::screen_model::ScreenModel {
                window_id: 1,
                buffer_id: 1,
                rect: saya::presentation::screen_model::PaneRect {
                    x: 0,
                    y: 0,
                    width: 20,
                    height: 3,
                },
                file_name: "alpha.txt".to_string(),
                mode_label: "NORMAL".to_string(),
                status_line: "test.txt | NORMAL".to_string(),
                cursor_style: ScreenCursorStyle::Block,
                dirty: false,
                lines: vec!["alpha".to_string()],
                line_projections: vec![],
                cursor_row: 0,
                cursor_col: 0,
                visual_selection: None,
                search_overlays: vec![],
                syntax_chunks: vec![],
                markdown_style_ranges: vec![],
                filer_style_ranges: vec![],
                resolved_theme: saya::presentation::theme::ResolvedTheme::default(),
                message_line: None,
                command_cursor_col: None,
                is_active: true,
            }],
            floats: vec![],
            active_window_id: 1,
            message_line: saya::core::notification_prompt::resolve_workspace_message_line(Vec::<
                saya::core::notification_prompt::MessageLineCandidate,
            >::new(
            )),
            message_area_height: 5,
            message_scroll_offset: 0,
            prompt_line: None,
            pager_prompt: None,
            suppressed_prompt_hints: vec![],
            bell: None,
            command_line: None,
        }
    }

    #[test]
    fn active_mermaid_block_appends_preview_float_without_changing_body_lines() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let source = "# Test\n```mermaid\ngraph TD\n  A-->B\n```\nafter\n";
        let bridge = saya::core::bridge::CoreBridge::new(source).expect("core bridge");
        let mut snapshot = bridge.snapshot();
        snapshot.cursor_row = 2;
        snapshot.windows[0].cursor_row = 2;
        snapshot.buffers[0].name = "diagram.md".to_string();
        let mut workspace = main_test_workspace();
        workspace.active_window_id = snapshot.windows[0].id;
        workspace.panes[0].window_id = snapshot.windows[0].id;
        workspace.panes[0].buffer_id = snapshot.windows[0].buf_id;
        workspace.panes[0].file_name = "diagram.md".to_string();
        workspace.panes[0].lines = vec![
            "# Test".to_string(),
            "```mermaid".to_string(),
            "graph TD".to_string(),
            "  A-->B".to_string(),
            "```".to_string(),
            "after".to_string(),
        ];
        let mut maps = BTreeMap::new();
        maps.insert(
            snapshot.windows[0].id,
            Arc::new(MarkdownDocumentMap::parse(source)),
        );
        let mut session_state = EditorSessionState::new(None);

        append_active_mermaid_preview_float(
            &mut workspace,
            &snapshot,
            source,
            &maps,
            80,
            24,
            &mut session_state,
        );

        assert_eq!(
            workspace.panes[0].lines,
            vec![
                "# Test",
                "```mermaid",
                "graph TD",
                "  A-->B",
                "```",
                "after"
            ],
            "preview must not collapse or reserve rows in the markdown body"
        );
        let preview = workspace
            .floats
            .iter()
            .find(|float| !float.images.is_empty())
            .expect("active Mermaid block should append an image preview float");
        let FloatingImageSource::Mermaid { source, row, .. } = &preview.images[0].source;
        assert_eq!(*row, 1);
        assert_eq!(source, "graph TD\n  A-->B");
    }

    #[test]
    fn mermaid_preview_float_uses_roomy_terminal_relative_size() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let source = "```mermaid\ngraph TD\n  A-->B\n```\n";
        let bridge = saya::core::bridge::CoreBridge::new(source).expect("core bridge");
        let mut snapshot = bridge.snapshot();
        snapshot.cursor_row = 1;
        snapshot.windows[0].cursor_row = 1;
        snapshot.buffers[0].name = "diagram.md".to_string();
        let mut workspace = main_test_workspace();
        workspace.active_window_id = snapshot.windows[0].id;
        workspace.panes[0].window_id = snapshot.windows[0].id;
        workspace.panes[0].buffer_id = snapshot.windows[0].buf_id;
        let mut maps = BTreeMap::new();
        maps.insert(
            snapshot.windows[0].id,
            Arc::new(MarkdownDocumentMap::parse(source)),
        );
        let mut session_state = EditorSessionState::new(None);

        append_active_mermaid_preview_float(
            &mut workspace,
            &snapshot,
            source,
            &maps,
            200,
            80,
            &mut session_state,
        );

        let preview = workspace
            .floats
            .iter()
            .find(|float| !float.images.is_empty())
            .expect("active Mermaid block should append an image preview float");
        assert!(
            preview.rect.width >= 60,
            "Mermaid preview should use more than the old narrow 46-column cap"
        );
        assert!(
            preview.rect.height >= 20,
            "Mermaid preview should use more than the old short 16-row cap"
        );
        assert_eq!(preview.images[0].max_width, preview.rect.width - 2);
    }

    #[test]
    fn mermaid_preview_float_size_uses_configured_window_percentages() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let source = "```mermaid\ngraph TD\n  A-->B\n```\n";
        let bridge = saya::core::bridge::CoreBridge::new(source).expect("core bridge");
        let mut snapshot = bridge.snapshot();
        snapshot.cursor_row = 1;
        snapshot.windows[0].cursor_row = 1;
        snapshot.buffers[0].name = "diagram.md".to_string();
        let mut workspace = main_test_workspace();
        workspace.active_window_id = snapshot.windows[0].id;
        workspace.panes[0].window_id = snapshot.windows[0].id;
        workspace.panes[0].buffer_id = snapshot.windows[0].buf_id;
        let mut maps = BTreeMap::new();
        maps.insert(
            snapshot.windows[0].id,
            Arc::new(MarkdownDocumentMap::parse(source)),
        );
        let mut session_state = EditorSessionState::new(None);
        session_state
            .apply_presentation_option(
                saya::runtime::options::SayaOptionName::MermaidPreviewWidth,
                saya::runtime::options::SayaOptionValue::Number(70),
            )
            .expect("width percent should apply");
        session_state
            .apply_presentation_option(
                saya::runtime::options::SayaOptionName::MermaidPreviewHeight,
                saya::runtime::options::SayaOptionValue::Number(60),
            )
            .expect("height percent should apply");

        append_active_mermaid_preview_float(
            &mut workspace,
            &snapshot,
            source,
            &maps,
            200,
            80,
            &mut session_state,
        );

        let preview = workspace
            .floats
            .iter()
            .find(|float| !float.images.is_empty())
            .expect("active Mermaid block should append an image preview float");
        assert_eq!(preview.rect.width, 140);
        assert_eq!(preview.rect.height, 48);
    }

    #[test]
    fn mermaid_preview_float_source_includes_configured_background() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let source = "```mermaid\ngraph TD\n  A-->B\n```\n";
        let bridge = saya::core::bridge::CoreBridge::new(source).expect("core bridge");
        let mut snapshot = bridge.snapshot();
        snapshot.cursor_row = 1;
        snapshot.windows[0].cursor_row = 1;
        snapshot.buffers[0].name = "diagram.md".to_string();
        let mut workspace = main_test_workspace();
        workspace.active_window_id = snapshot.windows[0].id;
        workspace.panes[0].window_id = snapshot.windows[0].id;
        workspace.panes[0].buffer_id = snapshot.windows[0].buf_id;
        let mut maps = BTreeMap::new();
        maps.insert(
            snapshot.windows[0].id,
            Arc::new(MarkdownDocumentMap::parse(source)),
        );
        let mut session_state = EditorSessionState::new(None);
        session_state
            .apply_presentation_option(
                saya::runtime::options::SayaOptionName::MermaidPreviewBackground,
                saya::runtime::options::SayaOptionValue::String("#ffffff".to_string()),
            )
            .expect("background should apply");

        append_active_mermaid_preview_float(
            &mut workspace,
            &snapshot,
            source,
            &maps,
            120,
            40,
            &mut session_state,
        );

        let preview = workspace
            .floats
            .iter()
            .find(|float| !float.images.is_empty())
            .expect("active Mermaid block should append an image preview float");
        let FloatingImageSource::Mermaid { background, .. } = &preview.images[0].source;
        assert_eq!(background, "#ffffff");
    }

    #[test]
    fn insert_mode_does_not_append_mermaid_preview_float() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let source = "```mermaid\ngraph TD\n  A-->B\n```\n";
        let bridge = saya::core::bridge::CoreBridge::new(source).expect("core bridge");
        let mut snapshot = bridge.snapshot();
        snapshot.mode = CoreMode::Insert;
        snapshot.cursor_row = 1;
        snapshot.windows[0].cursor_row = 1;
        snapshot.buffers[0].name = "diagram.md".to_string();
        let mut workspace = main_test_workspace();
        workspace.active_window_id = snapshot.windows[0].id;
        workspace.panes[0].window_id = snapshot.windows[0].id;
        workspace.panes[0].buffer_id = snapshot.windows[0].buf_id;
        let mut maps = BTreeMap::new();
        maps.insert(
            snapshot.windows[0].id,
            Arc::new(MarkdownDocumentMap::parse(source)),
        );
        let mut session_state = EditorSessionState::new(None);

        append_active_mermaid_preview_float(
            &mut workspace,
            &snapshot,
            source,
            &maps,
            80,
            24,
            &mut session_state,
        );

        assert!(
            workspace.floats.is_empty(),
            "Insert mode should keep editing responsive and avoid image preview rendering"
        );
    }

    #[test]
    fn mermaid_preview_auto_off_skips_float_but_manual_request_appends_once() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let source = "```mermaid\ngraph TD\n  A-->B\n```\n";
        let bridge = saya::core::bridge::CoreBridge::new(source).expect("core bridge");
        let mut snapshot = bridge.snapshot();
        snapshot.cursor_row = 1;
        snapshot.windows[0].cursor_row = 1;
        snapshot.buffers[0].name = "diagram.md".to_string();
        let mut maps = BTreeMap::new();
        maps.insert(
            snapshot.windows[0].id,
            Arc::new(MarkdownDocumentMap::parse(source)),
        );

        let mut auto_workspace = main_test_workspace();
        auto_workspace.active_window_id = snapshot.windows[0].id;
        auto_workspace.panes[0].window_id = snapshot.windows[0].id;
        auto_workspace.panes[0].buffer_id = snapshot.windows[0].buf_id;
        let mut auto_session_state = EditorSessionState::new(None);
        auto_session_state
            .apply_presentation_option(
                saya::runtime::options::SayaOptionName::MermaidPreview,
                saya::runtime::options::SayaOptionValue::Boolean(false),
            )
            .expect("mermaidpreview off should apply");
        append_active_mermaid_preview_float(
            &mut auto_workspace,
            &snapshot,
            source,
            &maps,
            80,
            24,
            &mut auto_session_state,
        );
        assert!(auto_workspace.floats.is_empty());

        let mut manual_workspace = main_test_workspace();
        manual_workspace.active_window_id = snapshot.windows[0].id;
        manual_workspace.panes[0].window_id = snapshot.windows[0].id;
        manual_workspace.panes[0].buffer_id = snapshot.windows[0].buf_id;
        let mut manual_session_state = EditorSessionState::new(None);
        manual_session_state
            .apply_presentation_option(
                saya::runtime::options::SayaOptionName::MermaidPreview,
                saya::runtime::options::SayaOptionValue::Boolean(false),
            )
            .expect("mermaidpreview off should apply");
        manual_session_state.request_mermaid_preview();
        append_active_mermaid_preview_float(
            &mut manual_workspace,
            &snapshot,
            source,
            &maps,
            80,
            24,
            &mut manual_session_state,
        );
        assert_eq!(manual_workspace.floats.len(), 1);

        let mut next_frame_workspace = main_test_workspace();
        next_frame_workspace.active_window_id = snapshot.windows[0].id;
        next_frame_workspace.panes[0].window_id = snapshot.windows[0].id;
        next_frame_workspace.panes[0].buffer_id = snapshot.windows[0].buf_id;
        append_active_mermaid_preview_float(
            &mut next_frame_workspace,
            &snapshot,
            source,
            &maps,
            80,
            24,
            &mut manual_session_state,
        );
        assert_eq!(
            next_frame_workspace.floats.len(),
            1,
            "manual Mermaid preview should survive the async renderer completion redraw"
        );

        let mut outside_snapshot = snapshot.clone();
        outside_snapshot.cursor_row = 4;
        outside_snapshot.windows[0].cursor_row = 4;
        let mut outside_workspace = main_test_workspace();
        outside_workspace.active_window_id = outside_snapshot.windows[0].id;
        outside_workspace.panes[0].window_id = outside_snapshot.windows[0].id;
        outside_workspace.panes[0].buffer_id = outside_snapshot.windows[0].buf_id;
        append_active_mermaid_preview_float(
            &mut outside_workspace,
            &outside_snapshot,
            source,
            &maps,
            80,
            24,
            &mut manual_session_state,
        );
        assert!(outside_workspace.floats.is_empty());
        assert!(!manual_session_state.mermaid_preview_manual_active());
    }

    #[test]
    fn focused_mermaid_preview_keys_zoom_pan_and_close_without_editor_passthrough() {
        let mut session_state = EditorSessionState::new(None);

        assert_eq!(
            handle_mermaid_preview_key(&mut session_state, &KeyInput::Char('j')),
            None,
            "non-focused preview must not steal normal j movement"
        );

        session_state.request_mermaid_preview();
        assert_eq!(
            handle_mermaid_preview_key(&mut session_state, &KeyInput::Char('+')),
            Some(FloatingWindowKeyHandling::Consumed)
        );
        assert_eq!(
            session_state.mermaid_preview_zoom(),
            MermaidPreviewZoom::Percent(125)
        );

        assert_eq!(
            handle_mermaid_preview_key(&mut session_state, &KeyInput::Char('j')),
            Some(FloatingWindowKeyHandling::Consumed)
        );
        assert_eq!(session_state.mermaid_preview_pan(), (0, 64));

        assert!(matches!(
            handle_mermaid_preview_key(&mut session_state, &KeyInput::Char('q')),
            Some(FloatingWindowKeyHandling::Closed { .. })
        ));
        assert!(!session_state.mermaid_preview_manual_active());
        assert!(!session_state.mermaid_preview_focused());
    }

    #[test]
    fn focused_mermaid_preview_handles_full_zoom_and_pan_key_contract() {
        let mut session_state = EditorSessionState::new(None);
        session_state.request_mermaid_preview();

        assert_eq!(
            handle_mermaid_preview_key(&mut session_state, &KeyInput::Char('=')),
            Some(FloatingWindowKeyHandling::Consumed)
        );
        assert_eq!(
            session_state.mermaid_preview_zoom(),
            MermaidPreviewZoom::Percent(125)
        );

        assert_eq!(
            handle_mermaid_preview_key(&mut session_state, &KeyInput::Char('-')),
            Some(FloatingWindowKeyHandling::Consumed)
        );
        assert_eq!(
            session_state.mermaid_preview_zoom(),
            MermaidPreviewZoom::Percent(100)
        );

        assert_eq!(
            handle_mermaid_preview_key(&mut session_state, &KeyInput::Char('0')),
            Some(FloatingWindowKeyHandling::Consumed)
        );
        assert_eq!(
            session_state.mermaid_preview_zoom(),
            MermaidPreviewZoom::Fit
        );

        assert_eq!(
            handle_mermaid_preview_key(&mut session_state, &KeyInput::Char('1')),
            Some(FloatingWindowKeyHandling::Consumed)
        );
        assert_eq!(
            session_state.mermaid_preview_zoom(),
            MermaidPreviewZoom::Percent(100)
        );

        assert_eq!(
            handle_mermaid_preview_key(&mut session_state, &KeyInput::Ctrl('f')),
            Some(FloatingWindowKeyHandling::Consumed)
        );
        assert_eq!(session_state.mermaid_preview_pan(), (0, 256));

        assert_eq!(
            handle_mermaid_preview_key(&mut session_state, &KeyInput::Ctrl('b')),
            Some(FloatingWindowKeyHandling::Consumed)
        );
        assert_eq!(session_state.mermaid_preview_pan(), (0, 0));

        assert_eq!(
            handle_mermaid_preview_key(&mut session_state, &KeyInput::Char('L')),
            Some(FloatingWindowKeyHandling::Consumed)
        );
        assert_eq!(session_state.mermaid_preview_pan(), (256, 0));

        assert_eq!(
            handle_mermaid_preview_key(&mut session_state, &KeyInput::Char('H')),
            Some(FloatingWindowKeyHandling::Consumed)
        );
        assert_eq!(session_state.mermaid_preview_pan(), (0, 0));

        assert!(matches!(
            handle_mermaid_preview_key(&mut session_state, &KeyInput::Escape),
            Some(FloatingWindowKeyHandling::Closed { .. })
        ));
        assert!(!session_state.mermaid_preview_focused());
    }

    #[test]
    fn mermaid_preview_mouse_wheel_pans_only_after_preview_focus() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let source = "```mermaid\ngraph TD\n  A-->B\n```\n";
        let bridge = saya::core::bridge::CoreBridge::new(source).expect("core bridge");
        let mut snapshot = bridge.snapshot();
        snapshot.cursor_row = 1;
        snapshot.windows[0].cursor_row = 1;
        snapshot.buffers[0].name = "diagram.md".to_string();
        let mut maps = BTreeMap::new();
        maps.insert(
            snapshot.windows[0].id,
            Arc::new(MarkdownDocumentMap::parse(source)),
        );
        let mut workspace = main_test_workspace();
        workspace.active_window_id = snapshot.windows[0].id;
        workspace.panes[0].window_id = snapshot.windows[0].id;
        workspace.panes[0].buffer_id = snapshot.windows[0].buf_id;
        let mut session_state = EditorSessionState::new(None);

        append_active_mermaid_preview_float(
            &mut workspace,
            &snapshot,
            source,
            &maps,
            80,
            24,
            &mut session_state,
        );
        let preview_rect = workspace
            .floats
            .iter()
            .find(|float| !float.images.is_empty())
            .expect("Mermaid preview float should be present")
            .rect;

        assert!(!handle_mermaid_preview_mouse_wheel(
            &mut session_state,
            Some(&workspace),
            preview_rect.x.saturating_add(1),
            preview_rect.y.saturating_add(1),
            0,
            1,
        ));
        assert_eq!(session_state.mermaid_preview_pan(), (0, 0));

        assert!(focus_mermaid_preview_from_mouse_click(
            &mut session_state,
            Some(&workspace),
            preview_rect.x.saturating_add(1),
            preview_rect.y.saturating_add(1),
        ));
        assert!(handle_mermaid_preview_mouse_wheel(
            &mut session_state,
            Some(&workspace),
            preview_rect.x.saturating_add(1),
            preview_rect.y.saturating_add(1),
            1,
            2,
        ));
        assert_eq!(session_state.mermaid_preview_pan(), (96, 192));
    }

    #[test]
    fn workspace_render_ignores_saya_float_demo_env() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = std::env::var_os("SAYA_FLOAT_DEMO");
        unsafe {
            std::env::set_var("SAYA_FLOAT_DEMO", "1");
        }
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        let mut viewport_store = WindowViewportStore::new();
        let mut search_refresh_store = WindowSearchRefreshStore::default();
        let mut markdown_metadata_cache = MarkdownMetadataCache::default();
        let mut floating_window_manager = FloatingWindowManager::default();

        let workspace = build_workspace_render_output(
            &mut outcome,
            &mut session_state,
            &mut viewport_store,
            ViewportSyncMode::Core,
            &mut search_refresh_store,
            &mut markdown_metadata_cache,
            None,
            "",
            0,
            None,
            None,
            None,
            None,
            None,
            80,
            24,
            Some(&mut floating_window_manager),
            None,
            None,
            None,
        )
        .expect("workspace should render");

        match previous {
            Some(value) => unsafe {
                std::env::set_var("SAYA_FLOAT_DEMO", value);
            },
            None => unsafe {
                std::env::remove_var("SAYA_FLOAT_DEMO");
            },
        }
        assert!(
            workspace.floats.is_empty(),
            "production render must not inject demo floats from SAYA_FLOAT_DEMO"
        );
        assert!(floating_window_manager.is_empty());
    }

    #[test]
    fn workspace_render_highlights_substitute_matches_before_commit() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let target_path = unique_path("substitute-preview").with_extension("txt");
        std::fs::write(&target_path, "foo foo\nbar foo\n").expect("test file");
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::File(target_path.clone()),
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        let mut viewport_store = WindowViewportStore::new();
        let mut search_refresh_store = WindowSearchRefreshStore::default();
        let mut markdown_metadata_cache = MarkdownMetadataCache::default();

        let workspace = build_workspace_render_output(
            &mut outcome,
            &mut session_state,
            &mut viewport_store,
            ViewportSyncMode::Core,
            &mut search_refresh_store,
            &mut markdown_metadata_cache,
            Some(':'),
            "%s/foo/baz",
            "%s/foo/baz".len(),
            None,
            None,
            None,
            None,
            None,
            80,
            24,
            None,
            None,
            None,
            None,
        )
        .expect("workspace should render");

        let active_pane = workspace
            .panes
            .iter()
            .find(|pane| pane.window_id == workspace.active_window_id)
            .expect("active pane should exist");
        assert_eq!(
            active_pane.search_overlays.len(),
            2,
            "substitute live preview should highlight each line's first replacement before Enter"
        );
        assert!(
            active_pane
                .lines
                .iter()
                .any(|line| line.contains("baz foo")),
            "substitute live preview should render the first visible line with replacement text: {:?}",
            active_pane.lines
        );
        assert!(
            active_pane
                .lines
                .iter()
                .any(|line| line.contains("bar baz")),
            "substitute live preview should render the second visible line with replacement text: {:?}",
            active_pane.lines
        );
        assert_eq!(
            outcome.core_bridge.snapshot().text,
            "foo foo\nbar foo\n",
            "substitute preview must not mutate the buffer before command commit"
        );
        std::fs::remove_file(target_path).expect("test file should be removed");
    }

    #[test]
    fn generic_floating_window_ignored_key_does_not_consume_colon_command_prompt() {
        let mut manager = FloatingWindowManager::default();
        let id = manager.open_static_lines(
            vec!["demo".to_string()],
            FloatingPlacement::editor_at(1, 1),
            FloatingSize {
                width: 10,
                height: 3,
            },
            FloatingChrome::borderless(),
            FloatingZIndex::Hover,
            true,
        );
        assert!(manager.focus_float(id));

        let handling = handle_floating_window_key(&mut manager, &KeyInput::Char(':'), 1);

        assert_eq!(handling, None);
        assert_eq!(
            manager.focus(),
            Some(saya::presentation::floating_window::WorkspaceFocus::Float { float_id: id }),
            "ignored keys must leave float focus unchanged but pass through to later handlers"
        );
    }

    #[test]
    fn floating_window_key_close_restores_active_pane_focus() {
        let mut manager = FloatingWindowManager::default();
        let id = manager.open_static_lines(
            vec!["demo".to_string()],
            FloatingPlacement::editor_at(1, 1),
            FloatingSize {
                width: 10,
                height: 3,
            },
            FloatingChrome::borderless(),
            FloatingZIndex::Hover,
            true,
        );
        assert!(manager.focus_float(id));

        let handling = handle_floating_window_key(&mut manager, &KeyInput::Escape, 9);

        assert_eq!(handling, Some(FloatingWindowKeyHandling::Closed { id }));
        assert_eq!(
            manager.focus(),
            Some(saya::presentation::floating_window::WorkspaceFocus::Pane { window_id: 9 }),
            "closed float should restore the active pane focus through the main routing helper"
        );
    }

    #[test]
    fn floating_window_mouse_focus_helper_focuses_float_before_core_mouse_dispatch() {
        let workspace = main_test_workspace();
        let mut manager = FloatingWindowManager::default();
        let id = manager.open_static_lines(
            vec!["demo".to_string()],
            FloatingPlacement::editor_at(1, 1),
            FloatingSize {
                width: 10,
                height: 3,
            },
            FloatingChrome::borderless(),
            FloatingZIndex::Hover,
            true,
        );

        let outcome =
            focus_floating_window_from_mouse_click(&mut manager, Some(&workspace), 2, 2, 80, 24);

        assert_eq!(outcome, FloatingMouseOutcome::Focused { id });
        assert_eq!(
            manager.focus(),
            Some(saya::presentation::floating_window::WorkspaceFocus::Float { float_id: id })
        );
    }

    #[test]
    fn floating_window_mouse_focus_helper_passes_through_non_mouse_float() {
        let workspace = main_test_workspace();
        let mut manager = FloatingWindowManager::default();
        let id = manager.open_static_lines(
            vec!["demo".to_string()],
            FloatingPlacement::editor_at(1, 1),
            FloatingSize {
                width: 10,
                height: 3,
            },
            FloatingChrome::borderless(),
            FloatingZIndex::Hover,
            true,
        );
        assert!(manager.set_mouse_enabled(id, false));

        let outcome =
            focus_floating_window_from_mouse_click(&mut manager, Some(&workspace), 2, 2, 80, 24);

        assert_eq!(outcome, FloatingMouseOutcome::PassThrough);
        assert_eq!(manager.focus(), None);
    }

    #[test]
    fn viewport_sync_mode_uses_smooth_line_motion_only_for_single_line_vertical_inputs() {
        assert_eq!(
            viewport_sync_mode_for_input(&KeyInput::Char('j')),
            ViewportSyncMode::SmoothLineMotion
        );
        assert_eq!(
            viewport_sync_mode_for_input(&KeyInput::Char('k')),
            ViewportSyncMode::SmoothLineMotion
        );
        assert_eq!(
            viewport_sync_mode_for_input(&KeyInput::Down),
            ViewportSyncMode::SmoothLineMotion
        );
        assert_eq!(
            viewport_sync_mode_for_input(&KeyInput::Up),
            ViewportSyncMode::SmoothLineMotion
        );

        assert_eq!(
            viewport_sync_mode_for_input(&KeyInput::PageDown),
            ViewportSyncMode::Core
        );
        assert_eq!(
            viewport_sync_mode_for_input(&KeyInput::PageUp),
            ViewportSyncMode::Core
        );
        assert_eq!(
            viewport_sync_mode_for_input(&KeyInput::Ctrl('f')),
            ViewportSyncMode::Core
        );
        assert_eq!(
            viewport_sync_mode_for_input(&KeyInput::Char('H')),
            ViewportSyncMode::Core
        );
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
            panes: vec![saya::presentation::screen_model::ScreenModel {
                rect: saya::presentation::screen_model::PaneRect {
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
    fn markdown_metadata_collection_is_skipped_when_markdown_render_is_disabled() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let target_path = unique_path("markdown-render-off").with_extension("md");
        std::fs::write(&target_path, "# Title\n").expect("test markdown file");
        let outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::File(target_path.clone()),
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        session_state
            .apply_presentation_option(
                saya::runtime::options::SayaOptionName::MarkdownRender,
                saya::runtime::options::SayaOptionValue::Boolean(false),
            )
            .expect("markdownrender option should apply");
        let mut markdown_metadata_cache = MarkdownMetadataCache::default();

        let maps = collect_workspace_markdown_document_maps(
            &mut markdown_metadata_cache,
            &session_state,
            &outcome.core_bridge,
            &outcome.core_bridge.snapshot(),
        );

        assert!(
            maps.is_empty(),
            "raw Markdown mode should not collect render metadata"
        );
        std::fs::remove_file(&target_path).expect("test markdown file should be removed");
    }

    #[cfg(feature = "tree-sitter-syntax")]
    #[test]
    fn workspace_render_collects_tree_sitter_highlight_only_when_vim_syntax_is_on() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let target_path = unique_path("syntax-toggle-main").with_extension("rs");
        let config_path = unique_path("syntax-toggle-empty-init").with_extension("ts");
        std::fs::write(&target_path, "fn main() {}\n").expect("test source file");
        std::fs::write(&config_path, "").expect("empty test config file");
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::File(target_path.clone()),
            config_source: saya::app::cli::ConfigSource::File(config_path.clone()),
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        outcome.core_bridge.set_screen_size(24, 80);
        let mut session_state = outcome.editor_session_state();
        let mut viewport_store = WindowViewportStore::new();
        let mut search_refresh_store = WindowSearchRefreshStore::default();
        let mut markdown_metadata_cache = MarkdownMetadataCache::default();

        let syntax_off_workspace = build_workspace_render_output(
            &mut outcome,
            &mut session_state,
            &mut viewport_store,
            ViewportSyncMode::Core,
            &mut search_refresh_store,
            &mut markdown_metadata_cache,
            None,
            "",
            0,
            None,
            None,
            None,
            None,
            None,
            80,
            24,
            None,
            None,
            None,
            None,
        )
        .expect("syntax-off workspace should render");
        assert!(
            syntax_off_workspace
                .panes
                .iter()
                .all(|pane| pane.syntax_chunks.is_empty()),
            "syntax off should skip Vim and Tree-sitter highlight collection"
        );

        outcome
            .core_bridge
            .apply_ex_command("syntax on")
            .expect("syntax on should enable highlight collection");
        assert!(
            outcome.core_bridge.is_syntax_enabled(),
            "syntax on should be visible before workspace render"
        );
        let snapshot = outcome.core_bridge.snapshot();
        assert!(
            snapshot
                .buffers
                .iter()
                .any(|buffer| buffer.name.ends_with(".rs")),
            "Rust source buffer name should be available for Tree-sitter language resolution: {:?}",
            snapshot.buffers
        );
        let mut syntax_on_workspace = None;
        for _ in 0..20 {
            let workspace = build_workspace_render_output(
                &mut outcome,
                &mut session_state,
                &mut viewport_store,
                ViewportSyncMode::Core,
                &mut search_refresh_store,
                &mut markdown_metadata_cache,
                None,
                "",
                0,
                None,
                None,
                None,
                None,
                None,
                80,
                24,
                None,
                None,
                None,
                None,
            )
            .expect("syntax-on workspace should render");
            if workspace_has_syntax_chunks(&workspace) {
                syntax_on_workspace = Some(workspace);
                break;
            }
            syntax_on_workspace = Some(workspace);
            std::thread::sleep(Duration::from_millis(10));
        }
        let syntax_on_workspace =
            syntax_on_workspace.expect("syntax-on workspace should render at least once");
        assert!(
            workspace_has_syntax_chunks(&syntax_on_workspace),
            "syntax on should allow Tree-sitter highlight chunks for Rust source"
        );
        std::fs::remove_file(&target_path).expect("test source file should be removed");
        std::fs::remove_file(&config_path).expect("test config file should be removed");
    }

    #[cfg(feature = "tree-sitter-syntax")]
    fn workspace_has_syntax_chunks(workspace: &WorkspaceScreenModel) -> bool {
        workspace
            .panes
            .iter()
            .any(|pane| !pane.syntax_chunks.is_empty())
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
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let target_path = unique_path("write-failure");
        std::fs::write(&target_path, "initial\n").expect("test file");

        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::File(target_path.clone()),
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let bad_path = PathBuf::from("/nonexistent/dir/file.txt");
        let mut session_state = saya::app::session::EditorSessionState::new(Some(bad_path));

        outcome.core_bridge.dispatch_key("i").unwrap();
        outcome.core_bridge.dispatch_key("X").unwrap();
        outcome.core_bridge.dispatch_key("\x1b").unwrap();
        sync_session_dirty_from_core(&mut session_state, &outcome.core_bridge);

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
    fn write_host_action_creates_missing_named_file_without_quit_warning() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let target_path = unique_path("write-missing-named-file.md");
        assert!(
            !target_path.exists(),
            "test starts with a nonexistent target"
        );

        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::File(target_path.clone()),
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();

        outcome.core_bridge.dispatch_key("i").unwrap();
        outcome.core_bridge.dispatch_key("hello").unwrap();
        outcome.core_bridge.dispatch_key("\x1b").unwrap();
        sync_session_dirty_from_core(&mut session_state, &outcome.core_bridge);

        let mut outcome_accumulator = MainOutcomeAccumulator::default();
        let mut transient_msg = None;
        let mut system_warning = None;
        let mut need_redraw = false;
        let mut host_action_runtime = HostActionRuntime::default();

        outcome
            .core_bridge
            .apply_ex_command(":q")
            .expect(":q command should succeed");
        consume_core_outcomes_from_core(
            &mut outcome.core_bridge,
            &mut outcome_accumulator,
            &mut need_redraw,
        );
        let quit_shutdown = process_pending_host_actions_without_runtime(
            &mut outcome,
            &mut outcome_accumulator,
            &mut session_state,
            &mut transient_msg,
            &mut system_warning,
            &mut host_action_runtime,
        );
        assert_eq!(quit_shutdown, None);
        assert_eq!(
            system_warning,
            Some(normal_quit_warning_message().to_string())
        );

        outcome
            .core_bridge
            .apply_ex_command(":w")
            .expect(":w command should succeed");
        consume_core_outcomes_from_core(
            &mut outcome.core_bridge,
            &mut outcome_accumulator,
            &mut need_redraw,
        );
        let write_shutdown = process_pending_host_actions_without_runtime(
            &mut outcome,
            &mut outcome_accumulator,
            &mut session_state,
            &mut transient_msg,
            &mut system_warning,
            &mut host_action_runtime,
        );

        assert_eq!(write_shutdown, None);
        assert_eq!(system_warning, None);
        assert_eq!(transient_msg, Some("Saved successfully".to_string()));
        assert_eq!(
            std::fs::read_to_string(&target_path).expect("missing target should be created"),
            outcome.core_bridge.snapshot().text
        );
        assert!(!session_state.is_dirty());

        std::fs::remove_file(&target_path).expect("cleanup");
    }

    #[test]
    fn directory_buffer_write_prepares_operation_preview_without_filesystem_mutation() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("directory-write-plan");
        let alpha_path = root_path.join("alpha.md");
        let beta_path = root_path.join("beta.md");
        let renamed_path = root_path.join("renamed.md");
        std::fs::create_dir_all(&root_path).expect("test directory");
        std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
        std::fs::write(&beta_path, "beta\n").expect("beta file");
        let mut session_state =
            saya::app::session::EditorSessionState::new(Some(root_path.clone()));

        let save_outcome = save_snapshot_result("renamed.md\nbeta.md\n", &mut session_state);

        let preview = session_state
            .pending_directory_operation_preview()
            .expect("plain write should prepare a preview");
        assert_eq!(
            save_outcome,
            SaveSnapshotOutcome {
                transient_message: Some(format!(
                    "Apply 1 dired operation(s) (0 high-risk)? y/Enter=OK n/Esc=Cancel id={}",
                    preview.id
                )),
                wrote: false,
                pending_directory_confirmation: true,
            }
        );
        assert!(alpha_path.exists(), "rename must not be applied in phase 7");
        assert!(!renamed_path.exists(), "rename target is only planned");
        assert!(beta_path.exists());

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }

    #[test]
    fn directory_buffer_write_reports_validation_error_without_filesystem_mutation() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("directory-write-validation");
        let alpha_path = root_path.join("alpha.md");
        std::fs::create_dir_all(&root_path).expect("test directory");
        std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
        let mut session_state =
            saya::app::session::EditorSessionState::new(Some(root_path.clone()));

        let save_outcome =
            save_snapshot_result("alpha.md\n\n../escape.md\nalpha.md\n", &mut session_state);

        assert_eq!(
            save_outcome,
            SaveSnapshotOutcome {
                transient_message: Some(
                    "Directory operation plan failed validation: 4 error(s)".to_string()
                ),
                wrote: false,
                pending_directory_confirmation: false,
            }
        );
        assert!(
            alpha_path.exists(),
            "invalid writable directory edits must not mutate the filesystem"
        );

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }

    #[test]
    fn directory_buffer_plain_write_previews_delete_without_filesystem_mutation() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("directory-write-preview-delete");
        let alpha_path = root_path.join("alpha.md");
        let beta_path = root_path.join("beta.md");
        std::fs::create_dir_all(&root_path).expect("test directory");
        std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
        std::fs::write(&beta_path, "beta\n").expect("beta file");
        let mut session_state =
            saya::app::session::EditorSessionState::new(Some(root_path.clone()));

        let save_outcome = save_snapshot_result("beta.md\n", &mut session_state);
        let preview = session_state
            .pending_directory_operation_preview()
            .expect("plain write should prepare a pending preview");

        assert_eq!(preview.operation_count, 1);
        assert_eq!(preview.high_risk_count, 1);
        assert_eq!(
            save_outcome.transient_message,
            Some(format!(
                "Apply 1 dired operation(s) (1 high-risk)? y/Enter=OK n/Esc=Cancel id={}",
                preview.id
            ))
        );
        assert!(!save_outcome.wrote);
        assert!(
            session_state.directory_operation_confirmation_dialog_active(),
            "plain :write should open the confirmation dialog"
        );
        assert!(alpha_path.exists(), "plain :write must not delete files");
        assert!(beta_path.exists());

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }

    #[test]
    fn directory_buffer_write_confirmation_ok_applies_delete_and_refreshes_metadata() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("directory-write-dialog-ok");
        let alpha_path = root_path.join("alpha.md");
        let beta_path = root_path.join("beta.md");
        std::fs::create_dir_all(&root_path).expect("test directory");
        std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
        std::fs::write(&beta_path, "beta\n").expect("beta file");
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        execute_runtime_host_command(
            &format!("edit {}", root_path.display()),
            &mut outcome,
            &mut session_state,
        )
        .expect("open directory listing");
        outcome
            .core_bridge
            .dispatch_key("dd")
            .expect("delete current listing line");

        let preview_effect =
            execute_runtime_host_command("write", &mut outcome, &mut session_state)
                .expect("plain write should prepare confirmation dialog");
        assert!(
            preview_effect
                .transient_message
                .as_deref()
                .is_some_and(|message| message.contains("y/Enter=OK n/Esc=Cancel")),
            "plain write should ask for confirmation: {:?}",
            preview_effect.transient_message
        );
        assert!(alpha_path.exists());

        let mut transient_msg = None;
        let mut need_redraw = false;
        let applied = handle_directory_operation_confirmation_key_without_runtime(
            &KeyInput::Char('y'),
            &mut outcome,
            &mut session_state,
            &mut transient_msg,
            &mut need_redraw,
        );

        assert_eq!(
            applied,
            Some(None),
            "y should confirm the pending directory write without requesting shutdown"
        );
        assert_eq!(
            transient_msg,
            Some("Directory operations applied: 1 operation(s)".to_string())
        );
        assert!(need_redraw);
        assert!(!alpha_path.exists(), "OK should delete alpha");
        assert!(beta_path.exists());
        assert_eq!(outcome.core_bridge.snapshot().text, "beta.md\n");
        assert!(
            !session_state.directory_operation_confirmation_dialog_active(),
            "confirmed dialog should be cleared"
        );

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }

    #[test]
    fn directory_buffer_write_confirmation_cancel_keeps_filesystem_unchanged() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("directory-write-dialog-cancel");
        let alpha_path = root_path.join("alpha.md");
        let beta_path = root_path.join("beta.md");
        std::fs::create_dir_all(&root_path).expect("test directory");
        std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
        std::fs::write(&beta_path, "beta\n").expect("beta file");
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        execute_runtime_host_command(
            &format!("edit {}", root_path.display()),
            &mut outcome,
            &mut session_state,
        )
        .expect("open directory listing");
        outcome
            .core_bridge
            .dispatch_key("dd")
            .expect("delete current listing line");
        execute_runtime_host_command("write", &mut outcome, &mut session_state)
            .expect("plain write should prepare confirmation dialog");

        let mut transient_msg = None;
        let mut need_redraw = false;
        let cancelled = handle_directory_operation_confirmation_key_without_runtime(
            &KeyInput::Escape,
            &mut outcome,
            &mut session_state,
            &mut transient_msg,
            &mut need_redraw,
        );

        assert_eq!(
            cancelled,
            Some(None),
            "Esc should cancel the pending directory write without requesting shutdown"
        );
        assert_eq!(
            transient_msg,
            Some("Directory operation cancelled; no filesystem changes were applied".to_string())
        );
        assert!(need_redraw);
        assert!(alpha_path.exists(), "cancel must not delete alpha");
        assert!(beta_path.exists());
        assert!(
            session_state
                .pending_directory_operation_preview()
                .is_none()
        );

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }

    #[test]
    fn directory_buffer_force_write_applies_latest_preview_and_refreshes_metadata() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("directory-write-apply");
        let alpha_path = root_path.join("alpha.md");
        let beta_path = root_path.join("beta.md");
        let renamed_path = root_path.join("renamed.md");
        let created_path = root_path.join("notes.md");
        let created_dir_path = root_path.join("src");
        std::fs::create_dir_all(&root_path).expect("test directory");
        std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
        std::fs::write(&beta_path, "beta\n").expect("beta file");
        let mut session_state =
            saya::app::session::EditorSessionState::new(Some(root_path.clone()));

        let edited_text = "renamed.md\nnotes.md\nsrc/\n";
        let preview_outcome = save_snapshot_result(edited_text, &mut session_state);
        assert!(!preview_outcome.wrote);

        let apply_outcome = save_snapshot_result_with_confirmation(
            edited_text,
            &mut session_state,
            None,
            true,
            None,
        );

        assert_eq!(
            apply_outcome.transient_message,
            Some("Directory operations applied: 3 operation(s)".to_string())
        );
        assert!(apply_outcome.wrote);
        assert!(!alpha_path.exists(), "rename source should be moved");
        assert!(renamed_path.is_file(), "rename target should exist");
        assert!(created_path.is_file(), "create file plan should be applied");
        assert!(
            created_dir_path.is_dir(),
            "create directory plan should be applied"
        );
        assert!(
            session_state
                .pending_directory_operation_preview()
                .is_none(),
            "successful apply should clear the pending preview"
        );
        let entries = session_state
            .directory_buffer()
            .expect("directory metadata should remain active")
            .entries
            .iter()
            .map(|entry| entry.display_text.clone())
            .collect::<Vec<_>>();
        assert!(entries.contains(&"renamed.md".to_string()));
        assert!(entries.contains(&"notes.md".to_string()));
        assert!(entries.contains(&"src/".to_string()));

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }

    #[test]
    fn directory_buffer_force_write_applies_confirmed_delete_and_refreshes_metadata() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("directory-write-apply-delete");
        let alpha_path = root_path.join("alpha.md");
        let beta_path = root_path.join("beta.md");
        std::fs::create_dir_all(&root_path).expect("test directory");
        std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
        std::fs::write(&beta_path, "beta\n").expect("beta file");
        let mut session_state =
            saya::app::session::EditorSessionState::new(Some(root_path.clone()));

        let preview_outcome = save_snapshot_result("beta.md\n", &mut session_state);
        let preview = session_state
            .pending_directory_operation_preview()
            .expect("plain write should prepare delete preview");
        assert!(!preview_outcome.wrote);
        assert_eq!(preview.operation_count, 1);
        assert_eq!(preview.high_risk_count, 1);
        assert!(alpha_path.exists());

        let apply_outcome = save_snapshot_result_with_confirmation(
            "beta.md\n",
            &mut session_state,
            None,
            true,
            None,
        );

        assert_eq!(
            apply_outcome.transient_message,
            Some("Directory operations applied: 1 operation(s)".to_string())
        );
        assert!(apply_outcome.wrote);
        assert!(!alpha_path.exists(), "confirmed delete should remove alpha");
        assert!(beta_path.is_file());
        let entries = session_state
            .directory_buffer()
            .expect("directory metadata should refresh after delete")
            .entries
            .iter()
            .map(|entry| entry.display_text.clone())
            .collect::<Vec<_>>();
        assert_eq!(entries, vec!["beta.md".to_string()]);

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }

    #[test]
    fn directory_buffer_write_host_action_previews_confirms_deletes_and_refreshes_listing() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("directory-write-host-action-delete");
        let alpha_path = root_path.join("alpha.md");
        let beta_path = root_path.join("beta.md");
        let gamma_path = root_path.join("gamma.md");
        std::fs::create_dir_all(&root_path).expect("test directory");
        std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
        std::fs::write(&beta_path, "beta\n").expect("beta file");
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        execute_runtime_host_command(
            &format!("edit {}", root_path.display()),
            &mut outcome,
            &mut session_state,
        )
        .expect("open directory listing");
        assert_eq!(outcome.core_bridge.snapshot().text, "alpha.md\nbeta.md\n");

        outcome
            .core_bridge
            .dispatch_key("dd")
            .expect("delete current listing line");
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
        outcome
            .core_bridge
            .apply_ex_command(":write")
            .expect(":write should produce a preview host action");
        consume_core_outcomes_from_core(
            &mut outcome.core_bridge,
            &mut outcome_accumulator,
            &mut need_redraw,
        );
        assert!(
            !outcome_accumulator.host_directives.is_empty(),
            ":write should emit a host directive before preview processing"
        );

        let preview_shutdown = process_pending_host_actions_without_runtime(
            &mut outcome,
            &mut outcome_accumulator,
            &mut session_state,
            &mut transient_msg,
            &mut system_warning,
            &mut host_action_runtime,
        );
        let preview = session_state
            .pending_directory_operation_preview()
            .unwrap_or_else(|| {
                panic!(
                    "plain :write should prepare a directory operation preview; transient={transient_msg:?}, target={:?}, directory_buffer_present={}",
                    session_state.target_path(),
                    session_state.directory_buffer().is_some()
                )
            });
        assert_eq!(preview_shutdown, None);
        assert_eq!(preview.operation_count, 1);
        assert_eq!(preview.high_risk_count, 1);
        assert!(
            transient_msg
                .as_deref()
                .is_some_and(|message| message.starts_with("Apply 1 dired operation")),
            "plain :write should report the pending preview: {transient_msg:?}"
        );
        assert!(
            alpha_path.exists(),
            "unconfirmed preview must not delete files"
        );
        std::fs::write(&gamma_path, "gamma\n").expect("external file before confirmed apply");

        outcome
            .core_bridge
            .apply_ex_command(":write!")
            .expect(":write! should produce a confirmed host action");
        consume_core_outcomes_from_core(
            &mut outcome.core_bridge,
            &mut outcome_accumulator,
            &mut need_redraw,
        );
        let apply_shutdown = process_pending_host_actions_without_runtime(
            &mut outcome,
            &mut outcome_accumulator,
            &mut session_state,
            &mut transient_msg,
            &mut system_warning,
            &mut host_action_runtime,
        );

        assert_eq!(apply_shutdown, None);
        assert_eq!(
            transient_msg,
            Some("Directory operations applied: 1 operation(s)".to_string())
        );
        assert!(
            !alpha_path.exists(),
            "confirmed :write! should delete alpha"
        );
        assert!(beta_path.exists());
        assert!(gamma_path.exists());
        assert_eq!(outcome.core_bridge.snapshot().text, "beta.md\ngamma.md\n");
        assert!(
            session_state
                .pending_directory_operation_preview()
                .is_none(),
            "confirmed apply should clear the pending preview"
        );

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }

    #[test]
    fn directory_buffer_wq_waits_for_preview_confirmation_then_quits() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("directory-wq-delete");
        let alpha_path = root_path.join("alpha.md");
        let beta_path = root_path.join("beta.md");
        std::fs::create_dir_all(&root_path).expect("test directory");
        std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
        std::fs::write(&beta_path, "beta\n").expect("beta file");
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        execute_runtime_host_command(
            &format!("edit {}", root_path.display()),
            &mut outcome,
            &mut session_state,
        )
        .expect("open directory listing");

        outcome
            .core_bridge
            .dispatch_key("dd")
            .expect("delete current listing line");
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
        sync_session_dirty_from_core(&mut session_state, &outcome.core_bridge);

        outcome
            .core_bridge
            .apply_ex_command(":wq")
            .expect(":wq should produce write then quit host actions");
        consume_core_outcomes_from_core(
            &mut outcome.core_bridge,
            &mut outcome_accumulator,
            &mut need_redraw,
        );
        let preview_shutdown = process_pending_host_actions_without_runtime(
            &mut outcome,
            &mut outcome_accumulator,
            &mut session_state,
            &mut transient_msg,
            &mut system_warning,
            &mut host_action_runtime,
        );

        assert_eq!(
            preview_shutdown, None,
            ":wq should wait for directory operation confirmation"
        );
        assert!(
            alpha_path.exists(),
            "unconfirmed preview must not delete files"
        );
        assert!(
            session_state
                .pending_directory_operation_preview()
                .is_some(),
            ":wq should keep a pending dired preview"
        );

        let handled = handle_directory_operation_confirmation_key_without_runtime(
            &KeyInput::Enter,
            &mut outcome,
            &mut session_state,
            &mut transient_msg,
            &mut need_redraw,
        );

        assert_eq!(
            handled,
            Some(Some(ShutdownReason::UserQuit)),
            "confirmation key should apply and resume the pending quit"
        );
        assert!(
            !alpha_path.exists(),
            "confirmed :wq should apply the dired operation"
        );
        assert!(beta_path.exists());
        assert_eq!(outcome.core_bridge.snapshot().text, "beta.md\n");

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }

    #[test]
    fn directory_buffer_force_write_rejects_stale_preview_without_mutation() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("directory-write-stale-preview");
        let alpha_path = root_path.join("alpha.md");
        let beta_path = root_path.join("beta.md");
        std::fs::create_dir_all(&root_path).expect("test directory");
        std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
        std::fs::write(&beta_path, "beta\n").expect("beta file");
        let mut session_state =
            saya::app::session::EditorSessionState::new(Some(root_path.clone()));

        let preview_outcome = save_snapshot_result("beta.md\n", &mut session_state);
        assert!(!preview_outcome.wrote);

        let apply_outcome = save_snapshot_result_with_confirmation(
            "alpha.md\n",
            &mut session_state,
            None,
            true,
            None,
        );

        assert_eq!(
            apply_outcome.transient_message,
            Some(
                "Directory operation preview is stale; run :write again before :write!".to_string()
            )
        );
        assert!(!apply_outcome.wrote);
        assert!(alpha_path.exists(), "stale preview must not delete files");
        assert!(beta_path.exists());

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }

    #[test]
    fn directory_buffer_cancel_command_clears_preview_without_mutation() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("directory-write-cancel-preview");
        let alpha_path = root_path.join("alpha.md");
        let beta_path = root_path.join("beta.md");
        std::fs::create_dir_all(&root_path).expect("test directory");
        std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
        std::fs::write(&beta_path, "beta\n").expect("beta file");
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        execute_runtime_host_command(
            &format!("edit {}", root_path.display()),
            &mut outcome,
            &mut session_state,
        )
        .expect("open directory listing");
        outcome
            .core_bridge
            .dispatch_key("dd")
            .expect("delete current listing line");

        let preview_effect =
            execute_runtime_host_command("write", &mut outcome, &mut session_state)
                .expect("plain write should prepare preview");
        let preview_id = session_state
            .pending_directory_operation_preview()
            .expect("preview should be pending")
            .id
            .clone();
        assert!(
            preview_effect
                .transient_message
                .as_deref()
                .is_some_and(|message| message.contains("y/Enter=OK n/Esc=Cancel")),
            "preview message should show the cancel command: {:?}",
            preview_effect.transient_message
        );

        let cancel_effect =
            execute_runtime_host_command("dired-cancel", &mut outcome, &mut session_state)
                .expect("cancel command should be host-handled");

        assert_eq!(
            cancel_effect.transient_message,
            Some(format!(
                "Directory operation preview cancelled: 1 operation(s), preview_id={preview_id}"
            ))
        );
        assert!(
            session_state
                .pending_directory_operation_preview()
                .is_none()
        );
        assert!(alpha_path.exists(), "cancel must not delete alpha");
        assert!(beta_path.exists());

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }

    #[test]
    fn directory_buffer_transaction_applies_multiple_renames_without_collision() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("directory-transaction-rename-collision");
        let alpha_path = root_path.join("alpha.md");
        let beta_path = root_path.join("beta.md");
        let gamma_path = root_path.join("gamma.md");
        std::fs::create_dir_all(&root_path).expect("test directory");
        std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
        std::fs::write(&beta_path, "beta\n").expect("beta file");
        let mut session_state =
            saya::app::session::EditorSessionState::new(Some(root_path.clone()));
        let plan = saya::app::session::DirectoryBufferOperationPlan {
            root_path: root_path.clone(),
            operations: vec![
                DirectoryBufferPlannedOperation::Rename {
                    from: alpha_path.clone(),
                    to: beta_path.clone(),
                    from_name: "alpha.md".to_string(),
                    to_name: "beta.md".to_string(),
                    kind: saya::app::session::DirectoryBufferEntryKind::File,
                },
                DirectoryBufferPlannedOperation::Rename {
                    from: beta_path.clone(),
                    to: gamma_path.clone(),
                    from_name: "beta.md".to_string(),
                    to_name: "gamma.md".to_string(),
                    kind: saya::app::session::DirectoryBufferEntryKind::File,
                },
            ],
        };

        let applied_count = apply_directory_buffer_operation_plan(&mut session_state, &plan)
            .expect("transaction should avoid rename target collisions");

        assert_eq!(applied_count, 2);
        assert!(!alpha_path.exists());
        assert_eq!(
            std::fs::read_to_string(&beta_path).expect("beta target"),
            "alpha\n"
        );
        assert_eq!(
            std::fs::read_to_string(&gamma_path).expect("gamma target"),
            "beta\n"
        );
        let entries = session_state
            .directory_buffer()
            .expect("directory metadata should refresh after transaction")
            .entries
            .iter()
            .map(|entry| entry.display_text.clone())
            .collect::<Vec<_>>();
        assert_eq!(entries, vec!["beta.md".to_string(), "gamma.md".to_string()]);

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }

    #[test]
    fn directory_buffer_transaction_reports_partial_failure_and_refreshes_metadata() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("directory-transaction-partial-failure");
        let created_path = root_path.join("created.md");
        let non_empty_dir = root_path.join("non-empty");
        std::fs::create_dir_all(&non_empty_dir).expect("test directory");
        std::fs::write(non_empty_dir.join("child.md"), "child\n").expect("child file");
        let mut session_state =
            saya::app::session::EditorSessionState::new(Some(root_path.clone()));
        let plan = saya::app::session::DirectoryBufferOperationPlan {
            root_path: root_path.clone(),
            operations: vec![
                DirectoryBufferPlannedOperation::CreateFile {
                    path: created_path.clone(),
                    name: "created.md".to_string(),
                },
                DirectoryBufferPlannedOperation::Delete {
                    path: non_empty_dir.clone(),
                    name: "non-empty".to_string(),
                    kind: saya::app::session::DirectoryBufferEntryKind::Directory,
                },
            ],
        };

        let error = apply_directory_buffer_operation_plan(&mut session_state, &plan)
            .expect_err("non-empty directory delete should report a partial failure");

        match error {
            RuntimeFilerError::OperationFailed { message, .. } => {
                assert!(
                    message.contains("successful=1")
                        && message.contains("failed=1")
                        && message.contains("manual_recovery_required=1"),
                    "partial failure report should include structured counts: {message}"
                );
            }
            other => panic!("unexpected error: {other:?}"),
        }
        assert!(
            created_path.is_file(),
            "successful operation should be left in place and reported"
        );
        assert!(
            non_empty_dir.is_dir(),
            "failed delete should leave the original directory"
        );
        let entries = session_state
            .directory_buffer()
            .expect("directory metadata should refresh even after partial failure")
            .entries
            .iter()
            .map(|entry| entry.display_text.clone())
            .collect::<Vec<_>>();
        assert!(
            entries.contains(&"created.md".to_string())
                && entries.contains(&"non-empty/".to_string()),
            "refreshed metadata should reflect the real filesystem: {entries:?}"
        );

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }

    #[test]
    fn directory_buffer_confirm_failure_reports_recovery_hint_and_keeps_real_listing() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("directory-write-recovery-hint");
        let non_empty_dir = root_path.join("non-empty");
        std::fs::create_dir_all(&non_empty_dir).expect("test directory");
        std::fs::write(non_empty_dir.join("child.md"), "child\n").expect("child file");
        let mut session_state =
            saya::app::session::EditorSessionState::new(Some(root_path.clone()));

        let preview_outcome = save_snapshot_result("", &mut session_state);
        assert!(!preview_outcome.wrote);

        let apply_outcome =
            save_snapshot_result_with_confirmation("", &mut session_state, None, true, None);

        let message = apply_outcome
            .transient_message
            .expect("failed directory apply should report a message");
        assert!(
            message.contains("Directory operation apply failed")
                && message.contains("Recovery:")
                && message.contains("inspect the listing before retrying"),
            "failed apply should include a recovery hint: {message}"
        );
        assert!(!apply_outcome.wrote);
        assert!(
            non_empty_dir.is_dir(),
            "failed delete must not remove directory"
        );
        let entries = session_state
            .directory_buffer()
            .expect("directory metadata should remain active")
            .entries
            .iter()
            .map(|entry| entry.display_text.clone())
            .collect::<Vec<_>>();
        assert_eq!(entries, vec!["non-empty/".to_string()]);

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }

    #[test]
    fn directory_buffer_transaction_rejects_existing_create_target_before_mutation() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("directory-transaction-create-conflict");
        let existing_path = root_path.join("existing.md");
        let later_path = root_path.join("later.md");
        std::fs::create_dir_all(&root_path).expect("test directory");
        std::fs::write(&existing_path, "existing\n").expect("existing file");
        let mut session_state =
            saya::app::session::EditorSessionState::new(Some(root_path.clone()));
        let plan = saya::app::session::DirectoryBufferOperationPlan {
            root_path: root_path.clone(),
            operations: vec![
                DirectoryBufferPlannedOperation::CreateFile {
                    path: existing_path.clone(),
                    name: "existing.md".to_string(),
                },
                DirectoryBufferPlannedOperation::CreateFile {
                    path: later_path.clone(),
                    name: "later.md".to_string(),
                },
            ],
        };

        let error = apply_directory_buffer_operation_plan(&mut session_state, &plan)
            .expect_err("existing create target should be rejected before execution");

        assert!(matches!(
            error,
            RuntimeFilerError::OperationFailed {
                kind: RuntimeFilerErrorKind::AlreadyExists,
                ..
            }
        ));
        assert_eq!(
            std::fs::read_to_string(&existing_path).expect("existing file"),
            "existing\n"
        );
        assert!(
            !later_path.exists(),
            "conflict check must stop before later operations mutate the filesystem"
        );

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }

    #[test]
    fn directory_buffer_transaction_rejects_missing_rename_source_before_mutation() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("directory-transaction-missing-source");
        let missing_path = root_path.join("missing.md");
        let renamed_path = root_path.join("renamed.md");
        let later_path = root_path.join("later.md");
        std::fs::create_dir_all(&root_path).expect("test directory");
        let mut session_state =
            saya::app::session::EditorSessionState::new(Some(root_path.clone()));
        let plan = saya::app::session::DirectoryBufferOperationPlan {
            root_path: root_path.clone(),
            operations: vec![
                DirectoryBufferPlannedOperation::Rename {
                    from: missing_path.clone(),
                    to: renamed_path.clone(),
                    from_name: "missing.md".to_string(),
                    to_name: "renamed.md".to_string(),
                    kind: saya::app::session::DirectoryBufferEntryKind::File,
                },
                DirectoryBufferPlannedOperation::CreateFile {
                    path: later_path.clone(),
                    name: "later.md".to_string(),
                },
            ],
        };

        let error = apply_directory_buffer_operation_plan(&mut session_state, &plan)
            .expect_err("missing rename source should be rejected before execution");

        assert!(matches!(
            error,
            RuntimeFilerError::OperationFailed {
                kind: RuntimeFilerErrorKind::NotFound,
                ..
            }
        ));
        assert!(!renamed_path.exists());
        assert!(
            !later_path.exists(),
            "conflict check must stop before later operations mutate the filesystem"
        );

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }

    #[test]
    fn directory_buffer_transaction_rejects_special_file_entries_before_mutation() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("directory-transaction-special-file");
        let special_path = root_path.join("special");
        let later_path = root_path.join("later.md");
        std::fs::create_dir_all(&root_path).expect("test directory");
        std::fs::write(&special_path, "special\n").expect("special placeholder");
        let mut session_state =
            saya::app::session::EditorSessionState::new(Some(root_path.clone()));
        let plan = saya::app::session::DirectoryBufferOperationPlan {
            root_path: root_path.clone(),
            operations: vec![
                DirectoryBufferPlannedOperation::Delete {
                    path: special_path.clone(),
                    name: "special".to_string(),
                    kind: saya::app::session::DirectoryBufferEntryKind::Other,
                },
                DirectoryBufferPlannedOperation::CreateFile {
                    path: later_path.clone(),
                    name: "later.md".to_string(),
                },
            ],
        };

        let error = apply_directory_buffer_operation_plan(&mut session_state, &plan)
            .expect_err("special entries should be rejected before execution");

        assert!(matches!(
            error,
            RuntimeFilerError::OperationFailed {
                kind: RuntimeFilerErrorKind::Unsupported,
                ..
            }
        ));
        assert!(special_path.is_file());
        assert!(
            !later_path.exists(),
            "unsupported special entry must stop before later operations mutate the filesystem"
        );

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }

    #[cfg(unix)]
    #[test]
    fn directory_buffer_transaction_rejects_unwritable_parent_before_mutation() {
        use std::os::unix::fs::PermissionsExt;

        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("directory-transaction-permission-conflict");
        let create_path = root_path.join("created.md");
        std::fs::create_dir_all(&root_path).expect("test directory");
        let original_permissions = std::fs::metadata(&root_path)
            .expect("root metadata")
            .permissions();
        std::fs::set_permissions(&root_path, std::fs::Permissions::from_mode(0o555))
            .expect("make root read-only");
        let mut session_state =
            saya::app::session::EditorSessionState::new(Some(root_path.clone()));
        let plan = saya::app::session::DirectoryBufferOperationPlan {
            root_path: root_path.clone(),
            operations: vec![DirectoryBufferPlannedOperation::CreateFile {
                path: create_path.clone(),
                name: "created.md".to_string(),
            }],
        };

        let error = apply_directory_buffer_operation_plan(&mut session_state, &plan)
            .expect_err("unwritable parent should be rejected before execution");

        assert!(matches!(
            error,
            RuntimeFilerError::OperationFailed {
                kind: RuntimeFilerErrorKind::PermissionDenied,
                ..
            }
        ));
        assert!(!create_path.exists());

        std::fs::set_permissions(&root_path, original_permissions).expect("restore permissions");
        std::fs::remove_dir_all(root_path).expect("cleanup directory");
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
        assert_eq!(
            parse_main_host_command("edit /tmp/project"),
            Some(MainHostCommand::Edit(std::path::PathBuf::from(
                "/tmp/project"
            )))
        );
        assert_eq!(
            parse_main_host_command("markdown.previewMermaid"),
            Some(MainHostCommand::MarkdownPreviewMermaid)
        );
        assert_eq!(
            parse_main_host_command(r#"lsp.floatHover {"result":{"contents":"hover"}}"#),
            Some(MainHostCommand::LspHoverFloat(
                r#"{"result":{"contents":"hover"}}"#.to_string()
            ))
        );
        assert_eq!(
            parse_main_host_command(r#"lsp.floatDiagnostics {"diagnostics":[]}"#),
            Some(MainHostCommand::LspDiagnosticFloat(
                r#"{"diagnostics":[]}"#.to_string()
            ))
        );
        assert_eq!(
            parse_main_host_command(r#"lsp.nextDiagnostic {"ui":{"width":40,"height":8}}"#),
            Some(MainHostCommand::LspNextDiagnostic(Some(
                r#"{"ui":{"width":40,"height":8}}"#.to_string()
            )))
        );
        assert_eq!(
            parse_main_host_command(r#"lsp.previewWorkspaceEdit {"title":"Rename"}"#),
            Some(MainHostCommand::LspWorkspaceEditPreview(
                r#"{"title":"Rename"}"#.to_string()
            ))
        );
        assert_eq!(
            parse_main_host_command(r#"lsp.floatCodeActions {"response":{"result":[]}}"#),
            Some(MainHostCommand::LspCodeActionsFloat(
                r#"{"response":{"result":[]}}"#.to_string()
            ))
        );
        assert_eq!(
            parse_main_host_command(r#"buffer.floatWindow {"width":20}"#),
            Some(MainHostCommand::BufferWindowFloat(
                r#"{"width":20}"#.to_string()
            ))
        );
        assert_eq!(
            parse_main_host_command(r#"terminal.float {"command":"sh"}"#),
            Some(MainHostCommand::TerminalFloat(
                r#"{"command":"sh"}"#.to_string()
            ))
        );
        assert_eq!(
            parse_main_host_command(r#"terminal.closeFloat {"terminalId":1}"#),
            Some(MainHostCommand::TerminalCloseFloat(
                r#"{"terminalId":1}"#.to_string()
            ))
        );
        assert_eq!(
            parse_main_host_command(r#"completion.floatMenu {"candidates":["alpha"]}"#),
            None
        );
        assert_eq!(parse_main_host_command("set number"), None);
    }

    #[test]
    fn markdown_preview_mermaid_host_command_requests_manual_preview() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();

        let effect = execute_runtime_host_command_with_floats(
            "markdown.previewMermaid",
            &mut outcome,
            &mut session_state,
            None,
            None,
            None,
            None,
        )
        .expect("manual Mermaid preview command should be accepted");

        assert_eq!(
            effect.transient_message.as_deref(),
            Some("Mermaid preview requested")
        );
        assert!(session_state.mermaid_preview_manual_active());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn startup_keymap_builtin_mermaid_preview_command_falls_back_to_host_command() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
            .expect("runtime session should initialize");
        let mut transient_msg = None;
        let mut need_redraw = false;
        let mut runtime_presentation_intents = Vec::new();
        let mut floating_window_manager = FloatingWindowManager::default();
        let mut completion_float_manager = CompletionFloatManager::default();
        let mut lsp_diagnostic_store = LspDiagnosticStore::default();
        let mut terminal_float_manager = TerminalFloatManager::default();
        let mut panel_manager = PanelManager::default();

        let shutdown = execute_startup_keymap_registered_command(
            Some(&mut runtime_session),
            "markdown.previewMermaid",
            &mut outcome,
            &mut session_state,
            &mut floating_window_manager,
            &mut completion_float_manager,
            &mut lsp_diagnostic_store,
            &mut terminal_float_manager,
            &mut panel_manager,
            None,
            &mut transient_msg,
            &mut need_redraw,
            &mut runtime_presentation_intents,
            None,
        )
        .await;

        assert_eq!(shutdown, None);
        assert_eq!(transient_msg.as_deref(), Some("Mermaid preview requested"));
        assert!(need_redraw);
        assert!(session_state.mermaid_preview_manual_active());
    }

    #[test]
    fn runtime_lsp_hover_float_host_command_opens_replacing_cursor_relative_float() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        let mut floating_window_manager = FloatingWindowManager::default();

        execute_runtime_host_command_with_floats(
            r#"lsp.floatHover {"result":{"contents":"old hover"}}"#,
            &mut outcome,
            &mut session_state,
            Some(&mut floating_window_manager),
            None,
            None,
            None,
        )
        .expect("first hover should open");
        execute_runtime_host_command_with_floats(
            r#"lsp.floatHover {"result":{"contents":"new hover"}}"#,
            &mut outcome,
            &mut session_state,
            Some(&mut floating_window_manager),
            None,
            None,
            None,
        )
        .expect("second hover should replace first");

        let active_window_id = outcome
            .core_bridge
            .light_snapshot()
            .active_window_id()
            .unwrap_or(1);
        let floats = floating_window_manager.resolve_screen_models_with_cursors(
            80,
            24,
            &[(
                active_window_id,
                saya::presentation::screen_model::PaneRect {
                    x: 0,
                    y: 0,
                    width: 80,
                    height: 24,
                },
            )],
            &[(active_window_id, 0, 0)],
            Some(active_window_id),
        );
        assert_eq!(floats.len(), 1);
        assert_eq!(floats[0].lines, vec!["new hover"]);
    }

    #[test]
    fn lsp_popup_size_percent_resolves_against_window_by_default() {
        let context = PopupSizingContext {
            terminal_width: 100,
            terminal_height: 40,
            parent_window_rect: saya::presentation::screen_model::PaneRect {
                x: 0,
                y: 0,
                width: 60,
                height: 20,
            },
        };

        let limit = resolve_lsp_popup_size_limit(
            PopupSizeSpec {
                width: PopupSizeValue::Percent(50),
                height: PopupSizeValue::Percent(50),
                basis: default_lsp_popup_basis(LspPopupKind::Hover),
            },
            &context,
        );

        assert_eq!(
            limit,
            ResolvedPopupSizeLimit {
                max_width: 30,
                max_height: 10,
            }
        );
    }

    #[test]
    fn lsp_popup_size_percent_can_resolve_against_editor_grid() {
        let context = PopupSizingContext {
            terminal_width: 100,
            terminal_height: 40,
            parent_window_rect: saya::presentation::screen_model::PaneRect {
                x: 0,
                y: 0,
                width: 60,
                height: 20,
            },
        };

        let limit = resolve_lsp_popup_size_limit(
            PopupSizeSpec {
                width: PopupSizeValue::Percent(50),
                height: PopupSizeValue::Percent(50),
                basis: PopupSizeBasis::Editor,
            },
            &context,
        );

        assert_eq!(
            limit,
            ResolvedPopupSizeLimit {
                max_width: 50,
                max_height: 20,
            }
        );
    }

    #[test]
    fn lsp_popup_size_payload_rejects_invalid_percentages() {
        let error = parse_lsp_popup_size_spec(
            Some(&serde_json::json!({ "width": "0%", "height": "101%" })),
            PopupSizeBasis::Window,
            "lsp.floatHover.ui",
        )
        .expect_err("invalid percentage should fail");

        assert!(
            error.contains("percentage string from 1% through 100%"),
            "error should explain percentage bounds: {error}"
        );
    }

    #[test]
    fn lsp_hover_payload_kind_accepts_signature_help_for_dedicated_ui_size() {
        assert_eq!(
            parse_lsp_hover_popup_kind(&serde_json::json!({ "kind": "signatureHelp" }))
                .expect("signatureHelp kind should be accepted"),
            LspHoverPopupKind::SignatureHelp
        );
        assert!(
            parse_lsp_hover_popup_kind(&serde_json::json!({ "kind": "typo" })).is_err(),
            "unknown hover kind should be rejected"
        );
    }

    #[test]
    fn runtime_lsp_hover_any_prefers_diagnostic_at_cursor() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        let mut floating_window_manager = FloatingWindowManager::default();
        let mut lsp_diagnostic_store = LspDiagnosticStore::default();

        execute_runtime_host_command_with_floats(
            r#"lsp.publishDiagnostics {"params":{"uri":"file:///Users/skudo/.config/saya/init.ts","diagnostics":[{"severity":1,"message":"Property 'lineNumber' does not exist on type 'SayaStartupOptionsSurface'.","range":{"start":{"line":0,"character":0},"end":{"line":0,"character":10}}}]}}"#,
            &mut outcome,
            &mut session_state,
            Some(&mut floating_window_manager),
            None,
            Some(&mut lsp_diagnostic_store),
            None,
        )
        .expect("diagnostics should publish");

        execute_runtime_host_command_with_floats(
            r#"lsp.floatHover {"response":{"source":"lsp","method":"textDocument/hover","result":{"contents":"\n```typescript\nany\n```\n","range":{"start":{"line":0,"character":0},"end":{"line":0,"character":10}}}}}"#,
            &mut outcome,
            &mut session_state,
            Some(&mut floating_window_manager),
            None,
            Some(&mut lsp_diagnostic_store),
            None,
        )
        .expect("hover should prefer diagnostic");

        let active_window_id = outcome
            .core_bridge
            .light_snapshot()
            .active_window_id()
            .unwrap_or(1);
        let floats = floating_window_manager.resolve_screen_models_with_cursors(
            80,
            24,
            &[(
                active_window_id,
                saya::presentation::screen_model::PaneRect {
                    x: 0,
                    y: 0,
                    width: 80,
                    height: 24,
                },
            )],
            &[(active_window_id, 3, 7)],
            Some(active_window_id),
        );

        assert_eq!(floats.len(), 1);
        assert!(
            floats[0].lines.join(" ").contains(
                "Error: Property 'lineNumber' does not exist on type 'SayaStartupOptionsSurface'."
            ),
            "diagnostic hover should render the TypeScript error: {:?}",
            floats[0].lines
        );
        assert_eq!((floats[0].rect.x, floats[0].rect.y), (7, 4));
    }

    #[test]
    fn runtime_lsp_feature_host_commands_render_lists_and_navigate_definition() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let target_path = unique_path("lsp-definition-target").with_extension("rs");
        std::fs::write(&target_path, "fn target() {}\nfn caller() {}\n")
            .expect("definition target should be written");
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        let mut floating_window_manager = FloatingWindowManager::default();

        execute_runtime_host_command_with_floats(
            &format!(
                r#"lsp.gotoDefinition {{"response":{{"result":{{"uri":"file://{}","range":{{"start":{{"line":1,"character":0}}}}}}}}}}"#,
                target_path.display()
            ),
            &mut outcome,
            &mut session_state,
            Some(&mut floating_window_manager),
            None,
            None,
            None,
        )
        .expect("definition navigation should apply");
        assert_eq!(session_state.target_path(), Some(&target_path));
        assert_eq!(outcome.core_bridge.light_snapshot().cursor_row, 1);

        execute_runtime_host_command_with_floats(
            r#"lsp.floatLocations {"title":"References","response":{"result":[{"uri":"file:///workspace/src/main.rs","range":{"start":{"line":4,"character":1}}},{"uri":"file:///workspace/src/lib.rs","range":{"start":{"line":9,"character":3}}}]}}"#,
            &mut outcome,
            &mut session_state,
            Some(&mut floating_window_manager),
            None,
            None,
            None,
        )
        .expect("references list should open");
        execute_runtime_host_command_with_floats(
            r#"lsp.floatSymbols {"response":{"result":[{"name":"main","kind":12,"range":{"start":{"line":0,"character":0}},"children":[{"name":"child","kind":6,"range":{"start":{"line":2,"character":2}}}]}]}}"#,
            &mut outcome,
            &mut session_state,
            Some(&mut floating_window_manager),
            None,
            None,
            None,
        )
        .expect("symbol outline should open");
        execute_runtime_host_command_with_floats(
            r#"lsp.previewWorkspaceEdit {"title":"Rename preview","response":{"result":{"changes":{"file:///workspace/src/main.rs":[{"range":{"start":{"line":4,"character":1},"end":{"line":4,"character":5}},"newText":"renamed"}]}}}}"#,
            &mut outcome,
            &mut session_state,
            Some(&mut floating_window_manager),
            None,
            None,
            None,
        )
        .expect("workspace edit preview should open");
        execute_runtime_host_command_with_floats(
            r#"lsp.floatCodeActions {"response":{"result":[{"title":"Organize Imports","kind":"source.organizeImports"},{"title":"Fix issue","kind":"quickfix"}]}}"#,
            &mut outcome,
            &mut session_state,
            Some(&mut floating_window_manager),
            None,
            None,
            None,
        )
        .expect("code action float should open");

        let active_window_id = outcome
            .core_bridge
            .light_snapshot()
            .active_window_id()
            .unwrap_or(1);
        let floats = floating_window_manager.resolve_screen_models(
            100,
            30,
            &[(
                active_window_id,
                saya::presentation::screen_model::PaneRect {
                    x: 0,
                    y: 0,
                    width: 100,
                    height: 30,
                },
            )],
            Some(active_window_id),
        );
        let rendered_lines = floats
            .iter()
            .flat_map(|float| float.lines.iter().cloned())
            .collect::<Vec<_>>();
        assert!(
            rendered_lines
                .iter()
                .any(|line| line.contains("src/main.rs:5:2")),
            "references should render line and column in a headless float: {rendered_lines:?}"
        );
        assert!(
            rendered_lines
                .iter()
                .any(|line| line.contains("main") && line.contains("Function")),
            "document symbols should render a selectable outline: {rendered_lines:?}"
        );
        assert!(
            rendered_lines.iter().any(|line| line.contains("child")),
            "nested document symbols should be rendered: {rendered_lines:?}"
        );
        assert!(
            rendered_lines
                .iter()
                .any(|line| line.contains("source.organizeImports: Organize Imports")),
            "code actions should render actionable titles: {rendered_lines:?}"
        );

        std::fs::remove_file(target_path).expect("cleanup definition target");
    }

    #[test]
    fn runtime_lsp_diagnostics_publish_and_cycle_open_diagnostic_floats() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        let mut floating_window_manager = FloatingWindowManager::default();
        let mut lsp_diagnostic_store = LspDiagnosticStore::default();

        execute_runtime_host_command_with_floats(
            r#"lsp.publishDiagnostics {"params":{"uri":"file:///workspace/src/main.rs","diagnostics":[{"severity":1,"message":"first error","range":{"start":{"line":2,"character":4}}},{"severity":2,"message":"second warning","range":{"start":{"line":5,"character":1}}}]}}"#,
            &mut outcome,
            &mut session_state,
            Some(&mut floating_window_manager),
            None,
            Some(&mut lsp_diagnostic_store),
            None,
        )
        .expect("diagnostics should publish");
        assert!(!lsp_diagnostic_store.is_empty());
        assert_eq!(
            floating_window_manager
                .resolve_screen_models(80, 24, &[], None)
                .len(),
            0,
            "publishing diagnostics should update the store without stealing focus"
        );

        execute_runtime_host_command_with_floats(
            "lsp.nextDiagnostic",
            &mut outcome,
            &mut session_state,
            Some(&mut floating_window_manager),
            None,
            Some(&mut lsp_diagnostic_store),
            None,
        )
        .expect("next diagnostic should open");
        execute_runtime_host_command_with_floats(
            "lsp.nextDiagnostic",
            &mut outcome,
            &mut session_state,
            Some(&mut floating_window_manager),
            None,
            Some(&mut lsp_diagnostic_store),
            None,
        )
        .expect("next diagnostic should cycle");

        let active_window_id = outcome
            .core_bridge
            .light_snapshot()
            .active_window_id()
            .unwrap_or(1);
        let floats = floating_window_manager.resolve_screen_models(
            80,
            24,
            &[(
                active_window_id,
                saya::presentation::screen_model::PaneRect {
                    x: 0,
                    y: 0,
                    width: 80,
                    height: 24,
                },
            )],
            Some(active_window_id),
        );
        assert_eq!(floats.len(), 1);
        assert_eq!(floats[0].lines, vec!["Warning: second warning"]);
        assert!(matches!(
            floating_window_manager
                .debug_window(floats[0].id)
                .expect("diagnostic float should exist")
                .placement
                .relative_to,
            FloatingRelativeTo::BufferPosition {
                line: 5,
                column: 1,
                ..
            }
        ));
    }

    #[test]
    fn runtime_typed_completion_accept_applies_replace_range_insert_text() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        outcome
            .core_bridge
            .replace_buffer_text("pri\n")
            .expect("seed buffer text");
        outcome
            .core_bridge
            .dispatch_key("A")
            .expect("enter insert mode at line end");
        let mut floating_window_manager = FloatingWindowManager::default();
        let mut completion_float_manager = CompletionFloatManager::default();

        assert!(completion_float_manager.show_typed(
            &mut floating_window_manager,
            1,
            0,
            0,
            CompletionShowRequest {
                session_id: "test-session".to_string(),
                request_id: 1,
                replace_range: saya::features::completion::session::CompletionRange {
                    start: saya::features::completion::session::CompletionPosition {
                        line: 0,
                        character: 0,
                    },
                    end: saya::features::completion::session::CompletionPosition {
                        line: 0,
                        character: 3,
                    },
                },
                candidates: vec![
                    saya::features::completion::session::HostCompletionCandidate {
                        label: "println!".to_string(),
                        insert_text: Some("println!($0);".to_string()),
                        kind: Some("Function".to_string()),
                        detail: Some("macro".to_string()),
                        documentation: Vec::new(),
                        source: Some("rust-analyzer".to_string()),
                    },
                ],
                selected_index: 0,
                max_visible_items: 8,
                documentation_max_width: 72,
                documentation_max_height: 12,
                keys: Some(
                    saya::features::completion::session::CompletionKeyBindingsRequest {
                        confirm: Some(vec!["<Enter>".to_string()]),
                        close: None,
                        next: None,
                        previous: None,
                        page_next: None,
                        page_previous: None,
                    }
                ),
            },
        ));
        let active_window_id = outcome
            .core_bridge
            .light_snapshot()
            .active_window_id()
            .unwrap_or(1);
        let _menu_id = floating_window_manager
            .windows()
            .iter()
            .find(|window| {
                matches!(
                    window.content,
                    saya::presentation::floating_window::FloatingContentRef::CompletionMenu { .. }
                )
            })
            .map(|window| window.id)
            .expect("completion menu should exist");
        assert_eq!(floating_window_manager.focused_float_id(), None);

        assert!(matches!(
            handle_completion_float_key(
                &mut completion_float_manager,
                &mut floating_window_manager,
                &mut outcome.core_bridge,
                &KeyInput::Enter,
                active_window_id,
            ),
            Some(FloatingWindowKeyHandling::Closed { .. })
        ));
        assert_eq!(outcome.core_bridge.buffer_text(), "println!($0);\n");
        let snapshot = outcome.core_bridge.light_snapshot();
        assert_eq!(snapshot.mode, vim_core_rs::CoreMode::Insert);
        assert_eq!(snapshot.cursor_row, 0);
        assert_eq!(
            snapshot.cursor_col,
            "println!($0);".len(),
            "typed completion confirmation should move the insert cursor to the replacement end"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn startup_completion_keymap_opens_pum_and_enter_confirms_candidate() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let target_path = unique_path("completion-keymap-target").with_extension("txt");
        let config_path = unique_path("completion-keymap-init").with_extension("ts");
        let completion_path =
            saya::support::paths::dev_ts_plugins_dir().join("bundled/completion/index.ts");
        std::fs::write(&target_path, "ty\ntype\n").expect("target file");
        std::fs::write(
            &config_path,
            format!(
                r#"
                    import {{ createBufferWordSource, setupSayaCompletion }} from "{}";
                    setupSayaCompletion({{
                        key: "<C-x>",
                        keys: {{ confirm: ["<Enter>"] }},
                        minPrefixLength: 2,
                        sourceTimeoutMs: 0,
                        sources: [createBufferWordSource()],
                    }});
                "#,
                completion_path.to_string_lossy()
            ),
        )
        .expect("config file");

        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::File(target_path.clone()),
            config_source: saya::app::cli::ConfigSource::File(config_path.clone()),
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
            .expect("runtime session should initialize");
        let mut floating_window_manager = FloatingWindowManager::default();
        let mut completion_float_manager = CompletionFloatManager::default();
        let mut lsp_diagnostic_store = LspDiagnosticStore::default();
        let mut terminal_float_manager = TerminalFloatManager::default();
        let mut panel_manager = PanelManager::default();
        let mut transient_msg = None;
        let mut need_redraw = false;
        let mut runtime_presentation_intents = Vec::new();

        outcome.core_bridge.dispatch_key("A").expect("enter insert");
        let mut startup_keymap_pending_lhs = None;
        let action = startup_keymap_action_for_snapshot_input(
            &outcome.startup_registry.keymaps,
            &outcome.core_bridge.light_snapshot(),
            &KeyInput::Ctrl('x'),
            &mut startup_keymap_pending_lhs,
        )
        .unwrap_or_else(|| {
            panic!(
                "insert completion keymap should resolve; keymaps={:?}, mode={:?}, warnings={:?}",
                outcome.startup_registry.keymaps,
                outcome.core_bridge.mode(),
                outcome.warnings
            )
        });
        let StartupKeymapAction::RegisteredCommand(command_name) = action else {
            panic!("completion keymap should point at a registered command");
        };
        assert_eq!(command_name, "completion.trigger");

        let shutdown = execute_startup_keymap_registered_command(
            Some(&mut runtime_session),
            &command_name,
            &mut outcome,
            &mut session_state,
            &mut floating_window_manager,
            &mut completion_float_manager,
            &mut lsp_diagnostic_store,
            &mut terminal_float_manager,
            &mut panel_manager,
            None,
            &mut transient_msg,
            &mut need_redraw,
            &mut runtime_presentation_intents,
            None,
        )
        .await;
        assert_eq!(shutdown, None);
        assert_eq!(
            transient_msg, None,
            "dired startup command should not surface swap or pager messages"
        );
        assert!(
            floating_window_manager.windows().iter().any(|window| {
                matches!(
                    window.content,
                    saya::presentation::floating_window::FloatingContentRef::CompletionMenu { .. }
                )
            }),
            "completion trigger should open a completion menu"
        );
        assert_eq!(floating_window_manager.focused_float_id(), None);

        let active_window_id = outcome
            .core_bridge
            .light_snapshot()
            .active_window_id()
            .unwrap_or(1);
        assert!(matches!(
            handle_completion_float_key(
                &mut completion_float_manager,
                &mut floating_window_manager,
                &mut outcome.core_bridge,
                &KeyInput::Enter,
                active_window_id,
            ),
            Some(FloatingWindowKeyHandling::Closed { .. })
        ));
        assert_eq!(outcome.core_bridge.buffer_text(), "type\ntype\n");
        let snapshot = outcome.core_bridge.light_snapshot();
        assert_eq!(snapshot.cursor_row, 0);
        assert_eq!(
            snapshot.cursor_col, 4,
            "startup completion keymap confirmation should move the insert cursor after the candidate"
        );

        std::fs::remove_file(target_path).expect("cleanup target");
        std::fs::remove_file(config_path).expect("cleanup config");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn startup_completion_auto_trigger_opens_menu_from_buffer_changed_event() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let target_path = unique_path("completion-auto-target").with_extension("txt");
        let config_path = unique_path("completion-auto-init").with_extension("ts");
        let completion_path =
            saya::support::paths::dev_ts_plugins_dir().join("bundled/completion/index.ts");
        std::fs::write(&target_path, "t\ntype\n").expect("target file");
        std::fs::write(
            &config_path,
            format!(
                r#"
                    import {{ createBufferWordSource, setupSayaCompletion }} from "{}";
                    setupSayaCompletion({{
                        key: "<C-x>",
                        autoTrigger: true,
                        autoTriggerDelayMs: 0,
                        sourceTimeoutMs: 0,
                        sources: [createBufferWordSource()],
                    }});
                "#,
                completion_path.to_string_lossy()
            ),
        )
        .expect("config file");

        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::File(target_path.clone()),
            config_source: saya::app::cli::ConfigSource::File(config_path.clone()),
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
            .expect("runtime session should initialize");
        let mut floating_window_manager = FloatingWindowManager::default();
        let mut completion_float_manager = CompletionFloatManager::default();
        let mut lsp_diagnostic_store = LspDiagnosticStore::default();
        let mut terminal_float_manager = TerminalFloatManager::default();
        let mut panel_manager = PanelManager::default();
        let mut transient_msg = None;
        let mut need_redraw = false;
        let mut runtime_presentation_intents = Vec::new();

        outcome.core_bridge.dispatch_key("A").expect("enter insert");
        let shutdown = dispatch_buffer_changed_with_runtime(
            Some(&mut runtime_session),
            &mut outcome,
            &mut session_state,
            &mut transient_msg,
            &mut need_redraw,
            &mut runtime_presentation_intents,
            &mut floating_window_manager,
            &mut completion_float_manager,
            &mut lsp_diagnostic_store,
            &mut terminal_float_manager,
            &mut panel_manager,
            None,
        )
        .await;
        assert_eq!(shutdown, None);
        assert_eq!(
            transient_msg, None,
            "auto completion must not surface runtime callback errors"
        );
        assert!(need_redraw, "auto completion should request redraw");
        assert!(
            floating_window_manager.windows().iter().any(|window| {
                matches!(
                    window.content,
                    saya::presentation::floating_window::FloatingContentRef::CompletionMenu { .. }
                )
            }),
            "bufferChanged auto trigger should open a completion menu after one character"
        );
        let active_window_id = outcome
            .core_bridge
            .light_snapshot()
            .active_window_id()
            .unwrap_or(1);
        assert!(matches!(
            handle_completion_float_key(
                &mut completion_float_manager,
                &mut floating_window_manager,
                &mut outcome.core_bridge,
                &KeyInput::Escape,
                active_window_id,
            ),
            Some(FloatingWindowKeyHandling::Closed { .. })
        ));
        assert!(
            !floating_window_manager.windows().iter().any(|window| {
                matches!(
                    window.content,
                    saya::presentation::floating_window::FloatingContentRef::CompletionMenu { .. }
                )
            }),
            "Esc should close the completion menu before leaving insert mode"
        );
        assert_eq!(
            outcome.core_bridge.light_snapshot().mode,
            vim_core_rs::CoreMode::Normal,
            "Esc should leave insert mode after closing the completion menu"
        );

        outcome
            .core_bridge
            .dispatch_key("A")
            .expect("re-enter insert");
        let shutdown = dispatch_buffer_changed_with_runtime(
            Some(&mut runtime_session),
            &mut outcome,
            &mut session_state,
            &mut transient_msg,
            &mut need_redraw,
            &mut runtime_presentation_intents,
            &mut floating_window_manager,
            &mut completion_float_manager,
            &mut lsp_diagnostic_store,
            &mut terminal_float_manager,
            &mut panel_manager,
            None,
        )
        .await;
        assert_eq!(shutdown, None);
        assert!(
            floating_window_manager.windows().iter().any(|window| {
                matches!(
                    window.content,
                    saya::presentation::floating_window::FloatingContentRef::CompletionMenu { .. }
                )
            }),
            "bufferChanged auto trigger should reopen the one-character completion menu"
        );

        outcome
            .core_bridge
            .replace_buffer_text("\n")
            .expect("empty prefix buffer text");
        let shutdown = dispatch_buffer_changed_with_runtime(
            Some(&mut runtime_session),
            &mut outcome,
            &mut session_state,
            &mut transient_msg,
            &mut need_redraw,
            &mut runtime_presentation_intents,
            &mut floating_window_manager,
            &mut completion_float_manager,
            &mut lsp_diagnostic_store,
            &mut terminal_float_manager,
            &mut panel_manager,
            None,
        )
        .await;
        assert_eq!(shutdown, None);
        assert_eq!(
            transient_msg, None,
            "auto completion close must not surface runtime callback errors"
        );
        assert!(
            !floating_window_manager.windows().iter().any(|window| {
                matches!(
                    window.content,
                    saya::presentation::floating_window::FloatingContentRef::CompletionMenu { .. }
                )
            }),
            "bufferChanged auto trigger should close the stale menu when the prefix is too short"
        );

        std::fs::remove_file(target_path).expect("cleanup target");
        std::fs::remove_file(config_path).expect("cleanup config");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn startup_completion_auto_trigger_disabled_does_not_open_from_buffer_changed_event() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let target_path = unique_path("completion-auto-disabled-target").with_extension("txt");
        let config_path = unique_path("completion-auto-disabled-init").with_extension("ts");
        let completion_path =
            saya::support::paths::dev_ts_plugins_dir().join("bundled/completion/index.ts");
        std::fs::write(&target_path, "ty\ntype\n").expect("target file");
        std::fs::write(
            &config_path,
            format!(
                r#"
                    import {{ createBufferWordSource, setupSayaCompletion }} from "{}";
                    setupSayaCompletion({{
                        key: "<C-x>",
                        autoTrigger: false,
                        autoTriggerDelayMs: 0,
                        minPrefixLength: 2,
                        sourceTimeoutMs: 0,
                        sources: [createBufferWordSource()],
                    }});
                "#,
                completion_path.to_string_lossy()
            ),
        )
        .expect("config file");

        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::File(target_path.clone()),
            config_source: saya::app::cli::ConfigSource::File(config_path.clone()),
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
            .expect("runtime session should initialize");
        let mut floating_window_manager = FloatingWindowManager::default();
        let mut completion_float_manager = CompletionFloatManager::default();
        let mut lsp_diagnostic_store = LspDiagnosticStore::default();
        let mut terminal_float_manager = TerminalFloatManager::default();
        let mut panel_manager = PanelManager::default();
        let mut transient_msg = None;
        let mut need_redraw = false;
        let mut runtime_presentation_intents = Vec::new();

        outcome.core_bridge.dispatch_key("A").expect("enter insert");
        let shutdown = dispatch_buffer_changed_with_runtime(
            Some(&mut runtime_session),
            &mut outcome,
            &mut session_state,
            &mut transient_msg,
            &mut need_redraw,
            &mut runtime_presentation_intents,
            &mut floating_window_manager,
            &mut completion_float_manager,
            &mut lsp_diagnostic_store,
            &mut terminal_float_manager,
            &mut panel_manager,
            None,
        )
        .await;

        assert_eq!(shutdown, None);
        assert_eq!(transient_msg, None);
        assert!(
            !floating_window_manager.windows().iter().any(|window| {
                matches!(
                    window.content,
                    saya::presentation::floating_window::FloatingContentRef::CompletionMenu { .. }
                )
            }),
            "disabled auto trigger must not open a completion menu from bufferChanged"
        );

        std::fs::remove_file(target_path).expect("cleanup target");
        std::fs::remove_file(config_path).expect("cleanup config");
    }

    #[test]
    fn runtime_buffer_float_host_command_opens_core_window_float_and_renders_buffer_lines() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let target_path = unique_path("buffer-float-render").with_extension("txt");
        std::fs::write(&target_path, "alpha\nbeta\ngamma\n").expect("target file");
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::File(target_path),
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        let mut floating_window_manager = FloatingWindowManager::default();
        let active_window_id = outcome
            .core_bridge
            .light_snapshot()
            .active_window_id()
            .expect("active window");

        execute_runtime_host_command_with_floats(
            r#"buffer.floatWindow {"width":24,"height":4,"border":"none"}"#,
            &mut outcome,
            &mut session_state,
            Some(&mut floating_window_manager),
            None,
            None,
            None,
        )
        .expect("buffer float should open");

        assert_eq!(
            floating_window_manager.focused_core_window_id(),
            Some(active_window_id)
        );
        refresh_buffer_backed_float_lines(
            &mut floating_window_manager,
            &outcome.core_bridge,
            &outcome.core_bridge.light_snapshot(),
        );
        let floats = floating_window_manager.resolve_screen_models(
            80,
            24,
            &[(
                active_window_id,
                saya::presentation::screen_model::PaneRect {
                    x: 0,
                    y: 0,
                    width: 80,
                    height: 24,
                },
            )],
            Some(active_window_id),
        );

        assert_eq!(floats.len(), 1);
        assert_eq!(
            floats[0].content,
            saya::presentation::floating_window::FloatingContentRef::CoreWindow {
                window_id: active_window_id
            }
        );
        assert_eq!(floats[0].lines, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn runtime_window_open_float_api_opens_static_lines_float_through_application_host() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut floating_window_manager = FloatingWindowManager::default();
        let request = RuntimeFloatOpenRequest {
            content: RuntimeFloatContentRequest::Lines {
                lines: vec!["phase8".to_string(), "runtime-api".to_string()],
            },
            relative_to: Some(RuntimeFloatRelativeToRequest::Editor),
            width: Some(24),
            height: Some(4),
            row: Some(1),
            col: Some(2),
            anchor: Some("nw".to_string()),
            focusable: Some(true),
            border: Some("single".to_string()),
            z_index: Some(RuntimeFloatZIndexRequest::Named("user".to_string())),
            lifecycle: Some("manual".to_string()),
            group: Some("phase8:api".to_string()),
        };

        let snapshot = execute_runtime_window_open_float(
            request,
            &mut outcome,
            Some(&mut floating_window_manager),
            None,
        )
        .expect("runtime window openFloat should open a float");

        assert_eq!(snapshot.kind, "lines");
        assert!(snapshot.focused);
        assert_eq!(snapshot.replacement_group.as_deref(), Some("phase8:api"));
        assert_eq!(
            floating_window_manager
                .debug_window(FloatingWindowId(snapshot.id))
                .expect("float should exist")
                .lines,
            vec!["phase8", "runtime-api"]
        );
    }

    #[test]
    fn runtime_buffer_float_uses_existing_backing_window_without_switching_active_window() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let first_path = unique_path("buffer-float-backed-first").with_extension("txt");
        std::fs::write(&first_path, "first\n").expect("first target file");
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::File(first_path.clone()),
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut floating_window_manager = FloatingWindowManager::default();
        let first_snapshot = outcome.core_bridge.light_snapshot();
        let first_buffer_id = first_snapshot
            .active_window()
            .expect("first active window")
            .buf_id;

        outcome
            .core_bridge
            .apply_ex_command(":split")
            .expect("split should create a backing window");
        outcome
            .core_bridge
            .apply_ex_command(":enew")
            .expect("enew should create a second active buffer");
        let second_snapshot = outcome.core_bridge.light_snapshot();
        let active_window_id = second_snapshot
            .active_window_id()
            .expect("second active window");
        let active_buffer_id = second_snapshot
            .active_window()
            .expect("second active window info")
            .buf_id;
        assert_ne!(
            first_buffer_id, active_buffer_id,
            "test setup requires a non-active buffer"
        );
        let backing_window_id = second_snapshot
            .windows
            .iter()
            .find(|window| window.buf_id == first_buffer_id)
            .expect("first buffer should still have an inactive backing window")
            .id;
        assert_ne!(
            backing_window_id, active_window_id,
            "test setup requires an inactive backing window"
        );

        let request = RuntimeFloatOpenRequest {
            content: RuntimeFloatContentRequest::Buffer {
                buffer_id: Some(first_buffer_id as u64),
                window_id: None,
            },
            relative_to: Some(RuntimeFloatRelativeToRequest::Editor),
            width: Some(24),
            height: Some(4),
            row: Some(1),
            col: Some(2),
            anchor: Some("nw".to_string()),
            focusable: Some(true),
            border: Some("single".to_string()),
            z_index: Some(RuntimeFloatZIndexRequest::Named("user".to_string())),
            lifecycle: Some("manual".to_string()),
            group: Some("phase10:buffer".to_string()),
        };

        let snapshot = execute_runtime_window_open_float(
            request,
            &mut outcome,
            Some(&mut floating_window_manager),
            None,
        )
        .expect("backed buffer float should use the existing core window");

        assert_eq!(snapshot.kind, "buffer");
        assert_eq!(
            floating_window_manager.focused_core_window_id(),
            Some(backing_window_id)
        );
        let after = outcome.core_bridge.light_snapshot();
        assert_eq!(after.active_window_id(), Some(active_window_id));
        assert_eq!(
            after.active_window().map(|window| window.buf_id),
            Some(active_buffer_id),
            "buffer float opening must not switch the active core window buffer"
        );
    }

    #[test]
    fn runtime_buffer_float_rejects_unbacked_buffer_without_partial_float() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut floating_window_manager = FloatingWindowManager::default();
        let before = outcome.core_bridge.light_snapshot();
        let active_window_id = before.active_window_id();
        let active_buffer_id = before.active_window().map(|window| window.buf_id);
        let unbacked_buffer_id = before
            .buffers
            .iter()
            .map(|buffer| buffer.id)
            .max()
            .unwrap_or(0)
            + 10_000;

        let request = RuntimeFloatOpenRequest {
            content: RuntimeFloatContentRequest::Buffer {
                buffer_id: Some(unbacked_buffer_id as u64),
                window_id: None,
            },
            relative_to: Some(RuntimeFloatRelativeToRequest::Editor),
            width: Some(24),
            height: Some(4),
            row: Some(1),
            col: Some(2),
            anchor: Some("nw".to_string()),
            focusable: Some(true),
            border: Some("single".to_string()),
            z_index: Some(RuntimeFloatZIndexRequest::Named("user".to_string())),
            lifecycle: Some("manual".to_string()),
            group: Some("phase10:unbacked-buffer".to_string()),
        };

        let error = execute_runtime_window_open_float(
            request,
            &mut outcome,
            Some(&mut floating_window_manager),
            None,
        )
        .expect_err("unbacked buffer float should be rejected");

        assert!(matches!(
            error,
            RuntimeCommandError::CommandFailed { ref name, ref message }
                if name == "window.openFloat"
                    && message.contains("hidden core-window creation is not available")
        ));
        let after = outcome.core_bridge.light_snapshot();
        assert_eq!(after.active_window_id(), active_window_id);
        assert_eq!(
            after.active_window().map(|window| window.buf_id),
            active_buffer_id
        );
        assert!(floating_window_manager.is_empty());
    }

    #[test]
    fn runtime_window_open_float_api_opens_pty_terminal_float_and_renders_output() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut floating_window_manager = FloatingWindowManager::default();
        let mut terminal_float_manager = TerminalFloatManager::default();
        let request = RuntimeFloatOpenRequest {
            content: RuntimeFloatContentRequest::Terminal {
                command: vec![
                    "sh".to_string(),
                    "-lc".to_string(),
                    "printf 'phase9-runtime-terminal\\n'".to_string(),
                ],
                close_behavior: Some("killOnClose".to_string()),
            },
            relative_to: Some(RuntimeFloatRelativeToRequest::Editor),
            width: Some(34),
            height: Some(6),
            row: Some(1),
            col: Some(2),
            anchor: Some("nw".to_string()),
            focusable: Some(true),
            border: Some("single".to_string()),
            z_index: Some(RuntimeFloatZIndexRequest::Named("user".to_string())),
            lifecycle: Some("manual".to_string()),
            group: Some("phase9:pty-api".to_string()),
        };

        let snapshot = execute_runtime_window_open_float(
            request,
            &mut outcome,
            Some(&mut floating_window_manager),
            Some(&mut terminal_float_manager),
        )
        .expect("runtime window openFloat should open a PTY terminal float");

        assert_eq!(snapshot.kind, "terminal");
        assert!(snapshot.focused);
        wait_for_test_condition(|| {
            refresh_terminal_float_lines(&mut floating_window_manager, &mut terminal_float_manager);
            floating_window_manager
                .debug_window(FloatingWindowId(snapshot.id))
                .expect("terminal float should exist")
                .lines
                .iter()
                .any(|line| line.contains("phase9-runtime-terminal"))
        });

        assert!(
            execute_runtime_window_close_float(
                snapshot.id,
                Some(&mut floating_window_manager),
                Some(&mut terminal_float_manager),
            )
            .expect("terminal float should close")
        );
    }

    #[test]
    fn runtime_terminal_float_host_command_opens_pty_float_and_renders_output() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        let mut floating_window_manager = FloatingWindowManager::default();
        let mut terminal_float_manager = TerminalFloatManager::default();

        execute_runtime_host_command_with_floats(
            r#"terminal.float {"command":"sh","args":["-lc","printf 'phase7-main-terminal\n'"],"width":30,"height":4,"border":"single"}"#,
            &mut outcome,
            &mut session_state,
            Some(&mut floating_window_manager),
            None,
            None,
            Some(&mut terminal_float_manager),
        )
        .expect("terminal float should open");

        let terminal_id = floating_window_manager
            .focused_terminal_id()
            .expect("terminal float should take focus");
        wait_for_test_condition(|| {
            refresh_terminal_float_lines(&mut floating_window_manager, &mut terminal_float_manager);
            floating_window_manager
                .debug_window(
                    floating_window_manager
                        .focused_float_id()
                        .expect("terminal float should stay focused"),
                )
                .expect("terminal float should exist")
                .lines
                .iter()
                .any(|line| line.contains("phase7-main-terminal"))
        });

        assert_eq!(
            floating_window_manager.focused_terminal_id(),
            Some(terminal_id)
        );
    }

    #[test]
    fn focused_terminal_float_routes_input_through_main_key_handler() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut floating_window_manager = FloatingWindowManager::default();
        let mut terminal_float_manager = TerminalFloatManager::default();
        let terminal_id = terminal_float_manager
            .spawn(TerminalFloatSpawnRequest {
                command: "sh".to_string(),
                args: vec![
                    "-lc".to_string(),
                    "read line; printf \"main-echo:%s\\n\" \"$line\"; sleep 30".to_string(),
                ],
                width: 32,
                height: 4,
                close_behavior: TerminalFloatCloseBehavior::KillOnClose,
            })
            .expect("terminal session should spawn");
        let float_id = floating_window_manager.open_terminal(
            terminal_id,
            FloatingPlacement::editor_at(1, 2),
            FloatingSize {
                width: 36,
                height: 6,
            },
            FloatingChrome {
                border: FloatingBorder::Single,
            },
            FloatingZIndex::User,
            true,
        );
        floating_window_manager.focus_float(float_id);

        assert_eq!(
            handle_terminal_float_key(
                &floating_window_manager,
                &mut terminal_float_manager,
                &KeyInput::Char('o'),
            ),
            Some(FloatingWindowKeyHandling::Consumed)
        );
        assert_eq!(
            handle_terminal_float_key(
                &floating_window_manager,
                &mut terminal_float_manager,
                &KeyInput::Char('k'),
            ),
            Some(FloatingWindowKeyHandling::Consumed)
        );
        assert_eq!(
            handle_terminal_float_key(
                &floating_window_manager,
                &mut terminal_float_manager,
                &KeyInput::Enter,
            ),
            Some(FloatingWindowKeyHandling::Consumed)
        );

        wait_for_test_condition(|| {
            refresh_terminal_float_lines(&mut floating_window_manager, &mut terminal_float_manager);
            floating_window_manager
                .debug_window(float_id)
                .expect("terminal float should exist")
                .lines
                .iter()
                .any(|line| line.contains("main-echo:ok"))
        });
        terminal_float_manager
            .kill(terminal_id)
            .expect("cleanup terminal session");
    }

    #[test]
    fn focused_buffer_float_routes_edit_keys_through_core_and_preserves_dirty_state() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let target_path = unique_path("buffer-float-edit").with_extension("txt");
        std::fs::write(&target_path, "hello\n").expect("target file");
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::File(target_path),
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut manager = FloatingWindowManager::default();
        let active_window_id = outcome
            .core_bridge
            .light_snapshot()
            .active_window_id()
            .expect("active window");
        let id = manager.open_core_window(
            active_window_id,
            FloatingPlacement::editor_at(1, 1),
            FloatingSize {
                width: 20,
                height: 4,
            },
            FloatingChrome::borderless(),
            FloatingZIndex::User,
            true,
        );
        assert!(manager.focus_float(id));

        assert_eq!(
            handle_core_window_float_key(
                &mut manager,
                &mut outcome.core_bridge,
                &KeyInput::Char('i')
            ),
            Some(FloatingWindowKeyHandling::Consumed)
        );
        assert_eq!(
            handle_core_window_float_key(
                &mut manager,
                &mut outcome.core_bridge,
                &KeyInput::Char('Z')
            ),
            Some(FloatingWindowKeyHandling::Consumed)
        );
        assert_eq!(
            handle_core_window_float_key(&mut manager, &mut outcome.core_bridge, &KeyInput::Escape),
            Some(FloatingWindowKeyHandling::Consumed)
        );

        let snapshot = outcome.core_bridge.snapshot();
        assert!(
            snapshot.dirty,
            "buffer-float edits must preserve dirty state"
        );
        assert!(
            snapshot.text.starts_with("Zhello"),
            "edit key should be routed through vim-core-rs: {:?}",
            snapshot.text
        );
        assert_eq!(
            manager.focus(),
            Some(saya::presentation::floating_window::WorkspaceFocus::Float { float_id: id }),
            "editing Escape should leave insert mode through core, not close the buffer float"
        );
    }

    #[test]
    fn focused_buffer_float_routes_normal_movement_and_page_scroll_through_core_window() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let target_path = unique_path("buffer-float-scroll").with_extension("txt");
        let text = (0..40)
            .map(|index| format!("line-{index:02}"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(&target_path, text).expect("target file");
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::File(target_path),
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        outcome.core_bridge.set_screen_size(6, 80);
        let mut manager = FloatingWindowManager::default();
        let active_window_id = outcome
            .core_bridge
            .light_snapshot()
            .active_window_id()
            .expect("active window");
        let id = manager.open_core_window(
            active_window_id,
            FloatingPlacement::editor_at(1, 1),
            FloatingSize {
                width: 20,
                height: 4,
            },
            FloatingChrome::borderless(),
            FloatingZIndex::User,
            true,
        );
        assert!(manager.focus_float(id));

        assert_eq!(
            handle_core_window_float_key(
                &mut manager,
                &mut outcome.core_bridge,
                &KeyInput::Char('j')
            ),
            Some(FloatingWindowKeyHandling::Consumed)
        );
        let after_move = outcome.core_bridge.light_snapshot();
        assert_eq!(after_move.cursor_row, 1);
        let before_scroll_topline = after_move
            .window(active_window_id)
            .expect("active window after movement")
            .topline;

        assert_eq!(
            handle_core_window_float_key(
                &mut manager,
                &mut outcome.core_bridge,
                &KeyInput::Ctrl('f')
            ),
            Some(FloatingWindowKeyHandling::Consumed)
        );
        let after_scroll = outcome.core_bridge.light_snapshot();
        let after_scroll_topline = after_scroll
            .window(active_window_id)
            .expect("active window after scroll")
            .topline;

        assert!(
            after_scroll_topline > before_scroll_topline,
            "focused buffer-float page scroll should update the backing core window viewport: {before_scroll_topline} -> {after_scroll_topline}"
        );
    }

    #[test]
    fn startup_keymap_action_for_input_resolves_registered_command_before_core_dispatch() {
        let keymaps = vec![saya::app::bootstrap::StartupKeymapSnapshot {
            mode: StartupKeymapMode::Normal,
            lhs: "-".to_string(),
            action: StartupKeymapAction::RegisteredCommand("dired.open".to_string()),
        }];

        assert_eq!(
            startup_keymap_action_for_input(&keymaps, CoreMode::Normal, &KeyInput::Char('-')),
            Some(StartupKeymapAction::RegisteredCommand(
                "dired.open".to_string()
            ))
        );
    }

    #[test]
    fn startup_registered_command_name_for_ex_command_matches_runtime_command_names() {
        let registry =
            saya::runtime::callback_registry_seed::CallbackRegistrySeed::from_startup_entries(
                vec![saya::runtime::config::StartupRegistryEntry::Command {
                    name: "panel.toggle".to_string(),
                    callback_source: "() => {}".to_string(),
                }],
            );

        assert_eq!(
            startup_registered_command_name_for_ex_command(":panel.toggle", &registry),
            Some("panel.toggle".to_string())
        );
        assert_eq!(
            startup_registered_command_name_for_ex_command("panel.toggle", &registry),
            Some("panel.toggle".to_string())
        );
        assert_eq!(
            startup_registered_command_name_for_ex_command(":panel.missing", &registry),
            None
        );
    }

    #[test]
    fn focused_terminal_panel_ctrl_w_returns_focus_to_editor() {
        let mut panel_manager = PanelManager::default();
        let mut terminal_float_manager = TerminalFloatManager::default();
        panel_manager.open(PanelOpenRequest {
            id: "ai-agent".to_string(),
            position: PanelPosition::Right,
            size: PanelSize::Percent(35),
            content: PanelContent::Terminal {
                terminal_id: 77,
                close_behavior: PanelCloseBehavior::Detach,
            },
            focus: true,
        });

        assert_eq!(panel_manager.focused_terminal_id(), Some(77));
        assert_eq!(
            handle_terminal_panel_key(
                &mut panel_manager,
                &mut terminal_float_manager,
                &KeyInput::Ctrl('w')
            ),
            Some(FloatingWindowKeyHandling::Consumed)
        );
        assert_eq!(panel_manager.focused_panel_id(), None);
        assert_eq!(panel_manager.focused_terminal_id(), None);
    }

    #[test]
    fn focused_terminal_panel_colon_enters_editor_command_line() {
        let mut panel_manager = PanelManager::default();
        panel_manager.open(PanelOpenRequest {
            id: "ai-agent".to_string(),
            position: PanelPosition::Right,
            size: PanelSize::Percent(35),
            content: PanelContent::Terminal {
                terminal_id: 77,
                close_behavior: PanelCloseBehavior::Detach,
            },
            focus: true,
        });

        assert_eq!(panel_manager.focused_terminal_id(), Some(77));
        assert_eq!(
            begin_command_line_from_focused_panel(
                &mut panel_manager,
                &KeyInput::Char(':'),
                CoreMode::Normal
            ),
            Some(':')
        );
        assert_eq!(panel_manager.focused_panel_id(), None);
        assert_eq!(panel_manager.focused_terminal_id(), None);
    }

    #[test]
    fn focused_terminal_panel_search_enters_editor_command_line() {
        let mut panel_manager = PanelManager::default();
        panel_manager.open(PanelOpenRequest {
            id: "ai-agent".to_string(),
            position: PanelPosition::Right,
            size: PanelSize::Percent(35),
            content: PanelContent::Terminal {
                terminal_id: 77,
                close_behavior: PanelCloseBehavior::Detach,
            },
            focus: true,
        });

        assert_eq!(
            begin_command_line_from_focused_panel(
                &mut panel_manager,
                &KeyInput::Char('/'),
                CoreMode::Normal
            ),
            Some('/')
        );
        assert_eq!(panel_manager.focused_terminal_id(), None);
    }

    #[test]
    fn focused_terminal_panel_plain_text_stays_terminal_input() {
        let mut panel_manager = PanelManager::default();
        panel_manager.open(PanelOpenRequest {
            id: "ai-agent".to_string(),
            position: PanelPosition::Right,
            size: PanelSize::Percent(35),
            content: PanelContent::Terminal {
                terminal_id: 77,
                close_behavior: PanelCloseBehavior::Detach,
            },
            focus: true,
        });

        assert_eq!(
            begin_command_line_from_focused_panel(
                &mut panel_manager,
                &KeyInput::Char('x'),
                CoreMode::Normal
            ),
            None
        );
        assert_eq!(panel_manager.focused_terminal_id(), Some(77));
    }

    #[test]
    fn focused_view_panel_does_not_enter_terminal_input_semantics() {
        let mut panel_manager = PanelManager::default();
        let mut terminal_float_manager = TerminalFloatManager::default();
        panel_manager.open(PanelOpenRequest {
            id: "dashboard".to_string(),
            position: PanelPosition::Right,
            size: PanelSize::Percent(35),
            content: PanelContent::View {
                nodes: vec![PanelNode::Text {
                    text: "status".to_string(),
                }],
            },
            focus: true,
        });

        assert_eq!(panel_manager.focused_panel_id(), Some("dashboard"));
        assert_eq!(panel_manager.focused_terminal_id(), None);
        assert_eq!(
            handle_terminal_panel_key(
                &mut panel_manager,
                &mut terminal_float_manager,
                &KeyInput::Ctrl('w')
            ),
            None
        );
        assert_eq!(
            begin_command_line_from_focused_panel(
                &mut panel_manager,
                &KeyInput::Char(':'),
                CoreMode::Normal
            ),
            None
        );
        assert_eq!(panel_manager.focused_panel_id(), Some("dashboard"));
    }

    #[test]
    fn runtime_panel_open_accepts_view_content_and_lists_rendered_view_panel() {
        let mut panel_manager = PanelManager::default();
        let snapshot = execute_runtime_panel_open(
            RuntimePanelOpenRequest {
                id: "dashboard".to_string(),
                position: "right".to_string(),
                size: "35%".to_string(),
                content: RuntimePanelContentRequest {
                    kind: "view".to_string(),
                    command: Vec::new(),
                    lines: Vec::new(),
                    nodes: vec![
                        RuntimePanelNodeRequest {
                            node_type: "heading".to_string(),
                            text: Some("Weather".to_string()),
                            label: None,
                            src: None,
                            alt: None,
                            value: None,
                        },
                        RuntimePanelNodeRequest {
                            node_type: "progress".to_string(),
                            text: None,
                            label: Some("build".to_string()),
                            src: None,
                            alt: None,
                            value: Some(140),
                        },
                    ],
                    close_behavior: None,
                },
                focus: true,
            },
            Some(&mut panel_manager),
            None,
        )
        .expect("runtime panel open should accept view content");

        assert_eq!(snapshot.id, "dashboard");
        assert_eq!(snapshot.kind, "view");
        assert!(snapshot.focused);
        assert_eq!(panel_manager.focused_terminal_id(), None);
        assert_eq!(panel_manager.snapshots()[0].kind, "view");
        assert_eq!(
            panel_manager.resolve_screen_models(100, 30)[0].lines,
            vec!["Weather".to_string(), "build [##########] 100%".to_string()]
        );
    }

    #[test]
    fn startup_keymap_action_for_input_resolves_pending_two_key_sequence() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let keymaps = vec![saya::app::bootstrap::StartupKeymapSnapshot {
            mode: StartupKeymapMode::Normal,
            lhs: "gr".to_string(),
            action: StartupKeymapAction::RegisteredCommand("dired.refresh".to_string()),
        }];
        let mut bridge = saya::core::bridge::CoreBridge::new("README.md\nsrc/\n")
            .expect("core bridge should initialize");

        bridge.dispatch_key("g").expect("g should become pending");

        let mut startup_keymap_pending_lhs = None;
        assert_eq!(
            startup_keymap_action_for_snapshot_input(
                &keymaps,
                &bridge.light_snapshot(),
                &KeyInput::Char('r'),
                &mut startup_keymap_pending_lhs,
            ),
            Some(StartupKeymapAction::RegisteredCommand(
                "dired.refresh".to_string()
            ))
        );
    }

    #[test]
    fn startup_keymap_action_for_input_tracks_custom_two_key_prefix() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let keymaps = vec![
            saya::app::bootstrap::StartupKeymapSnapshot {
                mode: StartupKeymapMode::Normal,
                lhs: "sg".to_string(),
                action: StartupKeymapAction::RegisteredCommand("selector.rg".to_string()),
            },
            saya::app::bootstrap::StartupKeymapSnapshot {
                mode: StartupKeymapMode::Normal,
                lhs: "sr".to_string(),
                action: StartupKeymapAction::RegisteredCommand("selector.resume".to_string()),
            },
        ];
        let bridge = saya::core::bridge::CoreBridge::new("README.md\n")
            .expect("core bridge should initialize");
        let snapshot = bridge.light_snapshot();
        let mut pending_lhs = None;

        assert_eq!(
            startup_keymap_action_for_snapshot_input(
                &keymaps,
                &snapshot,
                &KeyInput::Char('s'),
                &mut pending_lhs,
            ),
            None
        );
        assert_eq!(pending_lhs.as_deref(), Some("s"));

        assert_eq!(
            startup_keymap_action_for_snapshot_input(
                &keymaps,
                &snapshot,
                &KeyInput::Char('r'),
                &mut pending_lhs,
            ),
            Some(StartupKeymapAction::RegisteredCommand(
                "selector.resume".to_string()
            ))
        );
        assert_eq!(pending_lhs, None);
    }

    #[test]
    fn runtime_current_buffer_snapshot_includes_cursor_line_for_dired_navigation() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let target_path = unique_path("runtime-buffer-snapshot-current-line");
        std::fs::write(&target_path, "README.md\nsrc/\n").expect("test file");
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::File(target_path.clone()),
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        outcome.core_bridge.dispatch_key("j").expect("move to src");
        let mut host_session = MainRuntimeHostSession::new(&mut outcome, &mut session_state);

        let snapshot = host_session.current_buffer_snapshot();

        assert_eq!(snapshot.cursor_row, 1);
        assert_eq!(snapshot.cursor_col, 0);
        assert_eq!(snapshot.current_line, "src/");
        std::fs::remove_file(target_path).expect("cleanup");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runtime_current_filer_entry_uses_directory_metadata_not_rendered_text() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("runtime-current-filer-entry-root");
        let nested_path = root_path.join("src");
        let readme_path = root_path.join("README.md");
        std::fs::create_dir_all(&nested_path).expect("nested directory");
        std::fs::write(&readme_path, "hello\n").expect("readme file");
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();

        execute_runtime_host_command(
            &format!("edit {}", root_path.display()),
            &mut outcome,
            &mut session_state,
        )
        .expect("open root listing");
        outcome
            .core_bridge
            .dispatch_key("i")
            .expect("enter insert mode");
        outcome
            .core_bridge
            .dispatch_key("BROKEN-")
            .expect("mutate rendered listing text");
        outcome
            .core_bridge
            .dispatch_key("\x1b")
            .expect("normal mode");
        let mut host_session = MainRuntimeHostSession::new(&mut outcome, &mut session_state);

        let entry = host_session
            .current_filer_entry()
            .expect("directory metadata should resolve current entry")
            .expect("cursor row should map to directory entry");

        assert_eq!(entry.name, "src");
        assert_eq!(entry.path, nested_path.to_string_lossy());
        assert_eq!(
            entry.kind,
            saya::runtime::live::RuntimeFilerEntryKind::Directory
        );

        std::fs::remove_dir_all(root_path).expect("cleanup root directory");
    }

    #[test]
    fn runtime_current_filer_entry_is_none_for_regular_file_buffer() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let target_path = unique_path("runtime-current-filer-entry-file");
        std::fs::write(&target_path, "hello\n").expect("test file");
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::File(target_path.clone()),
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        let mut host_session = MainRuntimeHostSession::new(&mut outcome, &mut session_state);

        let entry = host_session
            .current_filer_entry()
            .expect("regular file should not fail metadata lookup");

        assert_eq!(entry, None);
        std::fs::remove_file(target_path).expect("cleanup file");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runtime_host_command_executor_routes_quit_family_through_coordinator() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let target_path = unique_path("runtime-host-command");
        std::fs::write(&target_path, "initial\n").expect("test file");

        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::File(target_path.clone()),
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();

        outcome.core_bridge.dispatch_key("i").unwrap();
        outcome.core_bridge.dispatch_key("X").unwrap();
        outcome.core_bridge.dispatch_key("\x1b").unwrap();
        sync_session_dirty_from_core(&mut session_state, &outcome.core_bridge);

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
            [saya::runtime::live::RuntimeEventPayload::BufferWritePost(_)]
        ));
        assert_eq!(
            std::fs::read_to_string(&target_path).expect("saved file should exist"),
            "Xinitial\n"
        );

        std::fs::remove_file(&target_path).expect("cleanup");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn save_then_dirty_sync_keeps_normal_quit_allowed() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let target_path = unique_path("save-then-quit-clean");
        std::fs::write(&target_path, "initial\n").expect("test file");

        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::File(target_path.clone()),
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        let mut outcome_accumulator = MainOutcomeAccumulator::default();
        let mut transient_msg = None;
        let mut system_warning = None;
        let mut host_action_runtime = HostActionRuntime::default();
        let mut runtime_presentation_intents = Vec::new();
        let mut need_redraw = false;

        outcome.core_bridge.dispatch_key("i").expect("enter insert");
        outcome.core_bridge.dispatch_key("X").expect("insert text");
        outcome
            .core_bridge
            .dispatch_key("\x1b")
            .expect("leave insert");
        consume_core_outcomes_from_core(
            &mut outcome.core_bridge,
            &mut outcome_accumulator,
            &mut need_redraw,
        );
        sync_session_dirty_from_core(&mut session_state, &outcome.core_bridge);
        assert!(session_state.is_dirty(), "edit should make session dirty");

        outcome
            .core_bridge
            .apply_ex_command(":write")
            .expect(":write should be accepted");
        consume_core_outcomes_from_core(
            &mut outcome.core_bridge,
            &mut outcome_accumulator,
            &mut need_redraw,
        );
        let save_shutdown = process_pending_host_actions_with_runtime(
            &mut outcome,
            &mut outcome_accumulator,
            &mut session_state,
            &mut transient_msg,
            &mut system_warning,
            &mut host_action_runtime,
            None,
            &mut need_redraw,
            &mut runtime_presentation_intents,
            None,
        )
        .await;
        assert_eq!(save_shutdown, None);
        assert_eq!(transient_msg, Some("Saved successfully".to_string()));
        assert_eq!(
            std::fs::read_to_string(&target_path).expect("saved file"),
            "Xinitial\n"
        );

        sync_session_dirty_from_core(&mut session_state, &outcome.core_bridge);
        assert!(
            !session_state.is_dirty(),
            "stale core dirty at the saved revision must not re-dirty the session"
        );

        outcome
            .core_bridge
            .apply_ex_command(":quit")
            .expect(":quit should be accepted");
        consume_core_outcomes_from_core(
            &mut outcome.core_bridge,
            &mut outcome_accumulator,
            &mut need_redraw,
        );
        sync_session_dirty_from_core(&mut session_state, &outcome.core_bridge);
        let quit_shutdown = process_pending_host_actions_with_runtime(
            &mut outcome,
            &mut outcome_accumulator,
            &mut session_state,
            &mut transient_msg,
            &mut system_warning,
            &mut host_action_runtime,
            None,
            &mut need_redraw,
            &mut runtime_presentation_intents,
            None,
        )
        .await;

        assert_eq!(quit_shutdown, Some(ShutdownReason::UserQuit));
        assert_eq!(system_warning, None);
        std::fs::remove_file(target_path).expect("cleanup");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runtime_host_command_executor_drains_vfs_until_directory_listing_loads() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("runtime-host-command-directory");
        let nested_path = root_path.join("src");
        let readme_path = root_path.join("README.md");
        std::fs::create_dir_all(&nested_path).expect("test directory");
        std::fs::write(&readme_path, "hello\n").expect("test file");

        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();

        execute_runtime_host_command(
            &format!("edit {}", root_path.display()),
            &mut outcome,
            &mut session_state,
        )
        .expect("runtime edit command should load directory listing");

        assert_eq!(outcome.core_bridge.snapshot().text, "src/\nREADME.md\n");

        std::fs::remove_file(readme_path).expect("cleanup file");
        std::fs::remove_dir(nested_path).expect("cleanup nested directory");
        std::fs::remove_dir(root_path).expect("cleanup root directory");
    }

    fn dired_phase1_config_source() -> &'static str {
        r#"
            const trimTrailingSlash = (path) => path.length > 1 && path.endsWith("/") ? path.slice(0, -1) : path;
            const dirname = (path) => {
                const normalized = trimTrailingSlash(path || ".");
                const index = normalized.lastIndexOf("/");
                if (index < 0) return ".";
                return index === 0 ? "/" : normalized.slice(0, index);
            };
            saya.commands.register("dired.enter", async () => {
                const entry = await saya.filer.currentEntry();
                if (entry) {
                    await saya.commands.execute(`edit ${entry.path}`);
                }
            });
            saya.commands.register("dired.up", async () => {
                const trimTrailingSlash = (path) => path.length > 1 && path.endsWith("/") ? path.slice(0, -1) : path;
                const dirname = (path) => {
                    const normalized = trimTrailingSlash(path || ".");
                    const index = normalized.lastIndexOf("/");
                    if (index < 0) return ".";
                    return index === 0 ? "/" : normalized.slice(0, index);
                };
                const buffer = await saya.buffer.current();
                await saya.commands.execute(`edit ${dirname(buffer.path || ".")}`);
            });
            saya.commands.register("dired.refresh", async () => {
                const buffer = await saya.buffer.current();
                await saya.commands.execute(`edit ${buffer.path || "."}`);
            });
            saya.keymap.set("normal", "-", saya.commands.execute("dired.up"));
            saya.keymap.set("normal", "<Enter>", saya.commands.execute("dired.enter"));
            saya.keymap.set("normal", "gr", saya.commands.execute("dired.refresh"));
        "#
    }

    fn dired_phase3_config_source() -> &'static str {
        r#"
            saya.commands.register("dired.createFile", async () => {
                const buffer = await saya.buffer.current();
                await saya.filer.createFile(`${buffer.path}/created.txt`);
            });
            saya.commands.register("dired.createDirectory", async () => {
                const buffer = await saya.buffer.current();
                await saya.filer.createDirectory(`${buffer.path}/created-dir`);
            });
            saya.commands.register("dired.rename", async () => {
                const entry = await saya.filer.currentEntry();
                await saya.filer.rename(entry.path, `${entry.rootPath}/renamed.txt`);
            });
            saya.commands.register("dired.deleteConfirmed", async () => {
                const entry = await saya.filer.currentEntry();
                await saya.filer.delete(entry.path, { confirm: true });
            });
            saya.commands.register("dired.deleteWithoutConfirm", async () => {
                const entry = await saya.filer.currentEntry();
                await saya.filer.delete(entry.path);
            });
            saya.commands.register("dired.createFileCollision", async () => {
                const buffer = await saya.buffer.current();
                await saya.filer.createFile(`${buffer.path}/existing.txt`);
            });
            saya.commands.register("dired.renameMissing", async () => {
                const buffer = await saya.buffer.current();
                await saya.filer.rename(`${buffer.path}/missing.txt`, `${buffer.path}/never.txt`);
            });
        "#
    }

    fn dired_phase4_config_source() -> &'static str {
        r#"
            saya.commands.register("dired.mark", async () => {
                const entry = await saya.filer.currentEntry();
                if (entry) {
                    await saya.filer.mark(entry.path);
                }
            });
            saya.commands.register("dired.unmark", async () => {
                const entry = await saya.filer.currentEntry();
                if (entry) {
                    await saya.filer.unmark(entry.path);
                }
            });
            saya.commands.register("dired.clearMarks", async () => {
                await saya.filer.clearMarks();
            });
            saya.commands.register("dired.bulkDeletePreview", async () => {
                await saya.filer.bulkDeletePreview();
            });
            saya.commands.register("dired.bulkDeleteWithoutPreview", async () => {
                await saya.filer.bulkDelete({ confirm: true, previewId: "stale" });
            });
        "#
    }

    fn dired_phase12_config_source() -> &'static str {
        r#"
            saya.commands.register("dired.filterRust", async () => {
                const buffer = await saya.buffer.current();
                await saya.filer.list(buffer.path || ".", {
                    showHidden: false,
                    sortBy: "name",
                    filter: "rs",
                });
            });
            saya.commands.register("dired.createFilteredRust", async () => {
                const buffer = await saya.buffer.current();
                await saya.filer.createFile(`${buffer.path}/beta.rs`);
            });
        "#
    }

    fn prepare_dired_runtime_fixture(
        config_name: &str,
        config_source: &str,
    ) -> (
        PathBuf,
        saya::app::bootstrap::BootstrapOutcome,
        saya::app::session::EditorSessionState,
        RuntimeSessionOwner,
    ) {
        let config_path = unique_path(config_name).with_extension("ts");
        std::fs::write(&config_path, config_source).expect("config file");
        let outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::File(config_path.clone()),
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let session_state = outcome.editor_session_state();
        let runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
            .expect("runtime session should initialize");
        (config_path, outcome, session_state, runtime_session)
    }

    fn open_dired_listing_for_test(
        root_path: &std::path::Path,
        outcome: &mut saya::app::bootstrap::BootstrapOutcome,
        session_state: &mut saya::app::session::EditorSessionState,
    ) {
        execute_runtime_host_command(
            &format!("edit {}", root_path.display()),
            outcome,
            session_state,
        )
        .expect("open directory listing");
    }

    fn directory_buffer_display_texts_for_test(
        session_state: &saya::app::session::EditorSessionState,
    ) -> Vec<String> {
        session_state
            .directory_buffer()
            .expect("directory metadata should remain active")
            .entries
            .iter()
            .map(|entry| entry.display_text.clone())
            .collect()
    }

    fn assert_directory_listing_state(
        outcome: &saya::app::bootstrap::BootstrapOutcome,
        session_state: &saya::app::session::EditorSessionState,
        root_path: &std::path::Path,
        expected_text: &str,
    ) {
        assert_eq!(outcome.target_path.as_deref(), Some(root_path));
        assert_eq!(
            session_state.target_path().map(PathBuf::as_path),
            Some(root_path)
        );
        assert_eq!(outcome.core_bridge.snapshot().text, expected_text);
        let expected_entries = expected_text
            .lines()
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>();
        assert_eq!(
            directory_buffer_display_texts_for_test(session_state),
            expected_entries
        );
    }

    async fn execute_runtime_command_for_test(
        outcome: &mut saya::app::bootstrap::BootstrapOutcome,
        session_state: &mut saya::app::session::EditorSessionState,
        runtime_session: &mut RuntimeSessionOwner,
        command_name: &str,
    ) {
        let mut transient_msg = None;
        let mut need_redraw = false;
        let mut runtime_presentation_intents = Vec::new();
        let mut floating_window_manager = FloatingWindowManager::default();
        let mut completion_float_manager = CompletionFloatManager::default();
        let mut lsp_diagnostic_store = LspDiagnosticStore::default();
        let mut terminal_float_manager = TerminalFloatManager::default();
        let mut panel_manager = PanelManager::default();
        let shutdown = execute_startup_keymap_registered_command(
            Some(runtime_session),
            command_name,
            outcome,
            session_state,
            &mut floating_window_manager,
            &mut completion_float_manager,
            &mut lsp_diagnostic_store,
            &mut terminal_float_manager,
            &mut panel_manager,
            None,
            &mut transient_msg,
            &mut need_redraw,
            &mut runtime_presentation_intents,
            None,
        )
        .await;

        assert_eq!(shutdown, None);
        assert_eq!(transient_msg, None);
    }

    async fn execute_runtime_command_outcome_for_test(
        outcome: &mut saya::app::bootstrap::BootstrapOutcome,
        session_state: &mut saya::app::session::EditorSessionState,
        runtime_session: &mut RuntimeSessionOwner,
        command_name: &str,
    ) -> (Option<String>, bool) {
        let mut transient_msg = None;
        let mut need_redraw = false;
        let mut runtime_presentation_intents = Vec::new();
        let mut floating_window_manager = FloatingWindowManager::default();
        let mut completion_float_manager = CompletionFloatManager::default();
        let mut lsp_diagnostic_store = LspDiagnosticStore::default();
        let mut terminal_float_manager = TerminalFloatManager::default();
        let mut panel_manager = PanelManager::default();
        let shutdown = execute_startup_keymap_registered_command(
            Some(runtime_session),
            command_name,
            outcome,
            session_state,
            &mut floating_window_manager,
            &mut completion_float_manager,
            &mut lsp_diagnostic_store,
            &mut terminal_float_manager,
            &mut panel_manager,
            None,
            &mut transient_msg,
            &mut need_redraw,
            &mut runtime_presentation_intents,
            None,
        )
        .await;

        assert_eq!(shutdown, None);
        assert!(runtime_presentation_intents.is_empty());
        (transient_msg, need_redraw)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runtime_buffer_changed_event_dispatches_after_text_revision_changes() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let target_path = unique_path("runtime-buffer-changed-target");
        std::fs::write(&target_path, "initial\n").expect("target file");
        let seed =
            saya::runtime::callback_registry_seed::CallbackRegistrySeed::from_startup_entries(
                vec![saya::runtime::config::StartupRegistryEntry::Event {
                    name: "bufferChanged".to_string(),
                    callback_source: r#"
                            async () => {
                                await saya.commands.execute("write");
                            }
                        "#
                    .to_string(),
                }],
            );
        let mut runtime_session =
            RuntimeSessionOwner::spawn(seed).expect("runtime owner should initialize");
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::File(target_path.clone()),
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        let mut transient_msg = None;
        let mut need_redraw = false;
        let mut runtime_presentation_intents = Vec::new();
        let mut floating_window_manager = FloatingWindowManager::default();
        let mut completion_float_manager = CompletionFloatManager::default();
        let mut lsp_diagnostic_store = LspDiagnosticStore::default();
        let mut terminal_float_manager = TerminalFloatManager::default();
        let mut panel_manager = PanelManager::default();

        let before = outcome.core_bridge.light_snapshot();
        outcome.core_bridge.dispatch_key("i").expect("enter insert");
        outcome.core_bridge.dispatch_key("x").expect("insert text");
        let after = outcome.core_bridge.light_snapshot();
        assert_ne!(before.revision, after.revision);
        assert!(
            after.dirty,
            "bufferChanged precondition should leave the edited buffer dirty before dispatch"
        );
        assert_eq!(
            std::fs::read_to_string(&target_path).expect("target file before event"),
            "initial\n",
            "the event side effect must be what changes the file on disk"
        );

        let shutdown = dispatch_buffer_changed_with_runtime(
            Some(&mut runtime_session),
            &mut outcome,
            &mut session_state,
            &mut transient_msg,
            &mut need_redraw,
            &mut runtime_presentation_intents,
            &mut floating_window_manager,
            &mut completion_float_manager,
            &mut lsp_diagnostic_store,
            &mut terminal_float_manager,
            &mut panel_manager,
            None,
        )
        .await;

        assert_eq!(shutdown, None);
        assert_eq!(transient_msg, Some("Saved successfully".to_string()));
        assert!(need_redraw, "runtime event dispatch should request redraw");
        assert_eq!(
            std::fs::read_to_string(&target_path).expect("target file after event"),
            outcome.core_bridge.snapshot().text,
            "bufferChanged callback should save the edited buffer contents"
        );
        assert!(
            !session_state.is_dirty(),
            "successful event-triggered write should clear session dirty state"
        );

        std::fs::remove_file(target_path).expect("cleanup target file");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dired_enter_opens_directory_entry_from_current_line() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("dired-enter-directory-root");
        let nested_path = root_path.join("src");
        let nested_file = nested_path.join("mod.rs");
        let readme_path = root_path.join("README.md");
        let config_path = unique_path("dired-enter-directory-init").with_extension("ts");
        std::fs::create_dir_all(&nested_path).expect("nested directory");
        std::fs::write(&nested_file, "mod\n").expect("nested file");
        std::fs::write(&readme_path, "hello\n").expect("readme file");
        std::fs::write(&config_path, dired_phase1_config_source()).expect("config file");
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::File(config_path.clone()),
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
            .expect("runtime session should initialize");

        execute_runtime_host_command(
            &format!("edit {}", root_path.display()),
            &mut outcome,
            &mut session_state,
        )
        .expect("open root listing");
        execute_runtime_command_for_test(
            &mut outcome,
            &mut session_state,
            &mut runtime_session,
            "dired.enter",
        )
        .await;

        assert_eq!(outcome.target_path, Some(nested_path.clone()));
        assert_eq!(outcome.core_bridge.snapshot().text, "mod.rs\n");

        std::fs::remove_file(config_path).expect("cleanup config");
        std::fs::remove_dir_all(root_path).expect("cleanup root directory");
    }

    async fn assert_dired_open_in_split_keeps_inactive_shared_buffer_unchanged(
        split_command: &str,
        fixture_name: &str,
    ) {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path(&format!("dired-open-{fixture_name}-root"));
        let nested_path = root_path.join("src");
        let readme_path = root_path.join("README.md");
        let target_path = root_path.join("notes.txt");
        let config_path =
            unique_path(&format!("dired-open-{fixture_name}-init")).with_extension("ts");
        std::fs::create_dir_all(&nested_path).expect("nested directory");
        std::fs::write(&readme_path, "hello\n").expect("readme file");
        std::fs::write(&target_path, "notes\n").expect("target file");
        std::fs::write(
            &config_path,
            r#"
                saya.commands.register("dired.open", async () => {
                    const buffer = await saya.buffer.current();
                    const currentPath = buffer.path || ".";
                    const directory = currentPath.endsWith("/")
                        ? (currentPath.slice(0, -1) || "/")
                        : (currentPath.lastIndexOf("/") >= 0 ? currentPath.slice(0, currentPath.lastIndexOf("/")) || "/" : ".");
                    await saya.commands.execute(`edit ${directory}`);
                });
            "#,
        )
        .expect("config file");

        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::File(target_path.clone()),
            config_source: saya::app::cli::ConfigSource::File(config_path.clone()),
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
            .expect("runtime session should initialize");

        outcome
            .core_bridge
            .apply_ex_command(split_command)
            .unwrap_or_else(|_| panic!("{split_command} should succeed"));
        let split_snapshot = outcome.core_bridge.snapshot();
        assert_eq!(split_snapshot.windows.len(), 2);
        let inactive_window = split_snapshot
            .windows
            .iter()
            .find(|window| !window.is_active)
            .expect("split should leave an inactive window")
            .clone();
        assert_eq!(
            split_snapshot
                .active_window()
                .expect("split should keep an active window")
                .buf_id,
            inactive_window.buf_id,
            "vsplit starts with both windows displaying the same buffer"
        );

        execute_runtime_command_for_test(
            &mut outcome,
            &mut session_state,
            &mut runtime_session,
            "dired.open",
        )
        .await;

        let after = outcome.core_bridge.snapshot();
        let active_window = after
            .active_window()
            .expect("dired.open should leave an active window");
        let inactive_after = after
            .window(inactive_window.id)
            .expect("inactive split window should stay open");
        assert_ne!(
            active_window.buf_id, inactive_after.buf_id,
            "dired.open must detach the active split before loading the directory"
        );
        let active_text = outcome.core_bridge.snapshot().text;
        assert!(
            active_text.contains("src/\n")
                && active_text.contains("README.md\n")
                && active_text.contains("notes.txt\n"),
            "active pane should show the directory listing: {active_text:?}"
        );
        let inactive_text = outcome
            .core_bridge
            .buffer_line_range(inactive_after.buf_id, 0, 16)
            .expect("inactive buffer text should remain readable")
            .lines
            .join("\n");
        assert_eq!(
            inactive_text, "notes",
            "inactive split pane should keep the original file buffer"
        );

        std::fs::remove_file(config_path).expect("cleanup config");
        std::fs::remove_dir_all(root_path).expect("cleanup root directory");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dired_open_in_vertical_split_keeps_inactive_shared_buffer_unchanged() {
        assert_dired_open_in_split_keeps_inactive_shared_buffer_unchanged(":vsplit", "vsplit")
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dired_open_in_horizontal_split_keeps_inactive_shared_buffer_unchanged() {
        assert_dired_open_in_split_keeps_inactive_shared_buffer_unchanged(":split", "split").await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dired_open_in_split_keeps_inactive_markdown_highlight_metadata() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("dired-open-markdown-highlight-root");
        let target_path = root_path.join("AGENTS.md");
        let config_path = unique_path("dired-open-markdown-highlight-init").with_extension("ts");
        std::fs::create_dir_all(&root_path).expect("root directory");
        std::fs::write(
            &target_path,
            "# AGENTS.md\n\n## Project\n\n- keep markdown metadata visible\n",
        )
        .expect("markdown file");
        std::fs::write(
            &config_path,
            r#"
                saya.commands.register("dired.open", async () => {
                    const buffer = await saya.buffer.current();
                    const currentPath = buffer.path || ".";
                    const directory = currentPath.endsWith("/")
                        ? (currentPath.slice(0, -1) || "/")
                        : (currentPath.lastIndexOf("/") >= 0 ? currentPath.slice(0, currentPath.lastIndexOf("/")) || "/" : ".");
                    await saya.commands.execute(`edit ${directory}`);
                });
            "#,
        )
        .expect("config file");

        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::File(target_path.clone()),
            config_source: saya::app::cli::ConfigSource::File(config_path.clone()),
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
            .expect("runtime session should initialize");

        outcome
            .core_bridge
            .apply_ex_command(":vsplit")
            .expect("vsplit should succeed");
        let inactive_window = outcome
            .core_bridge
            .snapshot()
            .windows
            .iter()
            .find(|window| !window.is_active)
            .expect("split should leave an inactive window")
            .clone();
        let mut markdown_metadata_cache = MarkdownMetadataCache::default();
        let before_dired = outcome.core_bridge.snapshot();
        let before_maps = collect_workspace_markdown_document_maps(
            &mut markdown_metadata_cache,
            &session_state,
            &outcome.core_bridge,
            &before_dired,
        );
        assert!(
            before_maps.contains_key(&inactive_window.id),
            "initial markdown render should populate metadata for the split buffer"
        );

        execute_runtime_command_for_test(
            &mut outcome,
            &mut session_state,
            &mut runtime_session,
            "dired.open",
        )
        .await;

        let after = outcome.core_bridge.snapshot();
        let active_window = after
            .active_window()
            .expect("dired.open should leave an active window");
        assert_ne!(
            active_window.buf_id, inactive_window.buf_id,
            "dired.open should detach the active pane from the shared markdown buffer"
        );

        let maps = collect_workspace_markdown_document_maps(
            &mut markdown_metadata_cache,
            &session_state,
            &outcome.core_bridge,
            &after,
        );

        assert!(
            maps.contains_key(&inactive_window.id),
            "inactive markdown pane should keep markdown metadata after active pane opens dired"
        );
        assert!(
            !maps.contains_key(&active_window.id),
            "active dired pane should not receive markdown metadata"
        );

        std::fs::remove_file(config_path).expect("cleanup config");
        std::fs::remove_dir_all(root_path).expect("cleanup root directory");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dired_enter_from_split_listing_keeps_inactive_file_buffer_unchanged() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("dired-enter-split-root");
        let nested_path = root_path.join("src");
        let readme_path = root_path.join("README.md");
        let target_path = root_path.join("notes.txt");
        let config_path = unique_path("dired-enter-split-init").with_extension("ts");
        std::fs::create_dir_all(&nested_path).expect("nested directory");
        std::fs::write(&readme_path, "hello\n").expect("readme file");
        std::fs::write(&target_path, "notes\n").expect("target file");
        std::fs::write(
            &config_path,
            r#"
                saya.commands.register("dired.open", async () => {
                    const dirname = (path) => {
                        const normalized = path.length > 1 && path.endsWith("/") ? path.slice(0, -1) : path;
                        const index = normalized.lastIndexOf("/");
                        if (index < 0) return ".";
                        return index === 0 ? "/" : normalized.slice(0, index);
                    };
                    const buffer = await saya.buffer.current();
                    await saya.commands.execute(`edit ${dirname(buffer.path || ".")}`);
                });
                saya.commands.register("dired.enter", async () => {
                    const entry = await saya.filer.currentEntry();
                    if (entry) {
                        await saya.commands.execute(`edit ${entry.path}`);
                    }
                });
            "#,
        )
        .expect("config file");

        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::File(target_path.clone()),
            config_source: saya::app::cli::ConfigSource::File(config_path.clone()),
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
            .expect("runtime session should initialize");

        outcome
            .core_bridge
            .apply_ex_command(":vsplit")
            .expect("vsplit should succeed");
        let inactive_window = outcome
            .core_bridge
            .snapshot()
            .windows
            .iter()
            .find(|window| !window.is_active)
            .expect("split should leave an inactive window")
            .clone();

        execute_runtime_command_for_test(
            &mut outcome,
            &mut session_state,
            &mut runtime_session,
            "dired.open",
        )
        .await;
        let readme_row = outcome
            .core_bridge
            .snapshot()
            .text
            .lines()
            .position(|line| line == "README.md")
            .expect("README.md should appear in the dired listing");
        for _ in 0..readme_row {
            outcome
                .core_bridge
                .dispatch_key("j")
                .expect("move in dired");
        }
        execute_runtime_command_for_test(
            &mut outcome,
            &mut session_state,
            &mut runtime_session,
            "dired.enter",
        )
        .await;

        assert_eq!(
            outcome.core_bridge.snapshot().text,
            "hello\n",
            "active dired pane should open the selected file"
        );
        let inactive_after = outcome
            .core_bridge
            .snapshot()
            .window(inactive_window.id)
            .expect("inactive split window should stay open")
            .clone();
        let inactive_text = outcome
            .core_bridge
            .buffer_line_range(inactive_after.buf_id, 0, 16)
            .expect("inactive buffer text should remain readable")
            .lines
            .join("\n");
        assert_eq!(
            inactive_text, "notes",
            "inactive split pane should keep the original file buffer after dired.enter"
        );

        std::fs::remove_file(config_path).expect("cleanup config");
        std::fs::remove_dir_all(root_path).expect("cleanup root directory");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dired_enter_opens_file_entry_from_current_line() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("dired-enter-file-root");
        let readme_path = root_path.join("README.md");
        let config_path = unique_path("dired-enter-file-init").with_extension("ts");
        std::fs::create_dir_all(&root_path).expect("root directory");
        std::fs::write(&readme_path, "hello\n").expect("readme file");
        std::fs::write(&config_path, dired_phase1_config_source()).expect("config file");
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::File(config_path.clone()),
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
            .expect("runtime session should initialize");

        execute_runtime_host_command(
            &format!("edit {}", root_path.display()),
            &mut outcome,
            &mut session_state,
        )
        .expect("open root listing");
        execute_runtime_command_for_test(
            &mut outcome,
            &mut session_state,
            &mut runtime_session,
            "dired.enter",
        )
        .await;

        assert_eq!(outcome.target_path, Some(readme_path.clone()));
        assert_eq!(outcome.core_bridge.snapshot().text, "hello\n");
        let mut markdown_metadata_cache = MarkdownMetadataCache::default();
        let snapshot = outcome.core_bridge.snapshot();
        let active_window = snapshot
            .active_window()
            .expect("dired.enter should leave an active markdown window");
        let active_buffer = snapshot
            .buffers
            .iter()
            .find(|buffer| buffer.id == active_window.buf_id)
            .expect("active buffer metadata should exist");
        assert_eq!(
            active_buffer.name,
            root_path.display().to_string(),
            "regression guard: dired-entered file keeps the stale directory buffer name"
        );
        assert!(
            active_buffer
                .document_id
                .as_deref()
                .is_some_and(|document_id| document_id.starts_with("file://")
                    && document_id.ends_with("README.md")),
            "dired VFS load should expose README.md through document_id"
        );
        let maps = collect_workspace_markdown_document_maps(
            &mut markdown_metadata_cache,
            &session_state,
            &outcome.core_bridge,
            &snapshot,
        );
        assert!(
            maps.contains_key(&active_window.id),
            "markdown file opened from dired should collect markdown metadata for highlighting; active_window={:?}, buffers={:?}, target_path={:?}, session_target={:?}",
            active_window,
            snapshot.buffers,
            outcome.target_path,
            session_state.target_path()
        );

        std::fs::remove_file(config_path).expect("cleanup config");
        std::fs::remove_dir_all(root_path).expect("cleanup root directory");
    }

    #[cfg(feature = "tree-sitter-syntax")]
    #[tokio::test(flavor = "current_thread")]
    async fn dired_enter_collects_tree_sitter_highlight_for_supported_languages() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for (file_name, source, expected_language) in [
            ("main.rs", "fn main() { let value = 1; }\n", "rust"),
            (
                "main.ts",
                "export function main(value: number): number { return value + 1; }\n",
                "typescript",
            ),
            ("main.go", "package main\n\nfunc main() {}\n", "go"),
            (
                "App.tsx",
                "export const App = () => <main>{1}</main>;\n",
                "tsx",
            ),
        ] {
            let root_path = unique_path(&format!("dired-enter-syntax-{expected_language}-root"));
            let source_path = root_path.join(file_name);
            let config_path = unique_path(&format!("dired-enter-syntax-{expected_language}-init"))
                .with_extension("ts");
            std::fs::create_dir_all(&root_path).expect("root directory");
            std::fs::write(&source_path, source).expect("source file");
            std::fs::write(&config_path, dired_phase1_config_source()).expect("config file");
            let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
                input_source: saya::app::cli::InputSource::Empty,
                config_source: saya::app::cli::ConfigSource::File(config_path.clone()),
                ..saya::app::cli::LaunchRequest::default()
            })
            .expect("launch should succeed");
            outcome.core_bridge.set_screen_size(24, 80);
            outcome
                .core_bridge
                .apply_ex_command("syntax on")
                .expect("syntax on should enable Tree-sitter highlight collection");
            let mut session_state = outcome.editor_session_state();
            let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
                .expect("runtime session should initialize");

            execute_runtime_host_command(
                &format!("edit {}", root_path.display()),
                &mut outcome,
                &mut session_state,
            )
            .expect("open root listing");
            execute_runtime_command_for_test(
                &mut outcome,
                &mut session_state,
                &mut runtime_session,
                "dired.enter",
            )
            .await;

            assert_eq!(outcome.target_path, Some(source_path.clone()));
            let snapshot = outcome.core_bridge.snapshot();
            let active_window = snapshot
                .active_window()
                .expect("dired.enter should leave an active source window");
            let active_buffer = snapshot
                .buffers
                .iter()
                .find(|buffer| buffer.id == active_window.buf_id)
                .expect("active buffer metadata should exist");
            assert_eq!(
                active_buffer.name,
                root_path.display().to_string(),
                "regression guard: dired-entered {file_name} keeps the stale directory buffer name"
            );
            assert!(
                active_buffer
                    .document_id
                    .as_deref()
                    .is_some_and(|document_id| document_id.starts_with("file://")
                        && document_id.ends_with(file_name)),
                "dired VFS load should expose the opened file through document_id"
            );
            let mut viewport_store = WindowViewportStore::new();
            let line_ranges =
                collect_workspace_line_ranges(&outcome.core_bridge, &snapshot, &viewport_store);
            let mut languages = BTreeSet::new();
            for _ in 0..20 {
                let syntax_by_window = collect_workspace_tree_sitter_syntax(
                    &mut outcome.core_bridge,
                    &snapshot,
                    &viewport_store,
                    &line_ranges,
                );
                languages = syntax_by_window
                    .get(&active_window.id)
                    .into_iter()
                    .map(|syntax| syntax.provenance.language_id.as_str())
                    .map(str::to_string)
                    .collect();
                if languages.contains(expected_language) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(
                languages.contains(expected_language),
                "dired-opened {file_name} should collect Tree-sitter syntax for {expected_language}; languages={languages:?}, active_buffer={active_buffer:?}"
            );
            let mut search_refresh_store = WindowSearchRefreshStore::default();
            let mut markdown_metadata_cache = MarkdownMetadataCache::default();
            let mut rendered_languages = BTreeSet::new();
            for _ in 0..20 {
                let workspace = build_workspace_render_output(
                    &mut outcome,
                    &mut session_state,
                    &mut viewport_store,
                    ViewportSyncMode::Core,
                    &mut search_refresh_store,
                    &mut markdown_metadata_cache,
                    None,
                    "",
                    0,
                    None,
                    None,
                    None,
                    None,
                    None,
                    80,
                    24,
                    None,
                    None,
                    None,
                    None,
                )
                .expect("dired-opened source workspace should render");
                rendered_languages = workspace
                    .panes
                    .iter()
                    .flat_map(|pane| pane.syntax_chunks.iter())
                    .filter_map(|chunk| chunk.language.as_deref())
                    .map(str::to_string)
                    .collect();
                if rendered_languages.contains(expected_language) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(
                rendered_languages.contains(expected_language),
                "dired-opened {file_name} should render Tree-sitter syntax for {expected_language}; rendered_languages={rendered_languages:?}"
            );

            std::fs::remove_file(config_path).expect("cleanup config");
            std::fs::remove_dir_all(root_path).expect("cleanup root directory");
        }
    }

    #[cfg(feature = "tree-sitter-syntax")]
    #[tokio::test(flavor = "current_thread")]
    async fn dired_open_then_enter_renders_tree_sitter_highlight_for_another_file() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for (file_name, source, expected_language) in [
            ("main.rs", "fn main() { let value = 1; }\n", "rust"),
            (
                "main.ts",
                "export function main(value: number): number { return value + 1; }\n",
                "typescript",
            ),
        ] {
            let root_path = unique_path(&format!("dired-open-enter-{expected_language}-root"));
            let initial_path = root_path.join("aaa.txt");
            let source_path = root_path.join(file_name);
            let config_path = unique_path(&format!("dired-open-enter-{expected_language}-init"))
                .with_extension("ts");
            std::fs::create_dir_all(&root_path).expect("root directory");
            std::fs::write(&initial_path, "initial\n").expect("initial file");
            std::fs::write(&source_path, source).expect("source file");
            std::fs::write(
                &config_path,
                r#"
                    saya.commands.register("dired.open", async () => {
                        const trimTrailingSlash = (path) => path.length > 1 && path.endsWith("/") ? path.slice(0, -1) : path;
                        const dirname = (path) => {
                            const normalized = trimTrailingSlash(path || ".");
                            const index = normalized.lastIndexOf("/");
                            if (index < 0) return ".";
                            return index === 0 ? "/" : normalized.slice(0, index);
                        };
                        const currentPath = await saya.buffer.currentPath() || ".";
                        await saya.commands.execute(`edit ${dirname(currentPath)}`);
                    });
                    saya.commands.register("dired.enter", async () => {
                        const entry = await saya.filer.currentEntry();
                        if (entry) {
                            await saya.commands.execute(`edit ${entry.path}`);
                        }
                    });
                    saya.keymap.set("normal", "-", saya.commands.execute("dired.open"));
                    saya.keymap.set("normal", "<Enter>", saya.commands.execute("dired.enter"));
                "#,
            )
            .expect("config file");

            let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
                input_source: saya::app::cli::InputSource::File(initial_path.clone()),
                config_source: saya::app::cli::ConfigSource::File(config_path.clone()),
                ..saya::app::cli::LaunchRequest::default()
            })
            .expect("launch should succeed");
            outcome.core_bridge.set_screen_size(24, 80);
            outcome
                .core_bridge
                .apply_ex_command("syntax on")
                .expect("syntax on should enable Tree-sitter highlight collection");
            let mut session_state = outcome.editor_session_state();
            let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
                .expect("runtime session should initialize");

            execute_runtime_command_for_test(
                &mut outcome,
                &mut session_state,
                &mut runtime_session,
                "dired.open",
            )
            .await;
            let target_row = outcome
                .core_bridge
                .snapshot()
                .text
                .lines()
                .position(|line| line == file_name)
                .unwrap_or_else(|| panic!("{file_name} should appear in dired listing"));
            for _ in 0..target_row {
                outcome
                    .core_bridge
                    .dispatch_key("j")
                    .expect("move in dired");
            }
            execute_runtime_command_for_test(
                &mut outcome,
                &mut session_state,
                &mut runtime_session,
                "dired.enter",
            )
            .await;

            assert_eq!(outcome.target_path, Some(source_path.clone()));
            let snapshot = outcome.core_bridge.snapshot();
            let active_window = snapshot
                .active_window()
                .expect("dired.enter should leave an active source window");
            let active_buffer = snapshot
                .buffers
                .iter()
                .find(|buffer| buffer.id == active_window.buf_id)
                .expect("active buffer metadata should exist");
            assert_eq!(
                active_buffer.name,
                root_path.display().to_string(),
                "regression guard: dired-open then dired-enter keeps stale directory buffer name"
            );
            assert!(
                active_buffer
                    .document_id
                    .as_deref()
                    .is_some_and(|document_id| document_id.starts_with("file://")
                        && document_id.ends_with(file_name)),
                "dired-open then dired-enter should expose the opened file through document_id"
            );

            let mut viewport_store = WindowViewportStore::new();
            let mut search_refresh_store = WindowSearchRefreshStore::default();
            let mut markdown_metadata_cache = MarkdownMetadataCache::default();
            let mut rendered_languages = BTreeSet::new();
            for _ in 0..20 {
                let workspace = build_workspace_render_output(
                    &mut outcome,
                    &mut session_state,
                    &mut viewport_store,
                    ViewportSyncMode::Core,
                    &mut search_refresh_store,
                    &mut markdown_metadata_cache,
                    None,
                    "",
                    0,
                    None,
                    None,
                    None,
                    None,
                    None,
                    80,
                    24,
                    None,
                    None,
                    None,
                    None,
                )
                .expect("dired-opened source workspace should render");
                rendered_languages = workspace
                    .panes
                    .iter()
                    .flat_map(|pane| pane.syntax_chunks.iter())
                    .filter_map(|chunk| chunk.language.as_deref())
                    .map(str::to_string)
                    .collect();
                if rendered_languages.contains(expected_language) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(
                rendered_languages.contains(expected_language),
                "dired-open then dired-enter should render Tree-sitter syntax for {expected_language}; rendered_languages={rendered_languages:?}, active_buffer={active_buffer:?}"
            );

            std::fs::remove_file(config_path).expect("cleanup config");
            std::fs::remove_dir_all(root_path).expect("cleanup root directory");
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dired_enter_keeps_directory_state_when_file_load_fails() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("dired-enter-load-failure-root");
        let binary_path = root_path.join("bad.bin");
        let config_path = unique_path("dired-enter-load-failure-init").with_extension("ts");
        std::fs::create_dir_all(&root_path).expect("root directory");
        std::fs::write(&binary_path, [0xff, 0xfe, 0xfd]).expect("binary file");
        std::fs::write(&config_path, dired_phase1_config_source()).expect("config file");
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::File(config_path.clone()),
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
            .expect("runtime session should initialize");

        execute_runtime_host_command(
            &format!("edit {}", root_path.display()),
            &mut outcome,
            &mut session_state,
        )
        .expect("open root listing");
        execute_runtime_command_for_test(
            &mut outcome,
            &mut session_state,
            &mut runtime_session,
            "dired.enter",
        )
        .await;

        assert_eq!(
            outcome.target_path,
            Some(root_path.clone()),
            "failed file loads must not retarget the host session away from the directory buffer"
        );
        assert_eq!(
            session_state.target_path().map(PathBuf::as_path),
            Some(root_path.as_path())
        );
        assert!(
            session_state.directory_buffer().is_some(),
            "dired metadata should remain active so filer styling and currentEntry keep working"
        );
        assert_eq!(outcome.core_bridge.snapshot().text, "bad.bin\n");

        std::fs::remove_file(config_path).expect("cleanup config");
        std::fs::remove_dir_all(root_path).expect("cleanup root directory");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dired_up_opens_parent_directory() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("dired-up-root");
        let child_path = root_path.join("child");
        let config_path = unique_path("dired-up-init").with_extension("ts");
        std::fs::create_dir_all(&child_path).expect("child directory");
        std::fs::write(&config_path, dired_phase1_config_source()).expect("config file");
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::File(config_path.clone()),
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
            .expect("runtime session should initialize");

        execute_runtime_host_command(
            &format!("edit {}", child_path.display()),
            &mut outcome,
            &mut session_state,
        )
        .expect("open child listing");
        execute_runtime_command_for_test(
            &mut outcome,
            &mut session_state,
            &mut runtime_session,
            "dired.up",
        )
        .await;

        assert_eq!(outcome.target_path, Some(root_path.clone()));
        assert!(outcome.core_bridge.snapshot().text.contains("child/\n"));

        std::fs::remove_file(config_path).expect("cleanup config");
        std::fs::remove_dir_all(root_path).expect("cleanup root directory");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dired_refresh_reloads_current_directory_listing() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("dired-refresh-root");
        let readme_path = root_path.join("README.md");
        let later_path = root_path.join("later.txt");
        let config_path = unique_path("dired-refresh-init").with_extension("ts");
        std::fs::create_dir_all(&root_path).expect("root directory");
        std::fs::write(&readme_path, "hello\n").expect("readme file");
        std::fs::write(&config_path, dired_phase1_config_source()).expect("config file");
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::File(config_path.clone()),
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
            .expect("runtime session should initialize");

        execute_runtime_host_command(
            &format!("edit {}", root_path.display()),
            &mut outcome,
            &mut session_state,
        )
        .expect("open root listing");
        assert!(!outcome.core_bridge.snapshot().text.contains("later.txt\n"));
        std::fs::write(&later_path, "later\n").expect("later file");
        execute_runtime_command_for_test(
            &mut outcome,
            &mut session_state,
            &mut runtime_session,
            "dired.refresh",
        )
        .await;

        assert!(outcome.core_bridge.snapshot().text.contains("later.txt\n"));

        std::fs::remove_file(config_path).expect("cleanup config");
        std::fs::remove_dir_all(root_path).expect("cleanup root directory");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dired_filter_projects_listing_and_current_entry_metadata() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("dired-filter-root");
        let alpha_path = root_path.join("alpha.rs");
        let notes_path = root_path.join("notes.txt");
        let hidden_path = root_path.join(".hidden.rs");
        let config_path = unique_path("dired-filter-init").with_extension("ts");
        std::fs::create_dir_all(&root_path).expect("root directory");
        std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
        std::fs::write(&notes_path, "notes\n").expect("notes file");
        std::fs::write(&hidden_path, "hidden\n").expect("hidden file");
        std::fs::write(&config_path, dired_phase12_config_source()).expect("config file");
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::File(config_path.clone()),
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
            .expect("runtime session should initialize");

        execute_runtime_host_command(
            &format!("edit {}", root_path.display()),
            &mut outcome,
            &mut session_state,
        )
        .expect("open root listing");
        execute_runtime_command_for_test(
            &mut outcome,
            &mut session_state,
            &mut runtime_session,
            "dired.filterRust",
        )
        .await;

        assert_eq!(outcome.core_bridge.snapshot().text, "alpha.rs\n");
        let entry = MainRuntimeHostSession::new(&mut outcome, &mut session_state)
            .current_filer_entry()
            .expect("current filer entry should resolve")
            .expect("filtered listing should keep a current entry");
        assert_eq!(entry.name, "alpha.rs");
        assert_eq!(entry.path, alpha_path.to_string_lossy());

        std::fs::remove_file(config_path).expect("cleanup config");
        std::fs::remove_dir_all(root_path).expect("cleanup root directory");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dired_filter_sort_and_hidden_state_survive_operation_refresh() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("dired-filter-refresh-root");
        let alpha_path = root_path.join("alpha.rs");
        let notes_path = root_path.join("notes.txt");
        let hidden_path = root_path.join(".hidden.rs");
        let beta_path = root_path.join("beta.rs");
        let config_path = unique_path("dired-filter-refresh-init").with_extension("ts");
        std::fs::create_dir_all(&root_path).expect("root directory");
        std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
        std::fs::write(&notes_path, "notes\n").expect("notes file");
        std::fs::write(&hidden_path, "hidden\n").expect("hidden file");
        std::fs::write(&config_path, dired_phase12_config_source()).expect("config file");
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::File(config_path.clone()),
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
            .expect("runtime session should initialize");

        execute_runtime_host_command(
            &format!("edit {}", root_path.display()),
            &mut outcome,
            &mut session_state,
        )
        .expect("open root listing");
        execute_runtime_command_for_test(
            &mut outcome,
            &mut session_state,
            &mut runtime_session,
            "dired.filterRust",
        )
        .await;
        execute_runtime_command_for_test(
            &mut outcome,
            &mut session_state,
            &mut runtime_session,
            "dired.createFilteredRust",
        )
        .await;

        assert!(beta_path.is_file());
        assert_eq!(outcome.core_bridge.snapshot().text, "alpha.rs\nbeta.rs\n");
        let entries = session_state
            .directory_buffer()
            .expect("directory metadata should remain active")
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(entries, vec!["alpha.rs", "beta.rs"]);

        std::fs::remove_file(config_path).expect("cleanup config");
        std::fs::remove_dir_all(root_path).expect("cleanup root directory");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dired_create_file_refreshes_directory_listing() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("dired-create-file-root");
        let created_path = root_path.join("created.txt");
        std::fs::create_dir_all(&root_path).expect("root directory");
        let (config_path, mut outcome, mut session_state, mut runtime_session) =
            prepare_dired_runtime_fixture("dired-create-file-init", dired_phase3_config_source());

        open_dired_listing_for_test(&root_path, &mut outcome, &mut session_state);
        assert_directory_listing_state(&outcome, &session_state, &root_path, "\n");
        execute_runtime_command_for_test(
            &mut outcome,
            &mut session_state,
            &mut runtime_session,
            "dired.createFile",
        )
        .await;

        assert!(created_path.is_file());
        assert_eq!(
            std::fs::read_to_string(&created_path).expect("created file contents"),
            ""
        );
        assert_directory_listing_state(&outcome, &session_state, &root_path, "created.txt\n");

        std::fs::remove_file(config_path).expect("cleanup config");
        std::fs::remove_dir_all(root_path).expect("cleanup root directory");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dired_create_directory_refreshes_directory_listing() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("dired-create-directory-root");
        let created_path = root_path.join("created-dir");
        std::fs::create_dir_all(&root_path).expect("root directory");
        let (config_path, mut outcome, mut session_state, mut runtime_session) =
            prepare_dired_runtime_fixture(
                "dired-create-directory-init",
                dired_phase3_config_source(),
            );

        open_dired_listing_for_test(&root_path, &mut outcome, &mut session_state);
        assert_directory_listing_state(&outcome, &session_state, &root_path, "\n");
        execute_runtime_command_for_test(
            &mut outcome,
            &mut session_state,
            &mut runtime_session,
            "dired.createDirectory",
        )
        .await;

        assert!(created_path.is_dir());
        assert_directory_listing_state(&outcome, &session_state, &root_path, "created-dir/\n");

        std::fs::remove_file(config_path).expect("cleanup config");
        std::fs::remove_dir_all(root_path).expect("cleanup root directory");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dired_rename_refreshes_directory_listing_and_preserves_cursor_target() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("dired-rename-root");
        let source_path = root_path.join("source.txt");
        let renamed_path = root_path.join("renamed.txt");
        std::fs::create_dir_all(&root_path).expect("root directory");
        std::fs::write(&source_path, "hello\n").expect("source file");
        let (config_path, mut outcome, mut session_state, mut runtime_session) =
            prepare_dired_runtime_fixture("dired-rename-init", dired_phase3_config_source());

        open_dired_listing_for_test(&root_path, &mut outcome, &mut session_state);
        assert_directory_listing_state(&outcome, &session_state, &root_path, "source.txt\n");
        execute_runtime_command_for_test(
            &mut outcome,
            &mut session_state,
            &mut runtime_session,
            "dired.rename",
        )
        .await;

        assert!(!source_path.exists());
        assert!(renamed_path.is_file());
        assert_eq!(
            std::fs::read_to_string(&renamed_path).expect("renamed file contents"),
            "hello\n"
        );
        let snapshot = outcome.core_bridge.snapshot();
        assert_eq!(snapshot.text, "renamed.txt\n");
        assert_eq!(snapshot.cursor_row, 0);
        assert_eq!(
            directory_buffer_display_texts_for_test(&session_state),
            vec!["renamed.txt".to_string()]
        );

        std::fs::remove_file(config_path).expect("cleanup config");
        std::fs::remove_dir_all(root_path).expect("cleanup root directory");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dired_copy_and_move_are_host_mediated_and_refresh_directory_listing() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("dired-copy-move-root");
        let source_path = root_path.join("source.txt");
        let copied_path = root_path.join("copied.txt");
        let moved_path = root_path.join("moved.txt");
        let directory_source_path = root_path.join("source-dir");
        let directory_moved_path = root_path.join("moved-dir");
        std::fs::create_dir_all(&root_path).expect("root directory");
        std::fs::create_dir_all(&directory_source_path).expect("source directory");
        std::fs::write(&source_path, "hello\n").expect("source file");
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();

        execute_runtime_host_command(
            &format!("edit {}", root_path.display()),
            &mut outcome,
            &mut session_state,
        )
        .expect("open root listing");
        let copy_report = execute_runtime_filer_operation(
            RuntimeFilerOperation::Copy {
                from: source_path.clone(),
                to: copied_path.clone(),
            },
            &mut outcome,
            &mut session_state,
        )
        .expect("copy should succeed through host-mediated filer operation");
        let move_report = execute_runtime_filer_operation(
            RuntimeFilerOperation::Move {
                from: copied_path.clone(),
                to: moved_path.clone(),
            },
            &mut outcome,
            &mut session_state,
        )
        .expect("move should succeed through host-mediated filer operation");
        let move_directory_report = execute_runtime_filer_operation(
            RuntimeFilerOperation::Move {
                from: directory_source_path.clone(),
                to: directory_moved_path.clone(),
            },
            &mut outcome,
            &mut session_state,
        )
        .expect("directory move should use the host rename path");

        assert_eq!(copy_report.operation, RuntimeFilerOperationKind::Copy);
        assert_eq!(move_report.operation, RuntimeFilerOperationKind::Move);
        assert_eq!(
            move_directory_report.operation,
            RuntimeFilerOperationKind::Move
        );
        assert_eq!(
            std::fs::read_to_string(&source_path).expect("source file remains after copy"),
            "hello\n"
        );
        assert!(
            !copied_path.exists(),
            "move should remove the intermediate path"
        );
        assert_eq!(
            std::fs::read_to_string(&moved_path).expect("moved file should exist"),
            "hello\n"
        );
        assert!(!directory_source_path.exists());
        assert!(directory_moved_path.is_dir());
        let snapshot = outcome.core_bridge.snapshot();
        assert!(snapshot.text.contains("source.txt\n"));
        assert!(snapshot.text.contains("moved.txt\n"));
        assert!(snapshot.text.contains("moved-dir/\n"));
        assert!(!snapshot.text.contains("copied.txt\n"));

        std::fs::remove_dir_all(root_path).expect("cleanup root directory");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dired_recursive_delete_and_trash_policy_fail_without_mutation() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("dired-recursive-delete-policy");
        let non_empty_dir = root_path.join("non-empty");
        let child_path = non_empty_dir.join("child.txt");
        let copy_dir = root_path.join("copy-dir");
        let copied_dir = root_path.join("copied-dir");
        let trash_target = root_path.join("trash-me.txt");
        std::fs::create_dir_all(&non_empty_dir).expect("nested directory");
        std::fs::create_dir_all(&copy_dir).expect("copy directory");
        std::fs::write(&child_path, "child\n").expect("child file");
        std::fs::write(&trash_target, "trash\n").expect("trash target");
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();

        execute_runtime_host_command(
            &format!("edit {}", root_path.display()),
            &mut outcome,
            &mut session_state,
        )
        .expect("open root listing");
        let recursive_without_opt_in = execute_runtime_filer_operation(
            RuntimeFilerOperation::Delete {
                path: non_empty_dir.clone(),
                confirm: true,
                recursive: false,
                trash: false,
            },
            &mut outcome,
            &mut session_state,
        );
        assert!(
            recursive_without_opt_in.is_err(),
            "non-empty directory delete must not silently become recursive"
        );
        assert!(child_path.is_file());

        let directory_copy = execute_runtime_filer_operation(
            RuntimeFilerOperation::Copy {
                from: copy_dir.clone(),
                to: copied_dir.clone(),
            },
            &mut outcome,
            &mut session_state,
        );
        assert!(matches!(
            directory_copy,
            Err(RuntimeFilerError::OperationFailed {
                kind: RuntimeFilerErrorKind::Unsupported,
                ..
            })
        ));
        assert!(copy_dir.is_dir());
        assert!(!copied_dir.exists());

        let trash_request = execute_runtime_filer_operation(
            RuntimeFilerOperation::Delete {
                path: trash_target.clone(),
                confirm: true,
                recursive: false,
                trash: true,
            },
            &mut outcome,
            &mut session_state,
        );
        assert!(matches!(
            trash_request,
            Err(RuntimeFilerError::OperationFailed {
                kind: RuntimeFilerErrorKind::Unsupported,
                ..
            })
        ));
        assert!(
            trash_target.is_file(),
            "unsupported trash backend must fail without deleting permanently"
        );

        std::fs::remove_dir_all(root_path).expect("cleanup root directory");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dired_delete_confirmed_single_file_refreshes_directory_listing() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("dired-delete-file-root");
        let delete_path = root_path.join("delete-me.txt");
        let keep_path = root_path.join("keep.txt");
        std::fs::create_dir_all(&root_path).expect("root directory");
        std::fs::write(&delete_path, "delete\n").expect("delete file");
        std::fs::write(&keep_path, "keep\n").expect("keep file");
        let (config_path, mut outcome, mut session_state, mut runtime_session) =
            prepare_dired_runtime_fixture("dired-delete-file-init", dired_phase3_config_source());

        open_dired_listing_for_test(&root_path, &mut outcome, &mut session_state);
        assert_directory_listing_state(
            &outcome,
            &session_state,
            &root_path,
            "delete-me.txt\nkeep.txt\n",
        );
        execute_runtime_command_for_test(
            &mut outcome,
            &mut session_state,
            &mut runtime_session,
            "dired.deleteConfirmed",
        )
        .await;

        assert!(!delete_path.exists());
        assert!(keep_path.is_file());
        assert_directory_listing_state(&outcome, &session_state, &root_path, "keep.txt\n");

        std::fs::remove_file(config_path).expect("cleanup config");
        std::fs::remove_dir_all(root_path).expect("cleanup root directory");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dired_delete_confirmed_single_empty_directory_refreshes_directory_listing() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("dired-delete-directory-root");
        let delete_path = root_path.join("delete-dir");
        std::fs::create_dir_all(&delete_path).expect("delete directory");
        let (config_path, mut outcome, mut session_state, mut runtime_session) =
            prepare_dired_runtime_fixture(
                "dired-delete-directory-init",
                dired_phase3_config_source(),
            );

        open_dired_listing_for_test(&root_path, &mut outcome, &mut session_state);
        assert_directory_listing_state(&outcome, &session_state, &root_path, "delete-dir/\n");
        execute_runtime_command_for_test(
            &mut outcome,
            &mut session_state,
            &mut runtime_session,
            "dired.deleteConfirmed",
        )
        .await;

        assert!(!delete_path.exists());
        assert_directory_listing_state(&outcome, &session_state, &root_path, "\n");

        std::fs::remove_file(config_path).expect("cleanup config");
        std::fs::remove_dir_all(root_path).expect("cleanup root directory");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dired_delete_requires_explicit_confirmation() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("dired-delete-confirm-root");
        let delete_path = root_path.join("delete-me.txt");
        std::fs::create_dir_all(&root_path).expect("root directory");
        std::fs::write(&delete_path, "delete\n").expect("delete file");
        let (config_path, mut outcome, mut session_state, mut runtime_session) =
            prepare_dired_runtime_fixture(
                "dired-delete-confirm-init",
                dired_phase3_config_source(),
            );

        open_dired_listing_for_test(&root_path, &mut outcome, &mut session_state);
        assert_directory_listing_state(&outcome, &session_state, &root_path, "delete-me.txt\n");
        let (transient_msg, need_redraw) = execute_runtime_command_outcome_for_test(
            &mut outcome,
            &mut session_state,
            &mut runtime_session,
            "dired.deleteWithoutConfirm",
        )
        .await;

        assert!(delete_path.is_file());
        let message = transient_msg.expect("delete without confirmation should surface an error");
        assert!(
            message.contains("confirmationRequired") && message.contains("delete-me.txt"),
            "message should include structured error kind and path, got: {message}"
        );
        assert!(need_redraw);
        assert_directory_listing_state(&outcome, &session_state, &root_path, "delete-me.txt\n");

        std::fs::remove_file(config_path).expect("cleanup config");
        std::fs::remove_dir_all(root_path).expect("cleanup root directory");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dired_mark_unmark_and_clear_are_available_from_typescript_commands() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("dired-mark-runtime-root");
        let alpha_path = root_path.join("alpha.txt");
        let beta_path = root_path.join("beta.txt");
        let config_path = unique_path("dired-mark-runtime-init").with_extension("ts");
        std::fs::create_dir_all(&root_path).expect("root directory");
        std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
        std::fs::write(&beta_path, "beta\n").expect("beta file");
        std::fs::write(&config_path, dired_phase4_config_source()).expect("config file");
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::File(config_path.clone()),
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
            .expect("runtime session should initialize");

        execute_runtime_host_command(
            &format!("edit {}", root_path.display()),
            &mut outcome,
            &mut session_state,
        )
        .expect("open root listing");
        execute_runtime_command_for_test(
            &mut outcome,
            &mut session_state,
            &mut runtime_session,
            "dired.mark",
        )
        .await;
        assert_eq!(session_state.marked_directory_entries().len(), 1);

        execute_runtime_command_for_test(
            &mut outcome,
            &mut session_state,
            &mut runtime_session,
            "dired.unmark",
        )
        .await;
        assert!(session_state.marked_directory_entries().is_empty());

        execute_runtime_command_for_test(
            &mut outcome,
            &mut session_state,
            &mut runtime_session,
            "dired.mark",
        )
        .await;
        outcome.core_bridge.dispatch_key("j").expect("move to beta");
        execute_runtime_command_for_test(
            &mut outcome,
            &mut session_state,
            &mut runtime_session,
            "dired.mark",
        )
        .await;
        assert_eq!(session_state.marked_directory_entries().len(), 2);

        execute_runtime_command_for_test(
            &mut outcome,
            &mut session_state,
            &mut runtime_session,
            "dired.clearMarks",
        )
        .await;
        assert!(session_state.marked_directory_entries().is_empty());

        std::fs::remove_file(config_path).expect("cleanup config");
        std::fs::remove_dir_all(root_path).expect("cleanup root directory");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dired_bulk_delete_requires_preview_id_and_confirmation_before_deleting_marks() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("dired-bulk-delete-root");
        let alpha_path = root_path.join("alpha.txt");
        let beta_path = root_path.join("beta.txt");
        let keep_path = root_path.join("keep.txt");
        std::fs::create_dir_all(&root_path).expect("root directory");
        std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
        std::fs::write(&beta_path, "beta\n").expect("beta file");
        std::fs::write(&keep_path, "keep\n").expect("keep file");
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();

        execute_runtime_host_command(
            &format!("edit {}", root_path.display()),
            &mut outcome,
            &mut session_state,
        )
        .expect("open root listing");
        assert_directory_listing_state(
            &outcome,
            &session_state,
            &root_path,
            "alpha.txt\nbeta.txt\nkeep.txt\n",
        );
        execute_runtime_filer_operation(
            RuntimeFilerOperation::Mark {
                path: alpha_path.clone(),
            },
            &mut outcome,
            &mut session_state,
        )
        .expect("mark alpha");
        execute_runtime_filer_operation(
            RuntimeFilerOperation::Mark {
                path: beta_path.clone(),
            },
            &mut outcome,
            &mut session_state,
        )
        .expect("mark beta");

        let without_preview = execute_runtime_filer_operation(
            RuntimeFilerOperation::BulkDelete {
                preview_id: String::new(),
                confirm: true,
            },
            &mut outcome,
            &mut session_state,
        );
        assert!(matches!(
            without_preview,
            Err(RuntimeFilerError::OperationFailed {
                kind: RuntimeFilerErrorKind::ConfirmationRequired,
                ..
            })
        ));
        assert!(alpha_path.is_file());
        assert!(beta_path.is_file());
        assert_eq!(session_state.marked_directory_entries().len(), 2);

        let preview = execute_runtime_filer_operation(
            RuntimeFilerOperation::BulkDeletePreview,
            &mut outcome,
            &mut session_state,
        )
        .expect("preview should be available");
        assert_eq!(
            preview.operation,
            RuntimeFilerOperationKind::BulkDeletePreview
        );
        assert_eq!(preview.entries.len(), 2);
        assert_eq!(
            preview
                .entries
                .iter()
                .map(|entry| entry.path.clone())
                .collect::<Vec<_>>(),
            vec![
                alpha_path.to_string_lossy().to_string(),
                beta_path.to_string_lossy().to_string(),
            ]
        );
        let preview_id = preview.preview_id.expect("preview id");

        let without_confirm = execute_runtime_filer_operation(
            RuntimeFilerOperation::BulkDelete {
                preview_id: preview_id.clone(),
                confirm: false,
            },
            &mut outcome,
            &mut session_state,
        );
        assert!(matches!(
            without_confirm,
            Err(RuntimeFilerError::OperationFailed {
                kind: RuntimeFilerErrorKind::ConfirmationRequired,
                ..
            })
        ));
        assert!(alpha_path.is_file());
        assert!(beta_path.is_file());
        assert_eq!(session_state.marked_directory_entries().len(), 2);
        assert_directory_listing_state(
            &outcome,
            &session_state,
            &root_path,
            "alpha.txt\nbeta.txt\nkeep.txt\n",
        );

        let report = execute_runtime_filer_operation(
            RuntimeFilerOperation::BulkDelete {
                preview_id,
                confirm: true,
            },
            &mut outcome,
            &mut session_state,
        )
        .expect("confirmed bulk delete should succeed");

        assert_eq!(report.operation, RuntimeFilerOperationKind::BulkDelete);
        assert_eq!(report.entries.len(), 2);
        assert_eq!(
            report
                .entries
                .iter()
                .map(|entry| entry.display_text.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha.txt", "beta.txt"]
        );
        assert!(!alpha_path.exists());
        assert!(!beta_path.exists());
        assert!(keep_path.is_file());
        assert!(session_state.marked_directory_entries().is_empty());
        assert_directory_listing_state(&outcome, &session_state, &root_path, "keep.txt\n");

        std::fs::remove_dir_all(root_path).expect("cleanup root directory");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dired_create_file_collision_surfaces_structured_error() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("dired-create-collision-root");
        let existing_path = root_path.join("existing.txt");
        std::fs::create_dir_all(&root_path).expect("root directory");
        std::fs::write(&existing_path, "existing\n").expect("existing file");
        let (config_path, mut outcome, mut session_state, mut runtime_session) =
            prepare_dired_runtime_fixture(
                "dired-create-collision-init",
                dired_phase3_config_source(),
            );

        open_dired_listing_for_test(&root_path, &mut outcome, &mut session_state);
        assert_directory_listing_state(&outcome, &session_state, &root_path, "existing.txt\n");
        let (transient_msg, need_redraw) = execute_runtime_command_outcome_for_test(
            &mut outcome,
            &mut session_state,
            &mut runtime_session,
            "dired.createFileCollision",
        )
        .await;

        let message = transient_msg.expect("collision should surface an error");
        assert!(
            message.contains("alreadyExists") && message.contains("existing.txt"),
            "message should include structured error kind and path, got: {message}"
        );
        assert!(need_redraw);
        assert_eq!(
            std::fs::read_to_string(&existing_path).expect("existing file"),
            "existing\n"
        );
        assert_directory_listing_state(&outcome, &session_state, &root_path, "existing.txt\n");

        std::fs::remove_file(config_path).expect("cleanup config");
        std::fs::remove_dir_all(root_path).expect("cleanup root directory");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dired_rename_missing_path_surfaces_structured_error() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("dired-rename-missing-root");
        std::fs::create_dir_all(&root_path).expect("root directory");
        let (config_path, mut outcome, mut session_state, mut runtime_session) =
            prepare_dired_runtime_fixture(
                "dired-rename-missing-init",
                dired_phase3_config_source(),
            );

        open_dired_listing_for_test(&root_path, &mut outcome, &mut session_state);
        assert_directory_listing_state(&outcome, &session_state, &root_path, "\n");
        let (transient_msg, need_redraw) = execute_runtime_command_outcome_for_test(
            &mut outcome,
            &mut session_state,
            &mut runtime_session,
            "dired.renameMissing",
        )
        .await;

        let message = transient_msg.expect("missing path should surface an error");
        assert!(
            message.contains("notFound") && message.contains("missing.txt"),
            "message should include structured error kind and path, got: {message}"
        );
        assert!(need_redraw);
        assert!(!root_path.join("never.txt").exists());
        assert_directory_listing_state(&outcome, &session_state, &root_path, "\n");

        std::fs::remove_file(config_path).expect("cleanup config");
        std::fs::remove_dir_all(root_path).expect("cleanup root directory");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn startup_registered_dired_keymap_opens_directory_listing() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root_path = unique_path("startup-dired-root");
        let nested_path = root_path.join("src");
        let readme_path = root_path.join("README.md");
        let target_path = root_path.join("notes.txt");
        let config_path = unique_path("startup-dired-init").with_extension("ts");
        std::fs::create_dir_all(&nested_path).expect("test directory");
        std::fs::write(&readme_path, "hello\n").expect("readme file");
        std::fs::write(&target_path, "notes\n").expect("target file");
        std::fs::write(
            &config_path,
            r#"
                saya.commands.register("dired.open", async () => {
                    const buffer = await saya.buffer.current();
                    const currentPath = buffer.path || ".";
                    const directory = currentPath.endsWith("/")
                        ? (currentPath.slice(0, -1) || "/")
                        : (currentPath.lastIndexOf("/") >= 0 ? currentPath.slice(0, currentPath.lastIndexOf("/")) || "/" : ".");
                    await saya.commands.execute(`edit ${directory}`);
                });
                saya.keymap.set("normal", "-", saya.commands.execute("dired.open"));
            "#,
        )
        .expect("config file");

        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::File(target_path.clone()),
            config_source: saya::app::cli::ConfigSource::File(config_path.clone()),
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
            .expect("runtime session should initialize");
        let mut transient_msg = None;
        let mut need_redraw = false;
        let mut runtime_presentation_intents = Vec::new();

        let mode = outcome.core_bridge.mode();
        let action = startup_keymap_action_for_input(
            &outcome.startup_registry.keymaps,
            mode,
            &KeyInput::Char('-'),
        )
        .unwrap_or_else(|| {
            panic!(
                "dired keymap should resolve; mode={mode:?}, keymaps={:?}, warnings={:?}",
                outcome.startup_registry.keymaps, outcome.warnings
            )
        });
        let StartupKeymapAction::RegisteredCommand(command_name) = action else {
            panic!("dired keymap should point at a registered command");
        };
        let mut floating_window_manager = FloatingWindowManager::default();
        let mut completion_float_manager = CompletionFloatManager::default();
        let mut lsp_diagnostic_store = LspDiagnosticStore::default();
        let mut terminal_float_manager = TerminalFloatManager::default();
        let mut panel_manager = PanelManager::default();
        let shutdown = execute_startup_keymap_registered_command(
            Some(&mut runtime_session),
            &command_name,
            &mut outcome,
            &mut session_state,
            &mut floating_window_manager,
            &mut completion_float_manager,
            &mut lsp_diagnostic_store,
            &mut terminal_float_manager,
            &mut panel_manager,
            None,
            &mut transient_msg,
            &mut need_redraw,
            &mut runtime_presentation_intents,
            None,
        )
        .await;

        assert_eq!(shutdown, None);
        assert_eq!(
            transient_msg, None,
            "dired startup command should not surface swap or pager messages"
        );
        assert!(runtime_presentation_intents.is_empty());
        assert!(floating_window_manager.is_empty());
        assert_eq!(completion_float_manager.active_documentation_id(), None);
        assert!(panel_manager.snapshots().is_empty());
        let mut expected_entries = vec!["src/", "README.md", "notes.txt"];
        if root_path.join(".notes.txt.swp").exists() {
            expected_entries.insert(1, ".notes.txt.swp");
        }
        let expected_listing = expected_entries.join("\n") + "\n";
        assert_directory_listing_state(&outcome, &session_state, &root_path, &expected_listing);

        std::fs::remove_file(config_path).expect("cleanup config");
        std::fs::remove_dir_all(root_path).expect("cleanup root directory");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn startup_registered_dired_keymap_opens_current_directory_for_relative_file() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let config_path = unique_path("startup-dired-relative-init").with_extension("ts");
        std::fs::write(
            &config_path,
            r#"
                saya.commands.register("dired.open", async () => {
                    const buffer = await saya.buffer.current();
                    const currentPath = buffer.path || ".";
                    const directory = currentPath.endsWith("/")
                        ? (currentPath.slice(0, -1) || "/")
                        : (currentPath.lastIndexOf("/") >= 0 ? currentPath.slice(0, currentPath.lastIndexOf("/")) || "/" : ".");
                    await saya.commands.execute(`edit ${directory}`);
                });
                saya.keymap.set("normal", "-", saya.commands.execute("dired.open"));
            "#,
        )
        .expect("config file");

        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::File(PathBuf::from("AGENTS.md")),
            config_source: saya::app::cli::ConfigSource::File(config_path.clone()),
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
            .expect("runtime session should initialize");
        let mut transient_msg = None;
        let mut need_redraw = false;
        let mut runtime_presentation_intents = Vec::new();

        let action = startup_keymap_action_for_input(
            &outcome.startup_registry.keymaps,
            outcome.core_bridge.mode(),
            &KeyInput::Char('-'),
        )
        .expect("dired keymap should resolve");
        let StartupKeymapAction::RegisteredCommand(command_name) = action else {
            panic!("dired keymap should point at a registered command");
        };
        let mut floating_window_manager = FloatingWindowManager::default();
        let mut completion_float_manager = CompletionFloatManager::default();
        let mut lsp_diagnostic_store = LspDiagnosticStore::default();
        let mut terminal_float_manager = TerminalFloatManager::default();
        let mut panel_manager = PanelManager::default();
        let shutdown = execute_startup_keymap_registered_command(
            Some(&mut runtime_session),
            &command_name,
            &mut outcome,
            &mut session_state,
            &mut floating_window_manager,
            &mut completion_float_manager,
            &mut lsp_diagnostic_store,
            &mut terminal_float_manager,
            &mut panel_manager,
            None,
            &mut transient_msg,
            &mut need_redraw,
            &mut runtime_presentation_intents,
            None,
        )
        .await;

        assert_eq!(shutdown, None);
        assert!(
            matches!(
                transient_msg.as_deref(),
                None | Some("E301: Oops, lost the swap file!!!")
            ),
            "dired startup command should not surface runtime command errors: {transient_msg:?}"
        );
        assert!(runtime_presentation_intents.is_empty());
        assert!(floating_window_manager.is_empty());
        assert_eq!(completion_float_manager.active_documentation_id(), None);
        assert!(panel_manager.snapshots().is_empty());
        assert_eq!(outcome.target_path, Some(PathBuf::from(".")));
        assert_eq!(
            session_state.target_path().map(PathBuf::as_path),
            Some(std::path::Path::new("."))
        );
        let directory_buffer = session_state
            .directory_buffer()
            .expect("relative dired command should leave directory metadata active");
        assert_eq!(
            outcome.core_bridge.snapshot().text,
            directory_buffer.display_text,
            "relative dired command should replace the previous file buffer with the directory listing"
        );
        let display_texts = directory_buffer_display_texts_for_test(&session_state);
        assert!(display_texts.contains(&"Cargo.toml".to_string()));
        assert_ne!(
            outcome.core_bridge.snapshot().text,
            std::fs::read_to_string("AGENTS.md").expect("source file should remain readable"),
            "relative dired listing should replace the previous file contents in the active buffer"
        );

        std::fs::remove_file(config_path).expect("cleanup config");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn repository_dired_keymap_moves_above_current_directory_from_relative_file() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let config_path = unique_path("repository-dired-up-relative-init").with_extension("ts");
        let root_path = unique_repo_relative_path("repository-dired-up-relative");
        let child_path = root_path.join("child");
        let target_path = child_path.join("file.txt");
        std::fs::create_dir_all(&child_path).expect("child directory");
        std::fs::write(&target_path, "relative\n").expect("relative test file");
        let plugin_path = saya::support::paths::dev_ts_plugins_dir().join("saya-dired.ts");
        std::fs::write(
            &config_path,
            format!(
                r#"
                    import {{ setupSayaDired }} from "{}";
                    setupSayaDired({{ keymap: {{}} }});
                "#,
                plugin_path.display()
            ),
        )
        .expect("config file");

        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::File(target_path.clone()),
            config_source: saya::app::cli::ConfigSource::File(config_path.clone()),
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();
        let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
            .expect("runtime session should initialize");

        for _ in 0..2 {
            let action = startup_keymap_action_for_input(
                &outcome.startup_registry.keymaps,
                outcome.core_bridge.mode(),
                &KeyInput::Char('-'),
            )
            .expect("dired keymap should resolve");
            let StartupKeymapAction::RegisteredCommand(command_name) = action else {
                panic!("dired keymap should point at a registered command");
            };
            let mut transient_msg = None;
            let mut need_redraw = false;
            let mut runtime_presentation_intents = Vec::new();
            let mut floating_window_manager = FloatingWindowManager::default();
            let mut completion_float_manager = CompletionFloatManager::default();
            let mut lsp_diagnostic_store = LspDiagnosticStore::default();
            let mut terminal_float_manager = TerminalFloatManager::default();
            let mut panel_manager = PanelManager::default();
            execute_startup_keymap_registered_command(
                Some(&mut runtime_session),
                &command_name,
                &mut outcome,
                &mut session_state,
                &mut floating_window_manager,
                &mut completion_float_manager,
                &mut lsp_diagnostic_store,
                &mut terminal_float_manager,
                &mut panel_manager,
                None,
                &mut transient_msg,
                &mut need_redraw,
                &mut runtime_presentation_intents,
                None,
            )
            .await;
            assert_eq!(
                transient_msg, None,
                "dired up should not surface swap or pager messages"
            );
        }

        assert_eq!(outcome.target_path, Some(root_path.clone()));
        assert_eq!(session_state.target_path(), Some(&root_path));

        std::fs::remove_file(config_path).expect("cleanup config");
        std::fs::remove_dir_all(root_path).expect("cleanup relative root directory");
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
            saya::app::session::EditorSessionState::new(Some(original_path.clone()));
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
                pending_directory_confirmation: false,
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
        let trace = |sequence| saya::core::outcome::OutcomeTrace {
            sequence,
            origin: saya::core::outcome::OutcomeOrigin::TransactionHostAction,
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
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let bridge = saya::core::bridge::CoreBridge::new("alpha\nbeta\n").expect("core bridge");
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
            panes: vec![saya::presentation::screen_model::ScreenModel {
                window_id: 1,
                buffer_id: 1,
                rect: saya::presentation::screen_model::PaneRect {
                    x: 0,
                    y: 0,
                    width: 20,
                    height: 3,
                },
                file_name: "alpha.txt".to_string(),
                mode_label: "NORMAL".to_string(),
                status_line: "test.txt | NORMAL".to_string(),
                cursor_style: ScreenCursorStyle::Block,
                dirty: false,
                lines: vec!["alpha".to_string()],
                line_projections: vec![],
                cursor_row: 0,
                cursor_col: 0,
                visual_selection: None,
                search_overlays: vec![],
                syntax_chunks: vec![],
                markdown_style_ranges: vec![],
                filer_style_ranges: vec![],
                resolved_theme: saya::presentation::theme::ResolvedTheme::default(),
                message_line: None,
                command_cursor_col: None,
                is_active: true,
            }],
            floats: vec![],
            active_window_id: 1,
            message_line: saya::core::notification_prompt::resolve_workspace_message_line(Vec::<
                saya::core::notification_prompt::MessageLineCandidate,
            >::new(
            )),
            message_area_height: 5,
            message_scroll_offset: 0,
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
    fn command_line_only_render_reuses_last_workspace_for_colon_prompt() {
        let mut last_workspace = main_test_workspace();
        last_workspace.panes[0].lines = vec!["keep full projection".to_string()];

        let rendered =
            build_command_line_only_workspace(Some(&last_workspace), Some(':'), "write", 5, 4)
                .expect("colon command preview should use command-line-only workspace");

        assert_eq!(rendered.panes, last_workspace.panes);
        assert_eq!(
            rendered.command_line,
            Some(saya::presentation::screen_model::CommandLineModel {
                text: ":write".to_string(),
                cursor_col: 6,
            })
        );
    }

    #[test]
    fn command_line_only_render_uses_command_line_edit_cursor_position() {
        let last_workspace = main_test_workspace();

        let rendered =
            build_command_line_only_workspace(Some(&last_workspace), Some(':'), "write", 1, 4)
                .expect("colon command preview should use command-line-only workspace");

        assert_eq!(
            rendered.command_line,
            Some(saya::presentation::screen_model::CommandLineModel {
                text: ":write".to_string(),
                cursor_col: 2,
            })
        );
    }

    #[test]
    fn command_line_only_render_is_not_used_for_search_prompt() {
        let last_workspace = main_test_workspace();

        let rendered =
            build_command_line_only_workspace(Some(&last_workspace), Some('/'), "pattern", 7, 4);

        assert_eq!(rendered, None);
    }

    #[test]
    fn command_line_only_render_is_not_used_for_substitute_live_preview() {
        let last_workspace = main_test_workspace();

        for command in [
            "%s/foo/bar",
            "s/foo/bar",
            "substitute/foo/bar",
            "10,20s/foo/bar",
        ] {
            let rendered =
                build_command_line_only_workspace(Some(&last_workspace), Some(':'), command, 3, 4);

            assert_eq!(
                rendered, None,
                "substitute input should rebuild workspace overlays instead of reusing stale projection: {command}"
            );
        }
    }

    #[test]
    fn command_line_only_render_requires_existing_workspace() {
        let rendered = build_command_line_only_workspace(None, Some(':'), "write", 5, 4);

        assert_eq!(rendered, None);
    }

    #[test]
    fn colon_command_input_uses_overlay_without_workspace_redraw_traces() {
        let _guard = redraw_trace_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reset_test_redraw_trace_counts();
        let mut coordinator = TuiRenderCoordinator::new_for_tests(
            OverlayAssetStore::default(),
            OptionalGraphicsAdapter::default(),
        );
        let mut writer = RecordingOverlayWriter::default();
        let mut last_workspace = Some(main_test_workspace());

        for buffer in ["w", "wq"] {
            let result = render_command_line_only_redraw_if_possible(
                &mut coordinator,
                Some(&mut writer),
                &mut last_workspace,
                None,
                false,
                Some(':'),
                buffer,
                buffer.len(),
                4,
            );

            assert_eq!(result, CommandLineOnlyRedraw::Rendered);
        }

        let counts = test_redraw_trace_counts();
        assert_eq!(counts.command_line_only_overlay, 2);
        assert_eq!(
            counts.workspace_render_build_started, 0,
            "colon command typing must not rebuild the workspace projection"
        );
        assert_eq!(
            counts.renderer_frame_requested, 0,
            "colon command typing must not request a full workspace frame"
        );
        assert_eq!(counts.command_line_overlay_fallback, 0);
        assert_eq!(
            last_workspace
                .as_ref()
                .and_then(|workspace| workspace.command_line.as_ref())
                .map(|command_line| command_line.text.as_str()),
            Some(":wq")
        );
        assert_eq!(
            writer.cursor_styles,
            vec![ScreenCursorStyle::SteadyBar],
            "unchanged command-line cursor style should be written only once"
        );
    }

    #[test]
    fn search_prompt_does_not_use_command_line_only_overlay_trace() {
        let _guard = redraw_trace_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reset_test_redraw_trace_counts();
        let mut coordinator = TuiRenderCoordinator::new_for_tests(
            OverlayAssetStore::default(),
            OptionalGraphicsAdapter::default(),
        );
        let mut writer = RecordingOverlayWriter::default();
        let mut last_workspace = Some(main_test_workspace());

        let result = render_command_line_only_redraw_if_possible(
            &mut coordinator,
            Some(&mut writer),
            &mut last_workspace,
            None,
            false,
            Some('/'),
            "word",
            4,
            4,
        );

        assert_eq!(result, CommandLineOnlyRedraw::NotApplicable);
        assert_eq!(test_redraw_trace_counts(), RedrawTraceCounts::default());
        assert!(writer.cursor_styles.is_empty());
    }

    #[test]
    fn command_line_only_render_is_not_used_when_workspace_projection_is_dirty() {
        let _guard = redraw_trace_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reset_test_redraw_trace_counts();
        let mut coordinator = TuiRenderCoordinator::new_for_tests(
            OverlayAssetStore::default(),
            OptionalGraphicsAdapter::default(),
        );
        let mut writer = RecordingOverlayWriter::default();
        let mut last_workspace = Some(main_test_workspace());

        let result = render_command_line_only_redraw_if_possible(
            &mut coordinator,
            Some(&mut writer),
            &mut last_workspace,
            None,
            true,
            Some(':'),
            "",
            0,
            4,
        );

        assert_eq!(result, CommandLineOnlyRedraw::NotApplicable);
        assert_eq!(test_redraw_trace_counts().command_line_only_overlay, 0);
        assert!(writer.cursor_styles.is_empty());
        assert!(
            last_workspace
                .as_ref()
                .is_some_and(|workspace| workspace.command_line.is_none()),
            "dirty workspace projection must not reuse the stale workspace for ':'"
        );
    }

    #[test]
    fn command_line_overlay_failure_is_observable_before_workspace_fallback() {
        let _guard = redraw_trace_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reset_test_redraw_trace_counts();
        let mut coordinator = TuiRenderCoordinator::new_for_tests(
            OverlayAssetStore::default(),
            OptionalGraphicsAdapter::default(),
        );
        let mut writer = FailingOverlayWriter;
        let mut last_workspace = Some(main_test_workspace());

        let result = render_command_line_only_redraw_if_possible(
            &mut coordinator,
            Some(&mut writer),
            &mut last_workspace,
            None,
            false,
            Some(':'),
            "write",
            5,
            4,
        );

        let counts = test_redraw_trace_counts();
        assert_eq!(result, CommandLineOnlyRedraw::Fallback);
        assert_eq!(counts.command_line_only_overlay, 1);
        assert_eq!(counts.command_line_overlay_fallback, 1);
        assert_eq!(counts.workspace_render_build_started, 0);
        assert_eq!(counts.renderer_frame_requested, 0);
    }

    struct FailingOverlayWriter;

    impl OverlayTerminalWriter for FailingOverlayWriter {
        fn write_overlay_bytes(&mut self, _bytes: &[u8]) -> Result<(), String> {
            Ok(())
        }

        fn set_cursor_style(&mut self, _style: ScreenCursorStyle) -> Result<(), String> {
            Err("forced cursor style failure".to_string())
        }
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
    fn core_screen_size_sync_does_not_enqueue_redraw_when_size_is_unchanged() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut outcome =
            saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest::default())
                .expect("launch should succeed");
        let mut last_synced_terminal_size = None;
        let terminal_size = TerminalSize {
            columns: 80,
            rows: 24,
        };

        assert!(sync_core_screen_size_if_changed(
            &mut outcome,
            &mut last_synced_terminal_size,
            terminal_size,
        ));
        let first_batch = outcome.core_bridge.take_normalized_outcomes();
        assert!(
            !first_batch.is_empty(),
            "first screen-size sync should expose the core layout redraw"
        );

        assert!(!sync_core_screen_size_if_changed(
            &mut outcome,
            &mut last_synced_terminal_size,
            terminal_size,
        ));
        assert!(
            outcome.core_bridge.take_normalized_outcomes().is_empty(),
            "unchanged screen-size sync must not leave a stale redraw for the next keypress"
        );
    }

    #[test]
    fn core_screen_size_sync_updates_when_size_changes() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut outcome =
            saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest::default())
                .expect("launch should succeed");
        let mut last_synced_terminal_size = Some(TerminalSize {
            columns: 80,
            rows: 24,
        });

        assert!(sync_core_screen_size_if_changed(
            &mut outcome,
            &mut last_synced_terminal_size,
            TerminalSize {
                columns: 100,
                rows: 30,
            },
        ));
        assert_eq!(
            last_synced_terminal_size,
            Some(TerminalSize {
                columns: 100,
                rows: 30,
            })
        );
        assert!(
            !outcome.core_bridge.take_normalized_outcomes().is_empty(),
            "changed size should still request the necessary layout redraw"
        );
    }

    #[test]
    fn rendered_structural_refresh_no_longer_blocks_command_line_overlay() {
        let mut accumulator = MainOutcomeAccumulator {
            last_structural_refresh: Some(StructuralRefresh::from_folded_effects(
                &saya::core::outcome::StructuralEffectSet {
                    redraw: Some(saya::core::outcome::RedrawEffect {
                        full: true,
                        clear_before_draw: false,
                        required_by_structure_change: true,
                        coalesced_count: 1,
                    }),
                    invalidate_buffers: vec![],
                    invalidate_windows: vec![],
                    layout_dirty: true,
                },
            )),
            ..MainOutcomeAccumulator::default()
        };

        mark_structural_refresh_rendered(&mut accumulator);

        let refresh = accumulator
            .last_structural_refresh
            .as_ref()
            .expect("rendered refresh should leave a neutral diagnostic state");
        assert!(
            structural_refresh_is_idle(Some(refresh)),
            "a structural refresh that has already been rendered must not force the next command-line key into a full redraw"
        );
    }

    #[test]
    fn terminal_display_invalidation_forces_full_clear_redraw_without_core_changes() {
        let redraw_plan = effective_workspace_redraw_plan(
            None,
            Some(&terminal_display_invalidated_redraw_plan()),
        );

        assert!(redraw_plan.requested);
        assert!(redraw_plan.full);
        assert!(redraw_plan.clear_before_draw);
        assert_eq!(
            redraw_plan.source,
            saya::presentation::structural_refresh::RedrawPlanSource::TerminalDisplayInvalidation
        );
    }

    #[test]
    fn terminal_display_invalidation_overrides_idle_structural_refresh_for_resume() {
        let idle_refresh =
            StructuralRefresh::from_folded_effects(&saya::core::outcome::StructuralEffectSet {
                redraw: None,
                invalidate_buffers: vec![],
                invalidate_windows: vec![],
                layout_dirty: false,
            });

        let redraw_plan = effective_workspace_redraw_plan(
            Some(&idle_refresh),
            Some(&terminal_display_invalidated_redraw_plan()),
        );

        assert!(redraw_plan.requested);
        assert!(redraw_plan.full);
        assert!(redraw_plan.clear_before_draw);
        assert_eq!(
            redraw_plan.source,
            saya::presentation::structural_refresh::RedrawPlanSource::TerminalDisplayInvalidation
        );
    }

    #[test]
    fn consume_core_outcomes_marks_need_redraw_when_bridge_has_pending_redraw() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut bridge = saya::core::bridge::CoreBridge::new("hello\n").expect("core bridge");
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
    fn consume_core_outcomes_replaces_stale_structural_refresh_on_empty_batch() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut bridge = saya::core::bridge::CoreBridge::new("hello\n").expect("core bridge");
        assert!(
            bridge.take_normalized_outcomes().is_empty(),
            "new bridge should not start with pending normalized outcomes"
        );
        let stale_full_refresh =
            StructuralRefresh::from_folded_effects(&saya::core::outcome::StructuralEffectSet {
                redraw: Some(saya::core::outcome::RedrawEffect {
                    full: true,
                    clear_before_draw: true,
                    required_by_structure_change: true,
                    coalesced_count: 1,
                }),
                invalidate_buffers: vec![1],
                invalidate_windows: vec![1],
                layout_dirty: true,
            });
        let mut accumulator = MainOutcomeAccumulator {
            last_structural_refresh: Some(stale_full_refresh),
            ..MainOutcomeAccumulator::default()
        };
        let mut need_redraw = false;

        consume_core_outcomes_from_core(&mut bridge, &mut accumulator, &mut need_redraw);

        assert!(
            !need_redraw,
            "empty batch should leave redraw scheduling to the caller policy"
        );
        let refresh = accumulator
            .last_structural_refresh
            .expect("empty batch should record a neutral structural refresh");
        assert!(!refresh.redraw_plan.requested);
        assert!(!refresh.redraw_plan.full);
        assert!(!refresh.redraw_plan.clear_before_draw);
        assert_eq!(
            refresh.redraw_plan.source,
            saya::presentation::structural_refresh::RedrawPlanSource::None
        );
    }

    #[test]
    fn consume_core_outcomes_tracks_active_prompt_in_projection_state() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut bridge = saya::core::bridge::CoreBridge::new("hello\n").expect("core bridge");
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
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut bridge = saya::core::bridge::CoreBridge::new("hello\n").expect("core bridge");
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

        let action = saya::core::notification_prompt::handle_prompt_key(
            &mut accumulator.projection,
            &KeyInput::Enter,
        );
        let command = match action {
            saya::core::notification_prompt::PromptInputAction::Submit(command) => command,
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
            Some(saya::core::notification_prompt::PromptTransitionKind::Submitted)
        );
    }

    #[test]
    fn prompt_response_end_to_end_preserves_typed_input_value() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut bridge = saya::core::bridge::CoreBridge::new("hello\n").expect("core bridge");
        let mut accumulator = MainOutcomeAccumulator::default();
        let mut need_redraw = false;

        bridge
            .apply_ex_command(":input Name")
            .expect("input request should succeed");
        consume_core_outcomes_from_core(&mut bridge, &mut accumulator, &mut need_redraw);

        assert!(matches!(
            saya::core::notification_prompt::handle_prompt_key(
                &mut accumulator.projection,
                &KeyInput::Char('a'),
            ),
            saya::core::notification_prompt::PromptInputAction::Consumed
        ));
        assert!(matches!(
            saya::core::notification_prompt::handle_prompt_key(
                &mut accumulator.projection,
                &KeyInput::Char('b'),
            ),
            saya::core::notification_prompt::PromptInputAction::Consumed
        ));
        let action = saya::core::notification_prompt::handle_prompt_key(
            &mut accumulator.projection,
            &KeyInput::Enter,
        );
        let command = match action {
            saya::core::notification_prompt::PromptInputAction::Submit(command) => command,
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
                saya::core::notification_prompt::PromptTransitionKind::Submitted,
                2
            ))
        );
    }

    #[test]
    fn structural_redraw_does_not_close_active_prompt() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut bridge = saya::core::bridge::CoreBridge::new("hello\n").expect("core bridge");
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
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut bridge = saya::core::bridge::CoreBridge::new("hello\n").expect("core bridge");
        let mut accumulator = MainOutcomeAccumulator::default();
        let mut need_redraw = false;

        bridge
            .apply_ex_command(":input Name")
            .expect("input request should succeed");
        consume_core_outcomes_from_core(&mut bridge, &mut accumulator, &mut need_redraw);
        assert!(matches!(
            saya::core::notification_prompt::handle_prompt_key(
                &mut accumulator.projection,
                &KeyInput::Char('x'),
            ),
            saya::core::notification_prompt::PromptInputAction::Consumed
        ));
        let action = saya::core::notification_prompt::handle_prompt_key(
            &mut accumulator.projection,
            &KeyInput::Enter,
        );
        let command = match action {
            saya::core::notification_prompt::PromptInputAction::Submit(command) => command,
            other => panic!("expected submit action, got {other:?}"),
        };

        dispatch_prompt_response_command(
            &mut bridge,
            &mut accumulator,
            saya::core::prompt::PromptResponseCommand::Submit {
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
            Some(saya::core::notification_prompt::InputPromptStatus::Active)
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

    #[tokio::test(flavor = "current_thread")]
    async fn selector_accept_action_opens_selected_rg_location_and_hides_reopenable_selector() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let target_path = unique_path("selector-rg-jump").with_extension("txt");
        std::fs::write(&target_path, "first\nabcdef\nthird\n").expect("target fixture");
        let target_literal =
            serde_json::to_string(&target_path.to_string_lossy()).expect("path JSON");
        let seed =
            saya::runtime::callback_registry_seed::CallbackRegistrySeed::from_startup_entries(
                vec![saya::runtime::config::StartupRegistryEntry::Event {
                    name: "bufferOpen".to_string(),
                    callback_source: format!(
                        r#"
                            async () => {{
                                await saya.selector.open({{
                                    source: {{
                                        kind: "static",
                                        items: [
                                            {{
                                                id: "rg-target",
                                                value: "target.txt:2:4:abcdef",
                                                kind: "rg",
                                                detail: {{ path: {target_literal}, line: 2, column: 4, text: "abcdef" }},
                                            }},
                                        ],
                                    }},
                                    matcher: "substringAnd",
                                    query: "target",
                                }});
                            }}
                        "#
                    ),
                }],
            );
        let mut runtime_session =
            RuntimeSessionOwner::spawn(seed).expect("runtime owner should initialize");
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();

        {
            let mut host_session = MainRuntimeHostSession::new(&mut outcome, &mut session_state);
            runtime_session
                .dispatch(
                    saya::runtime::live::RuntimeEventPayload::BufferOpen(
                        saya::runtime::live::BufferEventPayload {
                            buffer: host_session.current_buffer_snapshot(),
                        },
                    ),
                    &mut host_session,
                )
                .await;
        }
        let selector_model = runtime_session
            .selector_tui_projection_sink()
            .current_model()
            .expect("selector should be active before Enter");

        let dispatch_outcome = handle_selector_accept_action(
            &mut runtime_session,
            &selector_model,
            &mut outcome,
            &mut session_state,
        )
        .await;

        assert!(dispatch_outcome.requires_redraw);
        assert_eq!(session_state.target_path(), Some(&target_path));
        assert_eq!(outcome.target_path, Some(target_path.clone()));
        let snapshot = outcome.core_bridge.light_snapshot();
        assert_eq!(snapshot.cursor_row, 1, "rg line is 1-based");
        assert_eq!(snapshot.cursor_col, 3, "rg column is 1-based");
        let hidden_model = runtime_session
            .selector_tui_projection_sink()
            .current_model()
            .expect("selector hide should publish model");
        assert!(hidden_model.hidden);
        assert!(!hidden_model.cancelled);
        assert_eq!(
            hidden_model
                .selected_row
                .as_ref()
                .map(|row| row.item.id.as_str()),
            Some("rg-target")
        );
        {
            let mut host_session = MainRuntimeHostSession::new(&mut outcome, &mut session_state);
            let reopen_outcome = runtime_session
                .control_selector(
                    hidden_model.session_id,
                    RuntimeSelectorControllerCommand::Show,
                    &mut host_session,
                )
                .await;
            assert!(reopen_outcome.requires_redraw);
        }
        let reopened_model = runtime_session
            .selector_tui_projection_sink()
            .current_model()
            .expect("selector show should publish model");
        assert!(!reopened_model.hidden);
        assert!(!reopened_model.cancelled);
        assert_eq!(
            reopened_model
                .selected_row
                .as_ref()
                .map(|row| row.item.id.as_str()),
            Some("rg-target")
        );
        assert!(
            outcome.core_bridge.buffer_text().contains("abcdef"),
            "Enter must open the selected file instead of leaking into normal editing"
        );

        std::fs::remove_file(target_path).expect("cleanup target fixture");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn selector_accept_action_reports_invalid_rg_detail_without_normal_enter_leak() {
        let _lock = saya::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let seed =
            saya::runtime::callback_registry_seed::CallbackRegistrySeed::from_startup_entries(
                vec![saya::runtime::config::StartupRegistryEntry::Event {
                    name: "bufferOpen".to_string(),
                    callback_source: r#"
                        async () => {
                            await saya.selector.open({
                                source: {
                                    kind: "static",
                                    items: [
                                        {
                                            id: "rg-invalid",
                                            value: "broken",
                                            kind: "rg",
                                            detail: { path: "missing.txt", line: 1 },
                                        },
                                    ],
                                },
                                matcher: "substringAnd",
                                query: "broken",
                            });
                        }
                    "#
                    .to_string(),
                }],
            );
        let mut runtime_session =
            RuntimeSessionOwner::spawn(seed).expect("runtime owner should initialize");
        let mut outcome = saya::app::bootstrap::prepare_launch(saya::app::cli::LaunchRequest {
            input_source: saya::app::cli::InputSource::Empty,
            config_source: saya::app::cli::ConfigSource::Default,
            ..saya::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        let mut session_state = outcome.editor_session_state();

        {
            let mut host_session = MainRuntimeHostSession::new(&mut outcome, &mut session_state);
            runtime_session
                .dispatch(
                    saya::runtime::live::RuntimeEventPayload::BufferOpen(
                        saya::runtime::live::BufferEventPayload {
                            buffer: host_session.current_buffer_snapshot(),
                        },
                    ),
                    &mut host_session,
                )
                .await;
        }
        let before_text = outcome.core_bridge.buffer_text();
        let selector_model = runtime_session
            .selector_tui_projection_sink()
            .current_model()
            .expect("selector should be active before Enter");

        let dispatch_outcome = handle_selector_accept_action(
            &mut runtime_session,
            &selector_model,
            &mut outcome,
            &mut session_state,
        )
        .await;

        assert!(dispatch_outcome.requires_redraw);
        assert!(
            dispatch_outcome
                .transient_message
                .as_deref()
                .is_some_and(|message| message.contains("detail.column")),
            "invalid detail should report a user-visible failure: {:?}",
            dispatch_outcome.transient_message
        );
        assert_eq!(
            outcome.core_bridge.buffer_text(),
            before_text,
            "invalid selector Enter must be consumed without normal Enter editing"
        );
        assert_eq!(session_state.target_path(), None);
        let current_model = runtime_session
            .selector_tui_projection_sink()
            .current_model()
            .expect("failed action should retain selector state");
        assert!(!current_model.cancelled);
    }

    #[test]
    fn markdown_metadata_collection_is_limited_to_markdown_target_paths() {
        assert!(is_markdown_target_path(Some(&PathBuf::from("notes.md"))));
        assert!(is_markdown_target_path(Some(&PathBuf::from(
            "notes.markdown"
        ))));
        assert!(is_markdown_target_path(Some(&PathBuf::from("notes.MDOWN"))));
        assert!(!is_markdown_target_path(Some(&PathBuf::from("notes.txt"))));
        assert!(!is_markdown_target_path(None));
    }
}
