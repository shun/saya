use crate::app::bootstrap::{StartupKeymapAction, bootstrap_warning_message};
use crate::app::cli::{StartupAction, parse_launch_request};
use crate::app::cli_output::{
    format_bootstrap_error, format_cli_error, format_tui_startup_context_error, render_help_text,
    render_version_text,
};
use crate::app::event_loop::{EventLoopCoordinator, LoopAction, ShutdownReason, UiEvent};
use crate::app::outcome_consume::{
    MainOutcomeAccumulator, consume_core_outcomes_from_core, mark_structural_refresh_rendered,
};
use crate::app::runtime_dispatch::dispatch_command_line_key;
use crate::app::runtime_dispatch::dispatch_floating_ui_key;
use crate::app::runtime_dispatch::dispatch_selector_key_route;
use crate::app::runtime_dispatch::{
    BufferedResolution, Command, LsifBridgeHandle, dispatch_buffer_changed_with_runtime,
    dispatch_buffer_open_with_runtime, execute_startup_keymap_registered_command,
    handle_directory_operation_confirmation_key_with_runtime, resolve_pipeline_command_buffered,
    save_snapshot_result, startup_keymap_action_for_input,
};
use crate::app::runtime_dispatch::{
    dispatch_complete_keys_to_core, dispatch_completion_float_key, dispatch_floating_window_key,
    dispatch_resolved_intent_key, resolve_input_active_window_id,
};
use crate::app::runtime_dispatch::{
    dispatch_mouse_click, dispatch_mouse_wheel, dispatch_pasted_text,
};
use crate::app::runtime_dispatch::{
    dispatch_notification_prompt_key, dispatch_runtime_input_prompt_key,
};
use crate::app::runtime_dispatch::{
    process_pending_host_actions_with_runtime, process_pending_host_actions_without_runtime,
    sync_session_dirty_from_core,
};
use crate::app::startup::{PreparedTuiStartup, prepare_tui_startup_context};
use crate::core::host_actions::HostActionRuntime;
use crate::features::completion::float::CompletionFloatManager;
use crate::features::dired::RuntimeInputPromptUiState;
use crate::features::lsp::float::LspDiagnosticStore;
use crate::features::search::refresh::WindowSearchRefreshStore;
use crate::input::command_line_editor::CommandLineEdit;
use crate::input::command_line_history::{
    load_histories_from_default_cache, save_histories_to_default_cache,
};
use crate::input::router::KeyInput;
use crate::input::router::command_line_entry_for_key;
use crate::presentation::floating_input::{
    FloatingWindowKeyHandling, focused_input_target_for_key, handle_completion_float_key,
};
use crate::presentation::floating_window::{FloatingContentRef, FloatingWindowManager};
use crate::presentation::markdown::structure::{MarkdownDocumentMap, MarkdownMetadataCache};
use crate::presentation::overlay::asset_store::OverlayAssetStore;
use crate::presentation::overlay::effect::RuntimePresentationIntent;
use crate::presentation::overlay::optional_graphics::{
    OptionalGraphicsAdapter, OverlayTerminalWriter,
};
use crate::presentation::panel::PanelManager;
use crate::presentation::render::command_line_redraw::{
    CommandLineOnlyRedraw, render_command_line_only_redraw_if_possible,
    sync_core_screen_size_if_changed,
};
use crate::presentation::render::coordinator::TuiRenderCoordinator;
use crate::presentation::render::renderer::{CrosstermBackendImpl, TuiRenderer};
use crate::presentation::render::workspace_output::{
    WorkspaceRedrawError, build_workspace_render_output, effective_workspace_redraw_plan,
    terminal_display_invalidated_redraw_plan,
};
use crate::presentation::render::workspace_projection::trace_workspace_render_pipeline;
use crate::presentation::screen_model::{
    ProjectionInput, WorkspaceProjectionError, WorkspaceScreenModel, project,
};
use crate::presentation::structural_refresh::RedrawPlan;
use crate::presentation::viewport::{ViewportSyncMode, WindowViewportStore};
use crate::runtime::integration::RuntimeSessionOwner;
use crate::runtime::plugin::{PluginHost, render_plugin_report};
use crate::support::diagnostic_log::{
    configure_from_startup as configure_diagnostic_log_from_startup,
    init_from_env as init_diagnostic_log_from_env,
};
use crate::terminal::capability::TerminalCapabilityProbe;
use crate::terminal::float::TerminalFloatManager;
use crate::terminal::input_loop::CrosstermEventSource;
use crate::terminal::job_control::{
    start_job_control_signal_watcher, suspend_current_process_for_job_control,
    trace_job_control_diagnostic,
};
use crate::terminal::lifecycle::TerminalBackend;
use crate::terminal::lifecycle::TerminalSize;
use crate::terminal::lifecycle::current_terminal_size;
use vim_core_rs::CoreMode;
#[derive(Debug)]
struct MainInputPerfTrace {
    id: u64,
    key: String,
    command: Option<String>,
    started_at: std::time::Instant,
    command_elapsed_ms: Option<u128>,
}

pub async fn run() {
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
                crate::runtime::plugin::PluginCommand::Sync => {
                    match crate::app::bootstrap::collect_startup_registry_for_plugin_operation(
                        launch_request.config_source.clone(),
                    ) {
                        Ok(Some((registry, source_hash))) => {
                            host.sync_startup_plugin_declarations(&registry, source_hash)
                        }
                        Ok(None) => host.run_operation(*command),
                        Err(message) => {
                            Err(crate::runtime::plugin::PluginHostError::Operation { message })
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
    // ADR 0006 Phase 3: host 入力パイプラインの単一 pending 状態。
    // - host_count: count digit を host 側に蓄積（core へは流さない）。
    // - host_passthrough: keymap 非該当キーのうち、core 予測が pending と判定した
    //   未完成キー列を host にバッファする。完成と判定された時のみ core へ 1 回送る。
    let mut host_count: Option<usize> = None;
    let mut host_passthrough: String = String::new();
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
                            // 継ぎ目（#2）観測ログ: command-line 等ホスト所有導線を抜けた
                            // 後、どの focused サブシステムへキーが流れるはずかを純粋関数で
                            // 分類して記録する。実際の処理は後続の dispatch_* ラッパが行うが、
                            // この分類関数を本番ループが実際に通すことで、振り分け順序の
                            // 配線を単体テストで検証できる形にしている。
                            let focused_input_target = focused_input_target_for_key(
                                &key,
                                outcome.core_bridge.mode(),
                                panel_manager.focused_terminal_id().is_some(),
                                floating_window_manager.focused_terminal_id().is_some(),
                                floating_window_manager.focused_core_window_id().is_some(),
                                completion_float_manager.has_active_menu(),
                                floating_window_manager.focused_static_lines_id().is_some(),
                            );
                            log::debug!(
                                "[main][input] focused-subsystem routing classified: key={:?}, target={:?}",
                                key,
                                focused_input_target
                            );
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
                        } else if let Some(prompt) = (!handled)
                            .then(|| {
                                // ADR 0006 回帰修正: command-line（ex / search）入口は host の
                                // 責務（architecture.md: `src/input/` が command-line editing /
                                // ex-command routing を持つ）。`:` / `/` を単一パイプラインに
                                // 載せると `predict_input_completeness` が「完成 builtin」と判定し
                                // backend へ越境してしまい、host の command-line 入口がバイパス
                                // されてコマンドモードに入れなくなる。
                                //
                                // 判断ロジックは副作用のない `command_line_entry_for_key` に集約し、
                                // 本番イベントループが実際にその関数を通る形にすることで、単体
                                // テストが実コードの配線を検証できるようにしている。host pending /
                                // count / passthrough が積まれているケースでは None を返し、ここでは
                                // 横取りしない（count や keymap prefix を落とさないため）。
                                command_line_entry_for_key(
                                    &key,
                                    outcome.core_bridge.mode(),
                                    &startup_keymap_pending_lhs,
                                    &host_count,
                                    &host_passthrough,
                                )
                            })
                            .flatten()
                        {
                            command_line_prompt = Some(prompt);
                            command_line_edit.clear();
                            command_line_histories.reset_navigation();
                            handled = true;
                            need_redraw = true;
                            log::info!(
                                "[main][pipeline] command-line entry intercepted before pipeline (host-owned): key={:?}, prompt={:?}",
                                key,
                                command_line_prompt
                            );
                        } else if !handled {
                            // ADR 0006 Phase 3: モーダル入力を単一パイプラインで解決する。
                            // 完成判定をパイプラインに集約し、完成コマンドのみを backend へ
                            // 越境させる。部分入力（keymap prefix / count / operator の motion
                            // 待ち等）は host 側（host_pending / host_count / host_passthrough）に
                            // 留め、core を一切呼ばない。
                            //
                            // Phase 0 の止血（prefix を core に先行送出して同期 + ESC 巻き戻し）は
                            // 撤去した。完成判定は core の非破壊予測器
                            // (`classify_input_completeness`) を予測関数として使うため、
                            // host/core の pending が乖離する余地がなくなった。
                            let input_snapshot_for_pipeline = outcome.core_bridge.light_snapshot();
                            let predict_mode = input_snapshot_for_pipeline.mode;
                            let resolution = resolve_pipeline_command_buffered(
                                &outcome.startup_registry.keymaps,
                                &input_snapshot_for_pipeline,
                                &mut startup_keymap_pending_lhs,
                                &mut host_count,
                                &mut host_passthrough,
                                &key,
                                |seq: &str| {
                                    vim_core_rs::predict_input_completeness(seq, predict_mode)
                                },
                            );
                            log::info!(
                                "[main][pipeline] phase3 buffered resolution: key={:?}, resolution={:?}, host_pending={:?}, host_count={:?}, host_passthrough={:?}",
                                key,
                                resolution,
                                startup_keymap_pending_lhs,
                                host_count,
                                host_passthrough
                            );
                            match resolution {
                                BufferedResolution::Command(Command::BuiltinEdit(rhs)) => {
                                    handled = true;
                                    log::debug!(
                                        "[main][pipeline] dispatching BuiltinEdit complete command to core: rhs={:?}",
                                        rhs
                                    );
                                    if let Some(reason) = dispatch_complete_keys_to_core(
                                        &rhs,
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
                                        &mut workspace_projection_dirty,
                                    )
                                    .await
                                    {
                                        break 'main reason;
                                    }
                                }
                                BufferedResolution::Command(Command::HostCommand {
                                    name: command_name,
                                    count: host_command_count,
                                }) => {
                                    handled = true;
                                    log::info!(
                                        "[main][pipeline] executing HostCommand (count carried in type): command={}, count={:?}",
                                        command_name,
                                        host_command_count
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
                                    if let Some(reason) = execute_startup_keymap_registered_command(
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
                                    need_redraw = true;
                                }
                                BufferedResolution::DispatchComplete(complete_keys) => {
                                    handled = true;
                                    log::debug!(
                                        "[main][pipeline] dispatching complete builtin sequence to core (passthrough completed): keys={:?}",
                                        complete_keys
                                    );
                                    if let Some(reason) = dispatch_complete_keys_to_core(
                                        &complete_keys,
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
                                        &mut workspace_projection_dirty,
                                    )
                                    .await
                                    {
                                        break 'main reason;
                                    }
                                }
                                BufferedResolution::HoldPending
                                | BufferedResolution::CountAccumulated => {
                                    // 部分入力は host に留め backend を呼ばない。
                                    handled = true;
                                    need_redraw = true;
                                    log::debug!(
                                        "[main][pipeline] partial input held in host pipeline, backend NOT called: key={:?}, host_pending={:?}, host_count={:?}, host_passthrough={:?}",
                                        key,
                                        startup_keymap_pending_lhs,
                                        host_count,
                                        host_passthrough
                                    );
                                }
                                BufferedResolution::Unhandled => {
                                    // pipeline で扱えないキー（lhs 表現不能等）。
                                    // バッファに未完成キーが残っていれば core へ flush してから
                                    // 後続の従来処理（`:`/`/` や dispatch_resolved_intent_key）へ。
                                    if !host_passthrough.is_empty() {
                                        let flushed = std::mem::take(&mut host_passthrough);
                                        log::debug!(
                                            "[main][pipeline] unhandled key with residual passthrough buffer; flushing buffer to core before legacy path: flushed={:?}, key={:?}",
                                            flushed,
                                            key
                                        );
                                        if let Some(reason) = dispatch_complete_keys_to_core(
                                            &flushed,
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
                                            &mut workspace_projection_dirty,
                                        )
                                        .await
                                        {
                                            break 'main reason;
                                        }
                                    }
                                    log::debug!(
                                        "[main][pipeline] resolution unhandled, falling through to legacy path: key={:?}",
                                        key
                                    );
                                }
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

fn emit_binary_smoke_state(label: &str, state: serde_json::Value) {
    eprintln!("[main][smoke][state] {label} {state}");
}

async fn run_binary_smoke(launch_request: crate::app::cli::LaunchRequest) -> Result<(), String> {
    if std::env::var_os("SAYA_COMPLETION_SMOKE").is_some() {
        return run_binary_completion_smoke(launch_request).await;
    }
    eprintln!("[main][smoke] preparing headless launch");
    let mut outcome =
        crate::app::bootstrap::prepare_launch(launch_request).map_err(format_bootstrap_error)?;
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
    launch_request: crate::app::cli::LaunchRequest,
) -> Result<(), String> {
    eprintln!("[main][smoke][completion] preparing headless launch");
    let mut outcome =
        crate::app::bootstrap::prepare_launch(launch_request).map_err(format_bootstrap_error)?;
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
    // ADR 0006 Phase 2/3: legacy wrapper `startup_keymap_action_for_snapshot_input`
    // を廃止し、単キー解決は下位純粋関数 `startup_keymap_action_for_input` を直接使う。
    // スモークは単キー `<C-x>` を pending 無しで 1 回解決するだけなので、2 キー
    // prefix 結合能力（本番は単一パイプライン `resolve_pipeline_command_buffered`
    // が担う）は不要。`core_bridge.mode()` は `light_snapshot().mode` と同値であり、
    // 解決結果（mode + lhs から `startup_keymap_action_for_lhs`）は移管前と等価。
    let action = startup_keymap_action_for_input(
        &outcome.startup_registry.keymaps,
        outcome.core_bridge.mode(),
        &KeyInput::Ctrl('x'),
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
        let selected_window = floating_window_manager
            .windows()
            .iter()
            .find(|window| matches!(window.content, FloatingContentRef::CompletionMenu { .. }));
        let selected_lines = selected_window
            .map(|window| window.lines.clone())
            .unwrap_or_default();
        let selected_inline_styles = selected_window
            .map(|window| {
                window
                    .inline_styles
                    .iter()
                    .map(|style| {
                        serde_json::json!({
                            "kind": format!("{:?}", style.kind),
                            "line": style.line,
                            "columnStart": style.column_start,
                            "columnEnd": style.column_end,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        eprintln!(
            "[main][smoke][completion] menu after Down: lines={:?}",
            selected_lines
        );
        emit_binary_smoke_state(
            "completion-menu-after-down",
            serde_json::json!({
                "lines": selected_lines,
                "inlineStyles": selected_inline_styles,
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

async fn run_binary_pty_smoke(
    launch_request: crate::app::cli::LaunchRequest,
) -> Result<(), String> {
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

fn perform_job_control_suspend_cycle<B: TerminalBackend>(
    terminal_broker: &mut crate::terminal::io_broker::TerminalIoBroker<'_, B>,
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
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    outcome_accumulator: &mut MainOutcomeAccumulator,
    session_state: &mut crate::app::session::EditorSessionState,
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
    capability_profile: &crate::terminal::capability::TerminalCapabilityProfile,
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
#[path = "program_dired_candidates_test.rs"]
mod program_dired_candidates_test;
#[cfg(test)]
#[path = "program_host_command_test.rs"]
mod program_host_command_test;
#[cfg(test)]
#[path = "program_orchestration_test.rs"]
mod program_orchestration_test;
#[cfg(test)]
#[path = "program_shutdown_test.rs"]
mod program_shutdown_test;
#[cfg(test)]
#[path = "program_smoke_test.rs"]
mod program_smoke_test;
#[cfg(test)]
#[path = "program_test_support.rs"]
mod program_test_support;
