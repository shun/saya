//! dired 確認キーの処理とディレクトリバッファの VFS 保存・読込要求の処理。

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectoryOperationConfirmationKeyAction {
    Confirm,
    Cancel,
    KeepWaiting,
}

pub fn directory_operation_confirmation_key_action(
    key: &KeyInput,
    session_state: &crate::app::session::EditorSessionState,
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

pub fn directory_operation_cancel_message(
    session_state: &mut crate::app::session::EditorSessionState,
) -> String {
    match session_state.cancel_directory_buffer_operation_preview() {
        Some(_) => "Directory operation cancelled; no filesystem changes were applied".to_string(),
        None => "No directory operation preview to cancel".to_string(),
    }
}

pub async fn handle_directory_operation_confirmation_key_with_runtime(
    key: &KeyInput,
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    session_state: &mut crate::app::session::EditorSessionState,
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

pub fn merge_confirmation_shutdown_reason(
    write_post_reason: Option<ShutdownReason>,
    pending_quit_reason: Option<ShutdownReason>,
) -> Option<ShutdownReason> {
    let mut reason = write_post_reason;
    merge_shutdown_reason(&mut reason, pending_quit_reason);
    reason
}

pub fn refresh_directory_buffer_after_confirmed_save(
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    session_state: &mut crate::app::session::EditorSessionState,
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

pub fn handle_directory_buffer_vfs_save_request(
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    session_state: &mut crate::app::session::EditorSessionState,
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

pub fn handle_directory_buffer_vfs_load_request(
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    session_state: &mut crate::app::session::EditorSessionState,
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

pub fn path_from_core_vfs_document_id(document_id: &str) -> Option<std::path::PathBuf> {
    document_id
        .strip_prefix("file://")
        .map(std::path::PathBuf::from)
        .or_else(|| Some(std::path::PathBuf::from(document_id)).filter(|path| path.exists()))
}
