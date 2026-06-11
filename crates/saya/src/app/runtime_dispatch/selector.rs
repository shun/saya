//! selector の key routing ディスパッチ。route 解決済みの selector キーを
//! runtime セッションへ仲介し、dispatch outcome を UI 状態へ反映する。

use crate::app::bootstrap::BootstrapOutcome;
use crate::app::runtime_dispatch::{
    MainRuntimeHostSession, apply_runtime_dispatch_outcome, escape_runtime_edit_path,
    execute_runtime_host_command_with_floats, merge_runtime_dispatch_outcome,
};
use crate::app::session::EditorSessionState;
use crate::features::selector::keymap::{
    SelectorAction, SelectorKeyRoute, SelectorModeSwitch, selector_key_route_for_model,
};
use crate::features::selector::runtime::{
    RuntimeRgLocation, RuntimeSelectorControllerCommand, parse_rg_selector_location_detail,
};
use crate::features::selector::tui_state::{SelectorMode, SelectorTuiViewModel};
use crate::input::router::KeyInput;
use crate::presentation::overlay::effect::RuntimePresentationIntent;
use crate::runtime::integration::{RuntimeDispatchOutcome, RuntimeSessionOwner};
use crate::runtime::live::RuntimeCommandError;

/// selector の key routing を処理し、キーを消費したかどうかを返す。
pub async fn dispatch_selector_key_route(
    key: &KeyInput,
    runtime_session: &mut Option<RuntimeSessionOwner>,
    outcome: &mut BootstrapOutcome,
    session_state: &mut EditorSessionState,
    transient_msg: &mut Option<String>,
    need_redraw: &mut bool,
    runtime_presentation_intents: &mut Vec<RuntimePresentationIntent>,
) -> bool {
    let mut handled = false;
    let selector_model = runtime_session.as_ref().and_then(|runtime_session| {
        runtime_session
            .selector_tui_projection_sink()
            .current_model()
    });
    match selector_key_route_for_model(selector_model.as_ref(), key) {
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
                            outcome,
                            session_state,
                        )
                        .await
                    }
                };
                let _ = apply_runtime_dispatch_outcome(
                    transient_msg,
                    need_redraw,
                    runtime_presentation_intents,
                    dispatch_outcome,
                );
            } else {
                log::debug!(
                    "[main][selector] selector action route resolved without runtime session/model: key={:?}, session_id={}, action={:?}",
                    key,
                    session_id,
                    action
                );
                *transient_msg = Some("Selector runtime session is not available".to_string());
                *need_redraw = true;
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
                let mut host_session = MainRuntimeHostSession::new(outcome, session_state);
                let dispatch_outcome = runtime_session
                    .control_selector(session_id, command, &mut host_session)
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
                    transient_msg,
                    need_redraw,
                    runtime_presentation_intents,
                    dispatch_outcome,
                );
            } else {
                log::debug!(
                    "[main][selector] selector key route resolved without runtime session: key={:?}, session_id={}, command={:?}",
                    key,
                    session_id,
                    command
                );
                *transient_msg = Some("Selector runtime session is not available".to_string());
                *need_redraw = true;
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
                let mut host_session = MainRuntimeHostSession::new(outcome, session_state);
                let dispatch_outcome = runtime_session
                    .update_selector_query(session_id, next_query, &mut host_session)
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
                    transient_msg,
                    need_redraw,
                    runtime_presentation_intents,
                    dispatch_outcome,
                );
            } else {
                log::debug!(
                    "[main][selector] selector query edit route resolved without runtime session/model: key={:?}, session_id={}, edit={:?}",
                    key,
                    session_id,
                    edit
                );
                *transient_msg = Some("Selector runtime session is not available".to_string());
                *need_redraw = true;
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
                        *need_redraw = true;
                    }
                    Err(error) => {
                        log::debug!(
                            "[main][selector] selector mode switch failed: key={:?}, session_id={}, mode={:?}, error={:?}",
                            key,
                            session_id,
                            mode,
                            error
                        );
                        *transient_msg = Some(format!("Selector mode switch failed: {:?}", error));
                        *need_redraw = true;
                    }
                }
            } else {
                log::debug!(
                    "[main][selector] selector mode switch route resolved without runtime session: key={:?}, session_id={}, switch={:?}",
                    key,
                    session_id,
                    switch
                );
                *transient_msg = Some("Selector runtime session is not available".to_string());
                *need_redraw = true;
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
    handled
}

pub async fn handle_selector_accept_action(
    runtime_session: &mut RuntimeSessionOwner,
    selector_model: &SelectorTuiViewModel,
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    session_state: &mut crate::app::session::EditorSessionState,
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
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    session_state: &mut crate::app::session::EditorSessionState,
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
