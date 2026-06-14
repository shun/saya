//! Input 腕の個別キー処理ステージ。completion float、floating window、
//! resolve_intent 経由の core dispatch を main loop から分離する。

use crate::app::event_loop::ShutdownReason;
use crate::app::outcome_consume::{MainOutcomeAccumulator, consume_core_outcomes_from_core};
use crate::app::runtime_dispatch::{
    LsifBridgeHandle, dispatch_buffer_changed_with_runtime,
    dispatch_buffer_write_post_with_runtime, save_snapshot_result,
};
use crate::app::runtime_dispatch::{
    process_pending_host_actions_with_runtime, shutdown_reason_from_quit_decision,
    sync_session_dirty_from_core,
};
use crate::core::host_actions::HostActionRuntime;
use crate::features::completion::float::CompletionFloatManager;
use crate::features::lsp::float::LspDiagnosticStore;
use crate::input::router::{EditorIntent, KeyInput, NavigationKey, resolve_intent};
use crate::presentation::floating_input::{
    FloatingWindowKeyHandling, apply_floating_lifecycle_after_core_edit,
    handle_completion_float_key, handle_floating_window_key,
};
use crate::presentation::floating_window::FloatingWindowManager;
use crate::presentation::overlay::effect::RuntimePresentationIntent;
use crate::presentation::panel::PanelManager;
use crate::presentation::screen_model::WorkspaceScreenModel;
use crate::presentation::viewport::ViewportSyncMode;
use crate::runtime::integration::RuntimeSessionOwner;
use crate::terminal::float::TerminalFloatManager;

use crate::app::bootstrap::BootstrapOutcome;
use crate::app::session::EditorSessionState;
use crate::core::bridge::CoreBridge;
use crate::presentation::render::redraw_trace::trace_redraw_diagnostic;
use vim_core_rs::CoreLightSnapshot;

/// 入力処理で使う対象 window id を解決する。
pub fn resolve_input_active_window_id(
    input_snapshot: &CoreLightSnapshot,
    last_workspace_model: Option<&WorkspaceScreenModel>,
    core_bridge: &mut CoreBridge,
) -> i32 {
    input_snapshot
        .active_window_id()
        .or_else(|| last_workspace_model.map(|workspace| workspace.active_window_id))
        .or_else(|| core_bridge.snapshot().active_window_id())
        .unwrap_or(0)
}

pub fn viewport_sync_mode_for_input(key: &KeyInput) -> ViewportSyncMode {
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

/// completion float へのキー入力を処理する。shutdown が必要なら理由を返す。
#[allow(clippy::too_many_arguments)]
pub async fn dispatch_completion_float_key(
    key: &KeyInput,
    active_window_id: i32,
    outcome: &mut BootstrapOutcome,
    outcome_accumulator: &mut MainOutcomeAccumulator,
    session_state: &mut EditorSessionState,
    transient_msg: &mut Option<String>,
    system_warning: &mut Option<String>,
    host_action_runtime: &mut HostActionRuntime,
    runtime_session: &mut Option<RuntimeSessionOwner>,
    need_redraw: &mut bool,
    runtime_presentation_intents: &mut Vec<RuntimePresentationIntent>,
    lsif_bridge: &LsifBridgeHandle,
    floating_window_manager: &mut FloatingWindowManager,
    completion_float_manager: &mut CompletionFloatManager,
    lsp_diagnostic_store: &mut LspDiagnosticStore,
    terminal_float_manager: &mut TerminalFloatManager,
    panel_manager: &mut PanelManager,
    handled: &mut bool,
    workspace_projection_dirty: &mut bool,
) -> Option<ShutdownReason> {
    let before_completion_snapshot = outcome.core_bridge.light_snapshot();
    if let Some(effect) = handle_completion_float_key(
        completion_float_manager,
        floating_window_manager,
        &mut outcome.core_bridge,
        key,
        active_window_id,
    ) {
        match effect {
            FloatingWindowKeyHandling::Consumed => {
                *handled = true;
                *need_redraw = true;
                *workspace_projection_dirty = true;
            }
            FloatingWindowKeyHandling::Closed { id } => {
                *handled = true;
                *need_redraw = true;
                *workspace_projection_dirty = true;
                log::debug!(
                    "[main] completion float closed from focused input: id={}",
                    id.0
                );
                consume_core_outcomes_from_core(
                    &mut outcome.core_bridge,
                    outcome_accumulator,
                    need_redraw,
                );

                if let Some(reason) = process_pending_host_actions_with_runtime(
                    outcome,
                    outcome_accumulator,
                    session_state,
                    transient_msg,
                    system_warning,
                    host_action_runtime,
                    runtime_session.as_mut(),
                    need_redraw,
                    runtime_presentation_intents,
                    Some(lsif_bridge),
                )
                .await
                {
                    return Some(reason);
                }
                let after_completion_snapshot = outcome.core_bridge.light_snapshot();
                if after_completion_snapshot.revision != before_completion_snapshot.revision
                    && let Some(reason) = dispatch_buffer_changed_with_runtime(
                        runtime_session.as_mut(),
                        outcome,
                        session_state,
                        transient_msg,
                        need_redraw,
                        runtime_presentation_intents,
                        floating_window_manager,
                        completion_float_manager,
                        lsp_diagnostic_store,
                        terminal_float_manager,
                        panel_manager,
                        Some(lsif_bridge),
                    )
                    .await
                {
                    return Some(reason);
                }
                sync_session_dirty_from_core(session_state, &outcome.core_bridge);
            }
        }
    }
    None
}

/// floating window へのキー入力を処理する。
pub fn dispatch_floating_window_key(
    key: &KeyInput,
    active_window_id: i32,
    floating_window_manager: &mut FloatingWindowManager,
    handled: &mut bool,
    need_redraw: &mut bool,
    workspace_projection_dirty: &mut bool,
) {
    if let Some(effect) = handle_floating_window_key(floating_window_manager, key, active_window_id)
    {
        match effect {
            FloatingWindowKeyHandling::Consumed => {
                *handled = true;
                *need_redraw = true;
                *workspace_projection_dirty = true;
            }
            FloatingWindowKeyHandling::Closed { id } => {
                *handled = true;
                *need_redraw = true;
                *workspace_projection_dirty = true;
                log::debug!(
                    "[main] floating window closed from focused input: id={}",
                    id.0
                );
            }
        }
    }
}

/// ADR 0006 Phase 3: 完成したキー列を core へ 1 回 dispatch し、その後段処理
/// （outcome 回収・host action・buffer-changed・dirty 同期・redraw）をまとめて行う。
///
/// 旧 `dispatch_resolved_intent_key` の `EditKey` 腕から抽出した共通処理。
/// Phase 3 の buffered パイプライン（完成コマンドのみ越境）と、legacy intent 経路の
/// 両方から再利用する。引数 `keys` は **完成済みのキー列**（例 `"gg"`, `"dw"`, `"3j"`）
/// であり、部分入力（pending）は呼び出し側で host にバッファ済みである前提。
#[allow(clippy::too_many_arguments)]
pub async fn dispatch_complete_keys_to_core(
    keys: &str,
    outcome: &mut BootstrapOutcome,
    outcome_accumulator: &mut MainOutcomeAccumulator,
    session_state: &mut EditorSessionState,
    transient_msg: &mut Option<String>,
    system_warning: &mut Option<String>,
    host_action_runtime: &mut HostActionRuntime,
    runtime_session: &mut Option<RuntimeSessionOwner>,
    need_redraw: &mut bool,
    runtime_presentation_intents: &mut Vec<RuntimePresentationIntent>,
    lsif_bridge: &LsifBridgeHandle,
    floating_window_manager: &mut FloatingWindowManager,
    completion_float_manager: &mut CompletionFloatManager,
    lsp_diagnostic_store: &mut LspDiagnosticStore,
    terminal_float_manager: &mut TerminalFloatManager,
    panel_manager: &mut PanelManager,
    workspace_projection_dirty: &mut bool,
) -> Option<ShutdownReason> {
    let before_snapshot = outcome.core_bridge.light_snapshot();
    let need_redraw_before_dispatch = *need_redraw;
    log::info!(
        "[main][pipeline] dispatching complete command sequence to backend (only complete commands cross the boundary): keys={:?}, dispatch_key_count_before={}",
        keys,
        outcome.core_bridge.dispatch_key_count()
    );
    let dispatch_result = outcome.core_bridge.dispatch_key(keys);
    let after_snapshot = outcome.core_bridge.light_snapshot();
    log::info!(
        "[main][pipeline] complete command dispatched: keys={:?}, dispatch_key_count_after={}, result={:?}",
        keys,
        outcome.core_bridge.dispatch_key_count(),
        dispatch_result
    );
    if apply_floating_lifecycle_after_core_edit(
        floating_window_manager,
        &before_snapshot,
        &after_snapshot,
    ) {
        *workspace_projection_dirty = true;
    }
    trace_redraw_diagnostic(format_args!(
        "edit keys dispatched: keys={:?}, result={:?}, revision {}->{}, cursor ({},{}) -> ({},{}), mode {:?}->{:?}, need_redraw_before={}",
        keys,
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
    consume_core_outcomes_from_core(&mut outcome.core_bridge, outcome_accumulator, need_redraw);

    if let Some(reason) = process_pending_host_actions_with_runtime(
        outcome,
        outcome_accumulator,
        session_state,
        transient_msg,
        system_warning,
        host_action_runtime,
        runtime_session.as_mut(),
        need_redraw,
        runtime_presentation_intents,
        Some(lsif_bridge),
    )
    .await
    {
        return Some(reason);
    }

    if after_snapshot.revision != before_snapshot.revision {
        if let Some(reason) = dispatch_buffer_changed_with_runtime(
            runtime_session.as_mut(),
            outcome,
            session_state,
            transient_msg,
            need_redraw,
            runtime_presentation_intents,
            floating_window_manager,
            completion_float_manager,
            lsp_diagnostic_store,
            terminal_float_manager,
            panel_manager,
            Some(lsif_bridge),
        )
        .await
        {
            return Some(reason);
        }
    }

    sync_session_dirty_from_core(session_state, &outcome.core_bridge);
    trace_redraw_diagnostic(format_args!(
        "edit keys host policy forcing redraw after dispatch: keys={:?}, cursor=({},{}), revision={}, prior_need_redraw={}",
        keys,
        after_snapshot.cursor_row,
        after_snapshot.cursor_col,
        after_snapshot.revision,
        *need_redraw
    ));
    *need_redraw = true;
    None
}

/// startup keymap で消費されなかったキーを intent 解決して core へディスパッチする。
#[allow(clippy::too_many_arguments)]
pub async fn dispatch_resolved_intent_key(
    key: &KeyInput,
    outcome: &mut BootstrapOutcome,
    outcome_accumulator: &mut MainOutcomeAccumulator,
    session_state: &mut EditorSessionState,
    transient_msg: &mut Option<String>,
    system_warning: &mut Option<String>,
    host_action_runtime: &mut HostActionRuntime,
    runtime_session: &mut Option<RuntimeSessionOwner>,
    need_redraw: &mut bool,
    runtime_presentation_intents: &mut Vec<RuntimePresentationIntent>,
    lsif_bridge: &LsifBridgeHandle,
    floating_window_manager: &mut FloatingWindowManager,
    completion_float_manager: &mut CompletionFloatManager,
    lsp_diagnostic_store: &mut LspDiagnosticStore,
    terminal_float_manager: &mut TerminalFloatManager,
    panel_manager: &mut PanelManager,
    startup_keymap_pending_lhs: &mut Option<String>,
    viewport_sync_mode: &mut ViewportSyncMode,
    workspace_projection_dirty: &mut bool,
) -> Option<ShutdownReason> {
    *startup_keymap_pending_lhs = None;
    let intent = resolve_intent(key);
    log::info!(
        "[main][input] key not handled by startup keymap, dispatching intent: key={:?}, intent={:?}",
        key,
        intent
    );
    match intent {
        EditorIntent::EditKey(k) => {
            *viewport_sync_mode = viewport_sync_mode_for_input(key);
            if let Some(reason) = dispatch_complete_keys_to_core(
                &k,
                outcome,
                outcome_accumulator,
                session_state,
                transient_msg,
                system_warning,
                host_action_runtime,
                runtime_session,
                need_redraw,
                runtime_presentation_intents,
                lsif_bridge,
                floating_window_manager,
                completion_float_manager,
                lsp_diagnostic_store,
                terminal_float_manager,
                panel_manager,
                workspace_projection_dirty,
            )
            .await
            {
                return Some(reason);
            }
        }
        EditorIntent::Save => {
            let snapshot = outcome.core_bridge.snapshot();
            let save_outcome = save_snapshot_result(&snapshot.text, session_state);
            *transient_msg = save_outcome.transient_message;
            if save_outcome.wrote {
                if let Some(reason) = dispatch_buffer_write_post_with_runtime(
                    runtime_session.as_mut(),
                    outcome,
                    session_state,
                    transient_msg,
                    need_redraw,
                    runtime_presentation_intents,
                    Some(lsif_bridge),
                )
                .await
                {
                    return Some(reason);
                }
            }
            *need_redraw = true;
        }
        EditorIntent::Quit { force } => {
            let decision = session_state.evaluate_quit(force);
            if let Some(reason) =
                shutdown_reason_from_quit_decision(decision, force, system_warning)
            {
                return Some(reason);
            }
            *need_redraw = true;
        }
    }
    None
}
