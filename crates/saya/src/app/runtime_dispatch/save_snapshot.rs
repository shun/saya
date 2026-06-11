//! 保存スナップショットの構築と、ホスト書き込み向け保存要求の組み立て。

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveSnapshotOutcome {
    pub transient_message: Option<String>,
    pub wrote: bool,
    pub pending_directory_confirmation: bool,
}

pub fn save_snapshot_result(
    buffer_contents: &str,
    session_state: &mut crate::app::session::EditorSessionState,
) -> SaveSnapshotOutcome {
    save_snapshot_result_with_path_override(buffer_contents, session_state, None)
}

pub fn save_snapshot_result_with_path_override(
    buffer_contents: &str,
    session_state: &mut crate::app::session::EditorSessionState,
    path_override: Option<&str>,
) -> SaveSnapshotOutcome {
    save_snapshot_result_with_confirmation(
        buffer_contents,
        session_state,
        path_override,
        false,
        None,
    )
}

pub fn save_snapshot_result_with_confirmation(
    buffer_contents: &str,
    session_state: &mut crate::app::session::EditorSessionState,
    path_override: Option<&str>,
    confirmed: bool,
    core_revision: Option<u64>,
) -> SaveSnapshotOutcome {
    let path_override = effective_host_write_path_override(session_state, path_override);
    if path_override.is_none() && session_state.directory_buffer().is_some() {
        if confirmed {
            return match session_state.confirm_directory_buffer_operation_preview(buffer_contents) {
                Ok(plan) => match apply_directory_buffer_operation_plan(session_state, &plan) {
                    Ok(applied_count) => {
                        session_state.clear_pending_directory_operation_preview();
                        session_state.record_save_success_at_revision(core_revision);
                        log::info!(
                            "[main][dired][writable] applied confirmed directory operation plan: root_path={}, operations={}",
                            plan.root_path.display(),
                            applied_count
                        );
                        SaveSnapshotOutcome {
                            transient_message: Some(format!(
                                "Directory operations applied: {applied_count} operation(s)"
                            )),
                            wrote: true,
                            pending_directory_confirmation: false,
                        }
                    }
                    Err(error) => {
                        log::debug!(
                            "[main][dired][writable] confirmed directory operation plan failed during apply: root_path={}, error={:?}",
                            plan.root_path.display(),
                            error
                        );
                        session_state.record_save_failure(format!("{error:?}"));
                        SaveSnapshotOutcome {
                            transient_message: Some(format!(
                                "Directory operation apply failed: {error:?}. Recovery: directory metadata was refreshed from the filesystem; inspect the listing before retrying."
                            )),
                            wrote: false,
                            pending_directory_confirmation: false,
                        }
                    }
                },
                Err(DirectoryBufferPreviewConfirmationError::MissingPreview) => {
                    SaveSnapshotOutcome {
                        transient_message: Some(
                            "Directory operation preview is required before :write!".to_string(),
                        ),
                        wrote: false,
                        pending_directory_confirmation: false,
                    }
                }
                Err(DirectoryBufferPreviewConfirmationError::StalePreview { .. }) => {
                    SaveSnapshotOutcome {
                        transient_message: Some(
                            "Directory operation preview is stale; run :write again before :write!"
                                .to_string(),
                        ),
                        wrote: false,
                        pending_directory_confirmation: false,
                    }
                }
                Err(DirectoryBufferPreviewConfirmationError::Validation(errors)) => {
                    log::debug!(
                        "[main][dired][writable] confirmed directory operation plan validation failed before apply: errors={:?}",
                        errors
                    );
                    SaveSnapshotOutcome {
                        transient_message: Some(format!(
                            "Directory operation plan failed validation: {} error(s)",
                            errors.len()
                        )),
                        wrote: false,
                        pending_directory_confirmation: false,
                    }
                }
            };
        }
        return match session_state.prepare_directory_buffer_operation_preview(buffer_contents) {
            Ok(preview) => {
                let prompt = session_state.directory_buffer_operation_prompt();
                log::info!(
                    "[main][dired][writable] prepared directory operation preview instead of regular save: root_path={}, preview_id={}, operations={}, high_risk={}",
                    preview.root_path.display(),
                    preview.id,
                    preview.operation_count,
                    preview.high_risk_count
                );
                SaveSnapshotOutcome {
                    transient_message: Some(prompt.map_or_else(
                        || {
                            format!(
                                "Directory operation preview prepared: {} operation(s), high_risk={}, preview_id={}",
                                preview.operation_count, preview.high_risk_count, preview.id
                            )
                        },
                        |prompt| prompt.status_line,
                    )),
                    wrote: false,
                    pending_directory_confirmation: true,
                }
            }
            Err(errors) => {
                log::debug!(
                    "[main][dired][writable] directory operation plan validation failed before save: errors={:?}",
                    errors
                );
                SaveSnapshotOutcome {
                    transient_message: Some(format!(
                        "Directory operation plan failed validation: {} error(s)",
                        errors.len()
                    )),
                    wrote: false,
                    pending_directory_confirmation: false,
                }
            }
        };
    }

    match build_save_request_for_host_write(buffer_contents, session_state, path_override) {
        Ok(req) => match write_to_path(&req) {
            SaveResult::Saved => {
                session_state.record_save_success_at_revision(core_revision);
                SaveSnapshotOutcome {
                    transient_message: Some("Saved successfully".to_string()),
                    wrote: true,
                    pending_directory_confirmation: false,
                }
            }
            SaveResult::Failed { message } => {
                session_state.record_save_failure(message);
                SaveSnapshotOutcome {
                    transient_message: Some(format!(
                        "Save failed: {}",
                        session_state.last_save_error().unwrap_or("")
                    )),
                    wrote: false,
                    pending_directory_confirmation: false,
                }
            }
        },
        Err(error) => SaveSnapshotOutcome {
            transient_message: Some(save_error_message(&error)),
            wrote: false,
            pending_directory_confirmation: false,
        },
    }
}

pub fn build_save_request_for_host_write(
    buffer_contents: &str,
    session_state: &crate::app::session::EditorSessionState,
    path_override: Option<&str>,
) -> Result<SaveRequest, SaveRequestError> {
    let Some(path_override) = effective_host_write_path_override(session_state, path_override)
    else {
        return session_state.build_save_request(buffer_contents);
    };

    if session_state.read_only() {
        return Err(SaveRequestError::ReadOnly);
    }

    Ok(SaveRequest {
        path: std::path::PathBuf::from(path_override),
        contents: buffer_contents.to_string(),
    })
}

pub fn effective_host_write_path_override<'a>(
    session_state: &crate::app::session::EditorSessionState,
    path_override: Option<&'a str>,
) -> Option<&'a str> {
    let path_override = path_override.filter(|path| !path.is_empty())?;
    let Some(target_path) = session_state.target_path() else {
        return Some(path_override);
    };
    let override_path = path_override
        .strip_prefix("file://")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(path_override));
    let same_target = override_path == target_path.as_path()
        || override_path
            .canonicalize()
            .ok()
            .zip(target_path.canonicalize().ok())
            .is_some_and(|(left, right)| left == right);
    if same_target {
        log::debug!(
            "[main] treating host write path as current target instead of explicit override: path={}",
            target_path.display()
        );
        None
    } else {
        Some(path_override)
    }
}

pub fn save_error_message(error: &SaveRequestError) -> String {
    match error {
        SaveRequestError::NoTargetPath => "No file name to save".to_string(),
        SaveRequestError::ReadOnly => "Read-only option is set; add ! to override".to_string(),
        SaveRequestError::DirectoryBuffer => {
            "Directory listings are not saved as regular files".to_string()
        }
    }
}
