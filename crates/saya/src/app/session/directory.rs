//! ディレクトリバッファ（dired）操作の session 実装。

use super::*;

impl EditorSessionState {
    pub fn directory_buffer(&self) -> Option<&DirectoryBufferState> {
        self.directory_buffer.as_ref()
    }

    pub fn refresh_directory_buffer_listing(
        &mut self,
        root_path: PathBuf,
        options: DirectoryBufferListingOptions,
    ) -> std::io::Result<Vec<DirectoryBufferEntry>> {
        log::debug!(
            "[editor_session][dired] refreshing directory buffer listing with options: root_path={}, show_hidden={}, sort_by={:?}, filter={:?}",
            root_path.display(),
            options.show_hidden,
            options.sort_by,
            options.filter
        );
        let directory_buffer = read_directory_buffer_state_with_options(&root_path, options)?;
        let entries = directory_buffer.entries.clone();
        self.directory_buffer = Some(directory_buffer);
        self.target_path = Some(root_path);
        self.dirty = false;
        self.last_save_error = None;
        Ok(entries)
    }

    pub fn refresh_directory_buffer_for_target_path(&mut self, path: &Path) -> std::io::Result<()> {
        self.refresh_directory_buffer_for_path(path)?;
        self.target_path = Some(path.to_path_buf());
        self.dirty = false;
        self.last_save_error = None;
        Ok(())
    }

    pub(super) fn rebase_directory_buffer_root(&mut self, target_path: &Path) {
        let Some(directory_buffer) = self.directory_buffer.as_mut() else {
            return;
        };
        if directory_buffer.root_path == target_path
            || !paths_refer_to_same_location(&directory_buffer.root_path, target_path)
        {
            return;
        }

        let old_root = directory_buffer.root_path.clone();
        for entry in &mut directory_buffer.entries {
            if let Some(rebased) = rebase_path_under_root(&entry.path, &old_root, target_path) {
                entry.path = rebased;
            }
        }
        directory_buffer.root_path = target_path.to_path_buf();
        self.directory_marked_paths = self
            .directory_marked_paths
            .iter()
            .map(|path| {
                rebase_path_under_root(path, &old_root, target_path).unwrap_or_else(|| path.clone())
            })
            .collect();
    }

    pub fn current_directory_entry(&self, cursor_row: usize) -> Option<&DirectoryBufferEntry> {
        let directory_buffer = self.directory_buffer.as_ref()?;
        let entry = directory_buffer.entries.get(cursor_row);
        log::debug!(
            "[editor_session] resolving current directory entry: root_path={}, cursor_row={}, entries={}, found={}",
            directory_buffer.root_path.display(),
            cursor_row,
            directory_buffer.entries.len(),
            entry.is_some()
        );
        entry
    }

    pub fn mark_directory_entry(&mut self, entry: &DirectoryBufferEntry) {
        let inserted = self.directory_marked_paths.insert(entry.path.clone());
        log::debug!(
            "[editor_session][dired] mark entry: path={}, inserted={}, marked_count={}",
            entry.path.display(),
            inserted,
            self.directory_marked_paths.len()
        );
    }

    pub fn unmark_directory_entry(&mut self, entry: &DirectoryBufferEntry) {
        let removed = self.directory_marked_paths.remove(&entry.path);
        log::debug!(
            "[editor_session][dired] unmark entry: path={}, removed={}, marked_count={}",
            entry.path.display(),
            removed,
            self.directory_marked_paths.len()
        );
    }

    pub fn clear_directory_marks(&mut self) {
        let cleared = self.directory_marked_paths.len();
        self.directory_marked_paths.clear();
        log::debug!(
            "[editor_session][dired] clear directory marks: cleared_count={}",
            cleared
        );
    }

    pub fn record_directory_entry_rename(&mut self, from: &Path, to: &Path) {
        let was_marked = self.directory_marked_paths.remove(from);
        if was_marked {
            self.directory_marked_paths.insert(to.to_path_buf());
        }
        log::debug!(
            "[editor_session][dired] record directory entry rename: from={}, to={}, was_marked={}, marked_count={}",
            from.display(),
            to.display(),
            was_marked,
            self.directory_marked_paths.len()
        );
    }

    pub fn marked_directory_entries(&self) -> Vec<DirectoryBufferEntry> {
        let Some(directory_buffer) = &self.directory_buffer else {
            log::debug!(
                "[editor_session][dired] marked entries requested outside directory buffer: marked_count={}",
                self.directory_marked_paths.len()
            );
            return Vec::new();
        };
        let entries = directory_buffer
            .entries
            .iter()
            .filter(|entry| self.directory_marked_paths.contains(&entry.path))
            .cloned()
            .collect::<Vec<_>>();
        log::debug!(
            "[editor_session][dired] resolved marked directory entries: root_path={}, marked_count={}, resolved_count={}",
            directory_buffer.root_path.display(),
            self.directory_marked_paths.len(),
            entries.len()
        );
        entries
    }

    pub fn is_directory_entry_marked(&self, entry: &DirectoryBufferEntry) -> bool {
        self.directory_marked_paths.contains(&entry.path)
    }

    pub fn build_directory_buffer_operation_plan(
        &self,
        edited_text: &str,
    ) -> Result<DirectoryBufferOperationPlan, Vec<DirectoryBufferPlanValidationError>> {
        let Some(directory_buffer) = &self.directory_buffer else {
            log::debug!(
                "[editor_session][dired][writable] operation plan requested outside directory buffer: target_path={:?}, edited_len={}",
                self.target_path,
                edited_text.len()
            );
            return Err(vec![DirectoryBufferPlanValidationError::NoDirectoryBuffer]);
        };
        log::debug!(
            "[editor_session][dired][writable] building operation plan: root_path={}, mode={:?}, original_entries={}, edited_len={}",
            directory_buffer.root_path.display(),
            directory_buffer.mode,
            directory_buffer.entries.len(),
            edited_text.len()
        );

        let edited_lines = parse_directory_buffer_edited_lines(directory_buffer, edited_text)?;
        let matched_entry_ids = edited_lines
            .iter()
            .filter_map(|line| line.matched_entry_id)
            .collect::<BTreeSet<_>>();
        let removed_entries = directory_buffer
            .entries
            .iter()
            .filter(|entry| !matched_entry_ids.contains(&entry.id))
            .cloned()
            .collect::<Vec<_>>();
        let added_lines = edited_lines
            .iter()
            .filter(|line| line.matched_entry_id.is_none())
            .cloned()
            .collect::<Vec<_>>();
        let rename_count = removed_entries.len().min(added_lines.len());
        let mut operations = Vec::new();

        for (entry, line) in removed_entries.iter().zip(added_lines.iter()) {
            let to = directory_buffer.root_path.join(&line.name);
            if entry.path != to {
                operations.push(DirectoryBufferPlannedOperation::Rename {
                    from: entry.path.clone(),
                    to,
                    from_name: entry.name.clone(),
                    to_name: line.name.clone(),
                    kind: entry.kind,
                });
            }
        }

        for line in added_lines.iter().skip(rename_count) {
            let path = directory_buffer.root_path.join(&line.name);
            match line.create_kind {
                DirectoryBufferCreateKind::File => {
                    operations.push(DirectoryBufferPlannedOperation::CreateFile {
                        path,
                        name: line.name.clone(),
                    });
                }
                DirectoryBufferCreateKind::Directory => {
                    operations.push(DirectoryBufferPlannedOperation::CreateDirectory {
                        path,
                        name: line.name.clone(),
                    });
                }
            }
        }

        for entry in removed_entries.iter().skip(rename_count) {
            operations.push(DirectoryBufferPlannedOperation::Delete {
                path: entry.path.clone(),
                name: entry.name.clone(),
                kind: entry.kind,
            });
        }

        log::debug!(
            "[editor_session][dired][writable] built operation plan: root_path={}, operations={}, removed_entries={}, added_lines={}, rename_count={}",
            directory_buffer.root_path.display(),
            operations.len(),
            removed_entries.len(),
            added_lines.len(),
            rename_count
        );
        Ok(DirectoryBufferOperationPlan {
            root_path: directory_buffer.root_path.clone(),
            operations,
        })
    }

    pub fn prepare_directory_buffer_operation_preview(
        &mut self,
        edited_text: &str,
    ) -> Result<DirectoryBufferOperationPreview, Vec<DirectoryBufferPlanValidationError>> {
        let plan = self.build_directory_buffer_operation_plan(edited_text)?;
        let preview = directory_buffer_operation_preview(&plan);
        log::info!(
            "[editor_session][dired][writable] prepared save-time operation preview: root_path={}, preview_id={}, operations={}, high_risk={}",
            preview.root_path.display(),
            preview.id,
            preview.operation_count,
            preview.high_risk_count
        );
        self.pending_directory_operation_preview = Some((preview.clone(), plan));
        self.directory_operation_confirmation_dialog_active = true;
        Ok(preview)
    }

    pub fn confirm_directory_buffer_operation_preview(
        &self,
        edited_text: &str,
    ) -> Result<DirectoryBufferOperationPlan, DirectoryBufferPreviewConfirmationError> {
        let plan = self
            .build_directory_buffer_operation_plan(edited_text)
            .map_err(DirectoryBufferPreviewConfirmationError::Validation)?;
        let actual_preview = directory_buffer_operation_preview(&plan);
        let Some((expected_preview, expected_plan)) = &self.pending_directory_operation_preview
        else {
            log::debug!(
                "[editor_session][dired][writable] save-time operation confirmation rejected without preview: actual_preview_id={}",
                actual_preview.id
            );
            return Err(DirectoryBufferPreviewConfirmationError::MissingPreview);
        };
        if expected_preview.id != actual_preview.id {
            log::debug!(
                "[editor_session][dired][writable] save-time operation confirmation rejected as stale: expected_preview_id={}, actual_preview_id={}",
                expected_preview.id,
                actual_preview.id
            );
            return Err(DirectoryBufferPreviewConfirmationError::StalePreview {
                expected_preview_id: expected_preview.id.clone(),
                actual_preview_id: actual_preview.id,
            });
        }
        Ok(expected_plan.clone())
    }

    pub fn pending_directory_operation_preview(&self) -> Option<&DirectoryBufferOperationPreview> {
        self.pending_directory_operation_preview
            .as_ref()
            .map(|(preview, _)| preview)
    }

    pub fn directory_operation_confirmation_dialog_active(&self) -> bool {
        self.directory_operation_confirmation_dialog_active
    }

    pub fn directory_buffer_operation_prompt(&self) -> Option<DirectoryBufferOperationPrompt> {
        self.pending_directory_operation_preview
            .as_ref()
            .map(|(preview, _)| directory_buffer_operation_prompt(preview))
    }

    pub fn cancel_directory_buffer_operation_preview(
        &mut self,
    ) -> Option<DirectoryBufferOperationPreview> {
        let Some((preview, _)) = self.pending_directory_operation_preview.take() else {
            log::debug!(
                "[editor_session][dired][writable] cancel requested without pending operation preview"
            );
            return None;
        };
        log::info!(
            "[editor_session][dired][writable] cancelled pending operation preview: root_path={}, preview_id={}, operations={}, high_risk={}",
            preview.root_path.display(),
            preview.id,
            preview.operation_count,
            preview.high_risk_count
        );
        self.directory_operation_confirmation_dialog_active = false;
        self.pending_directory_save_then_quit_force = None;
        Some(preview)
    }

    pub fn clear_pending_directory_operation_preview(&mut self) {
        self.directory_operation_confirmation_dialog_active = false;
        if let Some((preview, _)) = self.pending_directory_operation_preview.take() {
            log::debug!(
                "[editor_session][dired][writable] cleared pending operation preview: preview_id={}, operations={}",
                preview.id,
                preview.operation_count
            );
        }
    }

    pub fn defer_directory_save_then_quit(&mut self, force: bool) {
        if self.directory_operation_confirmation_dialog_active {
            log::debug!(
                "[editor_session][dired][writable] deferring save-then-quit until directory operation confirmation: force={}",
                force
            );
            self.pending_directory_save_then_quit_force = Some(force);
        }
    }

    pub fn take_pending_directory_save_then_quit_decision(&mut self) -> Option<QuitDecision> {
        let force = self.pending_directory_save_then_quit_force.take()?;
        Some(self.evaluate_quit(force))
    }

    pub(super) fn refresh_directory_buffer_for_path(&mut self, path: &Path) -> std::io::Result<()> {
        if !path.is_dir() {
            if self.directory_buffer.is_some() {
                log::debug!(
                    "[editor_session] clearing directory buffer metadata for non-directory target: path={}",
                    path.display()
                );
            }
            self.directory_buffer = None;
            self.clear_directory_marks();
            self.clear_pending_directory_operation_preview();
            return Ok(());
        }

        if self
            .directory_buffer
            .as_ref()
            .is_some_and(|directory_buffer| {
                !paths_refer_to_same_location(&directory_buffer.root_path, path)
            })
        {
            log::debug!(
                "[editor_session][dired] clearing marks because directory root changed: old_root={}, new_root={}, marked_count={}",
                self.directory_buffer
                    .as_ref()
                    .map(|directory_buffer| directory_buffer.root_path.display().to_string())
                    .unwrap_or_default(),
                path.display(),
                self.directory_marked_paths.len()
            );
            self.directory_marked_paths.clear();
        }
        let options = self
            .directory_buffer
            .as_ref()
            .filter(|directory_buffer| {
                paths_refer_to_same_location(&directory_buffer.root_path, path)
            })
            .map(|directory_buffer| directory_buffer.listing_options.clone())
            .unwrap_or_default();
        let directory_buffer = read_directory_buffer_state_with_options(path, options)?;
        let entry_paths = directory_buffer
            .entries
            .iter()
            .map(|entry| entry.path.clone())
            .collect::<BTreeSet<_>>();
        let before_prune = self.directory_marked_paths.len();
        self.directory_marked_paths
            .retain(|marked_path| entry_paths.contains(marked_path));
        log::debug!(
            "[editor_session] refreshed directory buffer metadata: root_path={}, entries={}, display_len={}, marked_before_prune={}, marked_after_prune={}",
            directory_buffer.root_path.display(),
            directory_buffer.entries.len(),
            directory_buffer.display_text.len(),
            before_prune,
            self.directory_marked_paths.len()
        );
        self.directory_buffer = Some(directory_buffer);
        Ok(())
    }
}

pub(crate) fn read_directory_buffer_state_with_options(
    path: &Path,
    options: DirectoryBufferListingOptions,
) -> std::io::Result<DirectoryBufferState> {
    let started_at = std::time::Instant::now();
    let normalized_filter = options
        .filter
        .as_deref()
        .map(str::trim)
        .filter(|filter| !filter.is_empty())
        .map(|filter| filter.to_ascii_lowercase());
    let read_dir_started_at = std::time::Instant::now();
    let mut entries = fs::read_dir(path)?
        .map(|entry| {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let kind = if file_type.is_dir() {
                DirectoryBufferEntryKind::Directory
            } else if file_type.is_file() {
                DirectoryBufferEntryKind::File
            } else if file_type.is_symlink() {
                DirectoryBufferEntryKind::Symlink
            } else {
                DirectoryBufferEntryKind::Other
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            if !options.show_hidden && name.starts_with('.') {
                return Ok(None);
            }
            let display_text = match kind {
                DirectoryBufferEntryKind::Directory => format!("{name}/"),
                DirectoryBufferEntryKind::Symlink => format!("{name}@"),
                DirectoryBufferEntryKind::Other => format!("{name}?"),
                DirectoryBufferEntryKind::File => name.clone(),
            };
            if let Some(filter) = normalized_filter.as_deref() {
                let normalized_name = name.to_ascii_lowercase();
                let normalized_display_text = display_text.to_ascii_lowercase();
                if !normalized_name.contains(filter) && !normalized_display_text.contains(filter) {
                    return Ok(None);
                }
            }
            let metadata = entry.metadata()?;
            let modified_time_ms = metadata
                .modified()
                .ok()
                .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX));
            Ok(Some(DirectoryBufferEntry {
                id: directory_buffer_entry_id(path, &name, kind),
                name,
                path: entry.path(),
                kind,
                display_text,
                size: Some(metadata.len()),
                modified_time_ms,
            }))
        })
        .filter_map(|entry| match entry {
            Ok(Some(entry)) => Some(Ok(entry)),
            Ok(None) => None,
            Err(error) => Some(Err(error)),
        })
        .collect::<std::io::Result<Vec<_>>>()?;
    let read_dir_ms = read_dir_started_at.elapsed().as_millis();
    let sort_started_at = std::time::Instant::now();
    entries.sort_by(|left, right| directory_buffer_compare_entries(left, right, options.sort_by));
    let sort_ms = sort_started_at.elapsed().as_millis();
    if options.sort_by == DirectoryBufferSortKey::Kind {
        let directory_count = entries
            .iter()
            .filter(|entry| entry.kind == DirectoryBufferEntryKind::Directory)
            .count();
        log::debug!(
            "[editor_session][dired] sorted directory buffer with eza-style directory grouping: root_path={}, directories={}, non_directories={}",
            path.display(),
            directory_count,
            entries.len().saturating_sub(directory_count)
        );
    }
    let display_started_at = std::time::Instant::now();
    let display_text = if entries.is_empty() {
        String::new()
    } else {
        entries
            .iter()
            .map(|entry| entry.display_text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    };
    let display_ms = display_started_at.elapsed().as_millis();
    log::debug!(
        "[PERF][editor_session][dired] read directory buffer state: root_path={}, entries={}, text_len={}, read_dir_ms={}, sort_ms={}, display_ms={}, elapsed_ms={}",
        path.display(),
        entries.len(),
        display_text.len(),
        read_dir_ms,
        sort_ms,
        display_ms,
        started_at.elapsed().as_millis()
    );

    Ok(DirectoryBufferState {
        root_path: path.to_path_buf(),
        display_text,
        entries,
        mode: DirectoryBufferMode::Writable,
        listing_options: options,
    })
}

pub(super) fn directory_buffer_compare_entries(
    left: &DirectoryBufferEntry,
    right: &DirectoryBufferEntry,
    sort_by: DirectoryBufferSortKey,
) -> std::cmp::Ordering {
    match sort_by {
        DirectoryBufferSortKey::Name => left.display_text.cmp(&right.display_text),
        DirectoryBufferSortKey::Kind => directory_buffer_directory_group_rank(left.kind)
            .cmp(&directory_buffer_directory_group_rank(right.kind))
            .then_with(|| left.display_text.cmp(&right.display_text)),
        DirectoryBufferSortKey::ModifiedTime => left
            .modified_time_ms
            .cmp(&right.modified_time_ms)
            .then_with(|| left.display_text.cmp(&right.display_text)),
        DirectoryBufferSortKey::Size => left
            .size
            .cmp(&right.size)
            .then_with(|| left.display_text.cmp(&right.display_text)),
    }
}

pub(super) fn directory_buffer_directory_group_rank(kind: DirectoryBufferEntryKind) -> usize {
    match kind {
        DirectoryBufferEntryKind::Directory => 0,
        DirectoryBufferEntryKind::File
        | DirectoryBufferEntryKind::Symlink
        | DirectoryBufferEntryKind::Other => 1,
    }
}

pub(super) fn directory_buffer_entry_id(
    root_path: &Path,
    name: &str,
    kind: DirectoryBufferEntryKind,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    root_path.hash(&mut hasher);
    name.hash(&mut hasher);
    kind.hash(&mut hasher);
    hasher.finish()
}

pub(super) fn directory_buffer_operation_preview(
    plan: &DirectoryBufferOperationPlan,
) -> DirectoryBufferOperationPreview {
    let operations = plan
        .operations
        .iter()
        .map(directory_buffer_preview_operation)
        .collect::<Vec<_>>();
    let high_risk_count = operations
        .iter()
        .filter(|operation| operation.risk == DirectoryBufferOperationRisk::High)
        .count();
    let mut hasher = DefaultHasher::new();
    plan.root_path.hash(&mut hasher);
    plan.operations.hash(&mut hasher);
    let id = format!("{:016x}", hasher.finish());
    DirectoryBufferOperationPreview {
        id,
        root_path: plan.root_path.clone(),
        operation_count: operations.len(),
        high_risk_count,
        operations,
    }
}

pub(super) fn directory_buffer_operation_prompt(
    preview: &DirectoryBufferOperationPreview,
) -> DirectoryBufferOperationPrompt {
    let status_line = format!(
        "Apply {} dired operation(s) ({} high-risk)? y/Enter=OK n/Esc=Cancel id={}",
        preview.operation_count, preview.high_risk_count, preview.id
    );
    let detail_lines = preview
        .operations
        .iter()
        .map(directory_buffer_prompt_operation_line)
        .collect::<Vec<_>>();
    DirectoryBufferOperationPrompt {
        preview_id: preview.id.clone(),
        status_line,
        detail_lines,
        confirm_command: "OK".to_string(),
        cancel_command: "Cancel".to_string(),
        recovery_hint: "No filesystem changes have been applied yet. Cancel or edit the directory listing, then run :write again to prepare a fresh preview.".to_string(),
    }
}

pub(super) fn directory_buffer_prompt_operation_line(
    operation: &DirectoryBufferPreviewOperation,
) -> String {
    let risk = match operation.risk {
        DirectoryBufferOperationRisk::Low => "low risk",
        DirectoryBufferOperationRisk::High => "HIGH RISK",
    };
    match operation.kind {
        DirectoryBufferOperationKind::CreateFile => format!(
            "{risk}: create file {}",
            display_optional_path(operation.target_path.as_deref())
        ),
        DirectoryBufferOperationKind::CreateDirectory => format!(
            "{risk}: create directory {}",
            display_optional_path(operation.target_path.as_deref())
        ),
        DirectoryBufferOperationKind::Rename => format!(
            "{risk}: rename {} -> {}",
            display_optional_path(operation.source_path.as_deref()),
            display_optional_path(operation.target_path.as_deref())
        ),
        DirectoryBufferOperationKind::Delete => format!(
            "{risk}: delete {}",
            display_optional_path(operation.source_path.as_deref())
        ),
    }
}

pub(super) fn display_optional_path(path: Option<&Path>) -> String {
    path.map(|path| path.display().to_string())
        .unwrap_or_else(|| "<none>".to_string())
}

pub(super) fn directory_buffer_preview_operation(
    operation: &DirectoryBufferPlannedOperation,
) -> DirectoryBufferPreviewOperation {
    match operation {
        DirectoryBufferPlannedOperation::CreateFile { path, .. } => {
            DirectoryBufferPreviewOperation {
                kind: DirectoryBufferOperationKind::CreateFile,
                source_path: None,
                target_path: Some(path.clone()),
                risk: DirectoryBufferOperationRisk::Low,
            }
        }
        DirectoryBufferPlannedOperation::CreateDirectory { path, .. } => {
            DirectoryBufferPreviewOperation {
                kind: DirectoryBufferOperationKind::CreateDirectory,
                source_path: None,
                target_path: Some(path.clone()),
                risk: DirectoryBufferOperationRisk::Low,
            }
        }
        DirectoryBufferPlannedOperation::Rename { from, to, .. } => {
            DirectoryBufferPreviewOperation {
                kind: DirectoryBufferOperationKind::Rename,
                source_path: Some(from.clone()),
                target_path: Some(to.clone()),
                risk: DirectoryBufferOperationRisk::Low,
            }
        }
        DirectoryBufferPlannedOperation::Delete { path, .. } => DirectoryBufferPreviewOperation {
            kind: DirectoryBufferOperationKind::Delete,
            source_path: Some(path.clone()),
            target_path: None,
            risk: DirectoryBufferOperationRisk::High,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirectoryBufferCreateKind {
    File,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedDirectoryBufferLine {
    line_number: usize,
    name: String,
    create_kind: DirectoryBufferCreateKind,
    matched_entry_id: Option<u64>,
}

fn parse_directory_buffer_edited_lines(
    directory_buffer: &DirectoryBufferState,
    edited_text: &str,
) -> Result<Vec<ParsedDirectoryBufferLine>, Vec<DirectoryBufferPlanValidationError>> {
    let mut errors = Vec::new();
    let mut first_line_by_name = BTreeMap::new();
    let mut used_entry_ids = BTreeSet::new();
    let mut entry_by_display_text = BTreeMap::new();
    let mut entry_by_name = BTreeMap::new();
    for entry in &directory_buffer.entries {
        entry_by_display_text.insert(entry.display_text.as_str(), entry);
        entry_by_name.insert(entry.name.as_str(), entry);
    }

    let mut raw_lines = if edited_text.is_empty() {
        Vec::new()
    } else {
        edited_text.split('\n').collect::<Vec<_>>()
    };
    if edited_text.ends_with('\n') {
        raw_lines.pop();
    }
    let mut lines = Vec::new();
    for (index, raw_line) in raw_lines.into_iter().enumerate() {
        let line_number = index + 1;
        let Some(mut parsed) = parse_directory_buffer_edited_line(
            directory_buffer,
            raw_line,
            line_number,
            &mut errors,
        ) else {
            continue;
        };
        if let Some(first_line_number) = first_line_by_name.insert(parsed.name.clone(), line_number)
        {
            errors.push(DirectoryBufferPlanValidationError::DuplicateName {
                line_number,
                first_line_number,
                name: parsed.name.clone(),
            });
        }
        if parsed.matched_entry_id.is_none() {
            let matching_entry = entry_by_display_text
                .get(raw_line)
                .or_else(|| entry_by_name.get(parsed.name.as_str()));
            if let Some(entry) = matching_entry.filter(|entry| !used_entry_ids.contains(&entry.id))
            {
                parsed.name = entry.name.clone();
                parsed.matched_entry_id = Some(entry.id);
                used_entry_ids.insert(entry.id);
            }
        } else if let Some(entry_id) = parsed.matched_entry_id {
            used_entry_ids.insert(entry_id);
        }
        lines.push(parsed);
    }

    if errors.is_empty() {
        Ok(lines)
    } else {
        log::debug!(
            "[editor_session][dired][writable] edited directory listing validation failed: root_path={}, error_count={}",
            directory_buffer.root_path.display(),
            errors.len()
        );
        Err(errors)
    }
}

fn parse_directory_buffer_edited_line(
    directory_buffer: &DirectoryBufferState,
    raw_line: &str,
    line_number: usize,
    errors: &mut Vec<DirectoryBufferPlanValidationError>,
) -> Option<ParsedDirectoryBufferLine> {
    if raw_line.is_empty() {
        errors.push(DirectoryBufferPlanValidationError::EmptyLine { line_number });
        return None;
    }
    if let Some(entry) = directory_buffer
        .entries
        .iter()
        .find(|entry| entry.display_text == raw_line)
    {
        return Some(ParsedDirectoryBufferLine {
            line_number,
            name: entry.name.clone(),
            create_kind: DirectoryBufferCreateKind::File,
            matched_entry_id: Some(entry.id),
        });
    }
    if raw_line.ends_with('@') || raw_line.ends_with('?') {
        errors.push(DirectoryBufferPlanValidationError::UnsupportedDecoration {
            line_number,
            line: raw_line.to_string(),
        });
        return None;
    }

    let create_kind = if raw_line.ends_with('/') {
        DirectoryBufferCreateKind::Directory
    } else {
        DirectoryBufferCreateKind::File
    };
    let name = raw_line
        .strip_suffix('/')
        .map(str::to_string)
        .unwrap_or_else(|| raw_line.to_string());
    validate_directory_buffer_plan_name(line_number, &name, errors);
    Some(ParsedDirectoryBufferLine {
        line_number,
        name,
        create_kind,
        matched_entry_id: None,
    })
}

pub(super) fn validate_directory_buffer_plan_name(
    line_number: usize,
    name: &str,
    errors: &mut Vec<DirectoryBufferPlanValidationError>,
) {
    if name.is_empty() {
        errors.push(DirectoryBufferPlanValidationError::EmptyName { line_number });
        return;
    }
    if name == "." || name == ".." || name.starts_with("../") || name.contains("/../") {
        errors.push(DirectoryBufferPlanValidationError::ParentDirectoryEscape {
            line_number,
            name: name.to_string(),
        });
    }
    if name.contains('/') || name.contains('\\') {
        errors.push(DirectoryBufferPlanValidationError::PathSeparator {
            line_number,
            name: name.to_string(),
        });
    }
}
