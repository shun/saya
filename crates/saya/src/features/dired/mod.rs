//! Dired（ディレクトリバッファ）操作のホスト層実装。
//!
//! 確定済みのディレクトリ操作プランを、ロールバック可能なトランザクション
//! として組み立て・検証・実行する。ADR 0003「dired はホスト層」に従い、
//! ファイルシステム操作の責務をここに閉じ込める。`vim-core-rs` の編集
//! セマンティクスには関与しない。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::app::session::{
    DirectoryBufferListingOptions, DirectoryBufferPlannedOperation, DirectoryBufferSortKey,
};
use crate::core::notification_prompt::{InputPromptStatus, InputPromptView};
use crate::input::command_line_editor::{CommandLineEdit, command_line_edit_action_for_key};
use crate::input::router::KeyInput;
use crate::runtime::live::{
    RuntimeFilerCurrentEntry, RuntimeFilerEntry, RuntimeFilerEntryKind, RuntimeFilerError,
    RuntimeFilerErrorKind, RuntimeFilerListOptions, RuntimeFilerOperation,
    RuntimeFilerOperationKind, RuntimeFilerOperationReport, RuntimeFilerSortKey,
    RuntimeInputPromptRequest,
};
use vim_core_rs::CoreInputRequestKind;

pub fn apply_directory_buffer_operation_plan(
    session_state: &mut crate::app::session::EditorSessionState,
    plan: &crate::app::session::DirectoryBufferOperationPlan,
) -> Result<usize, RuntimeFilerError> {
    log::info!(
        "[main][dired][writable] applying confirmed directory operation plan through filer operations: root_path={}, operations={}",
        plan.root_path.display(),
        plan.operations.len()
    );
    let transaction = build_directory_buffer_operation_transaction(plan)?;
    match execute_directory_buffer_operation_transaction(&transaction) {
        Ok(_report) => {
            for (from, to) in &transaction.rename_marks {
                session_state.record_directory_entry_rename(from, to);
            }
            if let Err(error) =
                session_state.refresh_directory_buffer_for_target_path(&plan.root_path)
            {
                log::debug!(
                    "[main][dired][writable] failed to refresh directory buffer after confirmed operation plan: root_path={}, error={}",
                    plan.root_path.display(),
                    error
                );
            }
            session_state.replace_target_path(plan.root_path.clone());
            Ok(plan.operations.len())
        }
        Err(error) => {
            if let Err(refresh_error) =
                session_state.refresh_directory_buffer_for_target_path(&plan.root_path)
            {
                log::debug!(
                    "[main][dired][writable] failed to refresh directory buffer after failed operation plan: root_path={}, error={}",
                    plan.root_path.display(),
                    refresh_error
                );
            }
            session_state.replace_target_path(plan.root_path.clone());
            Err(error)
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DirectoryBufferOperationTransaction {
    steps: Vec<DirectoryBufferOperationTransactionStep>,
    rename_marks: Vec<(std::path::PathBuf, std::path::PathBuf)>,
}

#[derive(Debug, Clone)]
pub(crate) struct DirectoryBufferOperationTransactionStep {
    operation: RuntimeFilerOperation,
    rollback: Option<RuntimeFilerOperation>,
    rollback_manual_recovery_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirectoryBufferOperationTransactionReport {
    successful: usize,
    failed: usize,
    rollback_succeeded: usize,
    rollback_failed: usize,
    manual_recovery_required: usize,
}

pub(crate) fn build_directory_buffer_operation_transaction(
    plan: &crate::app::session::DirectoryBufferOperationPlan,
) -> Result<DirectoryBufferOperationTransaction, RuntimeFilerError> {
    validate_directory_buffer_operation_conflicts(plan)?;
    let mut steps = Vec::new();
    let mut rename_second_phase = Vec::new();
    let mut rename_marks = Vec::new();

    for (index, operation) in plan
        .operations
        .iter()
        .filter_map(|operation| match operation {
            DirectoryBufferPlannedOperation::Rename { from, to, .. } => Some((from, to)),
            _ => None,
        })
        .enumerate()
    {
        let temp_path = unique_directory_transaction_temp_path(&plan.root_path, index);
        steps.push(DirectoryBufferOperationTransactionStep {
            operation: RuntimeFilerOperation::Rename {
                from: operation.0.clone(),
                to: temp_path.clone(),
            },
            rollback: Some(RuntimeFilerOperation::Rename {
                from: temp_path.clone(),
                to: operation.0.clone(),
            }),
            rollback_manual_recovery_required: false,
        });
        rename_second_phase.push(DirectoryBufferOperationTransactionStep {
            operation: RuntimeFilerOperation::Rename {
                from: temp_path,
                to: operation.1.clone(),
            },
            rollback: Some(RuntimeFilerOperation::Rename {
                from: operation.1.clone(),
                to: operation.0.clone(),
            }),
            rollback_manual_recovery_required: false,
        });
        rename_marks.push((operation.0.clone(), operation.1.clone()));
    }
    steps.extend(rename_second_phase);

    for operation in plan.operations.iter().filter(|operation| {
        matches!(
            operation,
            DirectoryBufferPlannedOperation::CreateDirectory { .. }
        )
    }) {
        if let DirectoryBufferPlannedOperation::CreateDirectory { path, .. } = operation {
            steps.push(DirectoryBufferOperationTransactionStep {
                operation: RuntimeFilerOperation::CreateDirectory { path: path.clone() },
                rollback: None,
                rollback_manual_recovery_required: true,
            });
        }
    }

    for operation in plan.operations.iter().filter(|operation| {
        matches!(
            operation,
            DirectoryBufferPlannedOperation::CreateFile { .. }
        )
    }) {
        if let DirectoryBufferPlannedOperation::CreateFile { path, .. } = operation {
            steps.push(DirectoryBufferOperationTransactionStep {
                operation: RuntimeFilerOperation::CreateFile { path: path.clone() },
                rollback: None,
                rollback_manual_recovery_required: true,
            });
        }
    }

    for operation in plan
        .operations
        .iter()
        .filter(|operation| matches!(operation, DirectoryBufferPlannedOperation::Delete { .. }))
    {
        if let DirectoryBufferPlannedOperation::Delete { path, .. } = operation {
            steps.push(DirectoryBufferOperationTransactionStep {
                operation: RuntimeFilerOperation::Delete {
                    path: path.clone(),
                    confirm: true,
                    recursive: false,
                    trash: false,
                },
                rollback: None,
                rollback_manual_recovery_required: true,
            });
        }
    }

    log::debug!(
        "[main][dired][transaction] built operation transaction: root_path={}, planned_operations={}, executable_steps={}, rename_count={}",
        plan.root_path.display(),
        plan.operations.len(),
        steps.len(),
        rename_marks.len()
    );
    Ok(DirectoryBufferOperationTransaction {
        steps,
        rename_marks,
    })
}

pub(crate) fn validate_directory_buffer_operation_conflicts(
    plan: &crate::app::session::DirectoryBufferOperationPlan,
) -> Result<(), RuntimeFilerError> {
    let rename_sources = plan
        .operations
        .iter()
        .filter_map(|operation| match operation {
            DirectoryBufferPlannedOperation::Rename { from, .. } => Some(from.clone()),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    for operation in &plan.operations {
        match operation {
            DirectoryBufferPlannedOperation::CreateFile { path, .. } => {
                validate_directory_transaction_parent_permission(
                    RuntimeFilerOperationKind::CreateFile,
                    path,
                    None,
                )?;
                if path.exists() && !rename_sources.contains(path) {
                    return Err(directory_transaction_conflict_error(
                        RuntimeFilerOperationKind::CreateFile,
                        path,
                        None,
                        RuntimeFilerErrorKind::AlreadyExists,
                        "operation transaction conflict check failed: target path already exists",
                    ));
                }
            }
            DirectoryBufferPlannedOperation::CreateDirectory { path, .. } => {
                validate_directory_transaction_parent_permission(
                    RuntimeFilerOperationKind::CreateDirectory,
                    path,
                    None,
                )?;
                if path.exists() && !rename_sources.contains(path) {
                    return Err(directory_transaction_conflict_error(
                        RuntimeFilerOperationKind::CreateDirectory,
                        path,
                        None,
                        RuntimeFilerErrorKind::AlreadyExists,
                        "operation transaction conflict check failed: target path already exists",
                    ));
                }
            }
            DirectoryBufferPlannedOperation::Rename { from, to, kind, .. } => {
                validate_directory_transaction_entry_kind(
                    RuntimeFilerOperationKind::Rename,
                    from,
                    *kind,
                )?;
                validate_directory_transaction_parent_permission(
                    RuntimeFilerOperationKind::Rename,
                    from,
                    Some(to),
                )?;
                if !from.exists() {
                    return Err(directory_transaction_conflict_error(
                        RuntimeFilerOperationKind::Rename,
                        from,
                        Some(to),
                        RuntimeFilerErrorKind::NotFound,
                        "operation transaction conflict check failed: source path is missing",
                    ));
                }
                if to.exists() && !rename_sources.contains(to) {
                    return Err(directory_transaction_conflict_error(
                        RuntimeFilerOperationKind::Rename,
                        from,
                        Some(to),
                        RuntimeFilerErrorKind::AlreadyExists,
                        "operation transaction conflict check failed: target path already exists",
                    ));
                }
            }
            DirectoryBufferPlannedOperation::Delete { path, kind, .. } => {
                validate_directory_transaction_entry_kind(
                    RuntimeFilerOperationKind::Delete,
                    path,
                    *kind,
                )?;
                validate_directory_transaction_parent_permission(
                    RuntimeFilerOperationKind::Delete,
                    path,
                    None,
                )?;
                if !path.exists() {
                    return Err(directory_transaction_conflict_error(
                        RuntimeFilerOperationKind::Delete,
                        path,
                        None,
                        RuntimeFilerErrorKind::NotFound,
                        "operation transaction conflict check failed: source path is missing",
                    ));
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_directory_transaction_parent_permission(
    operation: RuntimeFilerOperationKind,
    path: &std::path::Path,
    target_path: Option<&std::path::PathBuf>,
) -> Result<(), RuntimeFilerError> {
    let mut parents = Vec::new();
    if let Some(parent) = path.parent() {
        parents.push(parent.to_path_buf());
    }
    if let Some(target_parent) = target_path.and_then(|target_path| target_path.parent()) {
        parents.push(target_parent.to_path_buf());
    }
    for parent in parents {
        if !directory_transaction_path_has_write_permission(&parent) {
            return Err(directory_transaction_conflict_error(
                operation,
                path,
                target_path,
                RuntimeFilerErrorKind::PermissionDenied,
                "operation transaction conflict check failed: parent directory is not writable",
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn directory_transaction_path_has_write_permission(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata()
        .map(|metadata| metadata.permissions().mode() & 0o222 != 0)
        .unwrap_or(true)
}

#[cfg(not(unix))]
pub(crate) fn directory_transaction_path_has_write_permission(path: &std::path::Path) -> bool {
    path.metadata()
        .map(|metadata| !metadata.permissions().readonly())
        .unwrap_or(true)
}

pub(crate) fn validate_directory_transaction_entry_kind(
    operation: RuntimeFilerOperationKind,
    path: &std::path::Path,
    kind: crate::app::session::DirectoryBufferEntryKind,
) -> Result<(), RuntimeFilerError> {
    if kind == crate::app::session::DirectoryBufferEntryKind::Other {
        return Err(directory_transaction_conflict_error(
            operation,
            path,
            None,
            RuntimeFilerErrorKind::Unsupported,
            "operation transaction conflict check failed: special file entries are unsupported",
        ));
    }
    Ok(())
}

pub(crate) fn execute_directory_buffer_operation_transaction(
    transaction: &DirectoryBufferOperationTransaction,
) -> Result<DirectoryBufferOperationTransactionReport, RuntimeFilerError> {
    let mut report = DirectoryBufferOperationTransactionReport {
        successful: 0,
        failed: 0,
        rollback_succeeded: 0,
        rollback_failed: 0,
        manual_recovery_required: 0,
    };
    let mut applied_steps = Vec::new();
    for (index, step) in transaction.steps.iter().enumerate() {
        log::info!(
            "[main][dired][transaction] executing transaction step: index={}, operation={:?}",
            index,
            step.operation
        );
        match crate::runtime::live::execute_local_filer_operation(step.operation.clone()) {
            Ok(_) => {
                report.successful += 1;
                applied_steps.push(step);
            }
            Err(error) => {
                report.failed += 1;
                log::debug!(
                    "[main][dired][transaction] transaction step failed: index={}, error={:?}",
                    index,
                    error
                );
                for applied_step in applied_steps.into_iter().rev() {
                    if let Some(rollback) = &applied_step.rollback {
                        match crate::runtime::live::execute_local_filer_operation(rollback.clone())
                        {
                            Ok(_) => report.rollback_succeeded += 1,
                            Err(rollback_error) => {
                                report.rollback_failed += 1;
                                report.manual_recovery_required += 1;
                                log::debug!(
                                    "[main][dired][transaction] transaction rollback failed: rollback={:?}, error={:?}",
                                    rollback,
                                    rollback_error
                                );
                            }
                        }
                    } else if applied_step.rollback_manual_recovery_required {
                        report.manual_recovery_required += 1;
                    }
                }
                return Err(directory_transaction_report_error(report, error));
            }
        }
    }
    log::info!(
        "[main][dired][transaction] transaction completed: successful={}, failed={}, rollback_succeeded={}, rollback_failed={}, manual_recovery_required={}",
        report.successful,
        report.failed,
        report.rollback_succeeded,
        report.rollback_failed,
        report.manual_recovery_required
    );
    Ok(report)
}

pub(crate) fn directory_transaction_report_error(
    report: DirectoryBufferOperationTransactionReport,
    error: RuntimeFilerError,
) -> RuntimeFilerError {
    let (operation, path, target_path, kind, cause) = match error {
        RuntimeFilerError::OperationFailed {
            operation,
            path,
            target_path,
            kind,
            message,
        } => (operation, path, target_path, kind, message),
        RuntimeFilerError::ReadFailed { path, message } => (
            RuntimeFilerOperationKind::BulkDelete,
            path,
            None,
            RuntimeFilerErrorKind::Io,
            message,
        ),
    };
    let message = format!(
        "directory operation transaction failed: successful={}, failed={}, rollback_succeeded={}, rollback_failed={}, manual_recovery_required={}, cause={}",
        report.successful,
        report.failed,
        report.rollback_succeeded,
        report.rollback_failed,
        report.manual_recovery_required,
        cause
    );
    log::debug!(
        "[main][dired][transaction] transaction failed report: operation={:?}, path={}, target_path={:?}, kind={:?}, {}",
        operation,
        path.display(),
        target_path.as_ref().map(|path| path.display().to_string()),
        kind,
        message
    );
    RuntimeFilerError::OperationFailed {
        operation,
        path,
        target_path,
        kind,
        message,
    }
}

pub(crate) fn directory_transaction_conflict_error(
    operation: RuntimeFilerOperationKind,
    path: &std::path::Path,
    target_path: Option<&std::path::PathBuf>,
    kind: RuntimeFilerErrorKind,
    message: &str,
) -> RuntimeFilerError {
    log::debug!(
        "[main][dired][transaction] conflict check failed: operation={:?}, path={}, target_path={:?}, kind={:?}, message={}",
        operation,
        path.display(),
        target_path.map(|path| path.display().to_string()),
        kind,
        message
    );
    RuntimeFilerError::OperationFailed {
        operation,
        path: path.to_path_buf(),
        target_path: target_path.cloned(),
        kind,
        message: message.to_string(),
    }
}

pub(crate) fn unique_directory_transaction_temp_path(
    root_path: &std::path::Path,
    index: usize,
) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    for attempt in 0..1000 {
        let candidate = root_path.join(format!(".saya-dired-txn-{nanos}-{index}-{attempt}.tmp"));
        if !candidate.exists() {
            return candidate;
        }
    }
    root_path.join(format!(".saya-dired-txn-{nanos}-{index}-fallback.tmp"))
}

pub fn directory_buffer_listing_options_from_runtime(
    options: RuntimeFilerListOptions,
) -> DirectoryBufferListingOptions {
    DirectoryBufferListingOptions {
        show_hidden: options.show_hidden,
        sort_by: match options.sort_by {
            RuntimeFilerSortKey::Name => DirectoryBufferSortKey::Name,
            RuntimeFilerSortKey::Kind => DirectoryBufferSortKey::Kind,
            RuntimeFilerSortKey::ModifiedTime => DirectoryBufferSortKey::ModifiedTime,
            RuntimeFilerSortKey::Size => DirectoryBufferSortKey::Size,
        },
        filter: options.filter,
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeInputPromptUiState {
    pub request: RuntimeInputPromptRequest,
    edit: CommandLineEdit,
    correlation_id: u64,
}

impl RuntimeInputPromptUiState {
    pub fn new(request: RuntimeInputPromptRequest) -> Self {
        Self {
            request,
            edit: CommandLineEdit::default(),
            correlation_id: 0,
        }
    }

    pub fn view(&self) -> InputPromptView {
        InputPromptView {
            prompt: format!("{}:", self.request.title),
            input: self.edit.buffer().to_string(),
            correlation_id: self.correlation_id,
            input_kind: CoreInputRequestKind::CommandLine,
            status: InputPromptStatus::Active,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeInputPromptKeyAction {
    Editing,
    Submit(String),
    Cancel,
}

pub fn handle_runtime_input_prompt_key(
    state: Option<&mut RuntimeInputPromptUiState>,
    key: &KeyInput,
) -> Option<RuntimeInputPromptKeyAction> {
    let state = state?;
    match key {
        KeyInput::Enter => Some(RuntimeInputPromptKeyAction::Submit(
            state.edit.buffer().to_string(),
        )),
        KeyInput::Escape | KeyInput::Ctrl('c') | KeyInput::Ctrl('C') => {
            Some(RuntimeInputPromptKeyAction::Cancel)
        }
        KeyInput::Char(ch) => {
            state.edit.insert_char(*ch);
            Some(RuntimeInputPromptKeyAction::Editing)
        }
        _ => {
            if let Some(action) = command_line_edit_action_for_key(key) {
                state.edit.apply_action(action);
            }
            Some(RuntimeInputPromptKeyAction::Editing)
        }
    }
}

pub fn execute_runtime_filer_operation(
    operation: RuntimeFilerOperation,
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    session_state: &mut crate::app::session::EditorSessionState,
) -> Result<RuntimeFilerOperationReport, RuntimeFilerError> {
    let refresh_path = runtime_filer_operation_refresh_path(&operation).or_else(|| {
        matches!(
            operation,
            RuntimeFilerOperation::BulkDelete { confirm: true, .. }
        )
        .then(|| session_state.target_path().cloned())
        .flatten()
    });
    log::info!(
        "[main][runtime][filer] executing host-mediated filer operation: operation={:?}, refresh_path={:?}",
        operation,
        refresh_path
    );

    let operation_for_refresh = operation.clone();
    let report = match operation {
        RuntimeFilerOperation::Mark { path } => {
            let entry = find_directory_entry_by_path(session_state, &path).ok_or_else(|| {
                RuntimeFilerError::OperationFailed {
                    operation: RuntimeFilerOperationKind::Mark,
                    path: path.clone(),
                    target_path: None,
                    kind: RuntimeFilerErrorKind::NotFound,
                    message: "mark target is not in the active directory buffer".to_string(),
                }
            })?;
            session_state.mark_directory_entry(&entry);
            RuntimeFilerOperationReport {
                operation: RuntimeFilerOperationKind::Mark,
                path: path.to_string_lossy().into_owned(),
                target_path: None,
                entries: directory_entries_for_runtime(session_state.marked_directory_entries()),
                preview_id: None,
            }
        }
        RuntimeFilerOperation::Unmark { path } => {
            let entry = find_directory_entry_by_path(session_state, &path).ok_or_else(|| {
                RuntimeFilerError::OperationFailed {
                    operation: RuntimeFilerOperationKind::Unmark,
                    path: path.clone(),
                    target_path: None,
                    kind: RuntimeFilerErrorKind::NotFound,
                    message: "unmark target is not in the active directory buffer".to_string(),
                }
            })?;
            session_state.unmark_directory_entry(&entry);
            RuntimeFilerOperationReport {
                operation: RuntimeFilerOperationKind::Unmark,
                path: path.to_string_lossy().into_owned(),
                target_path: None,
                entries: directory_entries_for_runtime(session_state.marked_directory_entries()),
                preview_id: None,
            }
        }
        RuntimeFilerOperation::ClearMarks => {
            session_state.clear_directory_marks();
            RuntimeFilerOperationReport {
                operation: RuntimeFilerOperationKind::ClearMarks,
                path: session_state
                    .target_path()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                target_path: None,
                entries: Vec::new(),
                preview_id: None,
            }
        }
        RuntimeFilerOperation::BulkDeletePreview => {
            let entries = session_state.marked_directory_entries();
            let preview_id = runtime_filer_bulk_delete_preview_id(&entries);
            log::info!(
                "[main][runtime][filer] prepared bulk delete preview: preview_id={}, entry_count={}",
                preview_id,
                entries.len()
            );
            RuntimeFilerOperationReport {
                operation: RuntimeFilerOperationKind::BulkDeletePreview,
                path: session_state
                    .target_path()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                target_path: None,
                entries: directory_entries_for_runtime(entries),
                preview_id: Some(preview_id),
            }
        }
        RuntimeFilerOperation::BulkDelete {
            preview_id,
            confirm,
        } => {
            let entries = session_state.marked_directory_entries();
            let expected_preview_id = runtime_filer_bulk_delete_preview_id(&entries);
            if !confirm || preview_id.is_empty() || preview_id != expected_preview_id {
                return Err(RuntimeFilerError::OperationFailed {
                    operation: RuntimeFilerOperationKind::BulkDelete,
                    path: session_state.target_path().cloned().unwrap_or_default(),
                    target_path: None,
                    kind: RuntimeFilerErrorKind::ConfirmationRequired,
                    message: format!(
                        "bulk delete requires explicit confirmation with the latest preview id: expected={expected_preview_id}, actual={preview_id}"
                    ),
                });
            }
            log::info!(
                "[main][runtime][filer] executing confirmed bulk delete: preview_id={}, entry_count={}",
                preview_id,
                entries.len()
            );
            for entry in &entries {
                crate::runtime::live::execute_local_filer_operation(
                    RuntimeFilerOperation::Delete {
                        path: entry.path.clone(),
                        confirm: true,
                        recursive: false,
                        trash: false,
                    },
                )?;
            }
            session_state.clear_directory_marks();
            RuntimeFilerOperationReport {
                operation: RuntimeFilerOperationKind::BulkDelete,
                path: session_state
                    .target_path()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                target_path: None,
                entries: directory_entries_for_runtime(entries),
                preview_id: Some(preview_id),
            }
        }
        operation => {
            let report = crate::runtime::live::execute_local_filer_operation(operation)?;
            if let RuntimeFilerOperation::Rename { from, to }
            | RuntimeFilerOperation::Move { from, to } = &operation_for_refresh
            {
                session_state.record_directory_entry_rename(from, to);
            }
            report
        }
    };

    if let Some(refresh_path) = refresh_path.filter(|path| path.is_dir()) {
        let previous_row = outcome.core_bridge.light_snapshot().cursor_row;
        log::debug!(
            "[main][runtime][filer] refreshing directory buffer after operation: path={}, previous_cursor_row={}",
            refresh_path.display(),
            previous_row
        );
        session_state
            .refresh_directory_buffer_for_target_path(&refresh_path)
            .map_err(|error| RuntimeFilerError::OperationFailed {
                operation: report.operation,
                path: std::path::PathBuf::from(report.path.clone()),
                target_path: report.target_path.clone().map(std::path::PathBuf::from),
                kind: RuntimeFilerErrorKind::Io,
                message: format!("failed to refresh directory buffer: {error:?}"),
            })?;
        if let Some(directory_buffer) =
            session_state.directory_buffer().filter(|directory_buffer| {
                paths_refer_to_same_location_main(&directory_buffer.root_path, &refresh_path)
            })
        {
            outcome
                .core_bridge
                .replace_buffer_text(&directory_buffer.display_text)
                .map_err(|error| RuntimeFilerError::OperationFailed {
                    operation: report.operation,
                    path: std::path::PathBuf::from(report.path.clone()),
                    target_path: report.target_path.clone().map(std::path::PathBuf::from),
                    kind: RuntimeFilerErrorKind::Io,
                    message: format!("failed to project refreshed directory buffer: {error:?}"),
                })?;
        }
        let refreshed = outcome.core_bridge.light_snapshot();
        let refreshed_line_count = refreshed
            .active_window()
            .and_then(|window| {
                outcome.core_bridge.buffer_line_range(
                    window.buf_id,
                    window.topline.saturating_sub(1),
                    1,
                )
            })
            .map(|range| range.total_line_count)
            .unwrap_or(1);
        log::debug!(
            "[main][runtime][filer] refreshed directory buffer after operation: path={}, cursor_row_before={}, cursor_row_after={}, line_count={}",
            refresh_path.display(),
            previous_row,
            refreshed.cursor_row,
            refreshed_line_count
        );
    }

    Ok(report)
}

pub(crate) fn find_directory_entry_by_path(
    session_state: &crate::app::session::EditorSessionState,
    path: &std::path::Path,
) -> Option<crate::app::session::DirectoryBufferEntry> {
    session_state
        .directory_buffer()?
        .entries
        .iter()
        .find(|entry| entry.path == path)
        .cloned()
}

pub(crate) fn directory_entries_for_runtime(
    entries: Vec<crate::app::session::DirectoryBufferEntry>,
) -> Vec<RuntimeFilerCurrentEntry> {
    entries
        .into_iter()
        .map(|entry| RuntimeFilerCurrentEntry {
            id: entry.id,
            name: entry.name,
            path: entry.path.to_string_lossy().into_owned(),
            kind: runtime_filer_kind_from_directory_entry(entry.kind),
            root_path: entry
                .path
                .parent()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default(),
            display_text: entry.display_text,
        })
        .collect()
}

pub fn directory_entries_for_runtime_entries(
    entries: Vec<crate::app::session::DirectoryBufferEntry>,
) -> Vec<RuntimeFilerEntry> {
    entries
        .into_iter()
        .map(|entry| RuntimeFilerEntry {
            name: entry.name,
            path: entry.path.to_string_lossy().into_owned(),
            kind: runtime_filer_kind_from_directory_entry(entry.kind),
            display_text: entry.display_text,
            size: entry.size,
            modified_time_ms: entry.modified_time_ms,
        })
        .collect()
}

pub(crate) fn runtime_filer_bulk_delete_preview_id(
    entries: &[crate::app::session::DirectoryBufferEntry],
) -> String {
    let mut hasher = DefaultHasher::new();
    for entry in entries {
        entry.id.hash(&mut hasher);
        entry.path.hash(&mut hasher);
        entry.kind.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

pub(crate) fn runtime_filer_operation_refresh_path(
    operation: &RuntimeFilerOperation,
) -> Option<std::path::PathBuf> {
    match operation {
        RuntimeFilerOperation::CreateFile { path }
        | RuntimeFilerOperation::CreateDirectory { path }
        | RuntimeFilerOperation::Delete { path, .. } => {
            path.parent().map(|path| path.to_path_buf())
        }
        RuntimeFilerOperation::Rename { from, .. }
        | RuntimeFilerOperation::Copy { from, .. }
        | RuntimeFilerOperation::Move { from, .. } => from.parent().map(|path| path.to_path_buf()),
        RuntimeFilerOperation::Mark { .. }
        | RuntimeFilerOperation::Unmark { .. }
        | RuntimeFilerOperation::ClearMarks
        | RuntimeFilerOperation::BulkDeletePreview
        | RuntimeFilerOperation::BulkDelete { .. } => None,
    }
}

pub fn runtime_filer_kind_from_directory_entry(
    kind: crate::app::session::DirectoryBufferEntryKind,
) -> RuntimeFilerEntryKind {
    match kind {
        crate::app::session::DirectoryBufferEntryKind::Directory => {
            RuntimeFilerEntryKind::Directory
        }
        crate::app::session::DirectoryBufferEntryKind::File => RuntimeFilerEntryKind::File,
        crate::app::session::DirectoryBufferEntryKind::Symlink => RuntimeFilerEntryKind::Symlink,
        crate::app::session::DirectoryBufferEntryKind::Other => RuntimeFilerEntryKind::Other,
    }
}

/// 2つのパスが同じ場所を指すか（canonicalize で比較）。
pub fn paths_refer_to_same_location_main(left: &std::path::Path, right: &std::path::Path) -> bool {
    left == right
        || std::fs::canonicalize(left)
            .ok()
            .zip(std::fs::canonicalize(right).ok())
            .is_some_and(|(left, right)| left == right)
}
