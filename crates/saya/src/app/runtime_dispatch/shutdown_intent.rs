//! 終了インテントの導出・合成と、確認待ち時の保存後終了の保留処理。

use super::*;

pub fn runtime_shutdown_intent_from_quit_decision(
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

pub fn merge_runtime_shutdown_intent(
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

pub fn merge_shutdown_reason(current: &mut Option<ShutdownReason>, next: Option<ShutdownReason>) {
    match (current.clone(), next) {
        (None, Some(reason)) => *current = Some(reason),
        (Some(ShutdownReason::UserQuit), Some(ShutdownReason::UserForceQuit)) => {
            *current = Some(ShutdownReason::UserForceQuit);
        }
        _ => {}
    }
}

pub fn defer_directory_save_then_quit_if_confirmation_pending(
    session_state: &mut crate::app::session::EditorSessionState,
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

pub fn take_pending_directory_save_then_quit_shutdown(
    session_state: &mut crate::app::session::EditorSessionState,
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
