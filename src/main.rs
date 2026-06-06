use saya::app::bootstrap::{
    BootstrapError, StartupKeymapAction, StartupKeymapMode, bootstrap_warning_message,
};
use saya::app::cli::{CliParseError, StartupAction, parse_launch_request};
use saya::app::event_loop::{EventLoopCoordinator, LoopAction, ShutdownReason, UiEvent};
use saya::app::host_io::{SaveRequest, SaveResult, write_to_path};
use saya::app::session::{
    DirectoryBufferListingOptions, DirectoryBufferPlannedOperation,
    DirectoryBufferPreviewConfirmationError, DirectoryBufferSortKey, EditorSessionState,
    MermaidPreviewZoom, QuitDecision, SaveRequestError,
};
use saya::app::startup::{
    LaunchStartError, PreparedTuiStartup, TuiStartupContextError, prepare_tui_startup_context,
};
use saya::core::bridge::CoreBridge;
use saya::core::host_actions::HostActionRuntime;
use saya::core::notification_prompt::{
    InputPromptStatus, InputPromptView, NotificationPromptProjectionState, PagerPromptView,
    ProjectionFrame, PromptInputAction, handle_prompt_key, record_prompt_response_error,
};
use saya::core::outcome::{
    ApplicationDispatchEffects, ApplicationOutcomeState, NormalizedHostDirective,
    NormalizedOutcomeBatch, StructuralEffectSet, fold_normalized_outcomes,
};
use saya::core::prompt::PromptResponseCommand;
use saya::features::completion::float::{CompletionFloatInputOutcome, CompletionFloatManager};
use saya::features::completion::session::CompletionShowRequest;
use saya::features::lsp::float::{
    LspDiagnosticFloatRequest, LspDiagnosticStore, LspHoverFloatRequest, LspLocationListRequest,
    LspSymbolOutlineRequest, PopupSizeBasis, PopupSizeSpec, PopupSizeValue, ResolvedPopupSizeLimit,
    file_uri_to_path, open_lsp_diagnostic_float, open_lsp_hover_float,
    open_lsp_location_list_float, open_lsp_symbol_outline_float,
};
use saya::features::lsp::lsif_index::LsifIndexCache;
use saya::features::lsp::runtime_bridge::{LspRuntimeBridgeRequest, LspRuntimeBridgeResponse};
use saya::features::search::query::{SearchStateError, SearchVisibleState};
use saya::features::search::refresh::{
    SearchModeHint, SearchRefreshInput, WindowSearchRefreshStore,
};
use saya::features::search::substitute_preview::build_substitute_preview_render;
use saya::features::selector::keymap::{
    SelectorAction, SelectorKeyRoute, SelectorModeSwitch, selector_key_route_for_model,
};
use saya::features::selector::runtime::{
    RuntimeRgLocation, RuntimeSelectorControllerCommand, parse_rg_selector_location_detail,
};
use saya::features::selector::tui_state::{
    SelectorMode, SelectorTuiProjectionSink, SelectorTuiViewModel,
};
use saya::input::command_line_editor::{CommandLineEdit, command_line_edit_action_for_key};
use saya::input::command_line_history::{
    history_direction_for_key, load_histories_from_default_cache,
    record_history_and_save_to_default_cache, save_histories_to_default_cache,
};
use saya::input::ex_command::{ExCommandRoute, apply_local_ex_command, route_ex_command};
use saya::input::router::{EditorIntent, KeyInput, NavigationKey, resolve_intent};
use saya::presentation::floating_window::{
    FloatingAnchor, FloatingBorder, FloatingChrome, FloatingContentRef, FloatingCursor,
    FloatingFit, FloatingImage, FloatingImageSource, FloatingImageView, FloatingInputOutcome,
    FloatingLifecycle, FloatingLifecycleEvent, FloatingMouseOutcome, FloatingPlacement,
    FloatingRelativeTo, FloatingSize, FloatingWindowId, FloatingWindowManager, FloatingZIndex,
};
use saya::presentation::markdown::structure::{
    MarkdownBlockKind, MarkdownCacheStatus, MarkdownDocumentMap, MarkdownMetadataCache,
    MarkdownMetadataKey,
};
use saya::presentation::overlay::asset_store::OverlayAssetStore;
use saya::presentation::overlay::effect::RuntimePresentationIntent;
use saya::presentation::overlay::optional_graphics::{
    OptionalGraphicsAdapter, OverlayTerminalWriter,
};
use saya::presentation::panel::{
    PanelCloseBehavior, PanelContent, PanelManager, PanelNode, PanelOpenRequest, PanelPosition,
    PanelSize,
};
use saya::presentation::render::coordinator::{RenderFrameError, TuiRenderCoordinator};
use saya::presentation::render::renderer::{CrosstermBackendImpl, TuiRenderer};
use saya::presentation::screen_model::{
    CommandLineModel, PaneRect, ProjectionInput, WorkspaceProjectionError,
    WorkspaceProjectionInput, WorkspaceScreenModel, project, project_workspace,
};
use saya::presentation::structural_refresh::{
    RedrawPlan, RedrawPlanSource, StructuralRefresh, StructuralRefreshOutcome,
};
use saya::presentation::viewport::{ViewportSyncMode, WindowViewportStore};
use saya::runtime::integration::{
    RuntimeCommandEffect, RuntimeDispatchOutcome, RuntimeEventMapper, RuntimeHostSession,
    RuntimeInputPromptHostResponse, RuntimeSessionOwner, RuntimeShutdownIntent,
};
use saya::runtime::live::{
    ReadonlyBufferSnapshot, ReadonlyEditorSnapshot, ReadonlyWindowSnapshot, RuntimeCommandError,
    RuntimeFilerCurrentEntry, RuntimeFilerEntry, RuntimeFilerEntryKind, RuntimeFilerError,
    RuntimeFilerErrorKind, RuntimeFilerListOptions, RuntimeFilerOperation,
    RuntimeFilerOperationKind, RuntimeFilerOperationReport, RuntimeFilerSortKey,
    RuntimeFloatContentRequest, RuntimeFloatOpenRequest, RuntimeFloatRelativeToRequest,
    RuntimeFloatSnapshot, RuntimeFloatZIndexRequest, RuntimeInputPromptRequest,
    RuntimeInputPromptResponse, RuntimeMode, RuntimePanelContentRequest, RuntimePanelNodeRequest,
    RuntimePanelOpenRequest, RuntimePanelSnapshot,
};
use saya::runtime::plugin::{PluginHost, render_plugin_report};
use saya::support::diagnostic_log::{
    configure_from_startup as configure_diagnostic_log_from_startup,
    init_from_env as init_diagnostic_log_from_env,
};
use saya::terminal::capability::TerminalCapabilityProbe;
use saya::terminal::float::{
    TerminalFloatCloseBehavior, TerminalFloatManager, TerminalFloatSpawnRequest,
};
use saya::terminal::input_loop::CrosstermEventSource;
use saya::terminal::job_control::{
    start_job_control_signal_watcher, suspend_current_process_for_job_control,
};
use saya::terminal::lifecycle::TerminalBackend;
use saya::terminal::lifecycle::TerminalSize;
use vim_core_rs::CoreInputRequestKind;
#[cfg(test)]
use vim_core_rs::CoreMessageEvent;
use vim_core_rs::{
    CoreBufferLineRange, CoreLightSnapshot, CoreMode, CoreSnapshot, CoreVfsError, CoreVfsErrorKind,
    CoreVfsRequest, CoreVfsResponse,
};

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(test)]
use std::sync::{Mutex, OnceLock};
use unicode_width::UnicodeWidthChar;

#[derive(Debug, Default)]
struct MainOutcomeAccumulator {
    state: ApplicationOutcomeState,
    host_directives: Vec<NormalizedHostDirective>,
    projection: NotificationPromptProjectionState,
    last_projection_frame: Option<ProjectionFrame>,
    last_structural_refresh: Option<StructuralRefreshOutcome>,
    suspend_requested: bool,
}

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
                            if let Some(action) =
                                handle_runtime_input_prompt_key(runtime_input_prompt.as_mut(), &key)
                            {
                                handled = true;
                                need_redraw = true;
                                match action {
                                    RuntimeInputPromptKeyAction::Submit(value) => {
                                        log::info!(
                                            "[main][runtime_input] prompt submit from TUI: value_len={}",
                                            value.len()
                                        );
                                        runtime_input_prompt = None;
                                        if let Some(runtime_session) = runtime_session.as_mut() {
                                            let mut host_session =
                                                MainRuntimeHostSession::new_with_runtime_input(
                                                    &mut outcome,
                                                    &mut session_state,
                                                    &mut runtime_input_prompt,
                                                );
                                            let dispatch_outcome = runtime_session
                                                .respond_to_input_prompt(
                                                    RuntimeInputPromptResponse::Submitted { value },
                                                    &mut host_session,
                                                )
                                                .await;
                                            let _ = apply_runtime_dispatch_outcome(
                                                &mut transient_msg,
                                                &mut need_redraw,
                                                &mut runtime_presentation_intents,
                                                dispatch_outcome,
                                            );
                                        }
                                    }
                                    RuntimeInputPromptKeyAction::Cancel => {
                                        log::info!(
                                            "[main][runtime_input] prompt cancelled from TUI"
                                        );
                                        runtime_input_prompt = None;
                                        if let Some(runtime_session) = runtime_session.as_mut() {
                                            let mut host_session =
                                                MainRuntimeHostSession::new_with_runtime_input(
                                                    &mut outcome,
                                                    &mut session_state,
                                                    &mut runtime_input_prompt,
                                                );
                                            let dispatch_outcome = runtime_session
                                                .respond_to_input_prompt(
                                                    RuntimeInputPromptResponse::Cancelled,
                                                    &mut host_session,
                                                )
                                                .await;
                                            let _ = apply_runtime_dispatch_outcome(
                                                &mut transient_msg,
                                                &mut need_redraw,
                                                &mut runtime_presentation_intents,
                                                dispatch_outcome,
                                            );
                                        }
                                    }
                                    RuntimeInputPromptKeyAction::Editing => {}
                                }
                            }
                        }

                        if !handled {
                            let selector_model =
                                runtime_session.as_ref().and_then(|runtime_session| {
                                    runtime_session
                                        .selector_tui_projection_sink()
                                        .current_model()
                                });
                            match selector_key_route_for_model(selector_model.as_ref(), &key) {
                                SelectorKeyRoute::Action { session_id, action } => {
                                    handled = true;
                                    log::info!(
                                        "[main][selector] selector action routing start: key={:?}, session_id={}, action={:?}",
                                        key,
                                        session_id,
                                        action
                                    );
                                    if let (Some(runtime_session), Some(selector_model)) =
                                        (runtime_session.as_mut(), selector_model.as_ref())
                                    {
                                        let dispatch_outcome = match action {
                                            SelectorAction::AcceptSelected => {
                                                handle_selector_accept_action(
                                                    runtime_session,
                                                    selector_model,
                                                    &mut outcome,
                                                    &mut session_state,
                                                )
                                                .await
                                            }
                                        };
                                        let _ = apply_runtime_dispatch_outcome(
                                            &mut transient_msg,
                                            &mut need_redraw,
                                            &mut runtime_presentation_intents,
                                            dispatch_outcome,
                                        );
                                    } else {
                                        log::debug!(
                                            "[main][selector] selector action route resolved without runtime session/model: key={:?}, session_id={}, action={:?}",
                                            key,
                                            session_id,
                                            action
                                        );
                                        transient_msg = Some(
                                            "Selector runtime session is not available".to_string(),
                                        );
                                        need_redraw = true;
                                    }
                                }
                                SelectorKeyRoute::Control {
                                    session_id,
                                    command,
                                } => {
                                    handled = true;
                                    log::info!(
                                        "[main][selector] selector key routing start: key={:?}, session_id={}",
                                        key,
                                        session_id
                                    );
                                    log::info!(
                                        "[main][selector] selector key routing command conversion: key={:?}, command={:?}",
                                        key,
                                        command
                                    );
                                    if let Some(runtime_session) = runtime_session.as_mut() {
                                        let mut host_session = MainRuntimeHostSession::new(
                                            &mut outcome,
                                            &mut session_state,
                                        );
                                        let dispatch_outcome = runtime_session
                                            .control_selector(
                                                session_id,
                                                command,
                                                &mut host_session,
                                            )
                                            .await;
                                        if dispatch_outcome.transient_message.is_some() {
                                            log::debug!(
                                                "[main][selector] selector control failed from key routing: key={:?}, session_id={}, command={:?}",
                                                key,
                                                session_id,
                                                command
                                            );
                                        } else {
                                            log::info!(
                                                "[main][selector] selector control succeeded from key routing: key={:?}, session_id={}, command={:?}",
                                                key,
                                                session_id,
                                                command
                                            );
                                        }
                                        let _ = apply_runtime_dispatch_outcome(
                                            &mut transient_msg,
                                            &mut need_redraw,
                                            &mut runtime_presentation_intents,
                                            dispatch_outcome,
                                        );
                                    } else {
                                        log::debug!(
                                            "[main][selector] selector key route resolved without runtime session: key={:?}, session_id={}, command={:?}",
                                            key,
                                            session_id,
                                            command
                                        );
                                        transient_msg = Some(
                                            "Selector runtime session is not available".to_string(),
                                        );
                                        need_redraw = true;
                                    }
                                }
                                SelectorKeyRoute::QueryEdit { session_id, edit } => {
                                    handled = true;
                                    if let (Some(runtime_session), Some(selector_model)) =
                                        (runtime_session.as_mut(), selector_model.as_ref())
                                    {
                                        let next_query = edit.apply_to(&selector_model.query);
                                        log::info!(
                                            "[main][selector] selector query edit routing start: key={:?}, session_id={}, edit={:?}, prev_query_len={}, next_query_len={}",
                                            key,
                                            session_id,
                                            edit,
                                            selector_model.query.len(),
                                            next_query.len()
                                        );
                                        let mut host_session = MainRuntimeHostSession::new(
                                            &mut outcome,
                                            &mut session_state,
                                        );
                                        let dispatch_outcome = runtime_session
                                            .update_selector_query(
                                                session_id,
                                                next_query,
                                                &mut host_session,
                                            )
                                            .await;
                                        if dispatch_outcome.transient_message.is_some() {
                                            log::debug!(
                                                "[main][selector] selector query edit failed from key routing: key={:?}, session_id={}, edit={:?}",
                                                key,
                                                session_id,
                                                edit
                                            );
                                        } else {
                                            log::info!(
                                                "[main][selector] selector query edit succeeded from key routing: key={:?}, session_id={}, edit={:?}",
                                                key,
                                                session_id,
                                                edit
                                            );
                                        }
                                        let _ = apply_runtime_dispatch_outcome(
                                            &mut transient_msg,
                                            &mut need_redraw,
                                            &mut runtime_presentation_intents,
                                            dispatch_outcome,
                                        );
                                    } else {
                                        log::debug!(
                                            "[main][selector] selector query edit route resolved without runtime session/model: key={:?}, session_id={}, edit={:?}",
                                            key,
                                            session_id,
                                            edit
                                        );
                                        transient_msg = Some(
                                            "Selector runtime session is not available".to_string(),
                                        );
                                        need_redraw = true;
                                    }
                                }
                                SelectorKeyRoute::ModeSwitch { session_id, switch } => {
                                    handled = true;
                                    let mode = match switch {
                                        SelectorModeSwitch::EnterInsert => SelectorMode::Insert,
                                        SelectorModeSwitch::EnterNormal => SelectorMode::Normal,
                                    };
                                    log::info!(
                                        "[main][selector] selector mode switch routing start: key={:?}, session_id={}, switch={:?}, mode={:?}",
                                        key,
                                        session_id,
                                        switch,
                                        mode
                                    );
                                    if let Some(runtime_session) = runtime_session.as_ref() {
                                        match runtime_session
                                            .selector_tui_projection_sink()
                                            .set_mode(session_id, mode)
                                        {
                                            Ok(()) => {
                                                log::info!(
                                                    "[main][selector] selector mode switch succeeded: key={:?}, session_id={}, mode={:?}",
                                                    key,
                                                    session_id,
                                                    mode
                                                );
                                                need_redraw = true;
                                            }
                                            Err(error) => {
                                                log::debug!(
                                                    "[main][selector] selector mode switch failed: key={:?}, session_id={}, mode={:?}, error={:?}",
                                                    key,
                                                    session_id,
                                                    mode,
                                                    error
                                                );
                                                transient_msg = Some(format!(
                                                    "Selector mode switch failed: {:?}",
                                                    error
                                                ));
                                                need_redraw = true;
                                            }
                                        }
                                    } else {
                                        log::debug!(
                                            "[main][selector] selector mode switch route resolved without runtime session: key={:?}, session_id={}, switch={:?}",
                                            key,
                                            session_id,
                                            switch
                                        );
                                        transient_msg = Some(
                                            "Selector runtime session is not available".to_string(),
                                        );
                                        need_redraw = true;
                                    }
                                }
                                SelectorKeyRoute::Noop { session_id } => {
                                    handled = true;
                                    log::debug!(
                                        "[main][selector] selector consumed unmapped key as noop: key={:?}, session_id={}",
                                        key,
                                        session_id
                                    );
                                }
                                SelectorKeyRoute::Inactive => {
                                    log::debug!(
                                        "[main][selector] selector inactive; delegating key to normal input: key={:?}",
                                        key
                                    );
                                }
                                SelectorKeyRoute::Unmapped => {}
                            }
                        }

                        if !handled {
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
                                        Some(&lsif_bridge),
                                    )
                                    .await
                                    {
                                        break 'main reason;
                                    }
                                }
                                PromptInputAction::NotPromptInput => {}
                            }
                        }

                        if !handled && let Some(prompt) = command_line_prompt {
                            match key {
                                KeyInput::Up
                                | KeyInput::Down
                                | KeyInput::Ctrl('p')
                                | KeyInput::Ctrl('P')
                                | KeyInput::Ctrl('n')
                                | KeyInput::Ctrl('N') => {
                                    if let Some(direction) = history_direction_for_key(&key) {
                                        if let Some(selected_buffer) = command_line_histories
                                            .navigate(prompt, command_line_edit.buffer(), direction)
                                        {
                                            command_line_edit.set_buffer_to_end(selected_buffer);
                                            if prompt == '/' {
                                                let _ = outcome
                                                    .core_bridge
                                                    .sync_search_input(command_line_edit.buffer());
                                                consume_core_outcomes_from_core(
                                                    &mut outcome.core_bridge,
                                                    &mut outcome_accumulator,
                                                    &mut need_redraw,
                                                );
                                            }
                                        }
                                    }
                                }
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
                                    command_line_edit.clear();
                                    command_line_histories.reset_navigation();
                                }
                                KeyInput::Enter => {
                                    if prompt == ':' {
                                        let cmd =
                                            format!("{}{}", prompt, command_line_edit.buffer());
                                        record_history_and_save_to_default_cache(
                                            &mut command_line_histories,
                                            prompt,
                                            command_line_edit.buffer(),
                                        );
                                        command_line_prompt = None;
                                        command_line_edit.clear();
                                        match route_ex_command(&cmd) {
                                            ExCommandRoute::NoOp => {
                                                log::debug!(
                                                    "[main] empty ex command completed as no-op"
                                                );
                                            }
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
                                                if let Some(command_name) =
                                                    startup_registered_command_name_for_ex_command(
                                                        &cmd,
                                                        &outcome.callback_registry,
                                                    )
                                                {
                                                    log::info!(
                                                        "[main][command_line] executing startup registered command from ex command: command={}",
                                                        command_name
                                                    );
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
                                                } else {
                                                    let _ =
                                                        outcome.core_bridge.apply_ex_command(&cmd);
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
                                                }
                                            }
                                            ExCommandRoute::UnsupportedPlanned => {
                                                log::debug!(
                                                    "[main] set option command is registered but not implemented in host I/O yet: command={:?}",
                                                    cmd
                                                );
                                                transient_msg = Some(
                                                    "This option is planned but not supported yet"
                                                        .to_string(),
                                                );
                                            }
                                        }
                                    } else if prompt == '/' {
                                        record_history_and_save_to_default_cache(
                                            &mut command_line_histories,
                                            prompt,
                                            command_line_edit.buffer(),
                                        );
                                        let _ = outcome
                                            .core_bridge
                                            .commit_search_input(command_line_edit.buffer());
                                        consume_core_outcomes_from_core(
                                            &mut outcome.core_bridge,
                                            &mut outcome_accumulator,
                                            &mut need_redraw,
                                        );
                                        command_line_prompt = None;
                                        command_line_edit.clear();
                                    }
                                    sync_session_dirty_from_core(
                                        &mut session_state,
                                        &outcome.core_bridge,
                                    );
                                }
                                KeyInput::Backspace
                                | KeyInput::Ctrl('h')
                                | KeyInput::Ctrl('H')
                                | KeyInput::Delete => {
                                    command_line_histories.reset_navigation();
                                    let changed = command_line_edit.apply_action(
                                        command_line_edit_action_for_key(&key).expect(
                                            "backspace/delete must map to command-line edit action",
                                        ),
                                    );
                                    if prompt == '/' && changed {
                                        let _ = outcome
                                            .core_bridge
                                            .sync_search_input(command_line_edit.buffer());
                                        consume_core_outcomes_from_core(
                                            &mut outcome.core_bridge,
                                            &mut outcome_accumulator,
                                            &mut need_redraw,
                                        );
                                    } else if prompt == ':' && !changed {
                                        command_line_prompt = None;
                                    }
                                }
                                KeyInput::Left
                                | KeyInput::Right
                                | KeyInput::Home
                                | KeyInput::End
                                | KeyInput::Ctrl('b')
                                | KeyInput::Ctrl('B')
                                | KeyInput::Ctrl('f')
                                | KeyInput::Ctrl('F')
                                | KeyInput::Ctrl('a')
                                | KeyInput::Ctrl('A')
                                | KeyInput::Ctrl('e')
                                | KeyInput::Ctrl('E') => {
                                    if let Some(action) = command_line_edit_action_for_key(&key) {
                                        command_line_edit.apply_action(action);
                                    }
                                }
                                KeyInput::Char(c) => {
                                    command_line_histories.reset_navigation();
                                    command_line_edit.insert_char(c);
                                    if prompt == '/' {
                                        let _ = outcome
                                            .core_bridge
                                            .sync_search_input(command_line_edit.buffer());
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
                                Some(&lsif_bridge),
                            )
                            .await
                            {
                                break 'main reason;
                            }
                        }

                        if !handled {
                            if let Some(prompt) = begin_command_line_from_focused_panel(
                                &mut panel_manager,
                                &key,
                                outcome.core_bridge.mode(),
                            ) {
                                command_line_prompt = Some(prompt);
                                command_line_edit.clear();
                                command_line_histories.reset_navigation();
                                handled = true;
                                need_redraw = true;
                                workspace_projection_dirty = true;
                            }
                        }

                        if !handled {
                            if let Some(effect) = handle_terminal_panel_key(
                                &mut panel_manager,
                                &mut terminal_float_manager,
                                &key,
                            ) {
                                match effect {
                                    FloatingWindowKeyHandling::Consumed => {
                                        handled = true;
                                        need_redraw = true;
                                        workspace_projection_dirty = true;
                                    }
                                    FloatingWindowKeyHandling::Closed { id } => {
                                        handled = true;
                                        need_redraw = true;
                                        workspace_projection_dirty = true;
                                        log::debug!(
                                            "[main][panel] terminal panel closed from focused input: pseudo_float_id={}",
                                            id.0
                                        );
                                    }
                                }
                            }
                        }

                        if !handled {
                            if let Some(effect) = handle_terminal_float_key(
                                &floating_window_manager,
                                &mut terminal_float_manager,
                                &key,
                            ) {
                                match effect {
                                    FloatingWindowKeyHandling::Consumed => {
                                        handled = true;
                                        need_redraw = true;
                                        workspace_projection_dirty = true;
                                    }
                                    FloatingWindowKeyHandling::Closed { id } => {
                                        handled = true;
                                        need_redraw = true;
                                        workspace_projection_dirty = true;
                                        log::debug!(
                                            "[main][terminal_float] terminal float closed from focused input: id={}",
                                            id.0
                                        );
                                    }
                                }
                            }
                        }

                        if !handled {
                            if let Some(effect) = handle_core_window_float_key(
                                &mut floating_window_manager,
                                &mut outcome.core_bridge,
                                &key,
                            ) {
                                match effect {
                                    FloatingWindowKeyHandling::Consumed => {
                                        handled = true;
                                        need_redraw = true;
                                        workspace_projection_dirty = true;
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
                                    FloatingWindowKeyHandling::Closed { id } => {
                                        handled = true;
                                        need_redraw = true;
                                        workspace_projection_dirty = true;
                                        log::debug!(
                                            "[main] core-window float closed from focused input: id={}",
                                            id.0
                                        );
                                    }
                                }
                            }
                        }

                        if !handled {
                            if let Some(effect) =
                                handle_mermaid_preview_key(&mut session_state, &key)
                            {
                                handled = true;
                                need_redraw = true;
                                workspace_projection_dirty = true;
                                match effect {
                                    FloatingWindowKeyHandling::Consumed => {}
                                    FloatingWindowKeyHandling::Closed { id } => {
                                        log::debug!(
                                            "[main][markdown_preview] Mermaid preview closed from focused input: id={}",
                                            id.0
                                        );
                                    }
                                }
                            }
                        }

                        if !handled {
                            let active_window_id = input_snapshot
                                .active_window_id()
                                .or_else(|| {
                                    last_workspace_model
                                        .as_ref()
                                        .map(|workspace| workspace.active_window_id)
                                })
                                .or_else(|| outcome.core_bridge.snapshot().active_window_id())
                                .unwrap_or(0);
                            let before_completion_snapshot = outcome.core_bridge.light_snapshot();
                            if let Some(effect) = handle_completion_float_key(
                                &mut completion_float_manager,
                                &mut floating_window_manager,
                                &mut outcome.core_bridge,
                                &key,
                                active_window_id,
                            ) {
                                match effect {
                                    FloatingWindowKeyHandling::Consumed => {
                                        handled = true;
                                        need_redraw = true;
                                        workspace_projection_dirty = true;
                                    }
                                    FloatingWindowKeyHandling::Closed { id } => {
                                        handled = true;
                                        need_redraw = true;
                                        workspace_projection_dirty = true;
                                        log::debug!(
                                            "[main] completion float closed from focused input: id={}",
                                            id.0
                                        );
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
                                        let after_completion_snapshot =
                                            outcome.core_bridge.light_snapshot();
                                        if after_completion_snapshot.revision
                                            != before_completion_snapshot.revision
                                            && let Some(reason) =
                                                dispatch_buffer_changed_with_runtime(
                                                    runtime_session.as_mut(),
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
                                }
                            }
                        }

                        if !handled {
                            let active_window_id = input_snapshot
                                .active_window_id()
                                .or_else(|| {
                                    last_workspace_model
                                        .as_ref()
                                        .map(|workspace| workspace.active_window_id)
                                })
                                .or_else(|| outcome.core_bridge.snapshot().active_window_id())
                                .unwrap_or(0);
                            if let Some(effect) = handle_floating_window_key(
                                &mut floating_window_manager,
                                &key,
                                active_window_id,
                            ) {
                                match effect {
                                    FloatingWindowKeyHandling::Consumed => {
                                        handled = true;
                                        need_redraw = true;
                                        workspace_projection_dirty = true;
                                    }
                                    FloatingWindowKeyHandling::Closed { id } => {
                                        handled = true;
                                        need_redraw = true;
                                        workspace_projection_dirty = true;
                                        log::debug!(
                                            "[main] floating window closed from focused input: id={}",
                                            id.0
                                        );
                                    }
                                }
                            }
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
                            startup_keymap_pending_lhs = None;
                            let intent = resolve_intent(&key);
                            log::info!(
                                "[main][input] key not handled by startup keymap, dispatching intent: key={:?}, intent={:?}",
                                key,
                                intent
                            );
                            match intent {
                                EditorIntent::EditKey(k) => {
                                    viewport_sync_mode = viewport_sync_mode_for_input(&key);
                                    let before_snapshot = outcome.core_bridge.light_snapshot();
                                    let need_redraw_before_dispatch = need_redraw;
                                    let dispatch_result = outcome.core_bridge.dispatch_key(&k);
                                    let after_snapshot = outcome.core_bridge.light_snapshot();
                                    if apply_floating_lifecycle_after_core_edit(
                                        &mut floating_window_manager,
                                        &before_snapshot,
                                        &after_snapshot,
                                    ) {
                                        workspace_projection_dirty = true;
                                    }
                                    trace_redraw_diagnostic(format_args!(
                                        "edit key dispatched: key={:?}, result={:?}, revision {}->{}, cursor ({},{}) -> ({},{}), mode {:?}->{:?}, need_redraw_before={}",
                                        k,
                                        dispatch_result,
                                        before_snapshot.revision,
                                        after_snapshot.revision,
                                        before_snapshot.cursor_row,
                                        before_snapshot.cursor_col,
                                        after_snapshot.cursor_row,
                                        after_snapshot.cursor_col,
                                        before_snapshot.mode,
                                        after_snapshot.mode,
                                        need_redraw_before_dispatch
                                    ));
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
                                        Some(&lsif_bridge),
                                    )
                                    .await
                                    {
                                        break 'main reason;
                                    }

                                    if after_snapshot.revision != before_snapshot.revision {
                                        if let Some(reason) = dispatch_buffer_changed_with_runtime(
                                            runtime_session.as_mut(),
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
                                            Some(&lsif_bridge),
                                        )
                                        .await
                                        {
                                            break 'main reason;
                                        }
                                    }

                                    sync_session_dirty_from_core(
                                        &mut session_state,
                                        &outcome.core_bridge,
                                    );
                                    trace_redraw_diagnostic(format_args!(
                                        "edit key host policy forcing redraw after dispatch: key={:?}, cursor=({},{}), revision={}, prior_need_redraw={}",
                                        k,
                                        after_snapshot.cursor_row,
                                        after_snapshot.cursor_col,
                                        after_snapshot.revision,
                                        need_redraw
                                    ));
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
                                                Some(&lsif_bridge),
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
                        log::debug!(
                            "[main] processing mouse click event at terminal coordinates: column={}, row={}",
                            column,
                            row
                        );
                        let (terminal_width, terminal_height) = current_terminal_size();
                        let mermaid_preview_focused = focus_mermaid_preview_from_mouse_click(
                            &mut session_state,
                            last_workspace_model.as_ref(),
                            column,
                            row,
                        );
                        let mouse_focus = if mermaid_preview_focused {
                            FloatingMouseOutcome::Focused {
                                id: active_mermaid_preview_float_id(last_workspace_model.as_ref())
                                    .unwrap_or(FloatingWindowId(0)),
                            }
                        } else {
                            focus_floating_window_from_mouse_click(
                                &mut floating_window_manager,
                                last_workspace_model.as_ref(),
                                column,
                                row,
                                terminal_width,
                                terminal_height,
                            )
                        };
                        if matches!(mouse_focus, FloatingMouseOutcome::Focused { .. }) {
                            workspace_projection_dirty = true;
                        } else if let Some(sequence) =
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
                                Some(&lsif_bridge),
                            )
                            .await
                            {
                                break 'main reason;
                            }

                            sync_session_dirty_from_core(&mut session_state, &outcome.core_bridge);
                        } else {
                            log::debug!(
                                "[main] ignoring mouse click outside editor body: column={}, row={}",
                                column,
                                row
                            );
                        }
                        need_redraw = true;
                    }
                    UiEvent::MouseWheel {
                        column,
                        row,
                        delta_x,
                        delta_y,
                    } => {
                        log::debug!(
                            "[main] processing mouse wheel event: column={}, row={}, delta=({}, {})",
                            column,
                            row,
                            delta_x,
                            delta_y
                        );
                        if handle_mermaid_preview_mouse_wheel(
                            &mut session_state,
                            last_workspace_model.as_ref(),
                            column,
                            row,
                            delta_x,
                            delta_y,
                        ) {
                            need_redraw = true;
                            workspace_projection_dirty = true;
                        }
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
                            Some(&lsif_bridge),
                        )
                        .await
                        {
                            break 'main reason;
                        }

                        sync_session_dirty_from_core(&mut session_state, &outcome.core_bridge);
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

            if need_redraw {
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
                        &mut need_redraw,
                    );
                }
                if terminal_display_redraw_plan.is_none() {
                    match render_command_line_only_redraw_if_possible(
                        &mut render_coordinator,
                        Some(&mut terminal_broker),
                        &mut last_workspace_model,
                        outcome_accumulator.last_structural_refresh.as_ref(),
                        workspace_projection_dirty,
                        command_line_prompt,
                        command_line_edit.buffer(),
                        command_line_edit.cursor_byte_index(),
                        session_state.tab_size(),
                    ) {
                        CommandLineOnlyRedraw::Rendered => continue 'main,
                        CommandLineOnlyRedraw::NotApplicable | CommandLineOnlyRedraw::Fallback => {}
                    }
                } else {
                    trace_job_control_diagnostic(format_args!(
                        "command-line-only redraw bypassed because terminal display was invalidated"
                    ));
                }
                let redraw_started_at = std::time::Instant::now();
                let redraw_result = build_workspace_render_output(
                    &mut outcome,
                    &mut session_state,
                    &mut viewport_store,
                    viewport_sync_mode,
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
                viewport_sync_mode = ViewportSyncMode::Core;
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
                match render_coordinator
                    .render_workspace_result_with_structural_refresh_and_redraw_plan(
                        redraw_result,
                        &capability_profile,
                        &runtime_presentation_intents,
                        Some(&mut terminal_broker),
                        outcome_accumulator.last_structural_refresh.as_ref(),
                        redraw_plan,
                    ) {
                    Ok(render_output) => {
                        if let Some(message) = redraw_failure {
                            transient_msg = Some(message);
                        }
                        last_workspace_model = Some(render_output.rendered_workspace.clone());
                        mark_structural_refresh_rendered(&mut outcome_accumulator);
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
                        terminal_display_redraw_plan = None;
                        workspace_projection_dirty = false;
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

fn apply_floating_lifecycle_after_core_edit(
    floating_window_manager: &mut FloatingWindowManager,
    before: &CoreLightSnapshot,
    after: &CoreLightSnapshot,
) -> bool {
    let restore_window_id = after
        .active_window_id()
        .or_else(|| before.active_window_id());
    let mut closed = Vec::new();
    if before.cursor_row != after.cursor_row
        || before.cursor_col != after.cursor_col
        || before.active_window_id() != after.active_window_id()
    {
        if let Some(window_id) = restore_window_id {
            closed.extend(
                floating_window_manager
                    .apply_lifecycle_event(
                        FloatingLifecycleEvent::CursorMoved {
                            window_id,
                            row: after.cursor_row,
                            col: after.cursor_col,
                        },
                        Some(window_id),
                    )
                    .closed,
            );
        }
    }
    if before.mode != CoreMode::Insert
        && after.mode == CoreMode::Insert
        && let Some(window_id) = restore_window_id
    {
        closed.extend(
            floating_window_manager
                .apply_lifecycle_event(
                    FloatingLifecycleEvent::InsertStarted { window_id },
                    Some(window_id),
                )
                .closed,
        );
    }
    if before.mode != after.mode
        && let Some(window_id) = restore_window_id
    {
        closed.extend(
            floating_window_manager
                .apply_lifecycle_event(
                    FloatingLifecycleEvent::ModeChanged {
                        window_id,
                        from: floating_editor_mode_from_core(before.mode),
                        to: floating_editor_mode_from_core(after.mode),
                    },
                    Some(window_id),
                )
                .closed,
        );
    }
    if let (Some(before_window_id), Some(after_window_id)) =
        (before.active_window_id(), after.active_window_id())
        && before_window_id != after_window_id
    {
        closed.extend(
            floating_window_manager
                .apply_lifecycle_event(
                    FloatingLifecycleEvent::WindowLeft {
                        from_window_id: before_window_id,
                        to_window_id: after_window_id,
                    },
                    Some(after_window_id),
                )
                .closed,
        );
    }
    if before.revision != after.revision {
        closed.extend(
            floating_window_manager
                .apply_lifecycle_event(
                    FloatingLifecycleEvent::BufferChanged {
                        buffer_id: after
                            .buffers
                            .iter()
                            .find(|buffer| buffer.is_active)
                            .map(|buffer| buffer.id)
                            .unwrap_or(0),
                        revision: after.revision,
                    },
                    restore_window_id,
                )
                .closed,
        );
    }
    let did_close = !closed.is_empty();
    if did_close {
        log::debug!(
            "[main][floating_window] lifecycle closed float(s) after core edit: closed={:?}, cursor=({},{}), revision={}",
            closed.iter().map(|id| id.0).collect::<Vec<_>>(),
            after.cursor_row,
            after.cursor_col,
            after.revision
        );
    }
    did_close
}

/// `vim_core_rs::CoreMode` を float lifecycle が扱う中立 `EditorMode` に
/// 変換する。コアの細かいモード（VisualLine 等）は float ライフサイクル
/// 上は同一視して構わないため、Visual / Select 系は EditorMode::Visual に
/// 集約し、OperatorPending は Normal の延長として扱う。
fn floating_editor_mode_from_core(
    mode: CoreMode,
) -> saya::presentation::floating_window::EditorMode {
    use saya::presentation::floating_window::EditorMode;
    match mode {
        CoreMode::Normal | CoreMode::OperatorPending => EditorMode::Normal,
        CoreMode::Insert => EditorMode::Insert,
        CoreMode::Visual
        | CoreMode::VisualLine
        | CoreMode::VisualBlock
        | CoreMode::Select
        | CoreMode::SelectLine
        | CoreMode::SelectBlock => EditorMode::Visual,
        CoreMode::Replace => EditorMode::Replace,
        CoreMode::CommandLine => EditorMode::Command,
    }
}

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

async fn process_pending_host_actions_with_runtime(
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    outcome_accumulator: &mut MainOutcomeAccumulator,
    session_state: &mut saya::app::session::EditorSessionState,
    transient_msg: &mut Option<String>,
    system_warning: &mut Option<String>,
    host_action_runtime: &mut HostActionRuntime,
    mut runtime_session: Option<&mut RuntimeSessionOwner>,
    need_redraw: &mut bool,
    runtime_presentation_intents: &mut Vec<RuntimePresentationIntent>,
    lsif_bridge: Option<&LsifBridgeHandle>,
) -> Option<ShutdownReason> {
    let mut shutdown_reason = None;
    loop {
        if let Err(error) = host_action_runtime.drain_job_events(&mut outcome.core_bridge) {
            log::debug!("[main] failed to drain job events: {:?}", error);
        }
        consume_core_outcomes_from_core(&mut outcome.core_bridge, outcome_accumulator, need_redraw);

        let current_revision = outcome.core_bridge.revision();
        let directives = std::mem::take(&mut outcome_accumulator.host_directives);
        if directives.is_empty() {
            break;
        }

        let mut last_write_pending_directory_confirmation = false;
        for directive in prioritize_save_family_host_directives(directives, current_revision) {
            match directive {
                NormalizedHostDirective::Write { path, force, .. } => {
                    let write_effect = handle_write_host_action_with_runtime(
                        outcome,
                        session_state,
                        Some(path.as_str()),
                        force,
                        transient_msg,
                        system_warning,
                        runtime_session.as_deref_mut(),
                        need_redraw,
                        runtime_presentation_intents,
                        lsif_bridge,
                    )
                    .await;
                    if let Some(reason) = write_effect.shutdown_reason {
                        merge_shutdown_reason(&mut shutdown_reason, Some(reason));
                    }
                    last_write_pending_directory_confirmation =
                        write_effect.pending_directory_confirmation;
                }
                NormalizedHostDirective::Quit { force, .. } => {
                    if defer_directory_save_then_quit_if_confirmation_pending(
                        session_state,
                        force,
                        last_write_pending_directory_confirmation,
                    ) {
                        last_write_pending_directory_confirmation = false;
                        continue;
                    }
                    last_write_pending_directory_confirmation = false;
                    let decision = session_state.evaluate_quit(force);
                    if let Some(reason) =
                        shutdown_reason_from_quit_decision(decision, force, system_warning)
                    {
                        merge_shutdown_reason(&mut shutdown_reason, Some(reason));
                    }
                }
                NormalizedHostDirective::Suspend { trace } => {
                    log::debug!(
                        "[main] processing normalized suspend directive: sequence={}",
                        trace.sequence
                    );
                    outcome_accumulator.suspend_requested = true;
                }
                NormalizedHostDirective::VfsRequest { request, trace } => {
                    log::debug!(
                        "[main] processing normalized VFS directive: sequence={}, request={:?}",
                        trace.sequence,
                        request
                    );
                    if handle_directory_buffer_vfs_save_request(
                        outcome,
                        session_state,
                        request.clone(),
                        transient_msg,
                        system_warning,
                    )
                    .is_some()
                    {
                        continue;
                    }
                    if handle_directory_buffer_vfs_load_request(
                        outcome,
                        session_state,
                        request.clone(),
                    )
                    .is_some()
                    {
                        continue;
                    }
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

fn sync_session_dirty_from_core(
    session_state: &mut saya::app::session::EditorSessionState,
    core_bridge: &CoreBridge,
) {
    session_state.update_dirty_at_revision(core_bridge.dirty(), Some(core_bridge.revision()));
}

fn process_pending_host_actions_without_runtime(
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    outcome_accumulator: &mut MainOutcomeAccumulator,
    session_state: &mut saya::app::session::EditorSessionState,
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

        let current_revision = outcome.core_bridge.revision();
        let directives = std::mem::take(&mut outcome_accumulator.host_directives);
        if directives.is_empty() {
            break;
        }

        let mut last_write_pending_directory_confirmation = false;
        for directive in prioritize_save_family_host_directives(directives, current_revision) {
            match directive {
                NormalizedHostDirective::Write { path, force, .. } => {
                    let snapshot = outcome.core_bridge.snapshot();
                    let save_outcome = save_snapshot_result_with_confirmation(
                        &snapshot.text,
                        session_state,
                        Some(path.as_str()),
                        force,
                        Some(current_revision),
                    );
                    *transient_msg = save_outcome.transient_message;
                    clear_stale_quit_warning_after_write_attempt(
                        system_warning,
                        transient_msg.as_deref(),
                    );
                    if save_outcome.wrote {
                        refresh_directory_buffer_after_confirmed_save(
                            outcome,
                            session_state,
                            transient_msg,
                        );
                    }
                    last_write_pending_directory_confirmation =
                        save_outcome.pending_directory_confirmation;
                }
                NormalizedHostDirective::Quit { force, .. } => {
                    if defer_directory_save_then_quit_if_confirmation_pending(
                        session_state,
                        force,
                        last_write_pending_directory_confirmation,
                    ) {
                        last_write_pending_directory_confirmation = false;
                        continue;
                    }
                    last_write_pending_directory_confirmation = false;
                    let decision = session_state.evaluate_quit(force);
                    if let Some(reason) =
                        shutdown_reason_from_quit_decision(decision, force, system_warning)
                    {
                        return Some(reason);
                    }
                }
                NormalizedHostDirective::Suspend { trace } => {
                    log::debug!(
                        "[main] processing normalized suspend directive without runtime: sequence={}",
                        trace.sequence
                    );
                    outcome_accumulator.suspend_requested = true;
                }
                NormalizedHostDirective::VfsRequest { request, trace } => {
                    log::debug!(
                        "[main] processing normalized VFS directive without runtime: sequence={}, request={:?}",
                        trace.sequence,
                        request
                    );
                    if handle_directory_buffer_vfs_save_request(
                        outcome,
                        session_state,
                        request.clone(),
                        transient_msg,
                        system_warning,
                    )
                    .is_some()
                    {
                        continue;
                    }
                    if handle_directory_buffer_vfs_load_request(
                        outcome,
                        session_state,
                        request.clone(),
                    )
                    .is_some()
                    {
                        continue;
                    }
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

#[derive(Debug, Default)]
struct WriteHostActionEffect {
    shutdown_reason: Option<ShutdownReason>,
    pending_directory_confirmation: bool,
}

async fn handle_write_host_action_with_runtime(
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    session_state: &mut saya::app::session::EditorSessionState,
    path_override: Option<&str>,
    confirmed: bool,
    transient_msg: &mut Option<String>,
    system_warning: &mut Option<String>,
    runtime_session: Option<&mut RuntimeSessionOwner>,
    need_redraw: &mut bool,
    runtime_presentation_intents: &mut Vec<RuntimePresentationIntent>,
    lsif_bridge: Option<&LsifBridgeHandle>,
) -> WriteHostActionEffect {
    let snapshot = outcome.core_bridge.snapshot();
    log::debug!(
        "[main] processing write host action with runtime integration: path_present={}, contents_len={}",
        path_override.filter(|path| !path.is_empty()).is_some()
            || session_state.target_path().is_some(),
        snapshot.text.len()
    );
    let save_outcome = save_snapshot_result_with_confirmation(
        &snapshot.text,
        session_state,
        path_override,
        confirmed,
        Some(outcome.core_bridge.revision()),
    );
    *transient_msg = save_outcome.transient_message;
    clear_stale_quit_warning_after_write_attempt(system_warning, transient_msg.as_deref());
    if save_outcome.wrote {
        refresh_directory_buffer_after_confirmed_save(outcome, session_state, transient_msg);
        return WriteHostActionEffect {
            shutdown_reason: dispatch_buffer_write_post_with_runtime(
                runtime_session,
                outcome,
                session_state,
                transient_msg,
                need_redraw,
                runtime_presentation_intents,
                lsif_bridge,
            )
            .await,
            pending_directory_confirmation: false,
        };
    }

    WriteHostActionEffect {
        shutdown_reason: None,
        pending_directory_confirmation: save_outcome.pending_directory_confirmation,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirectoryOperationConfirmationKeyAction {
    Confirm,
    Cancel,
    KeepWaiting,
}

fn directory_operation_confirmation_key_action(
    key: &KeyInput,
    session_state: &saya::app::session::EditorSessionState,
) -> Option<DirectoryOperationConfirmationKeyAction> {
    if !session_state.directory_operation_confirmation_dialog_active() {
        return None;
    }
    let action = match key {
        KeyInput::Enter | KeyInput::Char('y') | KeyInput::Char('Y') => {
            DirectoryOperationConfirmationKeyAction::Confirm
        }
        KeyInput::Escape | KeyInput::Char('n') | KeyInput::Char('N') => {
            DirectoryOperationConfirmationKeyAction::Cancel
        }
        _ => DirectoryOperationConfirmationKeyAction::KeepWaiting,
    };
    Some(action)
}

fn directory_operation_cancel_message(
    session_state: &mut saya::app::session::EditorSessionState,
) -> String {
    match session_state.cancel_directory_buffer_operation_preview() {
        Some(_) => "Directory operation cancelled; no filesystem changes were applied".to_string(),
        None => "No directory operation preview to cancel".to_string(),
    }
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

async fn handle_directory_operation_confirmation_key_with_runtime(
    key: &KeyInput,
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    session_state: &mut saya::app::session::EditorSessionState,
    transient_msg: &mut Option<String>,
    need_redraw: &mut bool,
    runtime_session: Option<&mut RuntimeSessionOwner>,
    runtime_presentation_intents: &mut Vec<RuntimePresentationIntent>,
    lsif_bridge: Option<&LsifBridgeHandle>,
) -> Option<Option<ShutdownReason>> {
    let action = directory_operation_confirmation_key_action(key, session_state)?;
    *need_redraw = true;
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
                return Some(merge_confirmation_shutdown_reason(
                    dispatch_buffer_write_post_with_runtime(
                        runtime_session,
                        outcome,
                        session_state,
                        transient_msg,
                        need_redraw,
                        runtime_presentation_intents,
                        lsif_bridge,
                    )
                    .await,
                    take_pending_directory_save_then_quit_shutdown(session_state),
                ));
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
    Some(None)
}

fn merge_confirmation_shutdown_reason(
    write_post_reason: Option<ShutdownReason>,
    pending_quit_reason: Option<ShutdownReason>,
) -> Option<ShutdownReason> {
    let mut reason = write_post_reason;
    merge_shutdown_reason(&mut reason, pending_quit_reason);
    reason
}

fn refresh_directory_buffer_after_confirmed_save(
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    session_state: &mut saya::app::session::EditorSessionState,
    transient_msg: &mut Option<String>,
) {
    let Some(root_path) = session_state
        .target_path()
        .filter(|path| path.is_dir())
        .cloned()
    else {
        return;
    };
    log::debug!(
        "[main][dired][writable] refreshing directory buffer after confirmed save: root_path={}",
        root_path.display()
    );
    if let Err(error) = execute_runtime_host_command(
        &format!("edit {}", root_path.display()),
        outcome,
        session_state,
    ) {
        log::debug!(
            "[main][dired][writable] failed to refresh directory buffer after confirmed save: root_path={}, error={:?}",
            root_path.display(),
            error
        );
        *transient_msg = Some(format!(
            "Directory operations applied, but refresh failed: {error:?}"
        ));
    }
}

fn handle_directory_buffer_vfs_save_request(
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    session_state: &mut saya::app::session::EditorSessionState,
    request: CoreVfsRequest,
    transient_msg: &mut Option<String>,
    system_warning: &mut Option<String>,
) -> Option<SaveSnapshotOutcome> {
    let CoreVfsRequest::Save {
        request_id,
        document_id,
        target_locator,
        text,
        force,
        ..
    } = request
    else {
        return None;
    };
    let path_override = target_locator.as_deref().or(Some(document_id.as_str()));
    if effective_host_write_path_override(session_state, path_override).is_some()
        || session_state.directory_buffer().is_none()
    {
        return None;
    }

    log::debug!(
        "[main][dired][writable] handling directory buffer save through VFS request: document_id={}, force={}, text_len={}",
        document_id,
        force,
        text.len()
    );
    let save_outcome =
        save_snapshot_result_with_confirmation(&text, session_state, path_override, force, None);
    *transient_msg = save_outcome.transient_message.clone();
    clear_stale_quit_warning_after_write_attempt(system_warning, transient_msg.as_deref());
    let response = if save_outcome.wrote {
        CoreVfsResponse::Saved {
            request_id,
            document_id,
        }
    } else {
        CoreVfsResponse::Failed {
            request_id,
            error: CoreVfsError {
                kind: CoreVfsErrorKind::HostUnavailable,
                message: save_outcome.transient_message.clone(),
            },
        }
    };
    if let Err(error) = outcome.core_bridge.submit_vfs_response(response) {
        log::debug!(
            "[main][dired][writable] failed to submit directory buffer VFS save response: {:?}",
            error
        );
    }
    if save_outcome.wrote {
        refresh_directory_buffer_after_confirmed_save(outcome, session_state, transient_msg);
    }
    Some(save_outcome)
}

fn handle_directory_buffer_vfs_load_request(
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    session_state: &mut saya::app::session::EditorSessionState,
    request: CoreVfsRequest,
) -> Option<bool> {
    let CoreVfsRequest::Load {
        request_id,
        document_id,
        ..
    } = request
    else {
        return None;
    };
    let path = path_from_core_vfs_document_id(&document_id)?;
    if !path.is_dir() {
        return None;
    }

    let started_at = std::time::Instant::now();
    let existing_directory_buffer = session_state.directory_buffer().filter(|directory_buffer| {
        paths_refer_to_same_location_main(&directory_buffer.root_path, &path)
    });
    let listing_path = existing_directory_buffer
        .map(|directory_buffer| directory_buffer.root_path.clone())
        .unwrap_or_else(|| path.clone());
    let listing_options = existing_directory_buffer
        .map(|directory_buffer| directory_buffer.listing_options.clone())
        .unwrap_or_default();
    let response = match session_state
        .refresh_directory_buffer_listing(listing_path.clone(), listing_options)
    {
        Ok(entries) => {
            let text = session_state
                .directory_buffer()
                .map(|directory_buffer| directory_buffer.display_text.clone())
                .unwrap_or_default();
            log::debug!(
                "[PERF][main][dired] handled directory VFS load via session metadata: path={}, entries={}, text_len={}, elapsed_ms={}",
                listing_path.display(),
                entries.len(),
                text.len(),
                started_at.elapsed().as_millis()
            );
            CoreVfsResponse::Loaded {
                request_id,
                document_id,
                text,
            }
        }
        Err(error) => {
            log::debug!(
                "[main][dired] failed to handle directory VFS load via session metadata: path={}, elapsed_ms={}, error={}",
                listing_path.display(),
                started_at.elapsed().as_millis(),
                error
            );
            CoreVfsResponse::Failed {
                request_id,
                error: CoreVfsError {
                    kind: CoreVfsErrorKind::HostUnavailable,
                    message: Some(error.to_string()),
                },
            }
        }
    };
    let load_failed = matches!(response, CoreVfsResponse::Failed { .. });
    if let Err(error) = outcome.core_bridge.submit_vfs_response(response) {
        log::debug!(
            "[main][dired] failed to submit directory VFS load response: {:?}",
            error
        );
    }
    Some(load_failed)
}

fn path_from_core_vfs_document_id(document_id: &str) -> Option<std::path::PathBuf> {
    document_id
        .strip_prefix("file://")
        .map(std::path::PathBuf::from)
        .or_else(|| Some(std::path::PathBuf::from(document_id)).filter(|path| path.exists()))
}

fn paths_refer_to_same_location_main(left: &std::path::Path, right: &std::path::Path) -> bool {
    left == right
        || std::fs::canonicalize(left)
            .ok()
            .zip(std::fs::canonicalize(right).ok())
            .is_some_and(|(left, right)| left == right)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SaveSnapshotOutcome {
    transient_message: Option<String>,
    wrote: bool,
    pending_directory_confirmation: bool,
}

fn save_snapshot_result(
    buffer_contents: &str,
    session_state: &mut saya::app::session::EditorSessionState,
) -> SaveSnapshotOutcome {
    save_snapshot_result_with_path_override(buffer_contents, session_state, None)
}

fn save_snapshot_result_with_path_override(
    buffer_contents: &str,
    session_state: &mut saya::app::session::EditorSessionState,
    path_override: Option<&str>,
) -> SaveSnapshotOutcome {
    save_snapshot_result_with_confirmation(
        buffer_contents,
        session_state,
        path_override,
        false,
        None,
    )
}

fn save_snapshot_result_with_confirmation(
    buffer_contents: &str,
    session_state: &mut saya::app::session::EditorSessionState,
    path_override: Option<&str>,
    confirmed: bool,
    core_revision: Option<u64>,
) -> SaveSnapshotOutcome {
    let path_override = effective_host_write_path_override(session_state, path_override);
    if path_override.is_none() && session_state.directory_buffer().is_some() {
        if confirmed {
            return match session_state.confirm_directory_buffer_operation_preview(buffer_contents) {
                Ok(plan) => match apply_directory_buffer_operation_plan(session_state, &plan) {
                    Ok(applied_count) => {
                        session_state.clear_pending_directory_operation_preview();
                        session_state.record_save_success_at_revision(core_revision);
                        log::info!(
                            "[main][dired][writable] applied confirmed directory operation plan: root_path={}, operations={}",
                            plan.root_path.display(),
                            applied_count
                        );
                        SaveSnapshotOutcome {
                            transient_message: Some(format!(
                                "Directory operations applied: {applied_count} operation(s)"
                            )),
                            wrote: true,
                            pending_directory_confirmation: false,
                        }
                    }
                    Err(error) => {
                        log::debug!(
                            "[main][dired][writable] confirmed directory operation plan failed during apply: root_path={}, error={:?}",
                            plan.root_path.display(),
                            error
                        );
                        session_state.record_save_failure(format!("{error:?}"));
                        SaveSnapshotOutcome {
                            transient_message: Some(format!(
                                "Directory operation apply failed: {error:?}. Recovery: directory metadata was refreshed from the filesystem; inspect the listing before retrying."
                            )),
                            wrote: false,
                            pending_directory_confirmation: false,
                        }
                    }
                },
                Err(DirectoryBufferPreviewConfirmationError::MissingPreview) => {
                    SaveSnapshotOutcome {
                        transient_message: Some(
                            "Directory operation preview is required before :write!".to_string(),
                        ),
                        wrote: false,
                        pending_directory_confirmation: false,
                    }
                }
                Err(DirectoryBufferPreviewConfirmationError::StalePreview { .. }) => {
                    SaveSnapshotOutcome {
                        transient_message: Some(
                            "Directory operation preview is stale; run :write again before :write!"
                                .to_string(),
                        ),
                        wrote: false,
                        pending_directory_confirmation: false,
                    }
                }
                Err(DirectoryBufferPreviewConfirmationError::Validation(errors)) => {
                    log::debug!(
                        "[main][dired][writable] confirmed directory operation plan validation failed before apply: errors={:?}",
                        errors
                    );
                    SaveSnapshotOutcome {
                        transient_message: Some(format!(
                            "Directory operation plan failed validation: {} error(s)",
                            errors.len()
                        )),
                        wrote: false,
                        pending_directory_confirmation: false,
                    }
                }
            };
        }
        return match session_state.prepare_directory_buffer_operation_preview(buffer_contents) {
            Ok(preview) => {
                let prompt = session_state.directory_buffer_operation_prompt();
                log::info!(
                    "[main][dired][writable] prepared directory operation preview instead of regular save: root_path={}, preview_id={}, operations={}, high_risk={}",
                    preview.root_path.display(),
                    preview.id,
                    preview.operation_count,
                    preview.high_risk_count
                );
                SaveSnapshotOutcome {
                    transient_message: Some(prompt.map_or_else(
                        || {
                            format!(
                                "Directory operation preview prepared: {} operation(s), high_risk={}, preview_id={}",
                                preview.operation_count, preview.high_risk_count, preview.id
                            )
                        },
                        |prompt| prompt.status_line,
                    )),
                    wrote: false,
                    pending_directory_confirmation: true,
                }
            }
            Err(errors) => {
                log::debug!(
                    "[main][dired][writable] directory operation plan validation failed before save: errors={:?}",
                    errors
                );
                SaveSnapshotOutcome {
                    transient_message: Some(format!(
                        "Directory operation plan failed validation: {} error(s)",
                        errors.len()
                    )),
                    wrote: false,
                    pending_directory_confirmation: false,
                }
            }
        };
    }

    match build_save_request_for_host_write(buffer_contents, session_state, path_override) {
        Ok(req) => match write_to_path(&req) {
            SaveResult::Saved => {
                session_state.record_save_success_at_revision(core_revision);
                SaveSnapshotOutcome {
                    transient_message: Some("Saved successfully".to_string()),
                    wrote: true,
                    pending_directory_confirmation: false,
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
                    pending_directory_confirmation: false,
                }
            }
        },
        Err(error) => SaveSnapshotOutcome {
            transient_message: Some(save_error_message(&error)),
            wrote: false,
            pending_directory_confirmation: false,
        },
    }
}

fn apply_directory_buffer_operation_plan(
    session_state: &mut saya::app::session::EditorSessionState,
    plan: &saya::app::session::DirectoryBufferOperationPlan,
) -> Result<usize, RuntimeFilerError> {
    log::info!(
        "[main][dired][writable] applying confirmed directory operation plan through filer operations: root_path={}, operations={}",
        plan.root_path.display(),
        plan.operations.len()
    );
    let transaction = build_directory_buffer_operation_transaction(plan)?;
    match execute_directory_buffer_operation_transaction(&transaction) {
        Ok(_report) => {
            for (from, to) in &transaction.rename_marks {
                session_state.record_directory_entry_rename(from, to);
            }
            if let Err(error) =
                session_state.refresh_directory_buffer_for_target_path(&plan.root_path)
            {
                log::debug!(
                    "[main][dired][writable] failed to refresh directory buffer after confirmed operation plan: root_path={}, error={}",
                    plan.root_path.display(),
                    error
                );
            }
            session_state.replace_target_path(plan.root_path.clone());
            Ok(plan.operations.len())
        }
        Err(error) => {
            if let Err(refresh_error) =
                session_state.refresh_directory_buffer_for_target_path(&plan.root_path)
            {
                log::debug!(
                    "[main][dired][writable] failed to refresh directory buffer after failed operation plan: root_path={}, error={}",
                    plan.root_path.display(),
                    refresh_error
                );
            }
            session_state.replace_target_path(plan.root_path.clone());
            Err(error)
        }
    }
}

#[derive(Debug, Clone)]
struct DirectoryBufferOperationTransaction {
    steps: Vec<DirectoryBufferOperationTransactionStep>,
    rename_marks: Vec<(std::path::PathBuf, std::path::PathBuf)>,
}

#[derive(Debug, Clone)]
struct DirectoryBufferOperationTransactionStep {
    operation: RuntimeFilerOperation,
    rollback: Option<RuntimeFilerOperation>,
    rollback_manual_recovery_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DirectoryBufferOperationTransactionReport {
    successful: usize,
    failed: usize,
    rollback_succeeded: usize,
    rollback_failed: usize,
    manual_recovery_required: usize,
}

fn build_directory_buffer_operation_transaction(
    plan: &saya::app::session::DirectoryBufferOperationPlan,
) -> Result<DirectoryBufferOperationTransaction, RuntimeFilerError> {
    validate_directory_buffer_operation_conflicts(plan)?;
    let mut steps = Vec::new();
    let mut rename_second_phase = Vec::new();
    let mut rename_marks = Vec::new();

    for (index, operation) in plan
        .operations
        .iter()
        .filter_map(|operation| match operation {
            DirectoryBufferPlannedOperation::Rename { from, to, .. } => Some((from, to)),
            _ => None,
        })
        .enumerate()
    {
        let temp_path = unique_directory_transaction_temp_path(&plan.root_path, index);
        steps.push(DirectoryBufferOperationTransactionStep {
            operation: RuntimeFilerOperation::Rename {
                from: operation.0.clone(),
                to: temp_path.clone(),
            },
            rollback: Some(RuntimeFilerOperation::Rename {
                from: temp_path.clone(),
                to: operation.0.clone(),
            }),
            rollback_manual_recovery_required: false,
        });
        rename_second_phase.push(DirectoryBufferOperationTransactionStep {
            operation: RuntimeFilerOperation::Rename {
                from: temp_path,
                to: operation.1.clone(),
            },
            rollback: Some(RuntimeFilerOperation::Rename {
                from: operation.1.clone(),
                to: operation.0.clone(),
            }),
            rollback_manual_recovery_required: false,
        });
        rename_marks.push((operation.0.clone(), operation.1.clone()));
    }
    steps.extend(rename_second_phase);

    for operation in plan.operations.iter().filter(|operation| {
        matches!(
            operation,
            DirectoryBufferPlannedOperation::CreateDirectory { .. }
        )
    }) {
        if let DirectoryBufferPlannedOperation::CreateDirectory { path, .. } = operation {
            steps.push(DirectoryBufferOperationTransactionStep {
                operation: RuntimeFilerOperation::CreateDirectory { path: path.clone() },
                rollback: None,
                rollback_manual_recovery_required: true,
            });
        }
    }

    for operation in plan.operations.iter().filter(|operation| {
        matches!(
            operation,
            DirectoryBufferPlannedOperation::CreateFile { .. }
        )
    }) {
        if let DirectoryBufferPlannedOperation::CreateFile { path, .. } = operation {
            steps.push(DirectoryBufferOperationTransactionStep {
                operation: RuntimeFilerOperation::CreateFile { path: path.clone() },
                rollback: None,
                rollback_manual_recovery_required: true,
            });
        }
    }

    for operation in plan
        .operations
        .iter()
        .filter(|operation| matches!(operation, DirectoryBufferPlannedOperation::Delete { .. }))
    {
        if let DirectoryBufferPlannedOperation::Delete { path, .. } = operation {
            steps.push(DirectoryBufferOperationTransactionStep {
                operation: RuntimeFilerOperation::Delete {
                    path: path.clone(),
                    confirm: true,
                    recursive: false,
                    trash: false,
                },
                rollback: None,
                rollback_manual_recovery_required: true,
            });
        }
    }

    log::debug!(
        "[main][dired][transaction] built operation transaction: root_path={}, planned_operations={}, executable_steps={}, rename_count={}",
        plan.root_path.display(),
        plan.operations.len(),
        steps.len(),
        rename_marks.len()
    );
    Ok(DirectoryBufferOperationTransaction {
        steps,
        rename_marks,
    })
}

fn validate_directory_buffer_operation_conflicts(
    plan: &saya::app::session::DirectoryBufferOperationPlan,
) -> Result<(), RuntimeFilerError> {
    let rename_sources = plan
        .operations
        .iter()
        .filter_map(|operation| match operation {
            DirectoryBufferPlannedOperation::Rename { from, .. } => Some(from.clone()),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    for operation in &plan.operations {
        match operation {
            DirectoryBufferPlannedOperation::CreateFile { path, .. } => {
                validate_directory_transaction_parent_permission(
                    RuntimeFilerOperationKind::CreateFile,
                    path,
                    None,
                )?;
                if path.exists() && !rename_sources.contains(path) {
                    return Err(directory_transaction_conflict_error(
                        RuntimeFilerOperationKind::CreateFile,
                        path,
                        None,
                        RuntimeFilerErrorKind::AlreadyExists,
                        "operation transaction conflict check failed: target path already exists",
                    ));
                }
            }
            DirectoryBufferPlannedOperation::CreateDirectory { path, .. } => {
                validate_directory_transaction_parent_permission(
                    RuntimeFilerOperationKind::CreateDirectory,
                    path,
                    None,
                )?;
                if path.exists() && !rename_sources.contains(path) {
                    return Err(directory_transaction_conflict_error(
                        RuntimeFilerOperationKind::CreateDirectory,
                        path,
                        None,
                        RuntimeFilerErrorKind::AlreadyExists,
                        "operation transaction conflict check failed: target path already exists",
                    ));
                }
            }
            DirectoryBufferPlannedOperation::Rename { from, to, kind, .. } => {
                validate_directory_transaction_entry_kind(
                    RuntimeFilerOperationKind::Rename,
                    from,
                    *kind,
                )?;
                validate_directory_transaction_parent_permission(
                    RuntimeFilerOperationKind::Rename,
                    from,
                    Some(to),
                )?;
                if !from.exists() {
                    return Err(directory_transaction_conflict_error(
                        RuntimeFilerOperationKind::Rename,
                        from,
                        Some(to),
                        RuntimeFilerErrorKind::NotFound,
                        "operation transaction conflict check failed: source path is missing",
                    ));
                }
                if to.exists() && !rename_sources.contains(to) {
                    return Err(directory_transaction_conflict_error(
                        RuntimeFilerOperationKind::Rename,
                        from,
                        Some(to),
                        RuntimeFilerErrorKind::AlreadyExists,
                        "operation transaction conflict check failed: target path already exists",
                    ));
                }
            }
            DirectoryBufferPlannedOperation::Delete { path, kind, .. } => {
                validate_directory_transaction_entry_kind(
                    RuntimeFilerOperationKind::Delete,
                    path,
                    *kind,
                )?;
                validate_directory_transaction_parent_permission(
                    RuntimeFilerOperationKind::Delete,
                    path,
                    None,
                )?;
                if !path.exists() {
                    return Err(directory_transaction_conflict_error(
                        RuntimeFilerOperationKind::Delete,
                        path,
                        None,
                        RuntimeFilerErrorKind::NotFound,
                        "operation transaction conflict check failed: source path is missing",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_directory_transaction_parent_permission(
    operation: RuntimeFilerOperationKind,
    path: &std::path::Path,
    target_path: Option<&std::path::PathBuf>,
) -> Result<(), RuntimeFilerError> {
    let mut parents = Vec::new();
    if let Some(parent) = path.parent() {
        parents.push(parent.to_path_buf());
    }
    if let Some(target_parent) = target_path.and_then(|target_path| target_path.parent()) {
        parents.push(target_parent.to_path_buf());
    }
    for parent in parents {
        if !directory_transaction_path_has_write_permission(&parent) {
            return Err(directory_transaction_conflict_error(
                operation,
                path,
                target_path,
                RuntimeFilerErrorKind::PermissionDenied,
                "operation transaction conflict check failed: parent directory is not writable",
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn directory_transaction_path_has_write_permission(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata()
        .map(|metadata| metadata.permissions().mode() & 0o222 != 0)
        .unwrap_or(true)
}

#[cfg(not(unix))]
fn directory_transaction_path_has_write_permission(path: &std::path::Path) -> bool {
    path.metadata()
        .map(|metadata| !metadata.permissions().readonly())
        .unwrap_or(true)
}

fn validate_directory_transaction_entry_kind(
    operation: RuntimeFilerOperationKind,
    path: &std::path::Path,
    kind: saya::app::session::DirectoryBufferEntryKind,
) -> Result<(), RuntimeFilerError> {
    if kind == saya::app::session::DirectoryBufferEntryKind::Other {
        return Err(directory_transaction_conflict_error(
            operation,
            path,
            None,
            RuntimeFilerErrorKind::Unsupported,
            "operation transaction conflict check failed: special file entries are unsupported",
        ));
    }
    Ok(())
}

fn execute_directory_buffer_operation_transaction(
    transaction: &DirectoryBufferOperationTransaction,
) -> Result<DirectoryBufferOperationTransactionReport, RuntimeFilerError> {
    let mut report = DirectoryBufferOperationTransactionReport {
        successful: 0,
        failed: 0,
        rollback_succeeded: 0,
        rollback_failed: 0,
        manual_recovery_required: 0,
    };
    let mut applied_steps = Vec::new();
    for (index, step) in transaction.steps.iter().enumerate() {
        log::info!(
            "[main][dired][transaction] executing transaction step: index={}, operation={:?}",
            index,
            step.operation
        );
        match saya::runtime::live::execute_local_filer_operation(step.operation.clone()) {
            Ok(_) => {
                report.successful += 1;
                applied_steps.push(step);
            }
            Err(error) => {
                report.failed += 1;
                log::debug!(
                    "[main][dired][transaction] transaction step failed: index={}, error={:?}",
                    index,
                    error
                );
                for applied_step in applied_steps.into_iter().rev() {
                    if let Some(rollback) = &applied_step.rollback {
                        match saya::runtime::live::execute_local_filer_operation(rollback.clone()) {
                            Ok(_) => report.rollback_succeeded += 1,
                            Err(rollback_error) => {
                                report.rollback_failed += 1;
                                report.manual_recovery_required += 1;
                                log::debug!(
                                    "[main][dired][transaction] transaction rollback failed: rollback={:?}, error={:?}",
                                    rollback,
                                    rollback_error
                                );
                            }
                        }
                    } else if applied_step.rollback_manual_recovery_required {
                        report.manual_recovery_required += 1;
                    }
                }
                return Err(directory_transaction_report_error(report, error));
            }
        }
    }
    log::info!(
        "[main][dired][transaction] transaction completed: successful={}, failed={}, rollback_succeeded={}, rollback_failed={}, manual_recovery_required={}",
        report.successful,
        report.failed,
        report.rollback_succeeded,
        report.rollback_failed,
        report.manual_recovery_required
    );
    Ok(report)
}

fn directory_transaction_report_error(
    report: DirectoryBufferOperationTransactionReport,
    error: RuntimeFilerError,
) -> RuntimeFilerError {
    let (operation, path, target_path, kind, cause) = match error {
        RuntimeFilerError::OperationFailed {
            operation,
            path,
            target_path,
            kind,
            message,
        } => (operation, path, target_path, kind, message),
        RuntimeFilerError::ReadFailed { path, message } => (
            RuntimeFilerOperationKind::BulkDelete,
            path,
            None,
            RuntimeFilerErrorKind::Io,
            message,
        ),
    };
    let message = format!(
        "directory operation transaction failed: successful={}, failed={}, rollback_succeeded={}, rollback_failed={}, manual_recovery_required={}, cause={}",
        report.successful,
        report.failed,
        report.rollback_succeeded,
        report.rollback_failed,
        report.manual_recovery_required,
        cause
    );
    log::debug!(
        "[main][dired][transaction] transaction failed report: operation={:?}, path={}, target_path={:?}, kind={:?}, {}",
        operation,
        path.display(),
        target_path.as_ref().map(|path| path.display().to_string()),
        kind,
        message
    );
    RuntimeFilerError::OperationFailed {
        operation,
        path,
        target_path,
        kind,
        message,
    }
}

fn directory_transaction_conflict_error(
    operation: RuntimeFilerOperationKind,
    path: &std::path::Path,
    target_path: Option<&std::path::PathBuf>,
    kind: RuntimeFilerErrorKind,
    message: &str,
) -> RuntimeFilerError {
    log::debug!(
        "[main][dired][transaction] conflict check failed: operation={:?}, path={}, target_path={:?}, kind={:?}, message={}",
        operation,
        path.display(),
        target_path.map(|path| path.display().to_string()),
        kind,
        message
    );
    RuntimeFilerError::OperationFailed {
        operation,
        path: path.to_path_buf(),
        target_path: target_path.cloned(),
        kind,
        message: message.to_string(),
    }
}

fn unique_directory_transaction_temp_path(
    root_path: &std::path::Path,
    index: usize,
) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    for attempt in 0..1000 {
        let candidate = root_path.join(format!(".saya-dired-txn-{nanos}-{index}-{attempt}.tmp"));
        if !candidate.exists() {
            return candidate;
        }
    }
    root_path.join(format!(".saya-dired-txn-{nanos}-{index}-fallback.tmp"))
}

fn build_save_request_for_host_write(
    buffer_contents: &str,
    session_state: &saya::app::session::EditorSessionState,
    path_override: Option<&str>,
) -> Result<SaveRequest, SaveRequestError> {
    let Some(path_override) = effective_host_write_path_override(session_state, path_override)
    else {
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

fn effective_host_write_path_override<'a>(
    session_state: &saya::app::session::EditorSessionState,
    path_override: Option<&'a str>,
) -> Option<&'a str> {
    let path_override = path_override.filter(|path| !path.is_empty())?;
    let Some(target_path) = session_state.target_path() else {
        return Some(path_override);
    };
    let override_path = path_override
        .strip_prefix("file://")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(path_override));
    let same_target = override_path == target_path.as_path()
        || override_path
            .canonicalize()
            .ok()
            .zip(target_path.canonicalize().ok())
            .is_some_and(|(left, right)| left == right);
    if same_target {
        log::debug!(
            "[main] treating host write path as current target instead of explicit override: path={}",
            target_path.display()
        );
        None
    } else {
        Some(path_override)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MainHostCommand {
    Save,
    SaveThenQuit,
    CancelDirectoryPreview,
    Edit(std::path::PathBuf),
    BufferWindowFloat(String),
    TerminalFloat(String),
    TerminalCloseFloat(String),
    MarkdownPreviewMermaid,
    LspHoverFloat(String),
    LspDiagnosticFloat(String),
    LspLocationListFloat(String),
    LspSymbolOutlineFloat(String),
    LspGotoDefinition(String),
    LspWorkspaceEditPreview(String),
    LspCodeActionsFloat(String),
    LspPublishDiagnostics(String),
    LspNextDiagnostic(Option<String>),
    LspPreviousDiagnostic(Option<String>),
    LspStatus(String),
}

fn parse_main_host_command(command: &str) -> Option<MainHostCommand> {
    if let Some(command) = parse_buffer_float_host_command(command) {
        return Some(command);
    }
    if let Some(command) = parse_terminal_float_host_command(command) {
        return Some(command);
    }
    if let Some(command) = parse_lsp_float_host_command(command) {
        return Some(command);
    }
    let normalized = normalize_main_host_command(command)?;
    match normalized.as_str() {
        "w" | "write" => Some(MainHostCommand::Save),
        "wq" | "x" | "xit" | "exit" => Some(MainHostCommand::SaveThenQuit),
        "dired-cancel" | "diredcancel" => Some(MainHostCommand::CancelDirectoryPreview),
        "markdown.previewMermaid" | "markdown.previewmermaid" => {
            Some(MainHostCommand::MarkdownPreviewMermaid)
        }
        _ => parse_runtime_edit_command(&normalized).map(MainHostCommand::Edit),
    }
}

fn parse_buffer_float_host_command(command: &str) -> Option<MainHostCommand> {
    let trimmed = command.trim();
    let trimmed = trimmed.strip_prefix(':').unwrap_or(trimmed).trim();
    let payload = trimmed
        .strip_prefix("buffer.floatWindow ")
        .or_else(|| trimmed.strip_prefix("buffer.windowFloat "))
        .map(str::trim);
    payload
        .filter(|payload| !payload.is_empty())
        .map(|payload| MainHostCommand::BufferWindowFloat(payload.to_string()))
}

fn parse_terminal_float_host_command(command: &str) -> Option<MainHostCommand> {
    let trimmed = command.trim();
    let trimmed = trimmed.strip_prefix(':').unwrap_or(trimmed).trim();
    let payload = trimmed
        .strip_prefix("terminal.float ")
        .or_else(|| trimmed.strip_prefix("terminal.openFloat "))
        .map(str::trim);
    if let Some(payload) = payload.filter(|payload| !payload.is_empty()) {
        return Some(MainHostCommand::TerminalFloat(payload.to_string()));
    }
    let payload = trimmed
        .strip_prefix("terminal.closeFloat ")
        .or_else(|| trimmed.strip_prefix("terminal.detachFloat "))
        .map(str::trim);
    payload
        .filter(|payload| !payload.is_empty())
        .map(|payload| MainHostCommand::TerminalCloseFloat(payload.to_string()))
}

fn parse_lsp_float_host_command(command: &str) -> Option<MainHostCommand> {
    let trimmed = command.trim();
    let trimmed = trimmed.strip_prefix(':').unwrap_or(trimmed).trim();
    let payload = trimmed
        .strip_prefix("lsp.floatHover ")
        .or_else(|| trimmed.strip_prefix("lsp.hoverFloat "))
        .map(str::trim);
    if let Some(payload) = payload.filter(|payload| !payload.is_empty()) {
        return Some(MainHostCommand::LspHoverFloat(payload.to_string()));
    }
    let payload = trimmed
        .strip_prefix("lsp.floatDiagnostics ")
        .or_else(|| trimmed.strip_prefix("lsp.diagnosticFloat "))
        .map(str::trim);
    if let Some(payload) = payload.filter(|payload| !payload.is_empty()) {
        return Some(MainHostCommand::LspDiagnosticFloat(payload.to_string()));
    }
    let payload = trimmed
        .strip_prefix("lsp.floatLocations ")
        .or_else(|| trimmed.strip_prefix("lsp.locationsFloat "))
        .map(str::trim);
    if let Some(payload) = payload.filter(|payload| !payload.is_empty()) {
        return Some(MainHostCommand::LspLocationListFloat(payload.to_string()));
    }
    let payload = trimmed
        .strip_prefix("lsp.floatSymbols ")
        .or_else(|| trimmed.strip_prefix("lsp.symbolsFloat "))
        .map(str::trim);
    if let Some(payload) = payload.filter(|payload| !payload.is_empty()) {
        return Some(MainHostCommand::LspSymbolOutlineFloat(payload.to_string()));
    }
    let payload = trimmed
        .strip_prefix("lsp.gotoDefinition ")
        .or_else(|| trimmed.strip_prefix("lsp.definitionGoto "))
        .map(str::trim);
    if let Some(payload) = payload.filter(|payload| !payload.is_empty()) {
        return Some(MainHostCommand::LspGotoDefinition(payload.to_string()));
    }
    let payload = trimmed
        .strip_prefix("lsp.previewWorkspaceEdit ")
        .map(str::trim);
    if let Some(payload) = payload.filter(|payload| !payload.is_empty()) {
        return Some(MainHostCommand::LspWorkspaceEditPreview(
            payload.to_string(),
        ));
    }
    let payload = trimmed.strip_prefix("lsp.floatCodeActions ").map(str::trim);
    if let Some(payload) = payload.filter(|payload| !payload.is_empty()) {
        return Some(MainHostCommand::LspCodeActionsFloat(payload.to_string()));
    }
    let payload = trimmed
        .strip_prefix("lsp.publishDiagnostics ")
        .map(str::trim);
    if let Some(payload) = payload.filter(|payload| !payload.is_empty()) {
        return Some(MainHostCommand::LspPublishDiagnostics(payload.to_string()));
    }
    let payload = trimmed.strip_prefix("lsp.status ").map(str::trim);
    if let Some(payload) = payload.filter(|payload| !payload.is_empty()) {
        return Some(MainHostCommand::LspStatus(payload.to_string()));
    }
    if trimmed == "lsp.nextDiagnostic" {
        return Some(MainHostCommand::LspNextDiagnostic(None));
    }
    if trimmed == "lsp.previousDiagnostic" || trimmed == "lsp.prevDiagnostic" {
        return Some(MainHostCommand::LspPreviousDiagnostic(None));
    }
    let payload = trimmed.strip_prefix("lsp.nextDiagnostic ").map(str::trim);
    if let Some(payload) = payload.filter(|payload| !payload.is_empty()) {
        return Some(MainHostCommand::LspNextDiagnostic(Some(
            payload.to_string(),
        )));
    }
    let payload = trimmed
        .strip_prefix("lsp.previousDiagnostic ")
        .or_else(|| trimmed.strip_prefix("lsp.prevDiagnostic "))
        .map(str::trim);
    if let Some(payload) = payload.filter(|payload| !payload.is_empty()) {
        return Some(MainHostCommand::LspPreviousDiagnostic(Some(
            payload.to_string(),
        )));
    }
    None
}

fn parse_runtime_edit_command(normalized: &str) -> Option<std::path::PathBuf> {
    let path = normalized
        .strip_prefix("edit ")
        .or_else(|| normalized.strip_prefix("e "))?
        .trim();
    if path.is_empty() {
        return None;
    }
    Some(std::path::PathBuf::from(path))
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

fn startup_registered_command_name_for_ex_command(
    command: &str,
    registry: &saya::runtime::callback_registry_seed::CallbackRegistrySeed,
) -> Option<String> {
    let normalized = normalize_main_host_command(command)?;
    registry
        .commands()
        .iter()
        .find(|registered| registered.name() == normalized)
        .map(|registered| registered.name().to_string())
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

fn defer_directory_save_then_quit_if_confirmation_pending(
    session_state: &mut saya::app::session::EditorSessionState,
    force: bool,
    pending_directory_confirmation: bool,
) -> bool {
    if pending_directory_confirmation
        && session_state.directory_operation_confirmation_dialog_active()
    {
        session_state.defer_directory_save_then_quit(force);
        true
    } else {
        false
    }
}

fn take_pending_directory_save_then_quit_shutdown(
    session_state: &mut saya::app::session::EditorSessionState,
) -> Option<ShutdownReason> {
    match session_state.take_pending_directory_save_then_quit_decision()? {
        QuitDecision::Allow => Some(ShutdownReason::UserQuit),
        QuitDecision::ForceQuit => Some(ShutdownReason::UserForceQuit),
        QuitDecision::WarnUnsaved => {
            log::debug!(
                "[main][dired][writable] deferred save-then-quit still rejected after confirmation"
            );
            None
        }
    }
}

fn execute_runtime_host_command_through_core(
    ex_command: &str,
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    session_state: &mut saya::app::session::EditorSessionState,
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
    let mut outcome_state = ApplicationOutcomeState::default();
    loop {
        let folded = fold_normalized_outcomes(
            outcome.core_bridge.take_normalized_outcomes(),
            outcome_state,
        );
        outcome_state = folded.state;

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

        let current_revision = outcome.core_bridge.revision();
        let directives = prioritize_save_family_host_directives(
            folded.effects.host_directives,
            current_revision,
        );
        if directives.is_empty() {
            break;
        }

        let mut last_write_pending_directory_confirmation = false;
        for directive in directives {
            match directive {
                NormalizedHostDirective::Write { path, force, .. } => {
                    let snapshot = outcome.core_bridge.snapshot();
                    let save_outcome = save_snapshot_result_with_confirmation(
                        &snapshot.text,
                        session_state,
                        Some(path.as_str()),
                        force,
                        Some(current_revision),
                    );
                    effect.transient_message = save_outcome.transient_message;
                    if save_outcome.wrote {
                        refresh_directory_buffer_after_confirmed_save(
                            outcome,
                            session_state,
                            &mut effect.transient_message,
                        );
                        let mut host_session = MainRuntimeHostSession::new(outcome, session_state);
                        effect
                            .follow_up_events
                            .push(RuntimeEventMapper::buffer_write_post(
                                host_session.current_buffer_snapshot(),
                            ));
                    }
                    last_write_pending_directory_confirmation =
                        save_outcome.pending_directory_confirmation;
                }
                NormalizedHostDirective::Quit { force, .. } => {
                    if defer_directory_save_then_quit_if_confirmation_pending(
                        session_state,
                        force,
                        last_write_pending_directory_confirmation,
                    ) {
                        last_write_pending_directory_confirmation = false;
                        continue;
                    }
                    last_write_pending_directory_confirmation = false;
                    let decision = session_state.evaluate_quit(force);
                    merge_runtime_shutdown_intent(
                        &mut effect.shutdown_intent,
                        runtime_shutdown_intent_from_quit_decision(force, decision),
                    );
                }
                NormalizedHostDirective::Suspend { trace } => {
                    log::debug!(
                        "[main] runtime host command observed suspend directive but cannot suspend outside interactive terminal loop: sequence={}",
                        trace.sequence
                    );
                }
                NormalizedHostDirective::VfsRequest { request, trace } => {
                    log::debug!(
                        "[main] runtime host command processing normalized VFS directive: sequence={}, request={:?}",
                        trace.sequence,
                        request
                    );
                    let mut ignored_system_warning = None;
                    if let Some(save_outcome) = handle_directory_buffer_vfs_save_request(
                        outcome,
                        session_state,
                        request.clone(),
                        &mut effect.transient_message,
                        &mut ignored_system_warning,
                    ) {
                        if save_outcome.wrote {
                            let mut host_session =
                                MainRuntimeHostSession::new(outcome, session_state);
                            effect
                                .follow_up_events
                                .push(RuntimeEventMapper::buffer_write_post(
                                    host_session.current_buffer_snapshot(),
                                ));
                        }
                        continue;
                    }
                    if let Some(load_failed) = handle_directory_buffer_vfs_load_request(
                        outcome,
                        session_state,
                        request.clone(),
                    ) {
                        effect.vfs_load_failed |= load_failed;
                        continue;
                    }
                    match host_action_runtime.handle_vfs_request(&mut outcome.core_bridge, request)
                    {
                        Ok(vfs_effect) => {
                            effect.vfs_load_failed |= vfs_effect.load_failed;
                        }
                        Err(error) => {
                            log::debug!(
                                "[main] runtime host command VFS directive failed: {:?}",
                                error
                            );
                        }
                    }
                }
                NormalizedHostDirective::JobStart { request, trace } => {
                    log::debug!(
                        "[main] runtime host command processing job start directive: sequence={}, job_id={}, argv={:?}",
                        trace.sequence,
                        request.job_id,
                        request.argv
                    );
                    if let Err(error) =
                        host_action_runtime.start_job(&mut outcome.core_bridge, request)
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
                    if let Err(error) =
                        host_action_runtime.stop_job(&mut outcome.core_bridge, job_id)
                    {
                        log::debug!("[main] runtime host command job stop failed: {:?}", error);
                    }
                }
            }
        }
    }

    Ok(effect)
}

fn runtime_edit_ex_command(
    path: &std::path::Path,
    outcome: &saya::app::bootstrap::BootstrapOutcome,
) -> String {
    let snapshot = outcome.core_bridge.light_snapshot();
    let Some(active_window) = snapshot.active_window() else {
        return format!(":edit {}", escape_runtime_edit_path(path));
    };
    let active_buffer_id = active_window.buf_id;
    let visible_count = snapshot
        .windows
        .iter()
        .filter(|window| window.buf_id == active_buffer_id)
        .count();
    let command = if path.is_dir() && visible_count > 1 {
        "hide noswapfile edit"
    } else if path.is_dir() {
        "noswapfile edit"
    } else if visible_count > 1 {
        "hide edit"
    } else {
        "edit"
    };
    format!(":{command} {}", escape_runtime_edit_path(path))
}

fn execute_runtime_host_command(
    command: &str,
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    session_state: &mut saya::app::session::EditorSessionState,
) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
    execute_runtime_host_command_with_floats(
        command,
        outcome,
        session_state,
        None,
        None,
        None,
        None,
    )
}

fn is_swap_attention_message(message: &str) -> bool {
    message.starts_with("E301: ") || message.starts_with("E325: ")
}

fn execute_runtime_host_command_with_floats(
    command: &str,
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    session_state: &mut saya::app::session::EditorSessionState,
    floating_window_manager: Option<&mut FloatingWindowManager>,
    _completion_float_manager: Option<&mut CompletionFloatManager>,
    lsp_diagnostic_store: Option<&mut LspDiagnosticStore>,
    terminal_float_manager: Option<&mut TerminalFloatManager>,
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
        Some(MainHostCommand::CancelDirectoryPreview) => {
            let mut effect = RuntimeCommandEffect::default();
            effect.transient_message =
                match session_state.cancel_directory_buffer_operation_preview() {
                    Some(preview) => Some(format!(
                        "Directory operation preview cancelled: {} operation(s), preview_id={}",
                        preview.operation_count, preview.id
                    )),
                    None => Some("No directory operation preview to cancel".to_string()),
                };
            Ok(effect)
        }
        Some(MainHostCommand::Edit(path)) => {
            let is_directory_edit = path.is_dir();
            let ex_command = runtime_edit_ex_command(&path, outcome);
            log::debug!(
                "[main] routing runtime edit command through core VFS coordinator: path={}, core_command={}",
                path.display(),
                ex_command
            );
            let mut effect =
                execute_runtime_host_command_through_core(&ex_command, outcome, session_state)?;
            if is_directory_edit
                && effect
                    .transient_message
                    .as_deref()
                    .is_some_and(is_swap_attention_message)
            {
                log::debug!(
                    "[main][dired] suppressing swap attention message during directory edit: path={}, message={:?}",
                    path.display(),
                    effect.transient_message
                );
                effect.transient_message = None;
            }
            if effect.vfs_load_failed && !is_directory_edit {
                log::debug!(
                    "[main] runtime edit command left host target unchanged because core VFS load failed: path={}",
                    path.display()
                );
                return Ok(effect);
            }
            if effect.vfs_load_failed {
                log::debug!(
                    "[main][dired] continuing directory edit after core VFS load failure because host metadata will project listing: path={}",
                    path.display()
                );
            }
            if is_directory_edit {
                session_state
                    .refresh_directory_buffer_for_target_path(&path)
                    .map_err(|error| RuntimeCommandError::CommandFailed {
                        name: command.to_string(),
                        message: format!("failed to refresh directory buffer: {error:?}"),
                    })?;
            } else {
                session_state.replace_target_path(path.clone());
            }
            if let Some(directory_buffer) =
                session_state.directory_buffer().filter(|directory_buffer| {
                    paths_refer_to_same_location_main(&directory_buffer.root_path, &path)
                })
            {
                outcome
                    .core_bridge
                    .replace_buffer_text(&directory_buffer.display_text)
                    .map_err(|error| RuntimeCommandError::CommandFailed {
                        name: command.to_string(),
                        message: format!("failed to project directory buffer listing: {error:?}"),
                    })?;
            }
            outcome.target_path = Some(path);
            Ok(effect)
        }
        Some(MainHostCommand::BufferWindowFloat(payload)) => {
            execute_buffer_window_float_host_command(&payload, outcome, floating_window_manager)
        }
        Some(MainHostCommand::TerminalFloat(payload)) => execute_terminal_float_host_command(
            &payload,
            floating_window_manager,
            terminal_float_manager,
        ),
        Some(MainHostCommand::TerminalCloseFloat(payload)) => {
            execute_terminal_close_float_host_command(
                &payload,
                floating_window_manager,
                terminal_float_manager,
            )
        }
        Some(MainHostCommand::MarkdownPreviewMermaid) => {
            session_state.request_mermaid_preview();
            log::debug!(
                "[main][markdown_preview] manual Mermaid preview requested by host command"
            );
            Ok(RuntimeCommandEffect {
                transient_message: Some("Mermaid preview requested".to_string()),
                ..RuntimeCommandEffect::default()
            })
        }
        Some(MainHostCommand::LspHoverFloat(payload)) => execute_lsp_hover_float_host_command(
            &payload,
            outcome,
            floating_window_manager,
            lsp_diagnostic_store.as_deref(),
        ),
        Some(MainHostCommand::LspDiagnosticFloat(payload)) => {
            execute_lsp_diagnostic_float_host_command(&payload, outcome, floating_window_manager)
        }
        Some(MainHostCommand::LspLocationListFloat(payload)) => {
            execute_lsp_location_list_float_host_command(&payload, outcome, floating_window_manager)
        }
        Some(MainHostCommand::LspSymbolOutlineFloat(payload)) => {
            execute_lsp_symbol_outline_float_host_command(
                &payload,
                outcome,
                floating_window_manager,
            )
        }
        Some(MainHostCommand::LspGotoDefinition(payload)) => {
            execute_lsp_goto_definition_host_command(&payload, outcome, session_state)
        }
        Some(MainHostCommand::LspWorkspaceEditPreview(payload)) => {
            execute_lsp_workspace_edit_preview_host_command(
                &payload,
                outcome,
                floating_window_manager,
            )
        }
        Some(MainHostCommand::LspCodeActionsFloat(payload)) => {
            execute_lsp_code_actions_float_host_command(&payload, outcome, floating_window_manager)
        }
        Some(MainHostCommand::LspPublishDiagnostics(payload)) => {
            execute_lsp_publish_diagnostics_host_command(
                &payload,
                outcome,
                floating_window_manager,
                lsp_diagnostic_store,
            )
        }
        Some(MainHostCommand::LspNextDiagnostic(payload)) => {
            execute_lsp_cycle_diagnostic_host_command(
                outcome,
                floating_window_manager,
                lsp_diagnostic_store,
                true,
                payload.as_deref(),
            )
        }
        Some(MainHostCommand::LspPreviousDiagnostic(payload)) => {
            execute_lsp_cycle_diagnostic_host_command(
                outcome,
                floating_window_manager,
                lsp_diagnostic_store,
                false,
                payload.as_deref(),
            )
        }
        Some(MainHostCommand::LspStatus(payload)) => execute_lsp_status_host_command(&payload),
        None => Err(RuntimeCommandError::UnknownCommand {
            name: command.to_string(),
        }),
    }
}

fn execute_buffer_window_float_host_command(
    payload: &str,
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    floating_window_manager: Option<&mut FloatingWindowManager>,
) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
    let manager = floating_window_manager.ok_or_else(|| RuntimeCommandError::CommandFailed {
        name: "buffer.floatWindow".to_string(),
        message: "floating window manager is not available".to_string(),
    })?;
    let value: serde_json::Value =
        serde_json::from_str(payload).map_err(|error| RuntimeCommandError::CommandFailed {
            name: "buffer.floatWindow".to_string(),
            message: format!("invalid buffer float payload: {error}"),
        })?;
    let snapshot = outcome.core_bridge.light_snapshot();
    let requested_window_id = value
        .get("windowId")
        .or_else(|| value.get("window_id"))
        .and_then(serde_json::Value::as_i64)
        .map(|id| id as i32);
    let requested_buffer_id = value
        .get("bufferId")
        .or_else(|| value.get("buffer_id"))
        .and_then(serde_json::Value::as_i64)
        .map(|id| id as i32);
    let (window_id, buffer_id) = resolve_buffer_float_backing_window(
        "buffer.floatWindow",
        &snapshot,
        requested_window_id,
        requested_buffer_id,
    )?;
    let width = value
        .get("width")
        .and_then(serde_json::Value::as_u64)
        .map(|width| width as u16)
        .unwrap_or(60)
        .max(1);
    let height = value
        .get("height")
        .and_then(serde_json::Value::as_u64)
        .map(|height| height as u16)
        .unwrap_or(12)
        .max(1);
    let row = value
        .get("row")
        .and_then(serde_json::Value::as_i64)
        .map(|row| row as i16)
        .unwrap_or(1);
    let col = value
        .get("col")
        .or_else(|| value.get("column"))
        .and_then(serde_json::Value::as_i64)
        .map(|col| col as i16)
        .unwrap_or(2);
    let border = match value
        .get("border")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("single")
    {
        "none" | "borderless" => FloatingBorder::None,
        _ => FloatingBorder::Single,
    };
    let id = manager.open_core_window(
        window_id,
        FloatingPlacement {
            relative_to: FloatingRelativeTo::Editor,
            anchor: FloatingAnchor::NorthWest,
            row,
            col,
            fit: FloatingFit::TruncateToGrid,
        },
        FloatingSize { width, height },
        FloatingChrome { border },
        FloatingZIndex::User,
        true,
    );
    manager.focus_float(id);
    log::debug!(
        "[main][buffer_float] buffer float host command applied: float_id={}, window_id={}, buffer_id={}, size=({},{})",
        id.0,
        window_id,
        buffer_id,
        width,
        height
    );
    Ok(RuntimeCommandEffect::default())
}

fn resolve_buffer_float_backing_window(
    command_name: &str,
    snapshot: &CoreLightSnapshot,
    requested_window_id: Option<i32>,
    requested_buffer_id: Option<i32>,
) -> Result<(i32, i32), RuntimeCommandError> {
    let chosen_window_id = match (requested_window_id, requested_buffer_id) {
        (Some(window_id), _) => window_id,
        (None, Some(buffer_id)) => {
            let Some(window) = snapshot.windows.iter().find(|window| window.buf_id == buffer_id)
            else {
                log::debug!(
                    "[main][buffer_float] rejected unbacked buffer float because hidden core-window creation is unavailable: command={}, buffer_id={}",
                    command_name,
                    buffer_id
                );
                return Err(RuntimeCommandError::CommandFailed {
                    name: command_name.to_string(),
                    message: format!(
                        "hidden core-window creation is not available for buffer-backed floats: buffer_id={buffer_id}"
                    ),
                });
            };
            window.id
        }
        (None, None) => snapshot.active_window_id().ok_or_else(|| {
            log::debug!(
                "[main][buffer_float] rejected buffer float because active window is unavailable: command={}",
                command_name
            );
            RuntimeCommandError::CommandFailed {
                name: command_name.to_string(),
                message: "active window is not available".to_string(),
            }
        })?,
    };
    let Some(window) = snapshot.window(chosen_window_id) else {
        log::debug!(
            "[main][buffer_float] rejected buffer float because backing window is missing: command={}, window_id={}, requested_buffer_id={:?}",
            command_name,
            chosen_window_id,
            requested_buffer_id
        );
        return Err(RuntimeCommandError::CommandFailed {
            name: command_name.to_string(),
            message: format!("window not found: window_id={chosen_window_id}"),
        });
    };
    if let Some(buffer_id) = requested_buffer_id
        && window.buf_id != buffer_id
    {
        log::debug!(
            "[main][buffer_float] rejected buffer float because requested window does not display requested buffer: command={}, window_id={}, window_buffer_id={}, requested_buffer_id={}",
            command_name,
            chosen_window_id,
            window.buf_id,
            buffer_id
        );
        return Err(RuntimeCommandError::CommandFailed {
            name: command_name.to_string(),
            message: format!(
                "backing window {chosen_window_id} displays buffer {}, not requested buffer {buffer_id}; hidden core-window creation is not available",
                window.buf_id
            ),
        });
    }
    log::debug!(
        "[main][buffer_float] resolved existing backing window for buffer float: command={}, window_id={}, buffer_id={}, requested_window_id={:?}, requested_buffer_id={:?}",
        command_name,
        chosen_window_id,
        window.buf_id,
        requested_window_id,
        requested_buffer_id
    );
    Ok((chosen_window_id, window.buf_id))
}

fn execute_terminal_float_host_command(
    payload: &str,
    floating_window_manager: Option<&mut FloatingWindowManager>,
    terminal_float_manager: Option<&mut TerminalFloatManager>,
) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
    let floating_manager =
        floating_window_manager.ok_or_else(|| RuntimeCommandError::CommandFailed {
            name: "terminal.float".to_string(),
            message: "floating window manager is not available".to_string(),
        })?;
    let terminal_manager =
        terminal_float_manager.ok_or_else(|| RuntimeCommandError::CommandFailed {
            name: "terminal.float".to_string(),
            message: "terminal float manager is not available".to_string(),
        })?;
    let value: serde_json::Value =
        serde_json::from_str(payload).map_err(|error| RuntimeCommandError::CommandFailed {
            name: "terminal.float".to_string(),
            message: format!("invalid terminal float payload: {error}"),
        })?;
    let command = value
        .get("command")
        .or_else(|| value.get("cmd"))
        .and_then(serde_json::Value::as_str)
        .filter(|command| !command.trim().is_empty())
        .ok_or_else(|| RuntimeCommandError::CommandFailed {
            name: "terminal.float".to_string(),
            message: "terminal command is required".to_string(),
        })?
        .to_string();
    let args = value
        .get("args")
        .and_then(serde_json::Value::as_array)
        .map(|args| {
            args.iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let width = value
        .get("width")
        .and_then(serde_json::Value::as_u64)
        .map(|width| width as u16)
        .unwrap_or(80)
        .max(1);
    let height = value
        .get("height")
        .and_then(serde_json::Value::as_u64)
        .map(|height| height as u16)
        .unwrap_or(16)
        .max(1);
    let row = value
        .get("row")
        .and_then(serde_json::Value::as_i64)
        .map(|row| row as i16)
        .unwrap_or(1);
    let col = value
        .get("col")
        .or_else(|| value.get("column"))
        .and_then(serde_json::Value::as_i64)
        .map(|col| col as i16)
        .unwrap_or(2);
    let border = match value
        .get("border")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("single")
    {
        "none" | "borderless" => FloatingBorder::None,
        _ => FloatingBorder::Single,
    };
    let close_behavior = match value
        .get("closeBehavior")
        .or_else(|| value.get("close_behavior"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("kill")
    {
        "detach" | "detachOnClose" | "detach-on-close" => TerminalFloatCloseBehavior::DetachOnClose,
        _ => TerminalFloatCloseBehavior::KillOnClose,
    };
    let terminal_id = terminal_manager
        .spawn(TerminalFloatSpawnRequest {
            command: command.clone(),
            args: args.clone(),
            width,
            height,
            close_behavior,
        })
        .map_err(|error| RuntimeCommandError::CommandFailed {
            name: "terminal.float".to_string(),
            message: format!("failed to spawn terminal float: {error:?}"),
        })?;
    let float_id = floating_manager.open_terminal(
        terminal_id,
        FloatingPlacement {
            relative_to: FloatingRelativeTo::Editor,
            anchor: FloatingAnchor::NorthWest,
            row,
            col,
            fit: FloatingFit::TruncateToGrid,
        },
        FloatingSize {
            width: width.saturating_add(if matches!(border, FloatingBorder::Single) {
                2
            } else {
                0
            }),
            height: height.saturating_add(if matches!(border, FloatingBorder::Single) {
                2
            } else {
                0
            }),
        },
        FloatingChrome { border },
        FloatingZIndex::User,
        true,
    );
    floating_manager.focus_float(float_id);
    log::debug!(
        "[main][terminal_float] terminal float host command applied: float_id={}, terminal_id={}, command={}, args={:?}, size=({},{}), close_behavior={:?}",
        float_id.0,
        terminal_id,
        command,
        args,
        width,
        height,
        close_behavior
    );
    Ok(RuntimeCommandEffect::default())
}

fn execute_terminal_close_float_host_command(
    payload: &str,
    floating_window_manager: Option<&mut FloatingWindowManager>,
    terminal_float_manager: Option<&mut TerminalFloatManager>,
) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
    let floating_manager =
        floating_window_manager.ok_or_else(|| RuntimeCommandError::CommandFailed {
            name: "terminal.closeFloat".to_string(),
            message: "floating window manager is not available".to_string(),
        })?;
    let terminal_manager =
        terminal_float_manager.ok_or_else(|| RuntimeCommandError::CommandFailed {
            name: "terminal.closeFloat".to_string(),
            message: "terminal float manager is not available".to_string(),
        })?;
    let value: serde_json::Value =
        serde_json::from_str(payload).map_err(|error| RuntimeCommandError::CommandFailed {
            name: "terminal.closeFloat".to_string(),
            message: format!("invalid terminal close payload: {error}"),
        })?;
    let terminal_id = value
        .get("terminalId")
        .or_else(|| value.get("terminal_id"))
        .and_then(serde_json::Value::as_u64)
        .or_else(|| floating_manager.focused_terminal_id())
        .ok_or_else(|| RuntimeCommandError::CommandFailed {
            name: "terminal.closeFloat".to_string(),
            message: "terminal id is required when no terminal float is focused".to_string(),
        })?;
    let float_id = floating_manager
        .focused_float_id()
        .filter(|_| floating_manager.focused_terminal_id() == Some(terminal_id));
    if let Some(float_id) = float_id {
        floating_manager.close(float_id);
    }
    terminal_manager.close_view(terminal_id).map_err(|error| {
        RuntimeCommandError::CommandFailed {
            name: "terminal.closeFloat".to_string(),
            message: format!("failed to close terminal float: {error:?}"),
        }
    })?;
    log::debug!(
        "[main][terminal_float] terminal float close host command applied: float_id={:?}, terminal_id={}",
        float_id.map(|id| id.0),
        terminal_id
    );
    Ok(RuntimeCommandEffect::default())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LspHoverPopupKind {
    Hover,
    SignatureHelp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LspPopupKind {
    Hover,
    Diagnostics,
    Locations,
    Symbols,
    SignatureHelp,
}

struct PopupSizingContext {
    terminal_width: u16,
    terminal_height: u16,
    parent_window_rect: PaneRect,
}

fn parse_lsp_hover_popup_kind(
    value: &serde_json::Value,
) -> Result<LspHoverPopupKind, RuntimeCommandError> {
    match value.get("kind").and_then(serde_json::Value::as_str) {
        None | Some("hover") => Ok(LspHoverPopupKind::Hover),
        Some("signatureHelp") => Ok(LspHoverPopupKind::SignatureHelp),
        Some(kind) => Err(RuntimeCommandError::CommandFailed {
            name: "lsp.floatHover".to_string(),
            message: format!("invalid LSP hover float payload: unsupported kind {kind:?}"),
        }),
    }
}

fn default_lsp_popup_basis(kind: LspPopupKind) -> PopupSizeBasis {
    match kind {
        LspPopupKind::Locations | LspPopupKind::Symbols => PopupSizeBasis::Editor,
        LspPopupKind::Hover | LspPopupKind::Diagnostics | LspPopupKind::SignatureHelp => {
            PopupSizeBasis::Window
        }
    }
}

fn resolve_lsp_popup_size_limit_from_payload(
    command_name: &str,
    value: &serde_json::Value,
    kind: LspPopupKind,
    snapshot: &vim_core_rs::CoreLightSnapshot,
    window_id: i32,
) -> Result<ResolvedPopupSizeLimit, RuntimeCommandError> {
    let spec = parse_lsp_popup_size_spec(
        value.get("ui"),
        default_lsp_popup_basis(kind),
        &format!("{command_name}.ui"),
    )
    .map_err(|message| RuntimeCommandError::CommandFailed {
        name: command_name.to_string(),
        message,
    })?;
    let context = lsp_popup_sizing_context(snapshot, window_id);
    let limit = resolve_lsp_popup_size_limit(spec, &context);
    log::debug!(
        "[main][lsp_float] resolved popup size: command={}, kind={:?}, spec={:?}, terminal=({},{}), parent_rect=({},{} {}x{}), limit=({},{})",
        command_name,
        kind,
        spec,
        context.terminal_width,
        context.terminal_height,
        context.parent_window_rect.x,
        context.parent_window_rect.y,
        context.parent_window_rect.width,
        context.parent_window_rect.height,
        limit.max_width,
        limit.max_height
    );
    Ok(limit)
}

fn parse_lsp_popup_size_spec(
    value: Option<&serde_json::Value>,
    default_basis: PopupSizeBasis,
    field: &str,
) -> Result<PopupSizeSpec, String> {
    let Some(value) = value else {
        return Ok(PopupSizeSpec::lsp_default(default_basis));
    };
    let object = value
        .as_object()
        .ok_or_else(|| format!("invalid LSP popup size config: {field} must be an object"))?;
    for key in object.keys() {
        if !matches!(key.as_str(), "width" | "height" | "basis") {
            return Err(format!(
                "invalid LSP popup size config: unknown {field}.{key}"
            ));
        }
    }
    let basis = match object.get("basis").and_then(serde_json::Value::as_str) {
        None => default_basis,
        Some("window") => PopupSizeBasis::Window,
        Some("editor") => PopupSizeBasis::Editor,
        Some("available") => PopupSizeBasis::Available,
        Some(basis) => {
            return Err(format!(
                "invalid LSP popup size config: {field}.basis must be \"window\", \"editor\", or \"available\", got {basis:?}"
            ));
        }
    };
    Ok(PopupSizeSpec {
        width: parse_lsp_popup_size_value(object.get("width"), &format!("{field}.width"))?
            .unwrap_or(PopupSizeValue::Cells(72)),
        height: parse_lsp_popup_size_value(object.get("height"), &format!("{field}.height"))?
            .unwrap_or(PopupSizeValue::Cells(12)),
        basis,
    })
}

fn parse_lsp_popup_size_value(
    value: Option<&serde_json::Value>,
    field: &str,
) -> Result<Option<PopupSizeValue>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    if let Some(cells) = value.as_u64() {
        let cells = u16::try_from(cells).map_err(|_| {
            format!("invalid LSP popup size config: {field} must fit in terminal cells")
        })?;
        if cells == 0 {
            return Err(format!(
                "invalid LSP popup size config: {field} must be at least 1"
            ));
        }
        return Ok(Some(PopupSizeValue::Cells(cells)));
    }
    if let Some(percent) = value.as_str().and_then(parse_lsp_popup_percent) {
        return Ok(Some(PopupSizeValue::Percent(percent)));
    }
    Err(format!(
        "invalid LSP popup size config: {field} must be a positive integer or a percentage string from 1% through 100%"
    ))
}

fn parse_lsp_popup_percent(value: &str) -> Option<u8> {
    let digits = value.strip_suffix('%')?;
    if digits.is_empty() || !digits.chars().all(|char| char.is_ascii_digit()) {
        return None;
    }
    let percent = digits.parse::<u8>().ok()?;
    (1..=100).contains(&percent).then_some(percent)
}

fn lsp_popup_sizing_context(
    snapshot: &vim_core_rs::CoreLightSnapshot,
    window_id: i32,
) -> PopupSizingContext {
    let (terminal_width, terminal_height) = current_terminal_size();
    let parent_window_rect = snapshot
        .window(window_id)
        .map(PaneRect::from_core_window)
        .unwrap_or(PaneRect {
            x: 0,
            y: 0,
            width: terminal_width,
            height: terminal_height,
        });
    PopupSizingContext {
        terminal_width,
        terminal_height,
        parent_window_rect,
    }
}

fn resolve_lsp_popup_size_limit(
    spec: PopupSizeSpec,
    context: &PopupSizingContext,
) -> ResolvedPopupSizeLimit {
    if matches!(spec.basis, PopupSizeBasis::Available) {
        log::debug!(
            "[main][lsp_float] popup size basis \"available\" is accepted but origin-based resolution is not available yet; falling back to \"window\""
        );
    }
    let width = resolve_lsp_popup_size_value(
        spec.width,
        spec.basis,
        context.terminal_width,
        context.parent_window_rect.width,
    );
    let height = resolve_lsp_popup_size_value(
        spec.height,
        spec.basis,
        context.terminal_height,
        context.parent_window_rect.height,
    );
    let limit = ResolvedPopupSizeLimit::bordered(width, height);
    ResolvedPopupSizeLimit {
        max_width: limit.max_width.min(context.terminal_width.max(1)),
        max_height: limit.max_height.min(context.terminal_height.max(1)),
    }
}

fn resolve_lsp_popup_size_value(
    value: PopupSizeValue,
    basis: PopupSizeBasis,
    editor_dimension: u16,
    window_dimension: u16,
) -> u16 {
    let resolved = match value {
        PopupSizeValue::Cells(cells) => cells,
        PopupSizeValue::Percent(percent) => {
            let basis_dimension = match basis {
                PopupSizeBasis::Editor => editor_dimension,
                PopupSizeBasis::Window | PopupSizeBasis::Available => window_dimension,
            };
            let cells = u32::from(basis_dimension) * u32::from(percent) / 100;
            u16::try_from(cells).unwrap_or(u16::MAX).max(1)
        }
    };
    resolved.max(1).min(editor_dimension.max(1))
}

fn execute_lsp_hover_float_host_command(
    payload: &str,
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    floating_window_manager: Option<&mut FloatingWindowManager>,
    lsp_diagnostic_store: Option<&LspDiagnosticStore>,
) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
    let manager = floating_window_manager.ok_or_else(|| RuntimeCommandError::CommandFailed {
        name: "lsp.floatHover".to_string(),
        message: "floating window manager is not available".to_string(),
    })?;
    let value: serde_json::Value =
        serde_json::from_str(payload).map_err(|error| RuntimeCommandError::CommandFailed {
            name: "lsp.floatHover".to_string(),
            message: format!("invalid LSP hover float payload: {error}"),
        })?;
    let snapshot = outcome.core_bridge.light_snapshot();
    let window_id = snapshot.active_window_id().unwrap_or(1);
    let hover_kind = parse_lsp_hover_popup_kind(&value)?;
    let size_limit = resolve_lsp_popup_size_limit_from_payload(
        "lsp.floatHover",
        &value,
        match hover_kind {
            LspHoverPopupKind::Hover => LspPopupKind::Hover,
            LspHoverPopupKind::SignatureHelp => LspPopupKind::SignatureHelp,
        },
        &snapshot,
        window_id,
    )?;
    let response = value
        .get("response")
        .or_else(|| value.get("result"))
        .cloned()
        .unwrap_or(value);
    if hover_response_is_plain_any(&response)
        && let Some(diagnostic) = lsp_diagnostic_store.and_then(|store| {
            store.diagnostic_at_position(snapshot.cursor_row, snapshot.cursor_col)
        })
    {
        let response = serde_json::json!({
            "result": {
                "contents": format!("{}: {}", lsp_diagnostic_severity_label(diagnostic.severity), diagnostic.message)
            }
        });
        let outcome = open_lsp_hover_float(
            manager,
            LspHoverFloatRequest {
                window_id,
                cursor_row: snapshot.cursor_row,
                cursor_col: snapshot.cursor_col,
                response,
                size_limit,
            },
        );
        log::debug!(
            "[main][lsp_float] hover any replaced with cursor-relative diagnostic hover: outcome={:?}, window_id={}, cursor=({}, {}), diagnostic=({}, {})",
            outcome,
            window_id,
            snapshot.cursor_row,
            snapshot.cursor_col,
            diagnostic.line,
            diagnostic.column
        );
        return Ok(RuntimeCommandEffect {
            transient_message: outcome.is_none().then(|| "No LSP diagnostics".to_string()),
            ..RuntimeCommandEffect::default()
        });
    }
    let outcome = open_lsp_hover_float(
        manager,
        LspHoverFloatRequest {
            window_id,
            cursor_row: snapshot.cursor_row,
            cursor_col: snapshot.cursor_col,
            response,
            size_limit,
        },
    );
    log::debug!(
        "[main][lsp_float] hover float host command applied: outcome={:?}, window_id={}, cursor=({}, {})",
        outcome,
        window_id,
        snapshot.cursor_row,
        snapshot.cursor_col
    );
    Ok(RuntimeCommandEffect {
        transient_message: outcome
            .is_none()
            .then(|| "No LSP hover content".to_string()),
        ..RuntimeCommandEffect::default()
    })
}

fn lsp_diagnostic_severity_label(severity: Option<u64>) -> &'static str {
    match severity {
        Some(1) => "Error",
        Some(2) => "Warning",
        Some(3) => "Info",
        Some(4) => "Hint",
        _ => "Diagnostic",
    }
}

fn hover_response_is_plain_any(response: &serde_json::Value) -> bool {
    let Some(contents) = response
        .pointer("/result/contents")
        .or_else(|| response.get("contents"))
    else {
        return false;
    };
    hover_contents_plain_text(contents)
        .map(|text| {
            text.replace("```typescript", "")
                .replace("```ts", "")
                .replace("```", "")
                .trim()
                == "any"
        })
        .unwrap_or(false)
}

fn hover_contents_plain_text(contents: &serde_json::Value) -> Option<String> {
    if let Some(text) = contents.as_str() {
        return Some(text.to_string());
    }
    if let Some(value) = contents.get("value").and_then(serde_json::Value::as_str) {
        return Some(value.to_string());
    }
    let items = contents.as_array()?;
    let mut parts = Vec::new();
    for item in items {
        if let Some(text) = hover_contents_plain_text(item) {
            parts.push(text);
        }
    }
    Some(parts.join("\n"))
}

fn execute_lsp_diagnostic_float_host_command(
    payload: &str,
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    floating_window_manager: Option<&mut FloatingWindowManager>,
) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
    let manager = floating_window_manager.ok_or_else(|| RuntimeCommandError::CommandFailed {
        name: "lsp.floatDiagnostics".to_string(),
        message: "floating window manager is not available".to_string(),
    })?;
    let value: serde_json::Value =
        serde_json::from_str(payload).map_err(|error| RuntimeCommandError::CommandFailed {
            name: "lsp.floatDiagnostics".to_string(),
            message: format!("invalid LSP diagnostic float payload: {error}"),
        })?;
    let snapshot = outcome.core_bridge.light_snapshot();
    let window_id = snapshot.active_window_id().unwrap_or(1);
    let size_limit = resolve_lsp_popup_size_limit_from_payload(
        "lsp.floatDiagnostics",
        &value,
        LspPopupKind::Diagnostics,
        &snapshot,
        window_id,
    )?;
    let line = value
        .get("line")
        .and_then(serde_json::Value::as_u64)
        .map(|line| line as usize)
        .unwrap_or(snapshot.cursor_row);
    let column = value
        .get("column")
        .and_then(serde_json::Value::as_u64)
        .map(|column| column as usize)
        .unwrap_or(snapshot.cursor_col);
    let diagnostics = value
        .get("diagnostics")
        .or_else(|| value.pointer("/params/diagnostics"))
        .cloned()
        .unwrap_or(value);
    let id = open_lsp_diagnostic_float(
        manager,
        LspDiagnosticFloatRequest {
            window_id,
            line,
            column,
            diagnostics,
            size_limit,
        },
    );
    log::debug!(
        "[main][lsp_float] diagnostic float host command applied: opened={:?}, window_id={}, position=({}, {})",
        id.map(|id| id.0),
        window_id,
        line,
        column
    );
    Ok(RuntimeCommandEffect {
        transient_message: id.is_none().then(|| "No LSP diagnostics".to_string()),
        ..RuntimeCommandEffect::default()
    })
}

fn execute_lsp_location_list_float_host_command(
    payload: &str,
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    floating_window_manager: Option<&mut FloatingWindowManager>,
) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
    let manager = floating_window_manager.ok_or_else(|| RuntimeCommandError::CommandFailed {
        name: "lsp.floatLocations".to_string(),
        message: "floating window manager is not available".to_string(),
    })?;
    let value: serde_json::Value =
        serde_json::from_str(payload).map_err(|error| RuntimeCommandError::CommandFailed {
            name: "lsp.floatLocations".to_string(),
            message: format!("invalid LSP locations float payload: {error}"),
        })?;
    let snapshot = outcome.core_bridge.light_snapshot();
    let window_id = snapshot.active_window_id().unwrap_or(1);
    let size_limit = resolve_lsp_popup_size_limit_from_payload(
        "lsp.floatLocations",
        &value,
        LspPopupKind::Locations,
        &snapshot,
        window_id,
    )?;
    let title = value
        .get("title")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Locations")
        .to_string();
    let response = value
        .get("response")
        .or_else(|| value.get("result"))
        .cloned()
        .unwrap_or(value);
    let id = open_lsp_location_list_float(
        manager,
        LspLocationListRequest {
            window_id,
            title,
            response,
            size_limit,
        },
    );
    log::debug!(
        "[main][lsp_float] location list float host command applied: opened={:?}, window_id={}",
        id.map(|id| id.0),
        window_id
    );
    Ok(RuntimeCommandEffect {
        transient_message: id.is_none().then(|| "No LSP locations".to_string()),
        ..RuntimeCommandEffect::default()
    })
}

fn execute_lsp_symbol_outline_float_host_command(
    payload: &str,
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    floating_window_manager: Option<&mut FloatingWindowManager>,
) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
    let manager = floating_window_manager.ok_or_else(|| RuntimeCommandError::CommandFailed {
        name: "lsp.floatSymbols".to_string(),
        message: "floating window manager is not available".to_string(),
    })?;
    let value: serde_json::Value =
        serde_json::from_str(payload).map_err(|error| RuntimeCommandError::CommandFailed {
            name: "lsp.floatSymbols".to_string(),
            message: format!("invalid LSP symbols float payload: {error}"),
        })?;
    let snapshot = outcome.core_bridge.light_snapshot();
    let window_id = snapshot.active_window_id().unwrap_or(1);
    let size_limit = resolve_lsp_popup_size_limit_from_payload(
        "lsp.floatSymbols",
        &value,
        LspPopupKind::Symbols,
        &snapshot,
        window_id,
    )?;
    let response = value
        .get("response")
        .or_else(|| value.get("result"))
        .cloned()
        .unwrap_or(value);
    let id = open_lsp_symbol_outline_float(
        manager,
        LspSymbolOutlineRequest {
            window_id,
            response,
            size_limit,
        },
    );
    log::debug!(
        "[main][lsp_float] symbol outline float host command applied: opened={:?}, window_id={}",
        id.map(|id| id.0),
        window_id
    );
    Ok(RuntimeCommandEffect {
        transient_message: id.is_none().then(|| "No LSP symbols".to_string()),
        ..RuntimeCommandEffect::default()
    })
}

fn execute_lsp_goto_definition_host_command(
    payload: &str,
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    session_state: &mut saya::app::session::EditorSessionState,
) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
    let value: serde_json::Value =
        serde_json::from_str(payload).map_err(|error| RuntimeCommandError::CommandFailed {
            name: "lsp.gotoDefinition".to_string(),
            message: format!("invalid LSP definition payload: {error}"),
        })?;
    let location =
        first_lsp_location(&value).ok_or_else(|| RuntimeCommandError::CommandFailed {
            name: "lsp.gotoDefinition".to_string(),
            message: "No LSP definition target".to_string(),
        })?;
    let uri = location
        .get("uri")
        .or_else(|| location.get("targetUri"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RuntimeCommandError::CommandFailed {
            name: "lsp.gotoDefinition".to_string(),
            message: "LSP definition target is missing a URI".to_string(),
        })?;
    let path = file_uri_to_path(uri).ok_or_else(|| RuntimeCommandError::CommandFailed {
        name: "lsp.gotoDefinition".to_string(),
        message: format!("unsupported LSP definition URI: {uri}"),
    })?;
    let line = location
        .pointer("/range/start/line")
        .or_else(|| location.pointer("/targetSelectionRange/start/line"))
        .or_else(|| location.pointer("/targetRange/start/line"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as usize;
    let mut effect = execute_runtime_host_command_with_floats(
        &format!("edit {}", path.display()),
        outcome,
        session_state,
        None,
        None,
        None,
        None,
    )?;
    let line_command = format!(":{}", line.saturating_add(1));
    outcome
        .core_bridge
        .apply_ex_command(&line_command)
        .map_err(|error| RuntimeCommandError::CommandFailed {
            name: "lsp.gotoDefinition".to_string(),
            message: format!("failed to move to LSP definition line: {error:?}"),
        })?;
    effect.transient_message = Some(format!("LSP definition: {}:{}", path.display(), line + 1));
    log::debug!(
        "[main][lsp] definition navigation applied: path={}, line={}",
        path.display(),
        line
    );
    Ok(effect)
}

async fn handle_selector_accept_action(
    runtime_session: &mut RuntimeSessionOwner,
    selector_model: &SelectorTuiViewModel,
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    session_state: &mut saya::app::session::EditorSessionState,
) -> RuntimeDispatchOutcome {
    log::info!(
        "[main][selector] selector action accept selected start: session_id={}, hidden={}, cancelled={}",
        selector_model.session_id,
        selector_model.hidden,
        selector_model.cancelled
    );
    let Some(selected_row) = selector_model.selected_row.as_ref() else {
        log::debug!(
            "[main][selector] selected item detail parse failed: session_id={}, reason=no-selected-row",
            selector_model.session_id
        );
        return RuntimeDispatchOutcome {
            transient_message: Some("Selector action failed: no selected item".to_string()),
            requires_redraw: true,
            shutdown_intent: None,
            presentation_intents: Vec::new(),
        };
    };

    let location = match parse_rg_selector_location_detail(&selected_row.item) {
        Ok(location) => {
            log::info!(
                "[main][selector] selected item detail parse succeeded: session_id={}, item_id={}, path={}, line={}, column={}",
                selector_model.session_id,
                selected_row.item.id,
                location.path.display(),
                location.line,
                location.column
            );
            location
        }
        Err(error) => {
            log::debug!(
                "[main][selector] selected item detail parse failed: session_id={}, item_id={}, kind={}, error={}",
                selector_model.session_id,
                selected_row.item.id,
                selected_row.item.kind,
                error
            );
            return RuntimeDispatchOutcome {
                transient_message: Some(format!("Selector action failed: {error}")),
                requires_redraw: true,
                shutdown_intent: None,
                presentation_intents: Vec::new(),
            };
        }
    };

    match execute_selector_rg_jump(&location, outcome, session_state) {
        Ok(mut dispatch_outcome) => {
            let mut host_session = MainRuntimeHostSession::new(outcome, session_state);
            let hide_outcome = runtime_session
                .control_selector(
                    selector_model.session_id,
                    RuntimeSelectorControllerCommand::Hide,
                    &mut host_session,
                )
                .await;
            merge_runtime_dispatch_outcome(&mut dispatch_outcome, hide_outcome);
            dispatch_outcome
        }
        Err(error) => {
            log::debug!(
                "[main][selector] jump failed: session_id={}, path={}, line={}, column={}, error={:?}",
                selector_model.session_id,
                location.path.display(),
                location.line,
                location.column,
                error
            );
            RuntimeDispatchOutcome {
                transient_message: Some(format!("Selector jump failed: {error:?}")),
                requires_redraw: true,
                shutdown_intent: None,
                presentation_intents: Vec::new(),
            }
        }
    }
}

fn execute_selector_rg_jump(
    location: &RuntimeRgLocation,
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    session_state: &mut saya::app::session::EditorSessionState,
) -> Result<RuntimeDispatchOutcome, RuntimeCommandError> {
    log::info!(
        "[main][selector] jump execution start: path={}, line={}, column={}",
        location.path.display(),
        location.line,
        location.column
    );
    let mut effect = execute_runtime_host_command_with_floats(
        &format!("edit {}", escape_runtime_edit_path(&location.path)),
        outcome,
        session_state,
        None,
        None,
        None,
        None,
    )?;
    let line_command = format!("{}G", location.line);
    outcome
        .core_bridge
        .dispatch_key(&line_command)
        .map_err(|error| RuntimeCommandError::CommandFailed {
            name: "selector.rgJump".to_string(),
            message: format!("failed to move to selector rg line: {error:?}"),
        })?;
    let column_command = format!("{}|", location.column);
    outcome
        .core_bridge
        .dispatch_key(&column_command)
        .map_err(|error| RuntimeCommandError::CommandFailed {
            name: "selector.rgJump".to_string(),
            message: format!("failed to move to selector rg column: {error:?}"),
        })?;
    effect.transient_message = Some(format!(
        "Selector jump: {}:{}:{}",
        location.path.display(),
        location.line,
        location.column
    ));
    log::info!(
        "[main][selector] jump succeeded: path={}, line={}, column={}",
        location.path.display(),
        location.line,
        location.column
    );
    Ok(RuntimeDispatchOutcome {
        transient_message: effect.transient_message,
        requires_redraw: true,
        shutdown_intent: effect.shutdown_intent,
        presentation_intents: effect.presentation_intents,
    })
}

fn execute_lsp_workspace_edit_preview_host_command(
    payload: &str,
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    floating_window_manager: Option<&mut FloatingWindowManager>,
) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
    let value: serde_json::Value =
        serde_json::from_str(payload).map_err(|error| RuntimeCommandError::CommandFailed {
            name: "lsp.previewWorkspaceEdit".to_string(),
            message: format!("invalid LSP workspace edit preview payload: {error}"),
        })?;
    let title = value
        .get("title")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Workspace edit preview");
    let edit = value
        .pointer("/response/result")
        .or_else(|| value.get("response"))
        .or_else(|| value.get("result"))
        .unwrap_or(&value);
    let lines = workspace_edit_preview_lines(title, edit);
    let response = serde_json::json!({
        "result": {
            "contents": lines.join("\n")
        }
    });
    let payload = serde_json::json!({ "response": response }).to_string();
    let mut effect =
        execute_lsp_hover_float_host_command(&payload, outcome, floating_window_manager, None)?;
    effect.transient_message = Some(format!(
        "{title}: {} change(s)",
        lines.len().saturating_sub(1)
    ));
    log::debug!(
        "[main][lsp] workspace edit preview displayed: title={}, lines={}",
        title,
        lines.len()
    );
    Ok(effect)
}

fn execute_lsp_code_actions_float_host_command(
    payload: &str,
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    floating_window_manager: Option<&mut FloatingWindowManager>,
) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
    let value: serde_json::Value =
        serde_json::from_str(payload).map_err(|error| RuntimeCommandError::CommandFailed {
            name: "lsp.floatCodeActions".to_string(),
            message: format!("invalid LSP code action payload: {error}"),
        })?;
    let actions = value
        .pointer("/response/result")
        .or_else(|| value.get("response"))
        .or_else(|| value.get("result"))
        .unwrap_or(&value);
    let lines = code_action_preview_lines(actions);
    let response = serde_json::json!({
        "result": {
            "contents": lines.join("\n")
        }
    });
    let payload = serde_json::json!({ "response": response }).to_string();
    let mut effect =
        execute_lsp_hover_float_host_command(&payload, outcome, floating_window_manager, None)?;
    effect.transient_message = Some(format!(
        "LSP code actions: {} action(s)",
        lines.len().saturating_sub(1)
    ));
    log::debug!(
        "[main][lsp] code action float displayed: lines={}",
        lines.len()
    );
    Ok(effect)
}

fn execute_lsp_publish_diagnostics_host_command(
    payload: &str,
    _outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    _floating_window_manager: Option<&mut FloatingWindowManager>,
    lsp_diagnostic_store: Option<&mut LspDiagnosticStore>,
) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
    let value: serde_json::Value =
        serde_json::from_str(payload).map_err(|error| RuntimeCommandError::CommandFailed {
            name: "lsp.publishDiagnostics".to_string(),
            message: format!("invalid LSP diagnostics payload: {error}"),
        })?;
    if let Some(store) = lsp_diagnostic_store {
        store.replace_from_lsp_value(&value);
    }
    Ok(RuntimeCommandEffect {
        transient_message: None,
        ..RuntimeCommandEffect::default()
    })
}

fn execute_lsp_cycle_diagnostic_host_command(
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    floating_window_manager: Option<&mut FloatingWindowManager>,
    lsp_diagnostic_store: Option<&mut LspDiagnosticStore>,
    next: bool,
    payload: Option<&str>,
) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
    let store = lsp_diagnostic_store.ok_or_else(|| RuntimeCommandError::CommandFailed {
        name: "lsp.diagnosticNavigation".to_string(),
        message: "LSP diagnostic store is not available".to_string(),
    })?;
    let diagnostic = if next {
        store.next_diagnostic()
    } else {
        store.previous_diagnostic()
    }
    .cloned();
    let Some(diagnostic) = diagnostic else {
        return Ok(RuntimeCommandEffect {
            transient_message: Some("No LSP diagnostics".to_string()),
            ..RuntimeCommandEffect::default()
        });
    };
    let ui =
        match payload {
            Some(payload) => {
                let value: serde_json::Value = serde_json::from_str(payload).map_err(|error| {
                    RuntimeCommandError::CommandFailed {
                        name: "lsp.diagnosticNavigation".to_string(),
                        message: format!("invalid LSP diagnostic navigation payload: {error}"),
                    }
                })?;
                Some(value.get("ui").cloned().ok_or_else(|| {
                    RuntimeCommandError::CommandFailed {
                        name: "lsp.diagnosticNavigation".to_string(),
                        message: "diagnostic navigation payload must include ui".to_string(),
                    }
                })?)
            }
            None => None,
        };
    let mut payload = serde_json::json!({
        "line": diagnostic.line,
        "column": diagnostic.column,
        "diagnostics": [{
            "severity": diagnostic.severity,
            "message": diagnostic.message,
        }],
    });
    if let Some(ui) = ui {
        payload["ui"] = ui;
    }
    let payload = payload.to_string();
    let mut effect =
        execute_lsp_diagnostic_float_host_command(&payload, outcome, floating_window_manager)?;
    effect.transient_message = Some(format!(
        "LSP diagnostic: {}:{} {}",
        diagnostic.line + 1,
        diagnostic.column + 1,
        diagnostic.message
    ));
    Ok(effect)
}

fn first_lsp_location(value: &serde_json::Value) -> Option<&serde_json::Value> {
    let result = value
        .pointer("/response/result")
        .or_else(|| value.pointer("/result"))
        .or_else(|| value.get("response"))
        .unwrap_or(value);
    match result {
        serde_json::Value::Array(items) => items.first(),
        serde_json::Value::Object(_) => Some(result),
        _ => None,
    }
}

fn workspace_edit_preview_lines(title: &str, edit: &serde_json::Value) -> Vec<String> {
    let mut lines = vec![title.to_string()];
    if let Some(changes) = edit.get("changes").and_then(serde_json::Value::as_object) {
        for (uri, edits) in changes {
            let count = edits
                .as_array()
                .map(|items| items.len())
                .unwrap_or_default();
            lines.push(format!("{uri}: {count} edit(s)"));
        }
    }
    if let Some(document_changes) = edit
        .get("documentChanges")
        .and_then(serde_json::Value::as_array)
    {
        for change in document_changes {
            let uri = change
                .pointer("/textDocument/uri")
                .or_else(|| change.get("uri"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("<unknown>");
            if let Some(edits) = change.get("edits").and_then(serde_json::Value::as_array) {
                lines.push(format!("{uri}: {} edit(s)", edits.len()));
            } else if let Some(kind) = change.get("kind").and_then(serde_json::Value::as_str) {
                lines.push(format!("{kind}: {uri}"));
            }
        }
    }
    if lines.len() == 1 && edit.as_array().is_some() {
        lines.push(format!(
            "current document: {} edit(s)",
            edit.as_array().map(|items| items.len()).unwrap_or_default()
        ));
    }
    if lines.len() == 1 {
        lines.push("No workspace edits".to_string());
    }
    lines
}

fn code_action_preview_lines(actions: &serde_json::Value) -> Vec<String> {
    let mut lines = vec!["Code actions".to_string()];
    match actions {
        serde_json::Value::Array(items) => {
            for action in items {
                let title = action
                    .get("title")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("<untitled>");
                let kind = action
                    .get("kind")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("action");
                lines.push(format!("{kind}: {title}"));
            }
        }
        serde_json::Value::Object(_) => {
            let title = actions
                .get("title")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("<untitled>");
            let kind = actions
                .get("kind")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("action");
            lines.push(format!("{kind}: {title}"));
        }
        _ => {}
    }
    if lines.len() == 1 {
        lines.push("No code actions".to_string());
    }
    lines
}

fn execute_lsp_status_host_command(
    payload: &str,
) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
    let value: serde_json::Value =
        serde_json::from_str(payload).map_err(|error| RuntimeCommandError::CommandFailed {
            name: "lsp.status".to_string(),
            message: format!("invalid LSP status payload: {error}"),
        })?;
    let message = value
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Language server is not ready")
        .to_string();
    log::info!("[main][lsp] status: {message}");
    Ok(RuntimeCommandEffect {
        transient_message: Some(message),
        ..RuntimeCommandEffect::default()
    })
}

fn escape_runtime_edit_path(path: &std::path::Path) -> String {
    path.to_string_lossy()
        .chars()
        .flat_map(|ch| match ch {
            '\\' => ['\\', '\\'].into_iter().collect::<Vec<_>>(),
            ' ' => ['\\', ' '].into_iter().collect::<Vec<_>>(),
            _ => [ch].into_iter().collect::<Vec<_>>(),
        })
        .collect()
}

fn save_error_message(error: &SaveRequestError) -> String {
    match error {
        SaveRequestError::NoTargetPath => "No file name to save".to_string(),
        SaveRequestError::ReadOnly => "Read-only option is set; add ! to override".to_string(),
        SaveRequestError::DirectoryBuffer => {
            "Directory listings are not saved as regular files".to_string()
        }
    }
}

fn consume_core_outcomes_from_core(
    core_bridge: &mut saya::core::bridge::CoreBridge,
    accumulator: &mut MainOutcomeAccumulator,
    need_redraw: &mut bool,
) {
    let batch = core_bridge.take_normalized_outcomes();
    if batch.is_empty() {
        let neutral_refresh =
            StructuralRefresh::from_folded_effects(&StructuralEffectSet::default());
        trace_redraw_diagnostic(format_args!(
            "normalized batch was empty; switching to neutral structural refresh: redraw_requested={}, full={}, clear_before_draw={}, source={:?}, coalesced_count={}, invalidated_buffers={:?}, invalidated_windows={:?}, layout_dirty={}",
            neutral_refresh.redraw_plan.requested,
            neutral_refresh.redraw_plan.full,
            neutral_refresh.redraw_plan.clear_before_draw,
            neutral_refresh.redraw_plan.source,
            neutral_refresh.redraw_plan.coalesced_count,
            neutral_refresh.invalidation.buffer_ids,
            neutral_refresh.invalidation.window_ids,
            neutral_refresh.invalidation.layout_dirty
        ));
        accumulator.last_structural_refresh = Some(neutral_refresh);
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
    trace_redraw_diagnostic(format_args!(
        "normalized batch folded into structural refresh: redraw_requested={}, full={}, clear_before_draw={}, source={:?}, coalesced_count={}, invalidated_buffers={:?}, invalidated_windows={:?}, layout_dirty={}",
        structural_refresh.redraw_plan.requested,
        structural_refresh.redraw_plan.full,
        structural_refresh.redraw_plan.clear_before_draw,
        structural_refresh.redraw_plan.source,
        structural_refresh.redraw_plan.coalesced_count,
        structural_refresh.invalidation.buffer_ids,
        structural_refresh.invalidation.window_ids,
        structural_refresh.invalidation.layout_dirty
    ));
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

fn mark_structural_refresh_rendered(accumulator: &mut MainOutcomeAccumulator) {
    let neutral_refresh = StructuralRefresh::from_folded_effects(&StructuralEffectSet::default());
    trace_redraw_diagnostic(format_args!(
        "structural refresh marked rendered; switching to neutral state: previous_present={}, redraw_requested=false",
        accumulator.last_structural_refresh.is_some()
    ));
    accumulator.last_structural_refresh = Some(neutral_refresh);
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
    core_bridge: &mut saya::core::bridge::CoreBridge,
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
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    session_state: &mut saya::app::session::EditorSessionState,
    transient_msg: &mut Option<String>,
    need_redraw: &mut bool,
    runtime_presentation_intents: &mut Vec<RuntimePresentationIntent>,
    panel_manager: &mut PanelManager,
    terminal_float_manager: &mut TerminalFloatManager,
    lsif_bridge: Option<&LsifBridgeHandle>,
) -> Option<ShutdownReason> {
    let Some(runtime_session) = runtime_session else {
        return None;
    };
    let mut host_session =
        MainRuntimeHostSession::new_with_lsp_session(outcome, session_state, lsif_bridge);
    host_session.panel_manager = Some(panel_manager);
    host_session.terminal_float_manager = Some(terminal_float_manager);
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
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    session_state: &mut saya::app::session::EditorSessionState,
    transient_msg: &mut Option<String>,
    need_redraw: &mut bool,
    runtime_presentation_intents: &mut Vec<RuntimePresentationIntent>,
    lsif_bridge: Option<&LsifBridgeHandle>,
) -> Option<ShutdownReason> {
    let Some(runtime_session) = runtime_session else {
        return None;
    };
    let mut host_session =
        MainRuntimeHostSession::new_with_lsp_session(outcome, session_state, lsif_bridge);
    let payload = RuntimeEventMapper::buffer_write_post(host_session.current_buffer_snapshot());
    let dispatch_outcome = runtime_session.dispatch(payload, &mut host_session).await;
    apply_runtime_dispatch_outcome(
        transient_msg,
        need_redraw,
        runtime_presentation_intents,
        dispatch_outcome,
    )
}

async fn dispatch_buffer_changed_with_runtime(
    runtime_session: Option<&mut RuntimeSessionOwner>,
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    session_state: &mut saya::app::session::EditorSessionState,
    transient_msg: &mut Option<String>,
    need_redraw: &mut bool,
    runtime_presentation_intents: &mut Vec<RuntimePresentationIntent>,
    floating_window_manager: &mut FloatingWindowManager,
    completion_float_manager: &mut CompletionFloatManager,
    lsp_diagnostic_store: &mut LspDiagnosticStore,
    terminal_float_manager: &mut TerminalFloatManager,
    panel_manager: &mut PanelManager,
    lsif_bridge: Option<&LsifBridgeHandle>,
) -> Option<ShutdownReason> {
    let Some(runtime_session) = runtime_session else {
        return None;
    };
    let mut host_session = MainRuntimeHostSession::new_with_floating_windows(
        outcome,
        session_state,
        floating_window_manager,
        completion_float_manager,
        lsp_diagnostic_store,
        terminal_float_manager,
        panel_manager,
        lsif_bridge,
    );
    let payload = RuntimeEventMapper::buffer_changed(host_session.current_buffer_snapshot());
    let dispatch_outcome = runtime_session.dispatch(payload, &mut host_session).await;
    apply_runtime_dispatch_outcome(
        transient_msg,
        need_redraw,
        runtime_presentation_intents,
        dispatch_outcome,
    )
}

fn merge_runtime_dispatch_outcome(
    target: &mut RuntimeDispatchOutcome,
    next: RuntimeDispatchOutcome,
) {
    if next.transient_message.is_some() {
        target.transient_message = next.transient_message;
    }
    target.requires_redraw |= next.requires_redraw;
    merge_runtime_shutdown_intent(&mut target.shutdown_intent, next.shutdown_intent);
    if !next.presentation_intents.is_empty() {
        target.presentation_intents = next.presentation_intents;
    }
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

async fn execute_startup_keymap_registered_command(
    runtime_session: Option<&mut RuntimeSessionOwner>,
    command_name: &str,
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    session_state: &mut saya::app::session::EditorSessionState,
    floating_window_manager: &mut FloatingWindowManager,
    completion_float_manager: &mut CompletionFloatManager,
    lsp_diagnostic_store: &mut LspDiagnosticStore,
    terminal_float_manager: &mut TerminalFloatManager,
    panel_manager: &mut PanelManager,
    runtime_input_prompt: Option<&mut Option<RuntimeInputPromptUiState>>,
    transient_msg: &mut Option<String>,
    need_redraw: &mut bool,
    runtime_presentation_intents: &mut Vec<RuntimePresentationIntent>,
    lsif_bridge: Option<&LsifBridgeHandle>,
) -> Option<ShutdownReason> {
    let Some(runtime_session) = runtime_session else {
        log::info!(
            "[main][keymap] registered command skipped because runtime session is unavailable: command={}",
            command_name
        );
        *transient_msg = Some(format!("Runtime command unavailable: {}", command_name));
        *need_redraw = true;
        return None;
    };
    log::info!(
        "[main][keymap] executing startup registered command: command={}",
        command_name
    );
    let mut host_session = MainRuntimeHostSession::new_with_floating_windows(
        outcome,
        session_state,
        floating_window_manager,
        completion_float_manager,
        lsp_diagnostic_store,
        terminal_float_manager,
        panel_manager,
        lsif_bridge,
    );
    host_session.runtime_input_prompt = runtime_input_prompt;
    let dispatch_outcome = runtime_session
        .execute_command(command_name, &mut host_session)
        .await;
    apply_runtime_dispatch_outcome(
        transient_msg,
        need_redraw,
        runtime_presentation_intents,
        dispatch_outcome,
    )
}

#[cfg(test)]
fn startup_keymap_action_for_input(
    keymaps: &[saya::app::bootstrap::StartupKeymapSnapshot],
    mode: CoreMode,
    key: &KeyInput,
) -> Option<StartupKeymapAction> {
    let mode = startup_keymap_mode_from_core_mode(mode)?;
    let lhs = startup_keymap_lhs_from_input(key)?;
    startup_keymap_action_for_lhs(keymaps, mode, &lhs)
}

fn startup_keymap_action_for_snapshot_input(
    keymaps: &[saya::app::bootstrap::StartupKeymapSnapshot],
    snapshot: &vim_core_rs::CoreLightSnapshot,
    key: &KeyInput,
    pending_lhs: &mut Option<String>,
) -> Option<StartupKeymapAction> {
    let mode = startup_keymap_mode_from_core_mode(snapshot.mode)?;
    let key_lhs = startup_keymap_lhs_from_input(key)?;
    if let Some(prefix) = pending_lhs.take() {
        let lhs = format!("{prefix}{key_lhs}");
        if let Some(action) = startup_keymap_action_for_lhs(keymaps, mode, &lhs) {
            return Some(action);
        }
        if startup_keymap_has_longer_prefix(keymaps, mode, &lhs) {
            *pending_lhs = Some(lhs);
            return None;
        }
    }

    if snapshot.pending_input.pending_keys.is_empty() {
        if let Some(action) = startup_keymap_action_for_lhs(keymaps, mode, &key_lhs) {
            return Some(action);
        }
        if startup_keymap_has_longer_prefix(keymaps, mode, &key_lhs) {
            *pending_lhs = Some(key_lhs);
        }
        return None;
    }
    let pending_lhs = (!snapshot.pending_input.pending_keys.is_empty())
        .then(|| format!("{}{}", snapshot.pending_input.pending_keys, key_lhs));
    let direct_lhs = startup_keymap_lhs_from_input(key)?;

    [pending_lhs.as_deref(), Some(direct_lhs.as_str())]
        .into_iter()
        .flatten()
        .find_map(|lhs| {
            keymaps
                .iter()
                .rev()
                .find(|keymap| keymap.mode == mode && keymap.lhs == lhs)
                .map(|keymap| keymap.action.clone())
        })
}

fn startup_keymap_action_for_lhs(
    keymaps: &[saya::app::bootstrap::StartupKeymapSnapshot],
    mode: StartupKeymapMode,
    lhs: &str,
) -> Option<StartupKeymapAction> {
    keymaps
        .iter()
        .rev()
        .find(|keymap| keymap.mode == mode && keymap.lhs == lhs)
        .map(|keymap| keymap.action.clone())
}

fn startup_keymap_has_longer_prefix(
    keymaps: &[saya::app::bootstrap::StartupKeymapSnapshot],
    mode: StartupKeymapMode,
    lhs: &str,
) -> bool {
    keymaps
        .iter()
        .any(|keymap| keymap.mode == mode && keymap.lhs.starts_with(lhs) && keymap.lhs != lhs)
}

fn startup_keymap_mode_from_core_mode(mode: CoreMode) -> Option<StartupKeymapMode> {
    match mode {
        CoreMode::Insert => Some(StartupKeymapMode::Insert),
        CoreMode::Visual | CoreMode::VisualLine | CoreMode::VisualBlock => {
            Some(StartupKeymapMode::Visual)
        }
        CoreMode::Normal => Some(StartupKeymapMode::Normal),
        _ => None,
    }
}

fn startup_keymap_lhs_from_input(key: &KeyInput) -> Option<String> {
    match key {
        KeyInput::Char(ch) => Some(ch.to_string()),
        KeyInput::Ctrl(ch) => Some(format!("<C-{}>", ch.to_ascii_lowercase())),
        KeyInput::Tab => Some("<Tab>".to_string()),
        KeyInput::BackTab => Some("<S-Tab>".to_string()),
        KeyInput::Enter => Some("<Enter>".to_string()),
        KeyInput::Escape => Some("<Esc>".to_string()),
        KeyInput::Backspace => Some("<BS>".to_string()),
        _ => None,
    }
}

fn execute_runtime_window_open_float(
    request: RuntimeFloatOpenRequest,
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    floating_window_manager: Option<&mut FloatingWindowManager>,
    terminal_float_manager: Option<&mut TerminalFloatManager>,
) -> Result<RuntimeFloatSnapshot, RuntimeCommandError> {
    let manager = floating_window_manager.ok_or_else(|| RuntimeCommandError::CommandFailed {
        name: "window.openFloat".to_string(),
        message: "floating window manager is not available".to_string(),
    })?;
    let size = FloatingSize {
        width: request.width.unwrap_or(60).max(1),
        height: request.height.unwrap_or(12).max(1),
    };
    let chrome = FloatingChrome {
        border: runtime_float_border(request.border.as_deref()),
    };
    let placement = runtime_float_placement(&request, outcome)?;
    let zindex = runtime_float_zindex(request.z_index.as_ref());
    let lifecycle = runtime_float_lifecycle(request.lifecycle.as_deref());
    let focusable = request.focusable.unwrap_or(false);
    let replacement_group = request.group.clone();

    let id = match request.content.clone() {
        RuntimeFloatContentRequest::Lines { lines } => manager
            .open_static_lines_with_lifecycle_and_replacement_group(
                lines,
                lifecycle,
                replacement_group,
                placement,
                size,
                chrome,
                zindex,
                focusable,
            ),
        RuntimeFloatContentRequest::Buffer {
            buffer_id,
            window_id,
        } => {
            let snapshot = outcome.core_bridge.light_snapshot();
            let (window_id, _buffer_id) = resolve_buffer_float_backing_window(
                "window.openFloat",
                &snapshot,
                window_id.map(|id| id as i32),
                buffer_id.map(|id| id as i32),
            )?;
            manager.open_core_window_with_lifecycle(
                window_id,
                lifecycle,
                replacement_group,
                placement,
                size,
                chrome,
                zindex,
                focusable,
            )
        }
        RuntimeFloatContentRequest::Terminal {
            command,
            close_behavior,
        } => {
            let terminal_manager =
                terminal_float_manager.ok_or_else(|| RuntimeCommandError::CommandFailed {
                    name: "window.openFloat".to_string(),
                    message: "terminal float manager is not available".to_string(),
                })?;
            let (command, args) = runtime_terminal_command_parts(command).ok_or_else(|| {
                RuntimeCommandError::CommandFailed {
                    name: "window.openFloat".to_string(),
                    message: "terminal float command must not be empty".to_string(),
                }
            })?;
            let terminal_size = runtime_terminal_content_size(size, chrome);
            let terminal_id = terminal_manager
                .spawn(TerminalFloatSpawnRequest {
                    command,
                    args,
                    width: terminal_size.width,
                    height: terminal_size.height,
                    close_behavior: runtime_terminal_close_behavior(close_behavior.as_deref()),
                })
                .map_err(|error| RuntimeCommandError::CommandFailed {
                    name: "window.openFloat".to_string(),
                    message: format!("failed to spawn terminal float: {error:?}"),
                })?;
            manager.open_terminal_with_lifecycle(
                terminal_id,
                lifecycle,
                replacement_group,
                placement,
                size,
                chrome,
                zindex,
                focusable,
            )
        }
    };

    if focusable {
        manager.focus_float(id);
    }
    log::debug!(
        "[main][runtime_window] openFloat applied: float_id={}, content={:?}, focusable={}, size=({},{})",
        id.0,
        request.content,
        focusable,
        size.width,
        size.height
    );
    let focused_float_id = manager.focused_float_id();
    manager
        .debug_window(id)
        .map(|window| runtime_float_snapshot(window, focused_float_id))
        .ok_or_else(|| RuntimeCommandError::CommandFailed {
            name: "window.openFloat".to_string(),
            message: format!("opened float is missing: id={}", id.0),
        })
}

fn execute_runtime_window_close_float(
    id: u64,
    floating_window_manager: Option<&mut FloatingWindowManager>,
    terminal_float_manager: Option<&mut TerminalFloatManager>,
) -> Result<bool, RuntimeCommandError> {
    let manager = floating_window_manager.ok_or_else(|| RuntimeCommandError::CommandFailed {
        name: "window.close".to_string(),
        message: "floating window manager is not available".to_string(),
    })?;
    let float_id = FloatingWindowId(id);
    let terminal_id = manager
        .window_content(float_id)
        .and_then(|content| match content {
            saya::presentation::floating_window::FloatingContentRef::Terminal { terminal_id } => {
                Some(*terminal_id)
            }
            _ => None,
        });
    let closed = manager.close(float_id);
    if closed
        && let (Some(terminal_id), Some(terminal_manager)) = (terminal_id, terminal_float_manager)
    {
        let _ = terminal_manager.close_view(terminal_id);
    }
    log::debug!(
        "[main][runtime_window] close applied: float_id={}, closed={}, terminal_id={:?}",
        id,
        closed,
        terminal_id
    );
    Ok(closed)
}

fn runtime_float_snapshots(manager: &FloatingWindowManager) -> Vec<RuntimeFloatSnapshot> {
    let focused_float_id = manager.focused_float_id();
    manager
        .windows()
        .iter()
        .map(|window| runtime_float_snapshot(window, focused_float_id))
        .collect()
}

fn runtime_float_snapshot(
    window: &saya::presentation::floating_window::FloatingWindow,
    focused_float_id: Option<FloatingWindowId>,
) -> RuntimeFloatSnapshot {
    RuntimeFloatSnapshot {
        id: window.id.0,
        kind: runtime_float_content_kind(&window.content).to_string(),
        focused: focused_float_id == Some(window.id),
        focusable: window.focusable,
        width: window.size.width,
        height: window.size.height,
        row: window.placement.row,
        col: window.placement.col,
        border: runtime_float_border_label(window.chrome.border).to_string(),
        z_index: window.zindex,
        lifecycle: runtime_float_lifecycle_label(window.lifecycle).to_string(),
        replacement_group: window.replacement_group.clone(),
    }
}

fn runtime_float_content_kind(
    content: &saya::presentation::floating_window::FloatingContentRef,
) -> &'static str {
    match content {
        saya::presentation::floating_window::FloatingContentRef::CoreWindow { .. } => "buffer",
        saya::presentation::floating_window::FloatingContentRef::ScratchBuffer { .. } => "buffer",
        saya::presentation::floating_window::FloatingContentRef::Terminal { .. } => "terminal",
        saya::presentation::floating_window::FloatingContentRef::StaticLines { .. } => "lines",
        saya::presentation::floating_window::FloatingContentRef::CompletionMenu { .. } => {
            "completionMenu"
        }
    }
}

fn runtime_float_border_label(border: FloatingBorder) -> &'static str {
    match border {
        FloatingBorder::None => "none",
        FloatingBorder::Single => "single",
    }
}

fn runtime_float_lifecycle_label(lifecycle: FloatingLifecycle) -> &'static str {
    match lifecycle {
        FloatingLifecycle::Manual => "manual",
        FloatingLifecycle::CloseOnCursorMove => "closeOnCursorMove",
        FloatingLifecycle::CloseOnInsert => "closeOnInsert",
        FloatingLifecycle::CloseOnBufferChange => "closeOnBufferChange",
        FloatingLifecycle::CloseOnEvents(_) => "closeOnEvents",
        FloatingLifecycle::ReplaceByGroup(_) => "replaceByGroup",
    }
}

fn runtime_float_border(border: Option<&str>) -> FloatingBorder {
    match border.unwrap_or("single") {
        "none" | "borderless" => FloatingBorder::None,
        _ => FloatingBorder::Single,
    }
}

fn runtime_float_lifecycle(lifecycle: Option<&str>) -> FloatingLifecycle {
    match lifecycle.unwrap_or("manual") {
        "closeOnCursorMove" | "close-on-cursor-move" => FloatingLifecycle::CloseOnCursorMove,
        "closeOnInsert" | "close-on-insert" => FloatingLifecycle::CloseOnInsert,
        "closeOnBufferChange" | "close-on-buffer-change" => FloatingLifecycle::CloseOnBufferChange,
        _ => FloatingLifecycle::Manual,
    }
}

fn runtime_float_zindex(zindex: Option<&RuntimeFloatZIndexRequest>) -> FloatingZIndex {
    match zindex {
        Some(RuntimeFloatZIndexRequest::Custom(value)) => FloatingZIndex::Custom(*value),
        Some(RuntimeFloatZIndexRequest::Named(name)) => match name.as_str() {
            "hover" => FloatingZIndex::Hover,
            "completion" => FloatingZIndex::Completion,
            "completionDocumentation" | "completion-documentation" => {
                FloatingZIndex::CompletionDocumentation
            }
            "blockingPrompt" | "blocking-prompt" => FloatingZIndex::BlockingPrompt,
            _ => FloatingZIndex::User,
        },
        None => FloatingZIndex::User,
    }
}

fn runtime_float_placement(
    request: &RuntimeFloatOpenRequest,
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
) -> Result<FloatingPlacement, RuntimeCommandError> {
    let row = request.row.unwrap_or(1);
    let col = request.col.unwrap_or(2);
    let relative_to = match request.relative_to.as_ref() {
        Some(RuntimeFloatRelativeToRequest::Cursor { window_id }) => FloatingRelativeTo::Cursor {
            window_id: runtime_float_window_id(*window_id, outcome)?,
        },
        Some(RuntimeFloatRelativeToRequest::Window { window_id }) => FloatingRelativeTo::Window {
            window_id: runtime_float_window_id(*window_id, outcome)?,
        },
        Some(RuntimeFloatRelativeToRequest::BufferPosition {
            window_id,
            line,
            column,
        }) => FloatingRelativeTo::BufferPosition {
            window_id: runtime_float_window_id(*window_id, outcome)?,
            line: *line,
            column: *column,
        },
        _ => FloatingRelativeTo::Editor,
    };
    Ok(FloatingPlacement {
        relative_to,
        anchor: runtime_float_anchor(request.anchor.as_deref()),
        row,
        col,
        fit: FloatingFit::TruncateToGrid,
    })
}

fn runtime_float_anchor(anchor: Option<&str>) -> FloatingAnchor {
    match anchor.unwrap_or("nw") {
        "ne" => FloatingAnchor::NorthEast,
        "sw" => FloatingAnchor::SouthWest,
        "se" => FloatingAnchor::SouthEast,
        _ => FloatingAnchor::NorthWest,
    }
}

fn runtime_float_window_id(
    requested: Option<u64>,
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
) -> Result<i32, RuntimeCommandError> {
    requested
        .map(|id| id as i32)
        .or_else(|| outcome.core_bridge.light_snapshot().active_window_id())
        .ok_or_else(|| RuntimeCommandError::CommandFailed {
            name: "window.openFloat".to_string(),
            message: "active window is not available".to_string(),
        })
}

fn runtime_terminal_command_parts(command: Vec<String>) -> Option<(String, Vec<String>)> {
    let mut parts = command.into_iter();
    let command = parts.next()?.trim().to_string();
    if command.is_empty() {
        return None;
    }
    Some((command, parts.collect()))
}

fn runtime_terminal_content_size(size: FloatingSize, chrome: FloatingChrome) -> FloatingSize {
    match chrome.border {
        FloatingBorder::Single => FloatingSize {
            width: size.width.saturating_sub(2).max(1),
            height: size.height.saturating_sub(2).max(1),
        },
        FloatingBorder::None => size,
    }
}

fn runtime_terminal_close_behavior(close_behavior: Option<&str>) -> TerminalFloatCloseBehavior {
    match close_behavior.unwrap_or("kill") {
        "detach" | "detachOnClose" | "detach-on-close" => TerminalFloatCloseBehavior::DetachOnClose,
        _ => TerminalFloatCloseBehavior::KillOnClose,
    }
}

fn execute_runtime_panel_open(
    request: RuntimePanelOpenRequest,
    panel_manager: Option<&mut PanelManager>,
    terminal_float_manager: Option<&mut TerminalFloatManager>,
) -> Result<RuntimePanelSnapshot, RuntimeCommandError> {
    let manager = panel_manager.ok_or_else(|| RuntimeCommandError::CommandFailed {
        name: "panel.open".to_string(),
        message: "panel manager is not available".to_string(),
    })?;
    let position = runtime_panel_position(&request.position)?;
    let size = runtime_panel_size(&request.size)?;
    let content = runtime_panel_content(request.content.clone(), terminal_float_manager)?;
    let result = manager.open(PanelOpenRequest {
        id: request.id.clone(),
        position,
        size,
        content,
        focus: request.focus,
    });
    log::debug!(
        "[main][panel] open applied: id={}, numeric_id={}, position={:?}, size={:?}, focus={}",
        result.id,
        result.numeric_id,
        result.position,
        result.size,
        result.focused
    );
    manager
        .snapshots()
        .into_iter()
        .find(|snapshot| snapshot.id == request.id)
        .map(runtime_panel_snapshot)
        .ok_or_else(|| RuntimeCommandError::CommandFailed {
            name: "panel.open".to_string(),
            message: format!("opened panel is missing: id={}", request.id),
        })
}

fn execute_runtime_panel_close(
    id: String,
    panel_manager: Option<&mut PanelManager>,
    terminal_float_manager: Option<&mut TerminalFloatManager>,
) -> Result<bool, RuntimeCommandError> {
    let manager = panel_manager.ok_or_else(|| RuntimeCommandError::CommandFailed {
        name: "panel.close".to_string(),
        message: "panel manager is not available".to_string(),
    })?;
    let content = manager.close(&id);
    if let Some(content) = content.as_ref()
        && let Some(terminal_id) = content.terminal_id()
        && let Some(terminal_manager) = terminal_float_manager
    {
        let _ = terminal_manager.close_view(terminal_id);
    }
    log::debug!(
        "[main][panel] close applied: id={}, closed={}, content={:?}",
        id,
        content.is_some(),
        content
    );
    Ok(content.is_some())
}

fn runtime_panel_snapshot(
    snapshot: saya::presentation::panel::PanelSnapshot,
) -> RuntimePanelSnapshot {
    RuntimePanelSnapshot {
        id: snapshot.id,
        numeric_id: snapshot.numeric_id,
        position: runtime_panel_position_label(snapshot.position).to_string(),
        size: runtime_panel_size_label(snapshot.size),
        kind: snapshot.kind.to_string(),
        focused: snapshot.focused,
    }
}

fn runtime_panel_position(value: &str) -> Result<PanelPosition, RuntimeCommandError> {
    match value {
        "left" => Ok(PanelPosition::Left),
        "right" => Ok(PanelPosition::Right),
        "top" => Ok(PanelPosition::Top),
        "bottom" => Ok(PanelPosition::Bottom),
        _ => Err(RuntimeCommandError::CommandFailed {
            name: "panel.open".to_string(),
            message: format!("unsupported panel position: {value}"),
        }),
    }
}

fn runtime_panel_position_label(position: PanelPosition) -> &'static str {
    match position {
        PanelPosition::Left => "left",
        PanelPosition::Right => "right",
        PanelPosition::Top => "top",
        PanelPosition::Bottom => "bottom",
    }
}

fn runtime_panel_size(value: &str) -> Result<PanelSize, RuntimeCommandError> {
    let trimmed = value.trim();
    if let Some(percent) = trimmed.strip_suffix('%') {
        return percent
            .parse::<u16>()
            .map(PanelSize::Percent)
            .map_err(|error| RuntimeCommandError::CommandFailed {
                name: "panel.open".to_string(),
                message: format!("invalid panel percent size: {error}"),
            });
    }
    trimmed
        .parse::<u16>()
        .map(PanelSize::Cells)
        .map_err(|error| RuntimeCommandError::CommandFailed {
            name: "panel.open".to_string(),
            message: format!("invalid panel size: {error}"),
        })
}

fn runtime_panel_size_label(size: PanelSize) -> String {
    match size {
        PanelSize::Cells(cells) => cells.to_string(),
        PanelSize::Percent(percent) => format!("{percent}%"),
    }
}

fn runtime_panel_content(
    request: RuntimePanelContentRequest,
    terminal_float_manager: Option<&mut TerminalFloatManager>,
) -> Result<PanelContent, RuntimeCommandError> {
    match request.kind.as_str() {
        "lines" => Ok(PanelContent::Lines {
            lines: request.lines,
        }),
        "view" => {
            let nodes = request
                .nodes
                .into_iter()
                .map(runtime_panel_node)
                .collect::<Result<Vec<_>, _>>()?;
            log::debug!(
                "[main][panel] converted runtime view content: nodes={}",
                nodes.len()
            );
            Ok(PanelContent::View { nodes })
        }
        "terminal" => {
            let terminal_manager =
                terminal_float_manager.ok_or_else(|| RuntimeCommandError::CommandFailed {
                    name: "panel.open".to_string(),
                    message: "terminal manager is not available".to_string(),
                })?;
            let (command, args) =
                runtime_terminal_command_parts(request.command).ok_or_else(|| {
                    RuntimeCommandError::CommandFailed {
                        name: "panel.open".to_string(),
                        message: "panel terminal command must not be empty".to_string(),
                    }
                })?;
            let close_behavior = runtime_panel_close_behavior(request.close_behavior.as_deref());
            let terminal_id = terminal_manager
                .spawn(TerminalFloatSpawnRequest {
                    command,
                    args,
                    width: 80,
                    height: 24,
                    close_behavior: match close_behavior {
                        PanelCloseBehavior::Kill => TerminalFloatCloseBehavior::KillOnClose,
                        PanelCloseBehavior::Detach => TerminalFloatCloseBehavior::DetachOnClose,
                    },
                })
                .map_err(|error| RuntimeCommandError::CommandFailed {
                    name: "panel.open".to_string(),
                    message: format!("failed to spawn panel terminal: {error:?}"),
                })?;
            Ok(PanelContent::Terminal {
                terminal_id,
                close_behavior,
            })
        }
        other => Err(RuntimeCommandError::CommandFailed {
            name: "panel.open".to_string(),
            message: format!("unsupported panel content kind: {other}"),
        }),
    }
}

fn runtime_panel_node(request: RuntimePanelNodeRequest) -> Result<PanelNode, RuntimeCommandError> {
    let required_text = |field: Option<String>, node_type: &str, name: &str| {
        field
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| RuntimeCommandError::CommandFailed {
                name: "panel.open".to_string(),
                message: format!("panel view node {node_type} requires {name}"),
            })
    };
    match request.node_type.as_str() {
        "text" => Ok(PanelNode::Text {
            text: required_text(request.text, "text", "text")?,
        }),
        "heading" => Ok(PanelNode::Heading {
            text: required_text(request.text, "heading", "text")?,
        }),
        "divider" => Ok(PanelNode::Divider),
        "image" => Ok(PanelNode::Image {
            src: required_text(request.src, "image", "src")?,
            alt: request.alt,
        }),
        "badge" => Ok(PanelNode::Badge {
            label: required_text(request.label, "badge", "label")?,
        }),
        "progress" => Ok(PanelNode::Progress {
            label: request.label,
            value: request.value.unwrap_or(0).min(100),
        }),
        "button" => Ok(PanelNode::Button {
            label: required_text(request.label, "button", "label")?,
        }),
        other => Err(RuntimeCommandError::CommandFailed {
            name: "panel.open".to_string(),
            message: format!("unsupported panel view node type: {other}"),
        }),
    }
}

fn runtime_panel_close_behavior(close_behavior: Option<&str>) -> PanelCloseBehavior {
    match close_behavior.unwrap_or("kill") {
        "detach" | "detachOnClose" | "detach-on-close" => PanelCloseBehavior::Detach,
        _ => PanelCloseBehavior::Kill,
    }
}

struct MainRuntimeHostSession<'a> {
    outcome: &'a mut saya::app::bootstrap::BootstrapOutcome,
    session_state: &'a mut saya::app::session::EditorSessionState,
    runtime_input_prompt: Option<&'a mut Option<RuntimeInputPromptUiState>>,
    floating_window_manager: Option<&'a mut FloatingWindowManager>,
    completion_float_manager: Option<&'a mut CompletionFloatManager>,
    lsp_diagnostic_store: Option<&'a mut LspDiagnosticStore>,
    terminal_float_manager: Option<&'a mut TerminalFloatManager>,
    panel_manager: Option<&'a mut PanelManager>,
    lsif_bridge: Option<&'a LsifBridgeHandle>,
}

impl<'a> MainRuntimeHostSession<'a> {
    fn new(
        outcome: &'a mut saya::app::bootstrap::BootstrapOutcome,
        session_state: &'a mut saya::app::session::EditorSessionState,
    ) -> Self {
        Self {
            outcome,
            session_state,
            runtime_input_prompt: None,
            floating_window_manager: None,
            completion_float_manager: None,
            lsp_diagnostic_store: None,
            terminal_float_manager: None,
            panel_manager: None,
            lsif_bridge: None,
        }
    }

    fn new_with_lsp_session(
        outcome: &'a mut saya::app::bootstrap::BootstrapOutcome,
        session_state: &'a mut saya::app::session::EditorSessionState,
        lsif_bridge: Option<&'a LsifBridgeHandle>,
    ) -> Self {
        Self {
            outcome,
            session_state,
            runtime_input_prompt: None,
            floating_window_manager: None,
            completion_float_manager: None,
            lsp_diagnostic_store: None,
            terminal_float_manager: None,
            panel_manager: None,
            lsif_bridge,
        }
    }

    fn new_with_floating_windows(
        outcome: &'a mut saya::app::bootstrap::BootstrapOutcome,
        session_state: &'a mut saya::app::session::EditorSessionState,
        floating_window_manager: &'a mut FloatingWindowManager,
        completion_float_manager: &'a mut CompletionFloatManager,
        lsp_diagnostic_store: &'a mut LspDiagnosticStore,
        terminal_float_manager: &'a mut TerminalFloatManager,
        panel_manager: &'a mut PanelManager,
        lsif_bridge: Option<&'a LsifBridgeHandle>,
    ) -> Self {
        Self {
            outcome,
            session_state,
            runtime_input_prompt: None,
            floating_window_manager: Some(floating_window_manager),
            completion_float_manager: Some(completion_float_manager),
            lsp_diagnostic_store: Some(lsp_diagnostic_store),
            terminal_float_manager: Some(terminal_float_manager),
            panel_manager: Some(panel_manager),
            lsif_bridge,
        }
    }

    fn new_with_runtime_input(
        outcome: &'a mut saya::app::bootstrap::BootstrapOutcome,
        session_state: &'a mut saya::app::session::EditorSessionState,
        runtime_input_prompt: &'a mut Option<RuntimeInputPromptUiState>,
    ) -> Self {
        Self {
            outcome,
            session_state,
            runtime_input_prompt: Some(runtime_input_prompt),
            floating_window_manager: None,
            completion_float_manager: None,
            lsp_diagnostic_store: None,
            terminal_float_manager: None,
            panel_manager: None,
            lsif_bridge: None,
        }
    }
}

impl RuntimeHostSession for MainRuntimeHostSession<'_> {
    fn current_buffer_snapshot(&mut self) -> ReadonlyBufferSnapshot {
        let started_at = std::time::Instant::now();
        let snapshot = self.outcome.core_bridge.light_snapshot();
        let active_buffer_id = snapshot
            .buffers
            .iter()
            .find(|buffer| buffer.is_active)
            .map(|buffer| buffer.id as u64)
            .unwrap_or(1);
        let current_line_range = self.outcome.core_bridge.buffer_line_range(
            active_buffer_id as i32,
            snapshot.cursor_row,
            1,
        );
        let text_started_at = std::time::Instant::now();
        let text = self.outcome.core_bridge.buffer_text();
        log::debug!(
            "[PERF][main][runtime] fetched full current buffer snapshot text: buffer_id={}, bytes={}, elapsed_ms={}",
            active_buffer_id,
            text.len(),
            text_started_at.elapsed().as_millis()
        );
        let snapshot = ReadonlyBufferSnapshot {
            id: active_buffer_id,
            path: self.session_state.target_path().cloned(),
            line_count: current_line_range
                .as_ref()
                .map(|range| range.total_line_count)
                .unwrap_or(1),
            cursor_row: snapshot.cursor_row,
            cursor_col: snapshot.cursor_col,
            current_line: current_line_range
                .and_then(|range| range.lines.into_iter().next())
                .unwrap_or_default(),
            text,
        };
        log::debug!(
            "[PERF][main][runtime] built full current buffer snapshot: buffer_id={}, elapsed_ms={}",
            active_buffer_id,
            started_at.elapsed().as_millis()
        );
        snapshot
    }

    fn current_buffer_metadata_snapshot(&mut self) -> ReadonlyBufferSnapshot {
        let started_at = std::time::Instant::now();
        let snapshot = self.outcome.core_bridge.light_snapshot();
        let active_buffer = snapshot
            .buffers
            .iter()
            .find(|buffer| buffer.is_active)
            .cloned();
        let active_buffer_id = active_buffer
            .as_ref()
            .map(|buffer| buffer.id as u64)
            .unwrap_or(1);
        let current_line_range = self.outcome.core_bridge.buffer_line_range(
            active_buffer_id as i32,
            snapshot.cursor_row,
            1,
        );
        let snapshot = ReadonlyBufferSnapshot {
            id: active_buffer_id,
            path: self.session_state.target_path().cloned(),
            line_count: current_line_range
                .as_ref()
                .map(|range| range.total_line_count)
                .unwrap_or(1),
            cursor_row: snapshot.cursor_row,
            cursor_col: snapshot.cursor_col,
            current_line: current_line_range
                .and_then(|range| range.lines.into_iter().next())
                .unwrap_or_default(),
            text: String::new(),
        };
        log::debug!(
            "[PERF][main][runtime] built lightweight current buffer metadata snapshot: buffer_id={}, buffer_name={:?}, document_id={:?}, session_target_path={:?}, snapshot_path={:?}, cursor=({},{}), elapsed_ms={}",
            active_buffer_id,
            active_buffer.as_ref().map(|buffer| &buffer.name),
            active_buffer
                .as_ref()
                .and_then(|buffer| buffer.document_id.as_deref()),
            self.session_state.target_path(),
            snapshot.path,
            snapshot.cursor_row,
            snapshot.cursor_col,
            started_at.elapsed().as_millis()
        );
        snapshot
    }

    fn current_selection_snapshot(
        &mut self,
    ) -> Option<saya::runtime::live::ReadonlySelectionSnapshot> {
        let snapshot = self.outcome.core_bridge.light_snapshot();
        let active_buffer_id = snapshot
            .buffers
            .iter()
            .find(|buffer| buffer.is_active)
            .map(|buffer| buffer.id as u64)
            .unwrap_or(1);
        let selection = self.outcome.core_bridge.current_visual_selection()?;
        let line_count = selection
            .end_row
            .saturating_sub(selection.start_row)
            .saturating_add(1);
        let text = self
            .outcome
            .core_bridge
            .buffer_line_range(active_buffer_id as i32, selection.start_row, line_count)
            .map(|range| range.lines.join("\n"))
            .unwrap_or_default();
        let mode = match selection.mode {
            CoreMode::VisualLine => "visualLine",
            CoreMode::VisualBlock => "visualBlock",
            _ => "visual",
        }
        .to_string();
        Some(saya::runtime::live::ReadonlySelectionSnapshot {
            mode,
            start_line: selection.start_row,
            start_column: selection.start_col,
            end_line: selection.end_row,
            end_column: selection.end_col,
            text,
        })
    }

    fn current_window_snapshot(&mut self) -> ReadonlyWindowSnapshot {
        let snapshot = self.outcome.core_bridge.light_snapshot();
        let active_window_id = snapshot
            .active_window_id()
            .map(|window_id| window_id as u64)
            .expect("runtime current window should resolve from active window id");
        ReadonlyWindowSnapshot {
            id: active_window_id,
        }
    }

    fn open_float(
        &mut self,
        request: RuntimeFloatOpenRequest,
    ) -> Result<RuntimeFloatSnapshot, RuntimeCommandError> {
        execute_runtime_window_open_float(
            request,
            self.outcome,
            self.floating_window_manager.as_deref_mut(),
            self.terminal_float_manager.as_deref_mut(),
        )
    }

    fn close_float(&mut self, id: u64) -> Result<bool, RuntimeCommandError> {
        execute_runtime_window_close_float(
            id,
            self.floating_window_manager.as_deref_mut(),
            self.terminal_float_manager.as_deref_mut(),
        )
    }

    fn focus_float(&mut self, id: u64) -> Result<bool, RuntimeCommandError> {
        let manager = self.floating_window_manager.as_deref_mut().ok_or_else(|| {
            RuntimeCommandError::CommandFailed {
                name: "window.focus".to_string(),
                message: "floating window manager is not available".to_string(),
            }
        })?;
        Ok(manager.focus_float(FloatingWindowId(id)))
    }

    fn list_float_snapshots(&mut self) -> Result<Vec<RuntimeFloatSnapshot>, RuntimeCommandError> {
        let manager = self.floating_window_manager.as_deref().ok_or_else(|| {
            RuntimeCommandError::CommandFailed {
                name: "window.floats".to_string(),
                message: "floating window manager is not available".to_string(),
            }
        })?;
        Ok(runtime_float_snapshots(manager))
    }

    fn open_panel(
        &mut self,
        request: RuntimePanelOpenRequest,
    ) -> Result<RuntimePanelSnapshot, RuntimeCommandError> {
        execute_runtime_panel_open(
            request,
            self.panel_manager.as_deref_mut(),
            self.terminal_float_manager.as_deref_mut(),
        )
    }

    fn focus_panel(&mut self, id: String) -> Result<bool, RuntimeCommandError> {
        let manager = self.panel_manager.as_deref_mut().ok_or_else(|| {
            RuntimeCommandError::CommandFailed {
                name: "panel.focus".to_string(),
                message: "panel manager is not available".to_string(),
            }
        })?;
        Ok(manager.focus(&id))
    }

    fn unfocus_panel(&mut self) -> Result<bool, RuntimeCommandError> {
        let manager = self.panel_manager.as_deref_mut().ok_or_else(|| {
            RuntimeCommandError::CommandFailed {
                name: "panel.unfocus".to_string(),
                message: "panel manager is not available".to_string(),
            }
        })?;
        Ok(manager.unfocus())
    }

    fn close_panel(&mut self, id: String) -> Result<bool, RuntimeCommandError> {
        execute_runtime_panel_close(
            id,
            self.panel_manager.as_deref_mut(),
            self.terminal_float_manager.as_deref_mut(),
        )
    }

    fn list_panel_snapshots(&mut self) -> Result<Vec<RuntimePanelSnapshot>, RuntimeCommandError> {
        let manager =
            self.panel_manager
                .as_deref()
                .ok_or_else(|| RuntimeCommandError::CommandFailed {
                    name: "panel.list".to_string(),
                    message: "panel manager is not available".to_string(),
                })?;
        Ok(manager
            .snapshots()
            .into_iter()
            .map(runtime_panel_snapshot)
            .collect())
    }

    fn send_panel_text(&mut self, id: String, text: String) -> Result<bool, RuntimeCommandError> {
        let manager = self.panel_manager.as_deref_mut().ok_or_else(|| {
            RuntimeCommandError::CommandFailed {
                name: "panel.send".to_string(),
                message: "panel manager is not available".to_string(),
            }
        })?;
        let terminal_id =
            manager
                .send(&id, &text)
                .map_err(|message| RuntimeCommandError::CommandFailed {
                    name: "panel.send".to_string(),
                    message,
                })?;
        if let Some(terminal_id) = terminal_id {
            let terminal_manager = self.terminal_float_manager.as_deref_mut().ok_or_else(|| {
                RuntimeCommandError::CommandFailed {
                    name: "panel.send".to_string(),
                    message: "terminal manager is not available".to_string(),
                }
            })?;
            terminal_manager
                .write_bytes(terminal_id, text.as_bytes())
                .map_err(|error| RuntimeCommandError::CommandFailed {
                    name: "panel.send".to_string(),
                    message: format!("failed to send panel terminal input: {error:?}"),
                })?;
        }
        Ok(terminal_id.is_some())
    }

    fn current_editor_snapshot(&mut self) -> ReadonlyEditorSnapshot {
        let snapshot = self.outcome.core_bridge.light_snapshot();
        ReadonlyEditorSnapshot {
            mode: runtime_mode_from_core(snapshot.mode),
        }
    }

    fn current_filer_entry(
        &mut self,
    ) -> Result<Option<RuntimeFilerCurrentEntry>, saya::runtime::live::RuntimeFilerError> {
        let Some(directory_buffer) = self.session_state.directory_buffer() else {
            log::debug!(
                "[main][runtime] current filer entry requested outside directory buffer: target_path={:?}",
                self.session_state.target_path()
            );
            return Ok(None);
        };
        let snapshot = self.outcome.core_bridge.light_snapshot();
        let Some(entry) = self
            .session_state
            .current_directory_entry(snapshot.cursor_row)
        else {
            log::debug!(
                "[main][runtime] current filer entry missing for cursor row: root_path={}, cursor_row={}, entries={}",
                directory_buffer.root_path.display(),
                snapshot.cursor_row,
                directory_buffer.entries.len()
            );
            return Ok(None);
        };
        log::debug!(
            "[main][runtime] current filer entry resolved: root_path={}, cursor_row={}, entry_id={}, name={}, path={}, kind={:?}",
            directory_buffer.root_path.display(),
            snapshot.cursor_row,
            entry.id,
            entry.name,
            entry.path.display(),
            entry.kind
        );
        Ok(Some(RuntimeFilerCurrentEntry {
            id: entry.id,
            name: entry.name.clone(),
            path: entry.path.to_string_lossy().into_owned(),
            kind: runtime_filer_kind_from_directory_entry(entry.kind),
            root_path: directory_buffer.root_path.to_string_lossy().into_owned(),
            display_text: entry.display_text.clone(),
        }))
    }

    fn list_filer_entries(
        &mut self,
        path: std::path::PathBuf,
        options: RuntimeFilerListOptions,
    ) -> Result<Vec<RuntimeFilerEntry>, RuntimeFilerError> {
        log::debug!(
            "[main][runtime][filer] list requested through host session: path={}, show_hidden={}, sort_by={:?}, filter={:?}",
            path.display(),
            options.show_hidden,
            options.sort_by,
            options.filter
        );
        let listing_options = directory_buffer_listing_options_from_runtime(options);
        let entries = self
            .session_state
            .refresh_directory_buffer_listing(path.clone(), listing_options)
            .map_err(|error| RuntimeFilerError::ReadFailed {
                path: path.clone(),
                message: error.to_string(),
            })?;
        let display_text = self
            .session_state
            .directory_buffer()
            .map(|directory_buffer| directory_buffer.display_text.clone())
            .unwrap_or_default();
        self.outcome
            .core_bridge
            .replace_buffer_text(&display_text)
            .map_err(|error| RuntimeFilerError::ReadFailed {
                path: path.clone(),
                message: format!("failed to project filer listing into buffer: {error:?}"),
            })?;
        self.outcome.target_path = Some(path.clone());
        log::debug!(
            "[main][runtime][filer] projected directory listing into active buffer: path={}, entries={}, text_len={}",
            path.display(),
            entries.len(),
            display_text.len()
        );
        Ok(directory_entries_for_runtime_entries(entries))
    }

    fn execute_filer_operation(
        &mut self,
        operation: RuntimeFilerOperation,
    ) -> Result<RuntimeFilerOperationReport, RuntimeFilerError> {
        execute_runtime_filer_operation(operation, self.outcome, self.session_state)
    }

    fn execute_host_command(
        &mut self,
        name: &str,
    ) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
        log::info!(
            "[main][host_command] executing runtime host command through application session owner: {}",
            name
        );
        execute_runtime_host_command_with_floats(
            name,
            self.outcome,
            self.session_state,
            self.floating_window_manager.as_deref_mut(),
            self.completion_float_manager.as_deref_mut(),
            self.lsp_diagnostic_store.as_deref_mut(),
            self.terminal_float_manager.as_deref_mut(),
        )
    }

    fn request_input_prompt(
        &mut self,
        request: RuntimeInputPromptRequest,
    ) -> Result<RuntimeInputPromptHostResponse, RuntimeCommandError> {
        let Some(slot) = self.runtime_input_prompt.as_deref_mut() else {
            log::debug!(
                "[main][runtime_input] prompt requested without active TUI prompt slot: title={}",
                request.title
            );
            return Ok(RuntimeInputPromptHostResponse::Completed(
                RuntimeInputPromptResponse::Cancelled,
            ));
        };
        if slot.is_some() {
            return Err(RuntimeCommandError::CommandFailed {
                name: "input.prompt".to_string(),
                message: "another runtime input prompt is already active".to_string(),
            });
        }
        log::info!(
            "[main][runtime_input] prompt start: title={}, placeholder_present={}",
            request.title,
            request.placeholder.is_some()
        );
        *slot = Some(RuntimeInputPromptUiState::new(request));
        Ok(RuntimeInputPromptHostResponse::Pending)
    }

    fn execute_lsif_request(
        &mut self,
        request: LspRuntimeBridgeRequest,
    ) -> Result<LspRuntimeBridgeResponse, RuntimeCommandError> {
        let Some(bridge) = self.lsif_bridge else {
            return Err(RuntimeCommandError::CommandFailed {
                name: "lsif.request".to_string(),
                message: format!(
                    "LSIF bridge is not configured for runtime method {}",
                    request.method
                ),
            });
        };
        log::info!(
            "[main][lsif] executing runtime LSIF request through index cache: method={}, language={}, document={}",
            request.method,
            request.language_id,
            request
                .text_document
                .as_ref()
                .map(|document| document.uri.as_str())
                .unwrap_or("<none>")
        );
        bridge
            .cache
            .lock()
            .map_err(|_| RuntimeCommandError::CommandFailed {
                name: "lsif.request".to_string(),
                message: "LSIF index cache mutex poisoned".to_string(),
            })
            .and_then(|mut cache| cache.execute_request(request, &bridge.diagnostic_events))
    }

    fn show_completion(
        &mut self,
        request: CompletionShowRequest,
    ) -> Result<bool, RuntimeCommandError> {
        let floating_window_manager =
            self.floating_window_manager.as_deref_mut().ok_or_else(|| {
                RuntimeCommandError::CommandFailed {
                    name: "completion.show".to_string(),
                    message: "floating window manager is not available".to_string(),
                }
            })?;
        let completion_manager = self
            .completion_float_manager
            .as_deref_mut()
            .ok_or_else(|| RuntimeCommandError::CommandFailed {
                name: "completion.show".to_string(),
                message: "completion manager is not available".to_string(),
            })?;
        let snapshot = self.outcome.core_bridge.light_snapshot();
        let window_id = snapshot.active_window_id().unwrap_or(1);
        let shown = completion_manager.show_typed(
            floating_window_manager,
            window_id,
            snapshot.cursor_row,
            snapshot.cursor_col,
            request,
        );
        log::debug!(
            "[main][completion] typed completion show applied: window_id={}, cursor=({},{}), shown={}",
            window_id,
            snapshot.cursor_row,
            snapshot.cursor_col,
            shown
        );
        Ok(shown)
    }

    fn close_completion(&mut self) -> Result<bool, RuntimeCommandError> {
        let floating_window_manager =
            self.floating_window_manager.as_deref_mut().ok_or_else(|| {
                RuntimeCommandError::CommandFailed {
                    name: "completion.close".to_string(),
                    message: "floating window manager is not available".to_string(),
                }
            })?;
        let completion_manager = self
            .completion_float_manager
            .as_deref_mut()
            .ok_or_else(|| RuntimeCommandError::CommandFailed {
                name: "completion.close".to_string(),
                message: "completion manager is not available".to_string(),
            })?;
        let snapshot = self.outcome.core_bridge.light_snapshot();
        let restore_window_id = snapshot.active_window_id();
        let closed = completion_manager.close(floating_window_manager, restore_window_id);
        log::debug!(
            "[main][completion] typed completion close applied: restore_window_id={:?}, closed={}",
            restore_window_id,
            closed
        );
        Ok(closed)
    }
}

#[derive(Clone, Default)]
pub struct LsifBridgeHandle {
    cache: Arc<std::sync::Mutex<LsifIndexCache>>,
    diagnostic_events: Arc<std::sync::Mutex<Vec<String>>>,
}

fn directory_buffer_listing_options_from_runtime(
    options: RuntimeFilerListOptions,
) -> DirectoryBufferListingOptions {
    DirectoryBufferListingOptions {
        show_hidden: options.show_hidden,
        sort_by: match options.sort_by {
            RuntimeFilerSortKey::Name => DirectoryBufferSortKey::Name,
            RuntimeFilerSortKey::Kind => DirectoryBufferSortKey::Kind,
            RuntimeFilerSortKey::ModifiedTime => DirectoryBufferSortKey::ModifiedTime,
            RuntimeFilerSortKey::Size => DirectoryBufferSortKey::Size,
        },
        filter: options.filter,
    }
}

#[derive(Debug, Clone)]
struct RuntimeInputPromptUiState {
    request: RuntimeInputPromptRequest,
    edit: CommandLineEdit,
    correlation_id: u64,
}

impl RuntimeInputPromptUiState {
    fn new(request: RuntimeInputPromptRequest) -> Self {
        Self {
            request,
            edit: CommandLineEdit::default(),
            correlation_id: 0,
        }
    }

    fn view(&self) -> InputPromptView {
        InputPromptView {
            prompt: format!("{}:", self.request.title),
            input: self.edit.buffer().to_string(),
            correlation_id: self.correlation_id,
            input_kind: CoreInputRequestKind::CommandLine,
            status: InputPromptStatus::Active,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RuntimeInputPromptKeyAction {
    Editing,
    Submit(String),
    Cancel,
}

fn handle_runtime_input_prompt_key(
    state: Option<&mut RuntimeInputPromptUiState>,
    key: &KeyInput,
) -> Option<RuntimeInputPromptKeyAction> {
    let state = state?;
    match key {
        KeyInput::Enter => Some(RuntimeInputPromptKeyAction::Submit(
            state.edit.buffer().to_string(),
        )),
        KeyInput::Escape | KeyInput::Ctrl('c') | KeyInput::Ctrl('C') => {
            Some(RuntimeInputPromptKeyAction::Cancel)
        }
        KeyInput::Char(ch) => {
            state.edit.insert_char(*ch);
            Some(RuntimeInputPromptKeyAction::Editing)
        }
        _ => {
            if let Some(action) = command_line_edit_action_for_key(key) {
                state.edit.apply_action(action);
            }
            Some(RuntimeInputPromptKeyAction::Editing)
        }
    }
}

fn execute_runtime_filer_operation(
    operation: RuntimeFilerOperation,
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    session_state: &mut saya::app::session::EditorSessionState,
) -> Result<RuntimeFilerOperationReport, RuntimeFilerError> {
    let refresh_path = runtime_filer_operation_refresh_path(&operation).or_else(|| {
        matches!(
            operation,
            RuntimeFilerOperation::BulkDelete { confirm: true, .. }
        )
        .then(|| session_state.target_path().cloned())
        .flatten()
    });
    log::info!(
        "[main][runtime][filer] executing host-mediated filer operation: operation={:?}, refresh_path={:?}",
        operation,
        refresh_path
    );

    let operation_for_refresh = operation.clone();
    let report = match operation {
        RuntimeFilerOperation::Mark { path } => {
            let entry = find_directory_entry_by_path(session_state, &path).ok_or_else(|| {
                RuntimeFilerError::OperationFailed {
                    operation: RuntimeFilerOperationKind::Mark,
                    path: path.clone(),
                    target_path: None,
                    kind: RuntimeFilerErrorKind::NotFound,
                    message: "mark target is not in the active directory buffer".to_string(),
                }
            })?;
            session_state.mark_directory_entry(&entry);
            RuntimeFilerOperationReport {
                operation: RuntimeFilerOperationKind::Mark,
                path: path.to_string_lossy().into_owned(),
                target_path: None,
                entries: directory_entries_for_runtime(session_state.marked_directory_entries()),
                preview_id: None,
            }
        }
        RuntimeFilerOperation::Unmark { path } => {
            let entry = find_directory_entry_by_path(session_state, &path).ok_or_else(|| {
                RuntimeFilerError::OperationFailed {
                    operation: RuntimeFilerOperationKind::Unmark,
                    path: path.clone(),
                    target_path: None,
                    kind: RuntimeFilerErrorKind::NotFound,
                    message: "unmark target is not in the active directory buffer".to_string(),
                }
            })?;
            session_state.unmark_directory_entry(&entry);
            RuntimeFilerOperationReport {
                operation: RuntimeFilerOperationKind::Unmark,
                path: path.to_string_lossy().into_owned(),
                target_path: None,
                entries: directory_entries_for_runtime(session_state.marked_directory_entries()),
                preview_id: None,
            }
        }
        RuntimeFilerOperation::ClearMarks => {
            session_state.clear_directory_marks();
            RuntimeFilerOperationReport {
                operation: RuntimeFilerOperationKind::ClearMarks,
                path: session_state
                    .target_path()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                target_path: None,
                entries: Vec::new(),
                preview_id: None,
            }
        }
        RuntimeFilerOperation::BulkDeletePreview => {
            let entries = session_state.marked_directory_entries();
            let preview_id = runtime_filer_bulk_delete_preview_id(&entries);
            log::info!(
                "[main][runtime][filer] prepared bulk delete preview: preview_id={}, entry_count={}",
                preview_id,
                entries.len()
            );
            RuntimeFilerOperationReport {
                operation: RuntimeFilerOperationKind::BulkDeletePreview,
                path: session_state
                    .target_path()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                target_path: None,
                entries: directory_entries_for_runtime(entries),
                preview_id: Some(preview_id),
            }
        }
        RuntimeFilerOperation::BulkDelete {
            preview_id,
            confirm,
        } => {
            let entries = session_state.marked_directory_entries();
            let expected_preview_id = runtime_filer_bulk_delete_preview_id(&entries);
            if !confirm || preview_id.is_empty() || preview_id != expected_preview_id {
                return Err(RuntimeFilerError::OperationFailed {
                    operation: RuntimeFilerOperationKind::BulkDelete,
                    path: session_state.target_path().cloned().unwrap_or_default(),
                    target_path: None,
                    kind: RuntimeFilerErrorKind::ConfirmationRequired,
                    message: format!(
                        "bulk delete requires explicit confirmation with the latest preview id: expected={expected_preview_id}, actual={preview_id}"
                    ),
                });
            }
            log::info!(
                "[main][runtime][filer] executing confirmed bulk delete: preview_id={}, entry_count={}",
                preview_id,
                entries.len()
            );
            for entry in &entries {
                saya::runtime::live::execute_local_filer_operation(
                    RuntimeFilerOperation::Delete {
                        path: entry.path.clone(),
                        confirm: true,
                        recursive: false,
                        trash: false,
                    },
                )?;
            }
            session_state.clear_directory_marks();
            RuntimeFilerOperationReport {
                operation: RuntimeFilerOperationKind::BulkDelete,
                path: session_state
                    .target_path()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                target_path: None,
                entries: directory_entries_for_runtime(entries),
                preview_id: Some(preview_id),
            }
        }
        operation => {
            let report = saya::runtime::live::execute_local_filer_operation(operation)?;
            if let RuntimeFilerOperation::Rename { from, to }
            | RuntimeFilerOperation::Move { from, to } = &operation_for_refresh
            {
                session_state.record_directory_entry_rename(from, to);
            }
            report
        }
    };

    if let Some(refresh_path) = refresh_path.filter(|path| path.is_dir()) {
        let previous_row = outcome.core_bridge.light_snapshot().cursor_row;
        log::debug!(
            "[main][runtime][filer] refreshing directory buffer after operation: path={}, previous_cursor_row={}",
            refresh_path.display(),
            previous_row
        );
        session_state
            .refresh_directory_buffer_for_target_path(&refresh_path)
            .map_err(|error| RuntimeFilerError::OperationFailed {
                operation: report.operation,
                path: std::path::PathBuf::from(report.path.clone()),
                target_path: report.target_path.clone().map(std::path::PathBuf::from),
                kind: RuntimeFilerErrorKind::Io,
                message: format!("failed to refresh directory buffer: {error:?}"),
            })?;
        if let Some(directory_buffer) =
            session_state.directory_buffer().filter(|directory_buffer| {
                paths_refer_to_same_location_main(&directory_buffer.root_path, &refresh_path)
            })
        {
            outcome
                .core_bridge
                .replace_buffer_text(&directory_buffer.display_text)
                .map_err(|error| RuntimeFilerError::OperationFailed {
                    operation: report.operation,
                    path: std::path::PathBuf::from(report.path.clone()),
                    target_path: report.target_path.clone().map(std::path::PathBuf::from),
                    kind: RuntimeFilerErrorKind::Io,
                    message: format!("failed to project refreshed directory buffer: {error:?}"),
                })?;
        }
        let refreshed = outcome.core_bridge.light_snapshot();
        let refreshed_line_count = refreshed
            .active_window()
            .and_then(|window| {
                outcome.core_bridge.buffer_line_range(
                    window.buf_id,
                    window.topline.saturating_sub(1),
                    1,
                )
            })
            .map(|range| range.total_line_count)
            .unwrap_or(1);
        log::debug!(
            "[main][runtime][filer] refreshed directory buffer after operation: path={}, cursor_row_before={}, cursor_row_after={}, line_count={}",
            refresh_path.display(),
            previous_row,
            refreshed.cursor_row,
            refreshed_line_count
        );
    }

    Ok(report)
}

fn find_directory_entry_by_path(
    session_state: &saya::app::session::EditorSessionState,
    path: &std::path::Path,
) -> Option<saya::app::session::DirectoryBufferEntry> {
    session_state
        .directory_buffer()?
        .entries
        .iter()
        .find(|entry| entry.path == path)
        .cloned()
}

fn directory_entries_for_runtime(
    entries: Vec<saya::app::session::DirectoryBufferEntry>,
) -> Vec<RuntimeFilerCurrentEntry> {
    entries
        .into_iter()
        .map(|entry| RuntimeFilerCurrentEntry {
            id: entry.id,
            name: entry.name,
            path: entry.path.to_string_lossy().into_owned(),
            kind: runtime_filer_kind_from_directory_entry(entry.kind),
            root_path: entry
                .path
                .parent()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default(),
            display_text: entry.display_text,
        })
        .collect()
}

fn directory_entries_for_runtime_entries(
    entries: Vec<saya::app::session::DirectoryBufferEntry>,
) -> Vec<RuntimeFilerEntry> {
    entries
        .into_iter()
        .map(|entry| RuntimeFilerEntry {
            name: entry.name,
            path: entry.path.to_string_lossy().into_owned(),
            kind: runtime_filer_kind_from_directory_entry(entry.kind),
            display_text: entry.display_text,
            size: entry.size,
            modified_time_ms: entry.modified_time_ms,
        })
        .collect()
}

fn runtime_filer_bulk_delete_preview_id(
    entries: &[saya::app::session::DirectoryBufferEntry],
) -> String {
    let mut hasher = DefaultHasher::new();
    for entry in entries {
        entry.id.hash(&mut hasher);
        entry.path.hash(&mut hasher);
        entry.kind.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

fn runtime_filer_operation_refresh_path(
    operation: &RuntimeFilerOperation,
) -> Option<std::path::PathBuf> {
    match operation {
        RuntimeFilerOperation::CreateFile { path }
        | RuntimeFilerOperation::CreateDirectory { path }
        | RuntimeFilerOperation::Delete { path, .. } => {
            path.parent().map(|path| path.to_path_buf())
        }
        RuntimeFilerOperation::Rename { from, .. }
        | RuntimeFilerOperation::Copy { from, .. }
        | RuntimeFilerOperation::Move { from, .. } => from.parent().map(|path| path.to_path_buf()),
        RuntimeFilerOperation::Mark { .. }
        | RuntimeFilerOperation::Unmark { .. }
        | RuntimeFilerOperation::ClearMarks
        | RuntimeFilerOperation::BulkDeletePreview
        | RuntimeFilerOperation::BulkDelete { .. } => None,
    }
}

fn runtime_filer_kind_from_directory_entry(
    kind: saya::app::session::DirectoryBufferEntryKind,
) -> RuntimeFilerEntryKind {
    match kind {
        saya::app::session::DirectoryBufferEntryKind::Directory => RuntimeFilerEntryKind::Directory,
        saya::app::session::DirectoryBufferEntryKind::File => RuntimeFilerEntryKind::File,
        saya::app::session::DirectoryBufferEntryKind::Symlink => RuntimeFilerEntryKind::Symlink,
        saya::app::session::DirectoryBufferEntryKind::Other => RuntimeFilerEntryKind::Other,
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

fn clear_stale_quit_warning_after_write_attempt(
    system_warning: &mut Option<String>,
    write_message: Option<&str>,
) {
    if write_message.is_some() && system_warning.as_deref() == Some(normal_quit_warning_message()) {
        *system_warning = None;
    }
}

fn current_terminal_size() -> (u16, u16) {
    crossterm::terminal::size().unwrap_or((80, 24))
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

fn terminal_display_invalidated_redraw_plan() -> RedrawPlan {
    RedrawPlan {
        requested: true,
        full: true,
        clear_before_draw: true,
        required_by_structure_change: false,
        source: RedrawPlanSource::TerminalDisplayInvalidation,
        coalesced_count: 0,
    }
}

fn effective_workspace_redraw_plan(
    structural_refresh: Option<&StructuralRefreshOutcome>,
    terminal_display_redraw_plan: Option<&RedrawPlan>,
) -> RedrawPlan {
    let mut plan = structural_refresh
        .map(|refresh| refresh.redraw_plan.clone())
        .unwrap_or_default();
    let Some(terminal_plan) = terminal_display_redraw_plan else {
        return plan;
    };

    plan.requested |= terminal_plan.requested;
    plan.full |= terminal_plan.full;
    plan.clear_before_draw |= terminal_plan.clear_before_draw;
    plan.required_by_structure_change |= terminal_plan.required_by_structure_change;
    plan.coalesced_count += terminal_plan.coalesced_count;
    plan.source = if terminal_plan.requested {
        RedrawPlanSource::TerminalDisplayInvalidation
    } else {
        plan.source
    };
    plan
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
                    message_line: saya::presentation::overlay::effect::merge_presentation_message_line(
                        &last_successful.message_line,
                        [saya::core::notification_prompt::MessageLineCandidate::legacy(
                            saya::core::notification_prompt::MessageLineSource::RenderProjectionError,
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
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    session_state: &mut EditorSessionState,
    viewport_store: &mut WindowViewportStore,
    viewport_sync_mode: ViewportSyncMode,
    search_refresh_store: &mut WindowSearchRefreshStore,
    markdown_metadata_cache: &mut MarkdownMetadataCache,
    command_line_prompt: Option<char>,
    command_line_buffer: &str,
    command_line_cursor_byte_index: usize,
    projection_frame: Option<&ProjectionFrame>,
    runtime_input_prompt: Option<&RuntimeInputPromptUiState>,
    mut structural_refresh: Option<&mut StructuralRefreshOutcome>,
    system_warning: Option<&str>,
    transient_msg: Option<&str>,
    terminal_width: u16,
    terminal_height: u16,
    floating_window_manager: Option<&mut FloatingWindowManager>,
    panel_manager: Option<&mut PanelManager>,
    mut terminal_float_manager: Option<&mut TerminalFloatManager>,
    selector_tui_projection_sink: Option<Arc<SelectorTuiProjectionSink>>,
) -> Result<WorkspaceScreenModel, WorkspaceRedrawError> {
    let total_started_at = std::time::Instant::now();
    let snapshot_started_at = std::time::Instant::now();
    let light_snapshot = outcome.core_bridge.light_snapshot();
    let snapshot = snapshot_from_light_snapshot(&light_snapshot, String::new());
    let snapshot_ms = snapshot_started_at.elapsed().as_millis();
    if let Some(active_window) = snapshot.active_window() {
        let active_buffer = snapshot
            .buffers
            .iter()
            .find(|buffer| buffer.id == active_window.buf_id);
        log::debug!(
            "[main][render] active buffer identity: window_id={}, buffer_id={}, buffer_name={:?}, document_id={:?}, session_target_path={:?}, syntax_enabled={}",
            active_window.id,
            active_window.buf_id,
            active_buffer.map(|buffer| &buffer.name),
            active_buffer.and_then(|buffer| buffer.document_id.as_deref()),
            session_state.target_path(),
            outcome.core_bridge.is_syntax_enabled()
        );
    } else {
        log::debug!(
            "[main][render] no active window while building workspace render output: session_target_path={:?}",
            session_state.target_path()
        );
    }
    trace_redraw_diagnostic(format_args!(
        "workspace render build started: revision={}, mode={:?}, cursor=({},{}), windows={}, command_prompt={:?}, command_buffer_len={}, structural_refresh_present={}",
        snapshot.revision,
        snapshot.mode,
        snapshot.cursor_row,
        snapshot.cursor_col,
        snapshot.windows.len(),
        command_line_prompt,
        command_line_buffer.len(),
        structural_refresh.is_some()
    ));
    if let Some(refresh) = structural_refresh.as_deref() {
        trace_redraw_diagnostic(format_args!(
            "workspace render using structural refresh: redraw_requested={}, full={}, clear_before_draw={}, source={:?}, coalesced_count={}, invalidated_buffers={:?}, invalidated_windows={:?}, layout_dirty={}",
            refresh.redraw_plan.requested,
            refresh.redraw_plan.full,
            refresh.redraw_plan.clear_before_draw,
            refresh.redraw_plan.source,
            refresh.redraw_plan.coalesced_count,
            refresh.invalidation.buffer_ids,
            refresh.invalidation.window_ids,
            refresh.invalidation.layout_dirty
        ));
    }
    let visual_started_at = std::time::Instant::now();
    let visual_selection = if matches!(
        snapshot.mode,
        CoreMode::Visual | CoreMode::VisualLine | CoreMode::VisualBlock
    ) {
        outcome.core_bridge.current_visual_selection()
    } else {
        None
    };
    let visual_ms = visual_started_at.elapsed().as_millis();
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
    let buffer_line_counts = collect_workspace_buffer_line_counts(&outcome.core_bridge, &snapshot);
    let active_markdown_preview_source = snapshot.active_window().and_then(|window| {
        let line_count = buffer_line_counts.get(&window.buf_id).copied().unwrap_or(0);
        outcome
            .core_bridge
            .buffer_line_range(window.buf_id, 0, line_count)
            .map(|range| range.lines.join("\n"))
    });
    let viewport_summary = viewport_store.sync_from_windows_for_render(
        &snapshot.windows,
        &invalidated_windows,
        &buffer_line_counts,
        viewport_sync_mode,
    );
    if let Some(refresh) = structural_refresh.as_deref_mut() {
        *refresh = refresh
            .clone()
            .with_viewport_sync_summary(&viewport_summary);
    }
    let line_range_started_at = std::time::Instant::now();
    let mut line_ranges =
        collect_workspace_line_ranges(&outcome.core_bridge, &snapshot, viewport_store);
    let line_range_ms = line_range_started_at.elapsed().as_millis();
    search_refresh_store.retain_windows(
        &snapshot
            .windows
            .iter()
            .map(|window| window.id)
            .collect::<Vec<_>>(),
    );
    let search_started_at = std::time::Instant::now();
    let substitute_preview_search_states = collect_workspace_substitute_preview_states(
        &snapshot,
        &mut line_ranges,
        command_line_prompt,
        command_line_buffer,
    );
    let search_states = if substitute_preview_search_states.is_empty() {
        collect_workspace_search_states(
            &mut outcome.core_bridge,
            &snapshot,
            viewport_store,
            search_refresh_store,
            resolve_prompt_revision(command_line_prompt, command_line_buffer),
            resolve_search_mode_hint(command_line_prompt, command_line_buffer),
        )?
    } else {
        substitute_preview_search_states
    };
    let search_ms = search_started_at.elapsed().as_millis();
    let syntax_enabled = outcome.core_bridge.is_syntax_enabled();
    let syntax_started_at = std::time::Instant::now();
    let syntax_lines = if syntax_enabled {
        collect_workspace_syntax_lines(
            &outcome.core_bridge,
            &snapshot,
            viewport_store,
            &line_ranges,
        )
    } else {
        trace_redraw_diagnostic(format_args!(
            "workspace syntax collection skipped because :syntax is off"
        ));
        BTreeMap::new()
    };
    let syntax_ms = syntax_started_at.elapsed().as_millis();
    #[cfg(feature = "tree-sitter-syntax")]
    let tree_sitter_started_at = std::time::Instant::now();
    #[cfg(feature = "tree-sitter-syntax")]
    let tree_sitter_syntax = if syntax_enabled {
        collect_workspace_tree_sitter_syntax(
            &mut outcome.core_bridge,
            &snapshot,
            viewport_store,
            &line_ranges,
        )
    } else {
        trace_redraw_diagnostic(format_args!(
            "workspace Tree-sitter syntax collection skipped because :syntax is off"
        ));
        BTreeMap::new()
    };
    #[cfg(feature = "tree-sitter-syntax")]
    let tree_sitter_ms = tree_sitter_started_at.elapsed().as_millis();
    let markdown_started_at = std::time::Instant::now();
    let markdown_document_maps = collect_workspace_markdown_document_maps(
        markdown_metadata_cache,
        session_state,
        &outcome.core_bridge,
        &snapshot,
    );
    let markdown_ms = markdown_started_at.elapsed().as_millis();
    let command_preview =
        command_line_prompt.map(|prompt| format!("{}{}", prompt, command_line_buffer));
    let command_preview_cursor_col = command_line_prompt.map(|prompt| {
        command_line_cursor_display_col(
            prompt,
            command_line_buffer,
            command_line_cursor_byte_index,
            session_state.tab_size(),
        )
    });
    let mut notification_prompt = projection_frame.map(ProjectionFrame::workspace_view);
    if let Some(runtime_prompt) = runtime_input_prompt {
        let view = runtime_prompt.view();
        log::debug!(
            "[main][runtime_input] projecting active runtime prompt: title={}, input_len={}",
            runtime_prompt.request.title,
            view.input.len()
        );
        notification_prompt
            .get_or_insert_with(Default::default)
            .input_prompt = Some(view);
    }

    let projection_started_at = std::time::Instant::now();
    let mut projection_result = project_workspace(&WorkspaceProjectionInput {
        snapshot: &snapshot,
        light_snapshot: Some(&light_snapshot),
        line_ranges: &line_ranges,
        session_state,
        visual_selection: visual_selection.as_ref(),
        search_states: &search_states,
        syntax_lines: &syntax_lines,
        #[cfg(feature = "tree-sitter-syntax")]
        tree_sitter_syntax: &tree_sitter_syntax,
        markdown_document_maps: &markdown_document_maps,
        command_preview: command_preview.as_deref(),
        core_message: None,
        notification_prompt: notification_prompt.as_ref(),
        system_warning,
        transient_info: transient_msg,
        viewport_store,
        terminal_width,
        terminal_height,
    });
    let projection_ms = projection_started_at.elapsed().as_millis();
    log::debug!(
        "[PERF][main] build_workspace_render_output text_len={} windows={} snapshot_ms={} line_range_ms={} line_ranges={} visible_text_bytes={} visual_ms={} search_ms={} syntax_ms={} tree_sitter_ms={} markdown_ms={} projection_ms={} total_ms={}",
        snapshot.text.len(),
        snapshot.windows.len(),
        snapshot_ms,
        line_range_ms,
        line_ranges.len(),
        line_ranges
            .values()
            .map(|range| range.lines.iter().map(String::len).sum::<usize>())
            .sum::<usize>(),
        visual_ms,
        search_ms,
        syntax_ms,
        {
            #[cfg(feature = "tree-sitter-syntax")]
            {
                tree_sitter_ms
            }
            #[cfg(not(feature = "tree-sitter-syntax"))]
            {
                0
            }
        },
        markdown_ms,
        projection_ms,
        total_started_at.elapsed().as_millis()
    );
    if let Ok(workspace) = projection_result.as_mut() {
        sync_workspace_message_pager(session_state, workspace, terminal_width);
        if let Some(cursor_col) = command_preview_cursor_col
            && let Some(command_line) = workspace.command_line.as_mut()
        {
            command_line.cursor_col = cursor_col;
        }
        if let Some(manager) = floating_window_manager {
            refresh_buffer_backed_float_lines(manager, &outcome.core_bridge, &light_snapshot);
            if let Some(terminal_manager) = terminal_float_manager.as_deref_mut() {
                refresh_terminal_float_lines(manager, terminal_manager);
            }
            apply_workspace_floating_window_models(
                workspace,
                terminal_width,
                terminal_height,
                manager,
            );
        }
        if let Some(panel_manager) = panel_manager {
            if let Some(terminal_manager) = terminal_float_manager.as_deref_mut() {
                refresh_terminal_panel_lines(
                    panel_manager,
                    terminal_manager,
                    terminal_width,
                    terminal_height,
                );
            }
            let panel_floats =
                panel_manager.resolve_floating_screen_models(terminal_width, terminal_height);
            log::trace!(
                "[main][panel] applied workspace panels: panels={}, terminal=({},{})",
                panel_floats.len(),
                terminal_width,
                terminal_height
            );
            workspace.floats.extend(panel_floats);
        }
        if let Some(sink) = selector_tui_projection_sink
            && let Some(selector_model) = sink.current_model()
        {
            if let Some(selector_float) = sink.workspace_float(terminal_width, terminal_height) {
                log::debug!(
                    "[main][selector] appended selector TUI float to workspace model: session_id={}, rows={}, floats_before={}",
                    selector_model.session_id,
                    selector_model.visible_rows.len(),
                    workspace.floats.len()
                );
                workspace.floats.push(selector_float);
            } else {
                log::debug!(
                    "[main][selector] selector TUI model not visible in workspace render: session_id={}, intent={:?}, hidden={}, cancelled={}",
                    selector_model.session_id,
                    selector_model.intent,
                    selector_model.hidden,
                    selector_model.cancelled
                );
            }
        }
        append_active_mermaid_preview_float(
            workspace,
            &snapshot,
            active_markdown_preview_source
                .as_deref()
                .unwrap_or_default(),
            &markdown_document_maps,
            terminal_width,
            terminal_height,
            session_state,
        );
    }

    match projection_result {
        Ok(workspace) => {
            let projection_summary = workspace.projection_summary();
            trace_redraw_diagnostic(format_args!(
                "workspace render build succeeded: panes={}, active_window_id={}, visible_message={:?}, command_line_active={}, search_overlay_counts={:?}, syntax_chunk_counts={:?}",
                workspace.panes.len(),
                workspace.active_window_id,
                workspace.visible_message_text(),
                workspace.command_line.is_some(),
                workspace
                    .panes
                    .iter()
                    .map(|pane| (pane.window_id, pane.search_overlays.len()))
                    .collect::<Vec<_>>(),
                workspace
                    .panes
                    .iter()
                    .map(|pane| (pane.window_id, pane.syntax_chunks.len()))
                    .collect::<Vec<_>>()
            ));
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
            trace_redraw_diagnostic(format_args!(
                "workspace render build failed: revision={}, cursor=({},{}), error={}",
                snapshot.revision, snapshot.cursor_row, snapshot.cursor_col, error
            ));
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

fn append_active_mermaid_preview_float(
    workspace: &mut WorkspaceScreenModel,
    snapshot: &CoreSnapshot,
    source_text: &str,
    markdown_document_maps: &BTreeMap<i32, Arc<MarkdownDocumentMap>>,
    terminal_width: u16,
    terminal_height: u16,
    session_state: &mut EditorSessionState,
) {
    let auto_enabled = session_state.mermaid_preview_auto();
    let manual_active = session_state.mermaid_preview_manual_active();
    let preview_closed = session_state.mermaid_preview_closed();
    if !auto_enabled && !manual_active {
        log::debug!(
            "[main][markdown_preview] skip Mermaid preview because auto preview is disabled and no manual request is pending"
        );
        return;
    }
    if snapshot.mode != CoreMode::Normal {
        log::debug!(
            "[main][markdown_preview] skip Mermaid preview because mode is not Normal: mode={:?}, auto_enabled={}, manual_active={}",
            snapshot.mode,
            auto_enabled,
            manual_active
        );
        if manual_active {
            session_state.clear_mermaid_preview_manual("mode_not_normal");
        }
        return;
    }
    let Some(active_window) = snapshot.active_window() else {
        log::debug!(
            "[main][markdown_preview] skip Mermaid preview because active window is absent"
        );
        if manual_active {
            session_state.clear_mermaid_preview_manual("active_window_absent");
        }
        return;
    };
    let Some(map) = markdown_document_maps.get(&active_window.id) else {
        log::debug!(
            "[main][markdown_preview] skip Mermaid preview because markdown map is absent: window_id={}",
            active_window.id
        );
        if manual_active {
            session_state.clear_mermaid_preview_manual("markdown_map_absent");
        }
        return;
    };
    let Some(block) = map.blocks.iter().find(|block| {
        matches!(
            block.kind,
            MarkdownBlockKind::FencedCodeBlock { ref info, .. }
                if info
                    .as_deref()
                    .and_then(|info| info.split_whitespace().next())
                    .is_some_and(|language| language.eq_ignore_ascii_case("mermaid"))
        ) && (block.range.start.line..=block.range.end.line).contains(&active_window.cursor_row)
    }) else {
        log::debug!(
            "[main][markdown_preview] skip Mermaid preview because cursor is outside Mermaid blocks: window_id={}, cursor_row={}",
            active_window.id,
            active_window.cursor_row
        );
        session_state.reopen_mermaid_preview_if_closed("cursor_outside_mermaid_block");
        if manual_active {
            session_state.clear_mermaid_preview_manual("cursor_outside_mermaid_block");
        }
        return;
    };
    if preview_closed && !manual_active {
        log::debug!(
            "[main][markdown_preview] skip Mermaid preview because preview was closed for current block: window_id={}, cursor_row={}",
            active_window.id,
            active_window.cursor_row
        );
        return;
    }
    let source_lines = source_text.lines().collect::<Vec<_>>();
    if source_lines.get(block.range.end.line).is_none() {
        log::debug!(
            "[main][markdown_preview] skip Mermaid preview because block source is partial: window_id={}, start_row={}, end_row={}, available_lines={}",
            active_window.id,
            block.range.start.line,
            block.range.end.line,
            source_lines.len()
        );
        if manual_active {
            session_state.clear_mermaid_preview_manual("partial_block_source");
        }
        return;
    }
    let body = source_lines
        .iter()
        .take(block.range.end.line)
        .skip(block.range.start.line + 1)
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    if body.trim().is_empty() {
        log::debug!(
            "[main][markdown_preview] skip Mermaid preview because body is empty: window_id={}, start_row={}",
            active_window.id,
            block.range.start.line
        );
        if manual_active {
            session_state.clear_mermaid_preview_manual("empty_mermaid_body");
        }
        return;
    }
    let Some(active_pane) = workspace
        .panes
        .iter()
        .find(|pane| pane.window_id == active_window.id)
    else {
        log::debug!(
            "[main][markdown_preview] skip Mermaid preview because active pane is absent: window_id={}",
            active_window.id
        );
        if manual_active {
            session_state.clear_mermaid_preview_manual("active_pane_absent");
        }
        return;
    };
    let width = mermaid_preview_float_dimension(
        terminal_width,
        session_state.mermaid_preview_width_percent(),
        100,
        40,
        u16::MAX,
        2,
    );
    let height = mermaid_preview_float_dimension(
        terminal_height,
        session_state.mermaid_preview_height_percent(),
        100,
        12,
        u16::MAX,
        2,
    );
    let x = terminal_width.saturating_sub(width).saturating_sub(1);
    let y = active_pane
        .rect
        .y
        .saturating_add(1)
        .min(terminal_height.saturating_sub(height).saturating_sub(1));
    let content_width = width.saturating_sub(2).max(1);
    let content_height = height.saturating_sub(2).max(1);
    let float_id = FloatingWindowId(9_000_000_000u64.saturating_add(active_window.id as u64));
    let preview_view = session_state.mermaid_preview_view();
    let image_view = match preview_view.zoom {
        MermaidPreviewZoom::Fit => FloatingImageView::fit(),
        MermaidPreviewZoom::Percent(percent) => FloatingImageView {
            zoom_percent: Some(percent),
            pan_x_px: preview_view.pan_x_px,
            pan_y_px: preview_view.pan_y_px,
        },
    };
    let zoom_label = match preview_view.zoom {
        MermaidPreviewZoom::Fit => "fit".to_string(),
        MermaidPreviewZoom::Percent(percent) => format!("{percent}%"),
    };
    workspace
        .floats
        .push(saya::presentation::floating_window::FloatingScreenModel {
            id: float_id,
            content: FloatingContentRef::StaticLines {
                content_id: float_id.0,
            },
            rect: saya::presentation::screen_model::PaneRect {
                x,
                y,
                width,
                height,
            },
            lines: vec![
                format!("Mermaid preview [{zoom_label}]"),
                " ".repeat(usize::from(content_width)),
            ],
            inline_styles: Vec::new(),
            images: vec![FloatingImage {
                line: 1,
                column: 0,
                max_width: content_width,
                max_height: content_height.saturating_sub(1).max(1),
                view: image_view,
                source: FloatingImageSource::Mermaid {
                    buffer_id: active_window.buf_id,
                    row: block.range.start.line,
                    alt_text: "mermaid diagram".to_string(),
                    background: session_state.mermaid_preview_background().to_string(),
                    source: body,
                },
            }],
            cursor: None,
            focusable: preview_view.focused,
            mouse: true,
            chrome: FloatingChrome {
                border: FloatingBorder::Single,
            },
            zindex: FloatingZIndex::Hover.value(),
            creation_order: u64::MAX,
        });
    log::debug!(
        "[main][markdown_preview] appended Mermaid preview float: trigger={}, window_id={}, buffer_id={}, start_row={}, end_row={}, float_id={}, rect=({},{},{},{}), body_bytes={}",
        if manual_active { "manual" } else { "auto" },
        active_window.id,
        active_window.buf_id,
        block.range.start.line,
        block.range.end.line,
        float_id.0,
        x,
        y,
        width,
        height,
        workspace
            .floats
            .last()
            .and_then(|float| float.images.first())
            .map(|image| match &image.source {
                FloatingImageSource::Mermaid { source, .. } => source.len(),
            })
            .unwrap_or(0)
    );
}

fn mermaid_preview_float_dimension(
    terminal_extent: u16,
    numerator: u16,
    denominator: u16,
    min: u16,
    max: u16,
    reserved: u16,
) -> u16 {
    let available = terminal_extent.saturating_sub(reserved).max(1);
    let denominator = u32::from(denominator.max(1));
    let preferred = u32::from(terminal_extent)
        .saturating_mul(u32::from(numerator))
        .div_ceil(denominator)
        .min(u32::from(u16::MAX)) as u16;
    preferred.max(min).min(max).min(available).max(1)
}

fn apply_workspace_floating_window_models(
    workspace: &mut WorkspaceScreenModel,
    terminal_width: u16,
    terminal_height: u16,
    manager: &FloatingWindowManager,
) {
    let pane_rects = workspace
        .panes
        .iter()
        .map(|pane| (pane.window_id, pane.rect))
        .collect::<Vec<_>>();
    let cursors = workspace
        .panes
        .iter()
        .map(|pane| (pane.window_id, pane.cursor_row, pane.cursor_col))
        .collect::<Vec<_>>();
    workspace.floats = manager.resolve_screen_models_with_cursors(
        terminal_width,
        terminal_height,
        &pane_rects,
        &cursors,
        Some(workspace.active_window_id),
    );
    log::trace!(
        "[main][floating_window] applied workspace floats: floats={}, terminal=({},{}), active_window_id={}",
        workspace.floats.len(),
        terminal_width,
        terminal_height,
        workspace.active_window_id
    );
}

fn refresh_buffer_backed_float_lines(
    manager: &mut FloatingWindowManager,
    core_bridge: &saya::core::bridge::CoreBridge,
    snapshot: &CoreLightSnapshot,
) {
    for request in manager.core_window_float_view_requests() {
        let Some(window) = snapshot.window(request.window_id) else {
            log::debug!(
                "[main][buffer_float] skipping core-window float refresh because window is missing: float_id={}, window_id={}",
                request.float_id.0,
                request.window_id
            );
            continue;
        };
        let start_row = window.topline.saturating_sub(1);
        let Some(range) = core_bridge.buffer_line_range(
            window.buf_id,
            start_row,
            usize::from(request.content_height),
        ) else {
            log::debug!(
                "[main][buffer_float] skipping core-window float refresh because buffer range is missing: float_id={}, window_id={}, buffer_id={}",
                request.float_id.0,
                request.window_id,
                window.buf_id
            );
            continue;
        };
        let returned = range.lines.len();
        let _ = manager.replace_core_window_lines(request.float_id, range.lines);
        log::trace!(
            "[main][buffer_float] refreshed core-window float lines: float_id={}, window_id={}, buffer_id={}, start_row={}, requested_lines={}, returned_lines={}",
            request.float_id.0,
            request.window_id,
            window.buf_id,
            start_row,
            request.content_height,
            returned
        );
    }
}

fn refresh_terminal_float_lines(
    manager: &mut FloatingWindowManager,
    terminal_manager: &mut TerminalFloatManager,
) {
    terminal_manager.drain();
    for request in manager.terminal_float_view_requests() {
        let _ = terminal_manager.resize(
            request.terminal_id,
            request.content_width,
            request.content_height,
        );
        let snapshot = terminal_manager.screen_snapshot(request.terminal_id);
        let mut lines = snapshot
            .as_ref()
            .map(|snapshot| snapshot.rendered_lines())
            .unwrap_or_else(|| terminal_manager.rendered_lines(request.terminal_id));
        lines.truncate(usize::from(request.content_height));
        let inline_styles = snapshot
            .map(|snapshot| {
                snapshot
                    .inline_styles()
                    .into_iter()
                    .filter(|style| style.line < usize::from(request.content_height))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let returned = lines.len();
        let _ = manager.replace_terminal_lines(request.float_id, lines);
        let _ = manager.set_inline_styles(request.float_id, inline_styles);
        log::trace!(
            "[main][terminal_float] refreshed terminal float lines: float_id={}, terminal_id={}, requested_size=({},{}), returned_lines={}",
            request.float_id.0,
            request.terminal_id,
            request.content_width,
            request.content_height,
            returned
        );
    }
}

fn refresh_terminal_panel_lines(
    panel_manager: &mut PanelManager,
    terminal_manager: &mut TerminalFloatManager,
    terminal_width: u16,
    terminal_height: u16,
) {
    terminal_manager.drain();
    for request in panel_manager.terminal_view_requests(terminal_width, terminal_height) {
        let _ = terminal_manager.resize(
            request.terminal_id,
            request.content_width,
            request.content_height,
        );
        let snapshot = terminal_manager.screen_snapshot(request.terminal_id);
        let mut lines = snapshot
            .as_ref()
            .map(|snapshot| snapshot.rendered_lines())
            .unwrap_or_else(|| terminal_manager.rendered_lines(request.terminal_id));
        lines.truncate(usize::from(request.content_height));
        let cursor = terminal_manager
            .cursor_position(request.terminal_id)
            .map(|(line, column)| FloatingCursor {
                line: usize::from(line).min(usize::from(request.content_height.saturating_sub(1))),
                column: usize::from(column),
            });
        let inline_styles = snapshot
            .map(|snapshot| {
                snapshot
                    .inline_styles()
                    .into_iter()
                    .filter(|style| style.line < usize::from(request.content_height))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let returned = lines.len();
        let _ = panel_manager.replace_terminal_lines(&request.id, lines);
        let _ = panel_manager.replace_terminal_inline_styles(&request.id, inline_styles);
        let _ = panel_manager.replace_terminal_cursor(&request.id, cursor);
        log::trace!(
            "[main][panel] refreshed terminal panel lines: id={}, terminal_id={}, requested_size=({},{}), returned_lines={}, cursor={:?}",
            request.id,
            request.terminal_id,
            request.content_width,
            request.content_height,
            returned,
            cursor
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FloatingWindowKeyHandling {
    Consumed,
    Closed { id: FloatingWindowId },
}

fn handle_completion_float_key(
    completion_manager: &mut CompletionFloatManager,
    floating_manager: &mut FloatingWindowManager,
    core_bridge: &mut saya::core::bridge::CoreBridge,
    key: &KeyInput,
    restore_window_id: i32,
) -> Option<FloatingWindowKeyHandling> {
    match completion_manager.handle_key(floating_manager, key, Some(restore_window_id)) {
        CompletionFloatInputOutcome::Selected {
            menu_id,
            selected_index,
        } => {
            log::debug!(
                "[main] completion selection updated from focused input: menu_id={}, selected_index={}",
                menu_id.0,
                selected_index
            );
            Some(FloatingWindowKeyHandling::Consumed)
        }
        CompletionFloatInputOutcome::Accepted { menu_id, candidate } => {
            let insert_text = candidate.insert_text();
            if !insert_text.is_empty() {
                let result = if let Some(replace_range) = candidate.replace_range.as_ref() {
                    core_bridge.apply_completion_replace_range(replace_range, insert_text)
                } else {
                    core_bridge.dispatch_key(insert_text).map(|_| ())
                };
                if let Err(error) = result {
                    log::debug!(
                        "[main] completion candidate insertion failed: menu_id={}, label={:?}, insert_text_len={}, error={:?}",
                        menu_id.0,
                        candidate.label,
                        insert_text.len(),
                        error
                    );
                }
            }
            log::debug!(
                "[main] completion candidate accepted from focused input: menu_id={}, label={:?}",
                menu_id.0,
                candidate.label
            );
            Some(FloatingWindowKeyHandling::Closed { id: menu_id })
        }
        CompletionFloatInputOutcome::Closed {
            menu_id,
            editor_key,
        } => {
            if let Some(editor_key) = editor_key {
                if let EditorIntent::EditKey(core_key) = resolve_intent(&editor_key) {
                    let before = core_bridge.light_snapshot();
                    if let Err(error) = core_bridge.dispatch_key(&core_key) {
                        log::debug!(
                            "[main] completion close editor key dispatch failed: menu_id={}, key={:?}, core_key={:?}, error={:?}",
                            menu_id.0,
                            editor_key,
                            core_key,
                            error
                        );
                    }
                    let after = core_bridge.light_snapshot();
                    let _ =
                        apply_floating_lifecycle_after_core_edit(floating_manager, &before, &after);
                }
            }
            Some(FloatingWindowKeyHandling::Closed { id: menu_id })
        }
        CompletionFloatInputOutcome::Ignored => None,
    }
}

fn handle_core_window_float_key(
    manager: &mut FloatingWindowManager,
    core_bridge: &mut saya::core::bridge::CoreBridge,
    key: &KeyInput,
) -> Option<FloatingWindowKeyHandling> {
    let window_id = manager.focused_core_window_id()?;
    let EditorIntent::EditKey(core_key) = resolve_intent(key) else {
        log::debug!(
            "[main][buffer_float] focused core-window float ignored application command key: key={:?}, window_id={}",
            key,
            window_id
        );
        return None;
    };
    let before = core_bridge.light_snapshot();
    if before.active_window_id() != Some(window_id)
        && let Err(error) = core_bridge.switch_to_window(window_id)
    {
        log::debug!(
            "[main][buffer_float] failed to focus core window before dispatch: window_id={}, key={:?}, error={:?}",
            window_id,
            key,
            error
        );
        return None;
    }
    let dispatch_result = core_bridge.dispatch_key(&core_key);
    let after = core_bridge.light_snapshot();
    log::debug!(
        "[main][buffer_float] dispatched key to focused core-window float: window_id={}, key={:?}, core_key={:?}, result={:?}, revision {}->{}, cursor ({},{}) -> ({},{}), mode {:?}->{:?}",
        window_id,
        key,
        core_key,
        dispatch_result,
        before.revision,
        after.revision,
        before.cursor_row,
        before.cursor_col,
        after.cursor_row,
        after.cursor_col,
        before.mode,
        after.mode
    );
    Some(FloatingWindowKeyHandling::Consumed)
}

fn handle_terminal_float_key(
    manager: &FloatingWindowManager,
    terminal_manager: &mut TerminalFloatManager,
    key: &KeyInput,
) -> Option<FloatingWindowKeyHandling> {
    let terminal_id = manager.focused_terminal_id()?;
    let result = match key {
        KeyInput::PageUp => terminal_manager.scroll(terminal_id, -8),
        KeyInput::PageDown => terminal_manager.scroll(terminal_id, 8),
        _ => terminal_manager.write_key(terminal_id, key),
    };
    match result {
        Ok(()) => {
            log::debug!(
                "[main][terminal_float] focused terminal float consumed key: terminal_id={}, key={:?}",
                terminal_id,
                key
            );
            Some(FloatingWindowKeyHandling::Consumed)
        }
        Err(error) => {
            log::debug!(
                "[main][terminal_float] focused terminal float failed to consume key: terminal_id={}, key={:?}, error={:?}",
                terminal_id,
                key,
                error
            );
            None
        }
    }
}

fn handle_floating_window_key(
    manager: &mut FloatingWindowManager,
    key: &KeyInput,
    restore_window_id: i32,
) -> Option<FloatingWindowKeyHandling> {
    match manager.handle_focused_static_lines_key_with_restore(key, Some(restore_window_id)) {
        FloatingInputOutcome::Consumed => Some(FloatingWindowKeyHandling::Consumed),
        FloatingInputOutcome::Closed { id } => Some(FloatingWindowKeyHandling::Closed { id }),
        FloatingInputOutcome::Ignored => None,
    }
}

fn handle_mermaid_preview_key(
    session_state: &mut EditorSessionState,
    key: &KeyInput,
) -> Option<FloatingWindowKeyHandling> {
    if !session_state.mermaid_preview_focused() {
        return None;
    }
    let float_id = FloatingWindowId(9_000_000_000);
    let outcome = match key {
        KeyInput::Escape | KeyInput::Ctrl('[') | KeyInput::Char('q') => {
            session_state.close_mermaid_preview("focused_key_close");
            FloatingWindowKeyHandling::Closed { id: float_id }
        }
        KeyInput::Char('+') | KeyInput::Char('=') => {
            session_state.zoom_mermaid_preview_in();
            FloatingWindowKeyHandling::Consumed
        }
        KeyInput::Char('-') => {
            session_state.zoom_mermaid_preview_out();
            FloatingWindowKeyHandling::Consumed
        }
        KeyInput::Char('0') => {
            session_state.zoom_mermaid_preview_fit();
            FloatingWindowKeyHandling::Consumed
        }
        KeyInput::Char('1') => {
            session_state.zoom_mermaid_preview_actual_size();
            FloatingWindowKeyHandling::Consumed
        }
        KeyInput::Char('h') | KeyInput::Left => {
            session_state.pan_mermaid_preview(-64, 0);
            FloatingWindowKeyHandling::Consumed
        }
        KeyInput::Char('l') | KeyInput::Right => {
            session_state.pan_mermaid_preview(64, 0);
            FloatingWindowKeyHandling::Consumed
        }
        KeyInput::Char('k') | KeyInput::Up => {
            session_state.pan_mermaid_preview(0, -64);
            FloatingWindowKeyHandling::Consumed
        }
        KeyInput::Char('j') | KeyInput::Down => {
            session_state.pan_mermaid_preview(0, 64);
            FloatingWindowKeyHandling::Consumed
        }
        KeyInput::Ctrl('b') | KeyInput::Ctrl('B') | KeyInput::PageUp => {
            session_state.pan_mermaid_preview(0, -256);
            FloatingWindowKeyHandling::Consumed
        }
        KeyInput::Ctrl('f') | KeyInput::Ctrl('F') | KeyInput::PageDown => {
            session_state.pan_mermaid_preview(0, 256);
            FloatingWindowKeyHandling::Consumed
        }
        KeyInput::Char('H') | KeyInput::ShiftedNav(NavigationKey::Left) => {
            session_state.pan_mermaid_preview(-256, 0);
            FloatingWindowKeyHandling::Consumed
        }
        KeyInput::Char('L') | KeyInput::ShiftedNav(NavigationKey::Right) => {
            session_state.pan_mermaid_preview(256, 0);
            FloatingWindowKeyHandling::Consumed
        }
        _ => {
            log::debug!(
                "[main][markdown_preview] focused Mermaid preview ignored key: key={:?}",
                key
            );
            return None;
        }
    };
    log::debug!(
        "[main][markdown_preview] focused Mermaid preview handled key: key={:?}, outcome={:?}",
        key,
        outcome
    );
    Some(outcome)
}

fn active_mermaid_preview_float_id(
    workspace: Option<&WorkspaceScreenModel>,
) -> Option<FloatingWindowId> {
    workspace?
        .floats
        .iter()
        .find(|float| !float.images.is_empty())
        .map(|float| float.id)
}

fn focus_mermaid_preview_from_mouse_click(
    session_state: &mut EditorSessionState,
    workspace: Option<&WorkspaceScreenModel>,
    column: u16,
    row: u16,
) -> bool {
    if !mouse_cell_hits_mermaid_preview(workspace, column, row) {
        return false;
    }
    session_state.focus_mermaid_preview();
    log::debug!(
        "[main][markdown_preview] Mermaid preview focused by mouse click: column={}, row={}",
        column,
        row
    );
    true
}

fn handle_mermaid_preview_mouse_wheel(
    session_state: &mut EditorSessionState,
    workspace: Option<&WorkspaceScreenModel>,
    column: u16,
    row: u16,
    delta_x: i16,
    delta_y: i16,
) -> bool {
    if !session_state.mermaid_preview_focused()
        || !mouse_cell_hits_mermaid_preview(workspace, column, row)
    {
        return false;
    }
    session_state.pan_mermaid_preview(i32::from(delta_x) * 96, i32::from(delta_y) * 96);
    log::debug!(
        "[main][markdown_preview] Mermaid preview handled mouse wheel: column={}, row={}, delta=({}, {})",
        column,
        row,
        delta_x,
        delta_y
    );
    true
}

fn mouse_cell_hits_mermaid_preview(
    workspace: Option<&WorkspaceScreenModel>,
    column: u16,
    row: u16,
) -> bool {
    workspace
        .into_iter()
        .flat_map(|workspace| workspace.floats.iter())
        .filter(|float| !float.images.is_empty())
        .any(|float| {
            column >= float.rect.x
                && column < float.rect.x.saturating_add(float.rect.width)
                && row >= float.rect.y
                && row < float.rect.y.saturating_add(float.rect.height)
        })
}

fn handle_terminal_panel_key(
    manager: &mut PanelManager,
    terminal_manager: &mut TerminalFloatManager,
    key: &KeyInput,
) -> Option<FloatingWindowKeyHandling> {
    let terminal_id = manager.focused_terminal_id()?;
    if matches!(key, KeyInput::Ctrl('w') | KeyInput::Ctrl('W')) {
        let had_focus = manager.unfocus();
        log::debug!(
            "[main][panel] terminal panel unfocused from key: key={:?}, had_focus={}",
            key,
            had_focus
        );
        return had_focus.then_some(FloatingWindowKeyHandling::Consumed);
    }
    let result = match key {
        KeyInput::PageUp => terminal_manager.scroll(terminal_id, -8),
        KeyInput::PageDown => terminal_manager.scroll(terminal_id, 8),
        _ => terminal_manager.write_key(terminal_id, key),
    };
    match result {
        Ok(()) => {
            log::debug!(
                "[main][panel] focused terminal panel consumed key: terminal_id={}, key={:?}",
                terminal_id,
                key
            );
            Some(FloatingWindowKeyHandling::Consumed)
        }
        Err(error) => {
            log::debug!(
                "[main][panel] focused terminal panel failed to consume key: terminal_id={}, key={:?}, error={:?}",
                terminal_id,
                key,
                error
            );
            None
        }
    }
}

fn begin_command_line_from_focused_panel(
    manager: &mut PanelManager,
    key: &KeyInput,
    mode: CoreMode,
) -> Option<char> {
    let prompt = match (mode, key) {
        (CoreMode::Normal, KeyInput::Char(':')) => ':',
        (CoreMode::Normal, KeyInput::Char('/')) => '/',
        _ => return None,
    };
    let terminal_id = manager.focused_terminal_id()?;
    let had_focus = manager.unfocus();
    log::debug!(
        "[main][panel] focused terminal panel yielded command-line prompt: terminal_id={}, prompt={}, had_focus={}",
        terminal_id,
        prompt,
        had_focus
    );
    had_focus.then_some(prompt)
}

fn focus_floating_window_from_mouse_click(
    manager: &mut FloatingWindowManager,
    workspace: Option<&WorkspaceScreenModel>,
    column: u16,
    row: u16,
    terminal_width: u16,
    terminal_height: u16,
) -> FloatingMouseOutcome {
    let Some(workspace) = workspace else {
        log::debug!(
            "[main] floating mouse focus skipped because no workspace model is available: column={}, row={}",
            column,
            row
        );
        return FloatingMouseOutcome::PassThrough;
    };
    let pane_rects = workspace
        .panes
        .iter()
        .map(|pane| (pane.window_id, pane.rect))
        .collect::<Vec<_>>();
    let outcome = manager.focus_topmost_at(
        column,
        row,
        terminal_width,
        terminal_height,
        &pane_rects,
        Some(workspace.active_window_id),
    );
    log::debug!(
        "[main] floating mouse focus resolved: column={}, row={}, outcome={:?}",
        column,
        row,
        outcome
    );
    outcome
}

fn sync_workspace_message_pager(
    session_state: &mut EditorSessionState,
    workspace: &mut WorkspaceScreenModel,
    terminal_width: u16,
) {
    let visible_message = workspace
        .visible_message_text()
        .map(str::to_owned)
        .unwrap_or_default();
    let pager_message = wrap_message_for_pager(&visible_message, terminal_width);
    let activation_changed =
        session_state.sync_message_pager(&pager_message, workspace.message_area_height);
    if session_state.message_pager_hides_message(&pager_message) {
        workspace.message_line.visible = None;
        workspace.message_scroll_offset = 0;
        workspace.pager_prompt = None;
        log::debug!(
            "[main] hiding dismissed message pager text: message_lines={}",
            visible_message.lines().count()
        );
        return;
    }
    workspace.message_scroll_offset = session_state.message_scroll_offset();
    if workspace.pager_prompt.is_none()
        && let Some(kind) = session_state.message_pager_prompt_kind()
    {
        workspace.pager_prompt = Some(PagerPromptView {
            kind,
            one_shot: false,
        });
    }
    log::debug!(
        "[main] synced message pager: active={}, offset={}, height={}, message_lines={}, activation_changed={}",
        session_state.message_pager_active(),
        session_state.message_scroll_offset(),
        workspace.message_area_height,
        pager_message.lines().count(),
        activation_changed
    );
}

fn wrap_message_for_pager(message: &str, width: u16) -> String {
    let max_width = usize::from(width.max(1));
    message
        .lines()
        .flat_map(|line| wrap_message_line_for_pager(line, max_width))
        .collect::<Vec<_>>()
        .join("\n")
}

fn wrap_message_line_for_pager(line: &str, max_width: usize) -> Vec<String> {
    if command_line_display_width(line, 8) <= max_width {
        return vec![line.to_string()];
    }

    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;
    for ch in line.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if current_width > 0 && current_width.saturating_add(ch_width) > max_width {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
        }
        current.push(ch);
        current_width = current_width.saturating_add(ch_width);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

fn sync_core_screen_size_if_changed(
    outcome: &mut saya::app::bootstrap::BootstrapOutcome,
    last_synced_terminal_size: &mut Option<TerminalSize>,
    terminal_size: TerminalSize,
) -> bool {
    if *last_synced_terminal_size == Some(terminal_size) {
        log::debug!(
            "[main] skipping unchanged core screen size sync: rows={}, cols={}",
            terminal_size.rows,
            terminal_size.columns
        );
        return false;
    }

    trace_redraw_diagnostic(format_args!(
        "core screen size sync requested: previous={:?}, next={:?}",
        last_synced_terminal_size, terminal_size
    ));
    outcome.core_bridge.set_screen_size(
        i32::from(terminal_size.rows),
        i32::from(terminal_size.columns),
    );
    *last_synced_terminal_size = Some(terminal_size);
    true
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

fn snapshot_from_light_snapshot(light: &CoreLightSnapshot, text: String) -> CoreSnapshot {
    CoreSnapshot {
        text,
        revision: light.revision,
        dirty: light.dirty,
        mode: light.mode,
        pending_input: light.pending_input.clone(),
        cursor_row: light.cursor_row,
        cursor_col: light.cursor_col,
        pending_host_actions: light.pending_host_actions,
        buffers: light.buffers.clone(),
        windows: light.windows.clone(),
        pum: light.pum.clone(),
    }
}

fn collect_workspace_line_ranges(
    core_bridge: &saya::core::bridge::CoreBridge,
    snapshot: &vim_core_rs::CoreSnapshot,
    viewport_store: &WindowViewportStore,
) -> BTreeMap<i32, CoreBufferLineRange> {
    let mut line_ranges = BTreeMap::new();
    for window in &snapshot.windows {
        let body_height = window.height.saturating_sub(1).max(1);
        let viewport_top = viewport_store
            .get(window.id)
            .map(|viewport| viewport.top_line())
            .unwrap_or_else(|| window.topline.saturating_sub(1));
        let requested_lines = body_height.max(1);
        match core_bridge.buffer_line_range(window.buf_id, viewport_top, requested_lines) {
            Some(range) => {
                log::debug!(
                    "[main] workspace line range collected: window_id={}, buffer_id={}, viewport_top={}, requested_lines={}, returned_lines={}, total_line_count={}, source_revision={:?}",
                    window.id,
                    window.buf_id,
                    viewport_top,
                    requested_lines,
                    range.lines.len(),
                    range.total_line_count,
                    range.source_revision
                );
                line_ranges.insert(window.id, range);
            }
            None => {
                log::debug!(
                    "[main] workspace line range missing: window_id={}, buffer_id={}, viewport_top={}, requested_lines={}",
                    window.id,
                    window.buf_id,
                    viewport_top,
                    requested_lines
                );
            }
        }
    }
    line_ranges
}

fn collect_workspace_buffer_line_counts(
    core_bridge: &saya::core::bridge::CoreBridge,
    snapshot: &vim_core_rs::CoreSnapshot,
) -> BTreeMap<i32, usize> {
    let mut line_counts = BTreeMap::new();
    let mut seen_buffers = BTreeSet::new();
    for window in &snapshot.windows {
        if !seen_buffers.insert(window.buf_id) {
            continue;
        }
        match core_bridge.buffer_line_range(window.buf_id, 0, 0) {
            Some(range) => {
                log::debug!(
                    "[main] workspace buffer line count collected: buffer_id={}, total_line_count={}, source_revision={:?}",
                    window.buf_id,
                    range.total_line_count,
                    range.source_revision
                );
                line_counts.insert(window.buf_id, range.total_line_count);
            }
            None => {
                log::debug!(
                    "[main] workspace buffer line count missing: buffer_id={}",
                    window.buf_id
                );
            }
        }
    }
    line_counts
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

fn trace_workspace_render_pipeline(
    phase: &str,
    snapshot_text: &str,
    workspace_model: &saya::presentation::screen_model::WorkspaceScreenModel,
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
    let projected_display = visible_row
        .and_then(|row| active_pane.line_projections.get(row))
        .map(|projection| projection.display_text.as_str())
        .unwrap_or("");

    log::debug!(
        "[saya-trace][main][{phase}] viewport_top={viewport_top} abs_row=7 snapshot={snapshot_line:?} visible={projected_line:?} display={projected_display:?}"
    );
}

fn collect_workspace_search_states(
    core_bridge: &mut saya::core::bridge::CoreBridge,
    snapshot: &vim_core_rs::CoreSnapshot,
    viewport_store: &WindowViewportStore,
    search_refresh_store: &mut WindowSearchRefreshStore,
    prompt_revision: Option<u64>,
    search_mode_hint: SearchModeHint,
) -> Result<BTreeMap<i32, SearchVisibleState>, WorkspaceRedrawError> {
    let mut search_states = BTreeMap::new();
    if matches!(search_mode_hint, SearchModeHint::Hlsearch)
        && !core_bridge.has_search_highlight_activity()
    {
        log::debug!(
            "[main] workspace search refresh skipped because hlsearch has no active pattern"
        );
        return Ok(search_states);
    }
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
        trace_redraw_diagnostic(format_args!(
            "search refresh outcome: window_id={}, revision={}, viewport_top={}, viewport_height={}, cursor=({},{}), mode_hint={:?}, query_executed={}, cache_key={:?}, render_state_present={}, match_count={}, current_match={:?}",
            window.id,
            snapshot.revision,
            viewport_top,
            body_height,
            window.cursor_row,
            window.cursor_col,
            search_mode_hint,
            outcome.query_executed,
            outcome.cache_key,
            outcome.render_state.is_some(),
            outcome
                .render_state
                .as_ref()
                .map(|state| state.matches.len())
                .unwrap_or_default(),
            outcome.render_state.as_ref().and_then(|state| {
                state
                    .matches
                    .iter()
                    .find(|search_match| {
                        search_match.kind == saya::features::search::query::SearchMatchKind::Current
                    })
                    .map(|search_match| {
                        (
                            search_match.start_row,
                            search_match.start_col,
                            search_match.end_row,
                            search_match.end_col,
                        )
                    })
            })
        ));
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

fn collect_workspace_substitute_preview_states(
    snapshot: &vim_core_rs::CoreSnapshot,
    line_ranges: &mut BTreeMap<i32, CoreBufferLineRange>,
    command_line_prompt: Option<char>,
    command_line_buffer: &str,
) -> BTreeMap<i32, SearchVisibleState> {
    let mut search_states = BTreeMap::new();
    if command_line_prompt != Some(':') {
        return search_states;
    }

    for window in &snapshot.windows {
        let Some(line_range) = line_ranges.get(&window.id).cloned() else {
            log::debug!(
                "[main][substitute_preview] skipped window because visible line range is missing: window_id={}, buffer_id={}",
                window.id,
                window.buf_id
            );
            continue;
        };
        let Some(preview) = build_substitute_preview_render(
            window,
            &line_range,
            command_line_prompt,
            command_line_buffer,
        ) else {
            continue;
        };
        log::debug!(
            "[main][substitute_preview] using live substitute preview render state: window_id={}, matches={}, pattern={:?}, preview_lines={}",
            window.id,
            preview.search_state.matches.len(),
            preview.search_state.input_pattern,
            preview.line_range.lines.len()
        );
        line_ranges.insert(window.id, preview.line_range);
        search_states.insert(window.id, preview.search_state);
    }
    search_states
}

fn collect_workspace_syntax_lines(
    core_bridge: &saya::core::bridge::CoreBridge,
    snapshot: &vim_core_rs::CoreSnapshot,
    viewport_store: &WindowViewportStore,
    line_ranges: &BTreeMap<i32, CoreBufferLineRange>,
) -> BTreeMap<i32, BTreeMap<usize, Vec<vim_core_rs::CoreSyntaxChunk>>> {
    let mut syntax_lines = BTreeMap::new();
    for window in &snapshot.windows {
        let line_count = line_ranges
            .get(&window.id)
            .map(|range| range.total_line_count)
            .unwrap_or_else(|| buffer_line_count(&snapshot.text));
        let body_height = window.height.saturating_sub(1).max(1);
        let viewport_top = viewport_store
            .get(window.id)
            .map(|viewport| viewport.top_line())
            .unwrap_or_else(|| window.topline.saturating_sub(1));
        let viewport_bottom = viewport_top.saturating_add(body_height).saturating_sub(1);
        let mut window_lines = BTreeMap::new();

        for absolute_row in viewport_top..=viewport_bottom {
            if absolute_row >= line_count {
                break;
            }
            let lnum = i64::try_from(absolute_row.saturating_add(1)).unwrap_or(i64::MAX);
            match core_bridge.get_line_syntax(window.id, lnum) {
                Ok(chunks) if !chunks.is_empty() => {
                    if std::env::var_os("SAYA_TRACE_RENDER").is_some() {
                        log::debug!(
                            "[saya-trace][main][syntax] window_id={} row={} lnum={} chunks={} names={:?}",
                            window.id,
                            absolute_row,
                            lnum,
                            chunks.len(),
                            chunks
                                .iter()
                                .take(8)
                                .filter_map(|chunk| chunk.name.as_deref())
                                .collect::<Vec<_>>()
                        );
                    }
                    log::debug!(
                        "[main] syntax chunks collected: window_id={}, row={}, lnum={}, chunks={}",
                        window.id,
                        absolute_row,
                        lnum,
                        chunks.len()
                    );
                    window_lines.insert(absolute_row, chunks);
                }
                Ok(_) => {}
                Err(error) => {
                    log::debug!(
                        "[main] syntax chunk query skipped for line: window_id={}, row={}, lnum={}, error={:?}",
                        window.id,
                        absolute_row,
                        lnum,
                        error
                    );
                }
            }
        }

        if !window_lines.is_empty() {
            log::debug!(
                "[main] syntax lines collected for window: window_id={}, visible_lines={}",
                window.id,
                window_lines.len()
            );
            syntax_lines.insert(window.id, window_lines);
        }
    }
    syntax_lines
}

#[cfg(feature = "tree-sitter-syntax")]
fn collect_workspace_tree_sitter_syntax(
    core_bridge: &mut saya::core::bridge::CoreBridge,
    snapshot: &vim_core_rs::CoreSnapshot,
    viewport_store: &WindowViewportStore,
    line_ranges: &BTreeMap<i32, CoreBufferLineRange>,
) -> BTreeMap<i32, vim_core_rs::CoreTreeSitterRangeSyntax> {
    let mut syntax_by_window = BTreeMap::new();
    for window in &snapshot.windows {
        let line_count = line_ranges
            .get(&window.id)
            .map(|range| range.total_line_count)
            .unwrap_or_else(|| buffer_line_count(&snapshot.text));
        let Some(buffer) = snapshot
            .buffers
            .iter()
            .find(|buffer| buffer.id == window.buf_id)
        else {
            log::debug!(
                "[main] Tree-sitter syntax skipped because window buffer is missing: window_id={}, buffer_id={}",
                window.id,
                window.buf_id
            );
            continue;
        };
        let body_height = window.height.saturating_sub(1).max(1);
        let viewport_top = viewport_store
            .get(window.id)
            .map(|viewport| viewport.top_line())
            .unwrap_or_else(|| window.topline.saturating_sub(1));
        let viewport_bottom = viewport_top
            .saturating_add(body_height)
            .saturating_sub(1)
            .min(line_count.saturating_sub(1));
        if viewport_top > viewport_bottom {
            log::debug!(
                "[main] Tree-sitter syntax skipped because visible range is empty: window_id={}, viewport_top={}, viewport_bottom={}",
                window.id,
                viewport_top,
                viewport_bottom
            );
            continue;
        }
        let buffer_path_hint = buffer_path_hint(buffer);
        if buffer_path_hint != buffer.name {
            log::debug!(
                "[main] using buffer document identity for Tree-sitter language hint: window_id={}, buffer_id={}, buffer_name={:?}, path_hint={:?}",
                window.id,
                buffer.id,
                buffer.name,
                buffer_path_hint
            );
        }
        let range = vim_core_rs::CoreTextRange {
            start: vim_core_rs::CoreTextPosition {
                row: viewport_top,
                col: 0,
            },
            end: vim_core_rs::CoreTextPosition {
                row: viewport_bottom.saturating_add(1),
                col: 0,
            },
        };
        let root_language = vim_core_rs::VimCoreSession::resolve_tree_sitter_root_language(
            vim_core_rs::CoreRootLanguageResolutionRequest {
                range,
                vim_filetype: None,
                buffer_name: (!buffer_path_hint.is_empty()).then(|| buffer_path_hint.to_string()),
                host_language_hint: None,
            },
        );
        if !matches!(
            root_language.status,
            vim_core_rs::CoreLanguageResolutionStatus::Resolved
        ) {
            log::debug!(
                "[main] Tree-sitter syntax skipped because vim-core-rs could not resolve a supported language: window_id={}, buffer_id={}, buffer_name={:?}, path_hint={:?}, resolution={:?}",
                window.id,
                buffer.id,
                buffer.name,
                buffer_path_hint,
                root_language
            );
            continue;
        }
        let request = vim_core_rs::CoreTreeSitterPreparationRequest {
            buffer_id: buffer.id,
            source_revision: Some(buffer.source_revision),
            range,
            vim_filetype: None,
            buffer_name: (!buffer_path_hint.is_empty()).then(|| buffer_path_hint.to_string()),
            host_language_hint: None,
            snapshot_policy: vim_core_rs::CoreTreeSitterSnapshotPolicy::default(),
        };
        let preparation = match core_bridge.request_tree_sitter_syntax_preparation(request) {
            Ok(preparation) => preparation,
            Err(error) => {
                log::debug!(
                    "[main] Tree-sitter preparation request failed: window_id={}, buffer_id={}, source_revision={:?}, error={:?}",
                    window.id,
                    buffer.id,
                    buffer.source_revision,
                    error
                );
                continue;
            }
        };
        while let Some(completed) = core_bridge.poll_tree_sitter_preparation() {
            log::debug!(
                "[main] Tree-sitter preparation poll drained: request_id={}, buffer_id={}, source_revision={:?}, status={:?}, chunks={}",
                completed.request_id.value,
                completed.syntax.buffer_id,
                completed.syntax.source_revision,
                completed.syntax.status,
                completed.syntax.chunks.len()
            );
        }
        let Some(syntax) =
            core_bridge.query_tree_sitter_syntax_range(buffer.id, buffer.source_revision, range)
        else {
            log::debug!(
                "[main] Tree-sitter syntax cache unavailable after preparation: window_id={}, request_id={}, buffer_id={}, source_revision={:?}, preparation_status={:?}",
                window.id,
                preparation.request_id.value,
                buffer.id,
                buffer.source_revision,
                preparation.status
            );
            continue;
        };
        if syntax.source_revision != buffer.source_revision
            || !matches!(syntax.status, vim_core_rs::CoreTreeSitterStatus::Prepared)
            || syntax.has_error
            || !syntax.error_ranges.is_empty()
            || !matches!(
                syntax.budget_status,
                vim_core_rs::CoreTreeSitterBudgetStatus::WithinBudget
            )
            || !tree_sitter_coverage_contains_range(&syntax.covered_ranges, range)
        {
            log::debug!(
                "[main] Tree-sitter syntax not renderable as fresh highlight: window_id={}, buffer_id={}, syntax_revision={:?}, buffer_revision={:?}, status={:?}, has_error={}, error_ranges={}, covered_ranges={}, budget_status={:?}",
                window.id,
                buffer.id,
                syntax.source_revision,
                buffer.source_revision,
                syntax.status,
                syntax.has_error,
                syntax.error_ranges.len(),
                syntax.covered_ranges.len(),
                syntax.budget_status
            );
            continue;
        }
        log::debug!(
            "[main] Tree-sitter syntax render data collected: window_id={}, buffer_id={}, source_revision={:?}, chunks={}, provenance={:?}",
            window.id,
            buffer.id,
            syntax.source_revision,
            syntax.chunks.len(),
            syntax.provenance
        );
        syntax_by_window.insert(window.id, syntax);
    }
    syntax_by_window
}

#[cfg(feature = "tree-sitter-syntax")]
fn tree_sitter_coverage_contains_range(
    covered_ranges: &[vim_core_rs::CoreTextRange],
    range: vim_core_rs::CoreTextRange,
) -> bool {
    covered_ranges
        .iter()
        .any(|covered| covered.start <= range.start && range.end <= covered.end)
}

fn collect_workspace_markdown_document_maps(
    markdown_metadata_cache: &mut MarkdownMetadataCache,
    session_state: &saya::app::session::EditorSessionState,
    core_bridge: &saya::core::bridge::CoreBridge,
    snapshot: &vim_core_rs::CoreSnapshot,
) -> BTreeMap<i32, Arc<MarkdownDocumentMap>> {
    if !session_state.markdown_render() {
        if std::env::var_os("SAYA_TRACE_RENDER").is_some() {
            log::debug!(
                "[saya-trace][main][markdown] collected=false reason=markdownrender_off target_path={:?}",
                session_state.target_path()
            );
        }
        log::debug!(
            "[main] skipping markdown metadata collection because markdownrender is off: target_path={:?}",
            session_state.target_path()
        );
        return BTreeMap::new();
    }

    let mut document_maps_by_buffer = BTreeMap::<i32, Arc<MarkdownDocumentMap>>::new();
    let mut cache_status_by_buffer = BTreeMap::new();
    for window in &snapshot.windows {
        if document_maps_by_buffer.contains_key(&window.buf_id) {
            continue;
        }
        let Some(buffer) = snapshot
            .buffers
            .iter()
            .find(|buffer| buffer.id == window.buf_id)
        else {
            log::debug!(
                "[main] skipping markdown metadata collection because window buffer is missing: window_id={}, buffer_id={}",
                window.id,
                window.buf_id
            );
            continue;
        };
        let buffer_path_hint = buffer_path_hint(buffer);
        if buffer_path_hint != buffer.name {
            log::debug!(
                "[main] using buffer document identity for markdown metadata: window_id={}, buffer_id={}, buffer_name={:?}, path_hint={:?}",
                window.id,
                buffer.id,
                buffer.name,
                buffer_path_hint
            );
        }
        if !is_markdown_buffer_name(buffer_path_hint) {
            log::debug!(
                "[main] skipping markdown metadata collection because buffer is not markdown: window_id={}, buffer_id={}, buffer_name={:?}, path_hint={:?}",
                window.id,
                buffer.id,
                buffer.name,
                buffer_path_hint
            );
            continue;
        }
        let key = MarkdownMetadataKey {
            buffer_id: i64::from(buffer.id),
            revision: buffer.source_revision.value,
        };
        if let Some(document_map) = markdown_metadata_cache.cached_document_map(key) {
            cache_status_by_buffer.insert(buffer.id, MarkdownCacheStatus::Hit);
            document_maps_by_buffer.insert(buffer.id, document_map);
            continue;
        }
        if !window.is_active {
            log::debug!(
                "[main] skipping inactive markdown metadata cache miss to avoid fetching unrelated buffer text: window_id={}, buffer_id={}, buffer_name={:?}",
                window.id,
                buffer.id,
                buffer.name
            );
            continue;
        }
        let outcome = markdown_metadata_cache.document_map_with_source(key, || {
            let Some(line_count_range) = core_bridge.buffer_line_range(buffer.id, 0, 0) else {
                log::debug!(
                    "[main] markdown metadata source unavailable because buffer line count is missing: window_id={}, buffer_id={}",
                    window.id,
                    buffer.id
                );
                return String::new();
            };
            let Some(full_range) =
                core_bridge.buffer_line_range(buffer.id, 0, line_count_range.total_line_count)
            else {
                log::debug!(
                    "[main] markdown metadata source unavailable because buffer text is missing: window_id={}, buffer_id={}",
                    window.id,
                    buffer.id
                );
                return String::new();
            };
            let mut source_text = full_range.lines.join("\n");
            if !source_text.is_empty() {
                source_text.push('\n');
            }
            source_text
        });
        cache_status_by_buffer.insert(buffer.id, outcome.status);
        document_maps_by_buffer.insert(buffer.id, Arc::clone(&outcome.document_map));
    }

    let maps = snapshot
        .windows
        .iter()
        .filter_map(|window| {
            document_maps_by_buffer
                .get(&window.buf_id)
                .map(|document_map| (window.id, Arc::clone(document_map)))
        })
        .collect::<BTreeMap<_, _>>();
    if std::env::var_os("SAYA_TRACE_RENDER").is_some() {
        log::debug!(
            "[saya-trace][main][markdown] collected=true target_path={:?} markdownrender={} mapped_buffers={:?} mapped_windows={:?}",
            session_state.target_path(),
            session_state.markdown_render(),
            document_maps_by_buffer.keys().copied().collect::<Vec<_>>(),
            maps.keys().copied().collect::<Vec<_>>()
        );
    }
    log::debug!(
        "[main] collected workspace markdown metadata: mapped_buffers={:?}, cache_status_by_buffer={:?}, mapped_windows={:?}",
        document_maps_by_buffer.keys().copied().collect::<Vec<_>>(),
        cache_status_by_buffer,
        maps.keys().copied().collect::<Vec<_>>()
    );
    maps
}

fn buffer_path_hint(buffer: &vim_core_rs::CoreBufferInfo) -> &str {
    buffer
        .document_id
        .as_deref()
        .and_then(|document_id| document_id.strip_prefix("file://"))
        .filter(|document_id| !document_id.is_empty())
        .unwrap_or(&buffer.name)
}

fn is_markdown_buffer_name(buffer_name: &str) -> bool {
    std::path::Path::new(buffer_name)
        .extension()
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

fn structural_refresh_is_idle(refresh: Option<&StructuralRefreshOutcome>) -> bool {
    refresh.is_none_or(|refresh| {
        !refresh.redraw_plan.requested
            && !refresh.redraw_plan.full
            && !refresh.redraw_plan.clear_before_draw
            && !refresh.invalidation.has_any()
    })
}

fn build_command_line_only_workspace(
    last_workspace: Option<&WorkspaceScreenModel>,
    command_line_prompt: Option<char>,
    command_line_buffer: &str,
    command_line_cursor_byte_index: usize,
    tab_size: u16,
) -> Option<WorkspaceScreenModel> {
    if command_line_prompt != Some(':') {
        return None;
    }
    if substitute_preview_command_may_need_workspace_projection(command_line_buffer) {
        trace_redraw_diagnostic(format_args!(
            "command-line-only redraw bypassed because substitute live preview needs search overlay projection"
        ));
        return None;
    }
    let last_workspace = last_workspace?;
    let preview = format!(":{}", command_line_buffer);
    let cursor_col = command_line_cursor_display_col(
        ':',
        command_line_buffer,
        command_line_cursor_byte_index,
        tab_size,
    );
    log::debug!(
        "[main] reusing last workspace for command-line-only redraw: prompt=:, buffer_len={}, cursor_col={}",
        command_line_buffer.len(),
        cursor_col
    );

    let mut workspace = last_workspace.clone();
    workspace.command_line = Some(CommandLineModel {
        text: preview,
        cursor_col,
    });
    Some(workspace)
}

fn substitute_preview_command_may_need_workspace_projection(command_line_buffer: &str) -> bool {
    let input = command_line_buffer
        .trim_start()
        .strip_prefix(':')
        .unwrap_or(command_line_buffer.trim_start());
    let input = input
        .strip_prefix('%')
        .unwrap_or_else(|| strip_ex_range_prefix(input));
    let input = input.trim_start();
    input
        .strip_prefix("s")
        .is_some_and(|rest| substitute_command_boundary(rest))
        || input
            .strip_prefix("substitute")
            .is_some_and(|rest| substitute_command_boundary(rest))
}

fn strip_ex_range_prefix(input: &str) -> &str {
    let range_len = input
        .char_indices()
        .take_while(|(_, ch)| matches!(ch, '0'..='9' | '.' | '$' | ',' | ';' | '+' | '-'))
        .last()
        .map(|(index, ch)| index + ch.len_utf8())
        .unwrap_or(0);
    &input[range_len..]
}

fn substitute_command_boundary(rest: &str) -> bool {
    rest.bytes()
        .next()
        .is_none_or(|byte| byte.is_ascii_punctuation() || byte.is_ascii_whitespace())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandLineOnlyRedraw {
    Rendered,
    NotApplicable,
    Fallback,
}

fn render_command_line_only_redraw_if_possible(
    render_coordinator: &mut TuiRenderCoordinator,
    overlay_writer: Option<&mut dyn OverlayTerminalWriter>,
    last_workspace_model: &mut Option<WorkspaceScreenModel>,
    structural_refresh: Option<&StructuralRefreshOutcome>,
    workspace_projection_dirty: bool,
    command_line_prompt: Option<char>,
    command_line_buffer: &str,
    command_line_cursor_byte_index: usize,
    tab_size: u16,
) -> CommandLineOnlyRedraw {
    if !structural_refresh_is_idle(structural_refresh) {
        return CommandLineOnlyRedraw::NotApplicable;
    }
    if workspace_projection_dirty {
        trace_redraw_diagnostic(format_args!(
            "command-line-only redraw bypassed because workspace projection is dirty"
        ));
        return CommandLineOnlyRedraw::NotApplicable;
    }
    let Some(workspace) = build_command_line_only_workspace(
        last_workspace_model.as_ref(),
        command_line_prompt,
        command_line_buffer,
        command_line_cursor_byte_index,
        tab_size,
    ) else {
        return CommandLineOnlyRedraw::NotApplicable;
    };
    let Some(command_line) = workspace.command_line.as_ref() else {
        return CommandLineOnlyRedraw::NotApplicable;
    };

    trace_redraw_diagnostic(format_args!(
        "workspace redraw skipped for command-line-only overlay: command_prompt={:?}, command_buffer_len={}",
        command_line_prompt,
        command_line_buffer.len()
    ));
    match render_coordinator.render_command_line_overlay(command_line, overlay_writer) {
        Ok(()) => {
            *last_workspace_model = Some(workspace);
            CommandLineOnlyRedraw::Rendered
        }
        Err(error) => {
            trace_command_line_overlay_fallback(&error);
            log::debug!(
                "[main] command-line-only overlay failed; falling back to full workspace render: error={:?}",
                error
            );
            CommandLineOnlyRedraw::Fallback
        }
    }
}

fn trace_command_line_overlay_fallback(error: &RenderFrameError) {
    trace_redraw_diagnostic(format_args!(
        "command-line-only overlay fallback to workspace redraw: error={:?}",
        error
    ));
}

fn command_line_display_width(text: &str, tab_size: u16) -> usize {
    let tab_size = usize::from(tab_size.max(1));
    let mut display_col = 0usize;
    for ch in text.chars() {
        if ch == '\t' {
            display_col = next_command_line_tab_stop(display_col, tab_size);
        } else {
            display_col += UnicodeWidthChar::width(ch).unwrap_or(0);
        }
    }
    display_col
}

fn command_line_cursor_display_col(
    prompt: char,
    command_line_buffer: &str,
    cursor_byte_index: usize,
    tab_size: u16,
) -> u16 {
    let cursor_byte_index = cursor_byte_index.min(command_line_buffer.len());
    let cursor_byte_index =
        clamp_to_command_line_char_boundary(command_line_buffer, cursor_byte_index);
    let prefix = format!("{}{}", prompt, &command_line_buffer[..cursor_byte_index]);
    u16::try_from(command_line_display_width(&prefix, tab_size)).unwrap_or(u16::MAX)
}

fn clamp_to_command_line_char_boundary(buffer: &str, cursor_byte_index: usize) -> usize {
    let mut index = cursor_byte_index.min(buffer.len());
    while index > 0 && !buffer.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn next_command_line_tab_stop(display_col: usize, tab_size: usize) -> usize {
    display_col + (tab_size - (display_col % tab_size)).min(tab_size)
}

fn trace_redraw_diagnostic(args: std::fmt::Arguments<'_>) {
    let message = args.to_string();
    #[cfg(test)]
    record_test_redraw_trace(&message);
    log::debug!("[redraw_diagnostic] {message}");
}

fn trace_job_control_diagnostic(args: std::fmt::Arguments<'_>) {
    let message = args.to_string();
    log::debug!("[job_control_diagnostic] {message}");
    if std::env::var_os("SAYA_TRACE_JOB_CONTROL").is_some() {
        eprintln!("[saya-trace][job-control] {message}");
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct RedrawTraceCounts {
    command_line_only_overlay: usize,
    workspace_render_build_started: usize,
    renderer_frame_requested: usize,
    command_line_overlay_fallback: usize,
}

#[cfg(test)]
static COMMAND_LINE_ONLY_OVERLAY_TRACE_COUNT: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
static WORKSPACE_RENDER_BUILD_STARTED_TRACE_COUNT: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
static RENDERER_FRAME_REQUESTED_TRACE_COUNT: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
static COMMAND_LINE_OVERLAY_FALLBACK_TRACE_COUNT: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
fn record_test_redraw_trace(message: &str) {
    if message.contains("workspace redraw skipped for command-line-only overlay") {
        COMMAND_LINE_ONLY_OVERLAY_TRACE_COUNT.fetch_add(1, Ordering::SeqCst);
    }
    if message.contains("workspace render build started") {
        WORKSPACE_RENDER_BUILD_STARTED_TRACE_COUNT.fetch_add(1, Ordering::SeqCst);
    }
    if message.contains("renderer frame requested") {
        RENDERER_FRAME_REQUESTED_TRACE_COUNT.fetch_add(1, Ordering::SeqCst);
    }
    if message.contains("command-line-only overlay fallback to workspace redraw") {
        COMMAND_LINE_OVERLAY_FALLBACK_TRACE_COUNT.fetch_add(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
fn reset_test_redraw_trace_counts() {
    COMMAND_LINE_ONLY_OVERLAY_TRACE_COUNT.store(0, Ordering::SeqCst);
    WORKSPACE_RENDER_BUILD_STARTED_TRACE_COUNT.store(0, Ordering::SeqCst);
    RENDERER_FRAME_REQUESTED_TRACE_COUNT.store(0, Ordering::SeqCst);
    COMMAND_LINE_OVERLAY_FALLBACK_TRACE_COUNT.store(0, Ordering::SeqCst);
}

#[cfg(test)]
fn test_redraw_trace_counts() -> RedrawTraceCounts {
    RedrawTraceCounts {
        command_line_only_overlay: COMMAND_LINE_ONLY_OVERLAY_TRACE_COUNT.load(Ordering::SeqCst),
        workspace_render_build_started: WORKSPACE_RENDER_BUILD_STARTED_TRACE_COUNT
            .load(Ordering::SeqCst),
        renderer_frame_requested: RENDERER_FRAME_REQUESTED_TRACE_COUNT.load(Ordering::SeqCst),
        command_line_overlay_fallback: COMMAND_LINE_OVERLAY_FALLBACK_TRACE_COUNT
            .load(Ordering::SeqCst),
    }
}

#[cfg(test)]
fn redraw_trace_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn format_cli_error(error: CliParseError) -> String {
    match error {
        CliParseError::MissingConfigPath => "設定ファイルのパスが指定されていません".to_string(),
        CliParseError::MissingLineNumber => "開始行番号が指定されていません".to_string(),
        CliParseError::MissingPluginCommand => {
            "plugin サブコマンドが指定されていません".to_string()
        }
        CliParseError::InvalidLineNumber(value) => {
            format!("開始行番号が不正です: {}", value.to_string_lossy())
        }
        CliParseError::MultipleTargetPaths => "対象ファイルは 1 つだけ指定できます".to_string(),
        CliParseError::UnknownPluginCommand(command) => {
            format!(
                "未対応の plugin サブコマンドです: {}",
                command.to_string_lossy()
            )
        }
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
        "",
        "Plugin commands:",
        "  plugin sync      Generate plugin cache artifacts via the manager",
        "  plugin update    Update plugin cache artifacts via the manager",
        "  plugin list      List cached plugins",
        "  plugin clean     Remove generated startup and lazy cache artifacts",
        "  plugin doctor    Check plugin cache health",
    ]
    .join("\n")
}

fn render_version_text() -> String {
    format!("sy {}", env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::*;
    use saya::presentation::overlay::optional_graphics::RecordingOverlayWriter;
    use saya::presentation::screen_model::ScreenCursorStyle;

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
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/bundled/completion/index.ts");
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
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/bundled/completion/index.ts");
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
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/bundled/completion/index.ts");
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
        let plugin_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/saya-dired.ts");
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
    fn render_version_text_includes_package_version() {
        let version = render_version_text();

        assert_eq!(version, format!("sy {}", env!("CARGO_PKG_VERSION")));
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
