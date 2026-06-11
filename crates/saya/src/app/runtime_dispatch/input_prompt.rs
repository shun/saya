//! prompt 系（runtime input prompt / notification prompt）へのキー入力
//! ディスパッチを main loop から分離する。

use crate::app::event_loop::ShutdownReason;
use crate::app::outcome_consume::{MainOutcomeAccumulator, dispatch_prompt_response_command};
use crate::app::runtime_dispatch::process_pending_host_actions_with_runtime;
use crate::app::runtime_dispatch::{
    LsifBridgeHandle, MainRuntimeHostSession, apply_runtime_dispatch_outcome,
};
use crate::core::host_actions::HostActionRuntime;
use crate::core::notification_prompt::{PromptInputAction, handle_prompt_key};
use crate::features::dired::{
    RuntimeInputPromptKeyAction, RuntimeInputPromptUiState, handle_runtime_input_prompt_key,
};
use crate::input::router::KeyInput;
use crate::presentation::overlay::effect::RuntimePresentationIntent;
use crate::runtime::integration::RuntimeSessionOwner;
use crate::runtime::live::RuntimeInputPromptResponse;

use crate::app::bootstrap::BootstrapOutcome;
use crate::app::session::EditorSessionState;

/// runtime input prompt（TS ランタイムからの入力要求）へのキー入力を処理する。
#[allow(clippy::too_many_arguments)]
pub async fn dispatch_runtime_input_prompt_key(
    key: &KeyInput,
    runtime_input_prompt: &mut Option<RuntimeInputPromptUiState>,
    outcome: &mut BootstrapOutcome,
    session_state: &mut EditorSessionState,
    runtime_session: &mut Option<RuntimeSessionOwner>,
    transient_msg: &mut Option<String>,
    need_redraw: &mut bool,
    runtime_presentation_intents: &mut Vec<RuntimePresentationIntent>,
    handled: &mut bool,
) {
    if !*handled {
        if let Some(action) = handle_runtime_input_prompt_key(runtime_input_prompt.as_mut(), key) {
            *handled = true;
            *need_redraw = true;
            match action {
                RuntimeInputPromptKeyAction::Submit(value) => {
                    log::info!(
                        "[main][runtime_input] prompt submit from TUI: value_len={}",
                        value.len()
                    );
                    *runtime_input_prompt = None;
                    if let Some(runtime_session) = runtime_session.as_mut() {
                        let mut host_session = MainRuntimeHostSession::new_with_runtime_input(
                            outcome,
                            session_state,
                            runtime_input_prompt,
                        );
                        let dispatch_outcome = runtime_session
                            .respond_to_input_prompt(
                                RuntimeInputPromptResponse::Submitted { value },
                                &mut host_session,
                            )
                            .await;
                        let _ = apply_runtime_dispatch_outcome(
                            transient_msg,
                            need_redraw,
                            runtime_presentation_intents,
                            dispatch_outcome,
                        );
                    }
                }
                RuntimeInputPromptKeyAction::Cancel => {
                    log::info!("[main][runtime_input] prompt cancelled from TUI");
                    *runtime_input_prompt = None;
                    if let Some(runtime_session) = runtime_session.as_mut() {
                        let mut host_session = MainRuntimeHostSession::new_with_runtime_input(
                            outcome,
                            session_state,
                            runtime_input_prompt,
                        );
                        let dispatch_outcome = runtime_session
                            .respond_to_input_prompt(
                                RuntimeInputPromptResponse::Cancelled,
                                &mut host_session,
                            )
                            .await;
                        let _ = apply_runtime_dispatch_outcome(
                            transient_msg,
                            need_redraw,
                            runtime_presentation_intents,
                            dispatch_outcome,
                        );
                    }
                }
                RuntimeInputPromptKeyAction::Editing => {}
            }
        }
    }
}

/// notification prompt（通知バー上の確認入力）へのキー入力を処理する。
/// shutdown 要求が出たら理由を返す。
#[allow(clippy::too_many_arguments)]
pub async fn dispatch_notification_prompt_key(
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
    handled: &mut bool,
) -> Option<ShutdownReason> {
    if !*handled {
        match handle_prompt_key(&mut outcome_accumulator.projection, key) {
            PromptInputAction::Consumed | PromptInputAction::AwaitingCore => {
                *handled = true;
                *need_redraw = true;
            }
            PromptInputAction::Submit(command) | PromptInputAction::Cancel(command) => {
                *handled = true;
                dispatch_prompt_response_command(
                    &mut outcome.core_bridge,
                    outcome_accumulator,
                    command,
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
            PromptInputAction::NotPromptInput => {}
        }
    }
    None
}
