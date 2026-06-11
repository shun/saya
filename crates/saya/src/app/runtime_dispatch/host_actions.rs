//! pending host action の処理オーケストレーション。runtime 有無それぞれの
//! 消費経路と、quit 判定からの shutdown reason 解決を担う。

use crate::app::event_loop::ShutdownReason;
use crate::app::outcome_consume::{MainOutcomeAccumulator, consume_core_outcomes_from_core};
use crate::app::runtime_dispatch::{
    LsifBridgeHandle, clear_stale_quit_warning_after_write_attempt,
    defer_directory_save_then_quit_if_confirmation_pending,
    handle_directory_buffer_vfs_load_request, handle_directory_buffer_vfs_save_request,
    handle_write_host_action_with_runtime, merge_shutdown_reason, normal_quit_warning_message,
    prioritize_save_family_host_directives, refresh_directory_buffer_after_confirmed_save,
    save_snapshot_result_with_confirmation,
};
use crate::app::session::QuitDecision;
use crate::core::bridge::CoreBridge;
use crate::core::host_actions::HostActionRuntime;
use crate::core::outcome::NormalizedHostDirective;
use crate::presentation::overlay::effect::RuntimePresentationIntent;
use crate::runtime::integration::RuntimeSessionOwner;

pub async fn process_pending_host_actions_with_runtime(
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    outcome_accumulator: &mut MainOutcomeAccumulator,
    session_state: &mut crate::app::session::EditorSessionState,
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

pub fn sync_session_dirty_from_core(
    session_state: &mut crate::app::session::EditorSessionState,
    core_bridge: &CoreBridge,
) {
    session_state.update_dirty_at_revision(core_bridge.dirty(), Some(core_bridge.revision()));
}

pub fn process_pending_host_actions_without_runtime(
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    outcome_accumulator: &mut MainOutcomeAccumulator,
    session_state: &mut crate::app::session::EditorSessionState,
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

pub fn shutdown_reason_from_quit_decision(
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
