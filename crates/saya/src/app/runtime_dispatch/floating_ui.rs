//! フローティング UI へのキー入力ディスパッチ。panel / terminal float /
//! core window float / mermaid preview のキー処理を main loop から分離する。

use crate::app::event_loop::ShutdownReason;
use crate::app::outcome_consume::{MainOutcomeAccumulator, consume_core_outcomes_from_core};
use crate::app::runtime_dispatch::LsifBridgeHandle;
use crate::app::runtime_dispatch::{
    process_pending_host_actions_with_runtime, sync_session_dirty_from_core,
};
use crate::core::host_actions::HostActionRuntime;
use crate::input::command_line_editor::CommandLineEdit;
use crate::input::router::KeyInput;
use crate::presentation::floating_input::{
    FloatingWindowKeyHandling, begin_command_line_from_focused_panel, handle_core_window_float_key,
    handle_mermaid_preview_key, handle_terminal_float_key, handle_terminal_panel_key,
};
use crate::presentation::floating_window::FloatingWindowManager;
use crate::presentation::overlay::effect::RuntimePresentationIntent;
use crate::presentation::panel::PanelManager;
use crate::runtime::integration::RuntimeSessionOwner;
use crate::terminal::float::TerminalFloatManager;

use crate::app::bootstrap::BootstrapOutcome;
use crate::app::session::EditorSessionState;
use crate::input::command_line_history::CommandLineHistories;

/// フローティング UI（panel/terminal float/core window float/mermaid preview）への
/// キー入力を順に処理する。core window float 経由で shutdown 要求が出たら理由を返す。
#[allow(clippy::too_many_arguments)]
pub async fn dispatch_floating_ui_key(
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
    terminal_float_manager: &mut TerminalFloatManager,
    panel_manager: &mut PanelManager,
    command_line_prompt: &mut Option<char>,
    command_line_edit: &mut CommandLineEdit,
    command_line_histories: &mut CommandLineHistories,
    handled: &mut bool,
    workspace_projection_dirty: &mut bool,
) -> Option<ShutdownReason> {
    if !*handled {
        if let Some(prompt) =
            begin_command_line_from_focused_panel(panel_manager, key, outcome.core_bridge.mode())
        {
            *command_line_prompt = Some(prompt);
            command_line_edit.clear();
            command_line_histories.reset_navigation();
            *handled = true;
            *need_redraw = true;
            *workspace_projection_dirty = true;
        }
    }

    if !*handled {
        if let Some(effect) = handle_terminal_panel_key(panel_manager, terminal_float_manager, key)
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
                        "[main][panel] terminal panel closed from focused input: pseudo_float_id={}",
                        id.0
                    );
                }
            }
        }
    }

    if !*handled {
        if let Some(effect) =
            handle_terminal_float_key(floating_window_manager, terminal_float_manager, key)
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
                        "[main][terminal_float] terminal float closed from focused input: id={}",
                        id.0
                    );
                }
            }
        }
    }

    if !*handled {
        if let Some(effect) =
            handle_core_window_float_key(floating_window_manager, &mut outcome.core_bridge, key)
        {
            match effect {
                FloatingWindowKeyHandling::Consumed => {
                    *handled = true;
                    *need_redraw = true;
                    *workspace_projection_dirty = true;
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
                    sync_session_dirty_from_core(session_state, &outcome.core_bridge);
                }
                FloatingWindowKeyHandling::Closed { id } => {
                    *handled = true;
                    *need_redraw = true;
                    *workspace_projection_dirty = true;
                    log::debug!(
                        "[main] core-window float closed from focused input: id={}",
                        id.0
                    );
                }
            }
        }
    }

    if !*handled {
        if let Some(effect) = handle_mermaid_preview_key(session_state, key) {
            *handled = true;
            *need_redraw = true;
            *workspace_projection_dirty = true;
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
    None
}
