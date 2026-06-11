//! ランタイムホストコマンドのディスパッチと実行（オーケストレーション中核）。
//!
//! `MainRuntimeHostSession`（TypeScript ランタイムのホスト側セッション実装）と、
//! ホストコマンドの実行ハブ（保存・終了・編集・dired・各種フロート・LSP）、
//! バッファイベント dispatch、startup keymap 実行、保存スナップショット処理を
//! 担う。イベントループ本体（`process_pending_host_actions_*` / consume 系）と
//! `MainOutcomeAccumulator` はバイナリ側（`main.rs`）に残し、本モジュールは
//! それらから呼ばれる実行系を提供する。

use crate::app::bootstrap::{StartupKeymapAction, StartupKeymapMode};
use crate::app::event_loop::ShutdownReason;
use crate::app::host_command::{
    MainHostCommand, parse_main_host_command, runtime_save_then_quit_ex_command,
};
use crate::app::host_io::{SaveRequest, SaveResult, write_to_path};
use crate::app::session::{
    DirectoryBufferPreviewConfirmationError, QuitDecision, SaveRequestError,
};
use crate::core::host_actions::HostActionRuntime;
use crate::core::outcome::{
    ApplicationOutcomeState, NormalizedHostDirective, fold_normalized_outcomes,
};
use crate::features::completion::float::CompletionFloatManager;
use crate::features::completion::session::CompletionShowRequest;
use crate::features::dired::{
    RuntimeInputPromptUiState, apply_directory_buffer_operation_plan,
    directory_buffer_listing_options_from_runtime, directory_entries_for_runtime_entries,
    execute_runtime_filer_operation, paths_refer_to_same_location_main,
    runtime_filer_kind_from_directory_entry,
};
use crate::features::lsp::float::{LspDiagnosticStore, file_uri_to_path};
use crate::features::lsp::host_commands::{
    execute_lsp_code_actions_float_host_command, execute_lsp_cycle_diagnostic_host_command,
    execute_lsp_diagnostic_float_host_command, execute_lsp_hover_float_host_command,
    execute_lsp_location_list_float_host_command, execute_lsp_publish_diagnostics_host_command,
    execute_lsp_status_host_command, execute_lsp_symbol_outline_float_host_command,
    execute_lsp_workspace_edit_preview_host_command,
};
use crate::features::lsp::lsif_index::LsifIndexCache;
use crate::features::lsp::runtime_bridge::{LspRuntimeBridgeRequest, LspRuntimeBridgeResponse};
use crate::input::router::KeyInput;
use crate::presentation::floating_window::{FloatingWindowId, FloatingWindowManager};
use crate::presentation::overlay::effect::RuntimePresentationIntent;
use crate::presentation::panel::PanelManager;
use crate::presentation::runtime_commands::{
    execute_buffer_window_float_host_command, execute_runtime_panel_close,
    execute_runtime_panel_open, execute_runtime_window_close_float,
    execute_runtime_window_open_float, execute_terminal_close_float_host_command,
    execute_terminal_float_host_command, runtime_float_snapshots, runtime_panel_snapshot,
};
use crate::runtime::integration::{
    RuntimeCommandEffect, RuntimeDispatchOutcome, RuntimeEventMapper, RuntimeHostSession,
    RuntimeInputPromptHostResponse, RuntimeSessionOwner, RuntimeShutdownIntent,
};
use crate::runtime::live::{
    ReadonlyBufferSnapshot, ReadonlyEditorSnapshot, ReadonlyWindowSnapshot, RuntimeCommandError,
    RuntimeFilerCurrentEntry, RuntimeFilerEntry, RuntimeFilerError, RuntimeFilerListOptions,
    RuntimeFilerOperation, RuntimeFilerOperationReport, RuntimeFloatOpenRequest,
    RuntimeFloatSnapshot, RuntimeInputPromptRequest, RuntimeInputPromptResponse, RuntimeMode,
    RuntimePanelOpenRequest, RuntimePanelSnapshot,
};
use crate::terminal::float::TerminalFloatManager;
use vim_core_rs::{CoreMode, CoreVfsError, CoreVfsErrorKind, CoreVfsRequest, CoreVfsResponse};

use std::sync::Arc;
#[derive(Debug, Default)]
pub struct WriteHostActionEffect {
    pub shutdown_reason: Option<ShutdownReason>,
    pub pending_directory_confirmation: bool,
}

pub async fn handle_write_host_action_with_runtime(
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    session_state: &mut crate::app::session::EditorSessionState,
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

pub fn execute_runtime_host_command_through_core(
    ex_command: &str,
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    session_state: &mut crate::app::session::EditorSessionState,
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

pub fn runtime_edit_ex_command(
    path: &std::path::Path,
    outcome: &crate::app::bootstrap::BootstrapOutcome,
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

pub fn execute_runtime_host_command(
    command: &str,
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    session_state: &mut crate::app::session::EditorSessionState,
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

pub fn is_swap_attention_message(message: &str) -> bool {
    message.starts_with("E301: ") || message.starts_with("E325: ")
}

pub fn execute_runtime_host_command_with_floats(
    command: &str,
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    session_state: &mut crate::app::session::EditorSessionState,
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

pub fn execute_lsp_goto_definition_host_command(
    payload: &str,
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    session_state: &mut crate::app::session::EditorSessionState,
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

pub fn first_lsp_location(value: &serde_json::Value) -> Option<&serde_json::Value> {
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

pub fn escape_runtime_edit_path(path: &std::path::Path) -> String {
    path.to_string_lossy()
        .chars()
        .flat_map(|ch| match ch {
            '\\' => ['\\', '\\'].into_iter().collect::<Vec<_>>(),
            ' ' => ['\\', ' '].into_iter().collect::<Vec<_>>(),
            _ => [ch].into_iter().collect::<Vec<_>>(),
        })
        .collect()
}

pub async fn dispatch_buffer_open_with_runtime(
    runtime_session: Option<&mut RuntimeSessionOwner>,
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    session_state: &mut crate::app::session::EditorSessionState,
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

pub async fn dispatch_buffer_write_post_with_runtime(
    runtime_session: Option<&mut RuntimeSessionOwner>,
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    session_state: &mut crate::app::session::EditorSessionState,
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

pub async fn dispatch_buffer_changed_with_runtime(
    runtime_session: Option<&mut RuntimeSessionOwner>,
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    session_state: &mut crate::app::session::EditorSessionState,
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

pub fn merge_runtime_dispatch_outcome(
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

pub fn apply_runtime_dispatch_outcome(
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

#[derive(Clone, Default)]
pub struct LsifBridgeHandle {
    cache: Arc<std::sync::Mutex<LsifIndexCache>>,
    diagnostic_events: Arc<std::sync::Mutex<Vec<String>>>,
}

pub fn runtime_mode_from_core(mode: CoreMode) -> RuntimeMode {
    match mode {
        CoreMode::Insert => RuntimeMode::Insert,
        CoreMode::Visual | CoreMode::VisualLine | CoreMode::VisualBlock => RuntimeMode::Visual,
        _ => RuntimeMode::Normal,
    }
}

pub fn clear_stale_quit_warning_after_write_attempt(
    system_warning: &mut Option<String>,
    write_message: Option<&str>,
) {
    if write_message.is_some() && system_warning.as_deref() == Some(normal_quit_warning_message()) {
        *system_warning = None;
    }
}

pub fn prioritize_save_family_host_directives(
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

pub fn normal_quit_warning_message() -> &'static str {
    "No write since last change (add ! to override)"
}

mod command_line;
mod directory;
mod floating_ui;
mod host_actions;
mod host_session;
mod input_prompt;
mod input_stages;
mod mouse_input;
mod save_snapshot;
mod selector;
mod shutdown_intent;
mod startup_keymap;

pub use command_line::*;
pub use directory::*;
pub use floating_ui::*;
pub use host_actions::*;
pub use host_session::*;
pub use input_prompt::*;
pub use input_stages::*;
pub use mouse_input::*;
pub use save_snapshot::*;
pub use selector::*;
pub use shutdown_intent::*;
pub use startup_keymap::*;
