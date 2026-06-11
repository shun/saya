//! コマンドライン（`:` / `/`）編集中のキー入力ディスパッチ。履歴操作、
//! ex コマンドのルーティング、検索入力の同期、pending host action 処理を担う。

use crate::app::event_loop::ShutdownReason;
use crate::app::host_command::startup_registered_command_name_for_ex_command;
use crate::app::outcome_consume::{MainOutcomeAccumulator, consume_core_outcomes_from_core};
use crate::app::runtime_dispatch::{LsifBridgeHandle, execute_startup_keymap_registered_command};
use crate::app::runtime_dispatch::{
    process_pending_host_actions_with_runtime, sync_session_dirty_from_core,
};
use crate::core::host_actions::HostActionRuntime;
use crate::features::completion::float::CompletionFloatManager;
use crate::features::dired::RuntimeInputPromptUiState;
use crate::features::lsp::float::LspDiagnosticStore;
use crate::input::command_line_editor::{CommandLineEdit, command_line_edit_action_for_key};
use crate::input::command_line_history::{
    history_direction_for_key, record_history_and_save_to_default_cache,
};
use crate::input::ex_command::{ExCommandRoute, apply_local_ex_command, route_ex_command};
use crate::input::router::KeyInput;
use crate::presentation::floating_window::FloatingWindowManager;
use crate::presentation::overlay::effect::RuntimePresentationIntent;
use crate::presentation::panel::PanelManager;
use crate::runtime::integration::RuntimeSessionOwner;
use crate::terminal::float::TerminalFloatManager;

use crate::app::bootstrap::BootstrapOutcome;
use crate::app::session::EditorSessionState;
use crate::input::command_line_history::CommandLineHistories;

/// コマンドライン編集中のキー入力を処理する。shutdown が必要なら理由を返す。
#[allow(clippy::too_many_arguments)]
pub async fn dispatch_command_line_key(
    key: &KeyInput,
    prompt: char,
    command_line_prompt: &mut Option<char>,
    command_line_edit: &mut CommandLineEdit,
    command_line_histories: &mut CommandLineHistories,
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
    runtime_input_prompt: &mut Option<RuntimeInputPromptUiState>,
) -> Option<ShutdownReason> {
    match key {
        KeyInput::Up
        | KeyInput::Down
        | KeyInput::Ctrl('p')
        | KeyInput::Ctrl('P')
        | KeyInput::Ctrl('n')
        | KeyInput::Ctrl('N') => {
            if let Some(direction) = history_direction_for_key(key) {
                if let Some(selected_buffer) =
                    command_line_histories.navigate(prompt, command_line_edit.buffer(), direction)
                {
                    command_line_edit.set_buffer_to_end(selected_buffer);
                    if prompt == '/' {
                        let _ = outcome
                            .core_bridge
                            .sync_search_input(command_line_edit.buffer());
                        consume_core_outcomes_from_core(
                            &mut outcome.core_bridge,
                            outcome_accumulator,
                            need_redraw,
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
                    outcome_accumulator,
                    need_redraw,
                );
            }
            *command_line_prompt = None;
            command_line_edit.clear();
            command_line_histories.reset_navigation();
        }
        KeyInput::Enter => {
            if prompt == ':' {
                let cmd = format!("{}{}", prompt, command_line_edit.buffer());
                record_history_and_save_to_default_cache(
                    command_line_histories,
                    prompt,
                    command_line_edit.buffer(),
                );
                *command_line_prompt = None;
                command_line_edit.clear();
                match route_ex_command(&cmd) {
                    ExCommandRoute::NoOp => {
                        log::debug!("[main] empty ex command completed as no-op");
                    }
                    ExCommandRoute::PresentationLocal => {
                        if let Some(message) = apply_local_ex_command(session_state, &cmd) {
                            *transient_msg = Some(message);
                        } else {
                            log::debug!(
                                "[main] presentation-local route fell through to core-owned handler: command={:?}",
                                cmd
                            );
                            let _ = outcome.core_bridge.apply_ex_command(&cmd);
                            consume_core_outcomes_from_core(
                                &mut outcome.core_bridge,
                                outcome_accumulator,
                                need_redraw,
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
                            outcome_accumulator,
                            need_redraw,
                        );
                    }
                    ExCommandRoute::CoreOwned => {
                        if let Some(command_name) = startup_registered_command_name_for_ex_command(
                            &cmd,
                            &outcome.callback_registry,
                        ) {
                            log::info!(
                                "[main][command_line] executing startup registered command from ex command: command={}",
                                command_name
                            );
                            if let Some(reason) = execute_startup_keymap_registered_command(
                                runtime_session.as_mut(),
                                &command_name,
                                outcome,
                                session_state,
                                floating_window_manager,
                                completion_float_manager,
                                lsp_diagnostic_store,
                                terminal_float_manager,
                                panel_manager,
                                Some(runtime_input_prompt),
                                transient_msg,
                                need_redraw,
                                runtime_presentation_intents,
                                Some(lsif_bridge),
                            )
                            .await
                            {
                                return Some(reason);
                            }
                        } else {
                            let _ = outcome.core_bridge.apply_ex_command(&cmd);
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
                        }
                    }
                    ExCommandRoute::UnsupportedPlanned => {
                        log::debug!(
                            "[main] set option command is registered but not implemented in host I/O yet: command={:?}",
                            cmd
                        );
                        *transient_msg =
                            Some("This option is planned but not supported yet".to_string());
                    }
                }
            } else if prompt == '/' {
                record_history_and_save_to_default_cache(
                    command_line_histories,
                    prompt,
                    command_line_edit.buffer(),
                );
                let _ = outcome
                    .core_bridge
                    .commit_search_input(command_line_edit.buffer());
                consume_core_outcomes_from_core(
                    &mut outcome.core_bridge,
                    outcome_accumulator,
                    need_redraw,
                );
                *command_line_prompt = None;
                command_line_edit.clear();
            }
            sync_session_dirty_from_core(session_state, &outcome.core_bridge);
        }
        KeyInput::Backspace | KeyInput::Ctrl('h') | KeyInput::Ctrl('H') | KeyInput::Delete => {
            command_line_histories.reset_navigation();
            let changed = command_line_edit.apply_action(
                command_line_edit_action_for_key(key)
                    .expect("backspace/delete must map to command-line edit action"),
            );
            if prompt == '/' && changed {
                let _ = outcome
                    .core_bridge
                    .sync_search_input(command_line_edit.buffer());
                consume_core_outcomes_from_core(
                    &mut outcome.core_bridge,
                    outcome_accumulator,
                    need_redraw,
                );
            } else if prompt == ':' && !changed {
                *command_line_prompt = None;
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
            if let Some(action) = command_line_edit_action_for_key(key) {
                command_line_edit.apply_action(action);
            }
        }
        KeyInput::Char(c) => {
            command_line_histories.reset_navigation();
            command_line_edit.insert_char(*c);
            if prompt == '/' {
                let _ = outcome
                    .core_bridge
                    .sync_search_input(command_line_edit.buffer());
                consume_core_outcomes_from_core(
                    &mut outcome.core_bridge,
                    outcome_accumulator,
                    need_redraw,
                );
            }
        }
        _ => {}
    }
    *need_redraw = true;

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
    None
}
