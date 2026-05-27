/// エディタセッションの保存・終了を統括するモジュール。
///
/// CoreBridge から取得した buffer 情報と対象パスを組み合わせて、
/// 保存要求の生成、保存結果の反映、終了判定を行う。
use std::collections::{BTreeMap, BTreeSet, hash_map::DefaultHasher};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::app::host_io::SaveRequest;
use crate::input::router::KeyInput;
use crate::presentation::theme::ResolvedTheme;
use crate::runtime::options::{SayaOptionName, SayaOptionValue};
use vim_core_rs::CorePagerPromptKind;

/// 保存要求の生成に失敗した理由。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveRequestError {
    /// 保存先パスが未設定
    NoTargetPath,
    /// read-only 起動のため保存不可
    ReadOnly,
    /// directory listing buffer は通常ファイル保存の対象外
    DirectoryBuffer,
}

/// 終了要求の判定結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuitDecision {
    /// 即時終了可能
    Allow,
    /// 未保存変更があるため警告
    WarnUnsaved,
    /// 強制終了（未保存でも終了）
    ForceQuit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DirectoryBufferEntryKind {
    Directory,
    File,
    Symlink,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryBufferEntry {
    pub id: u64,
    pub name: String,
    pub path: PathBuf,
    pub kind: DirectoryBufferEntryKind,
    pub display_text: String,
    pub size: Option<u64>,
    pub modified_time_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryBufferState {
    pub root_path: PathBuf,
    pub display_text: String,
    pub entries: Vec<DirectoryBufferEntry>,
    pub mode: DirectoryBufferMode,
    pub listing_options: DirectoryBufferListingOptions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectoryBufferMode {
    Writable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryBufferListingOptions {
    pub show_hidden: bool,
    pub sort_by: DirectoryBufferSortKey,
    pub filter: Option<String>,
}

impl Default for DirectoryBufferListingOptions {
    fn default() -> Self {
        Self {
            show_hidden: true,
            sort_by: DirectoryBufferSortKey::Kind,
            filter: None,
        }
    }
}

fn paths_refer_to_same_location(left: &Path, right: &Path) -> bool {
    left == right
        || std::fs::canonicalize(left)
            .ok()
            .zip(std::fs::canonicalize(right).ok())
            .is_some_and(|(left, right)| left == right)
}

fn rebase_path_under_root(path: &Path, old_root: &Path, new_root: &Path) -> Option<PathBuf> {
    path.strip_prefix(old_root)
        .ok()
        .map(|relative_path| new_root.join(relative_path))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectoryBufferSortKey {
    Name,
    Kind,
    ModifiedTime,
    Size,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryBufferOperationPlan {
    pub root_path: PathBuf,
    pub operations: Vec<DirectoryBufferPlannedOperation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DirectoryBufferPlannedOperation {
    CreateFile {
        path: PathBuf,
        name: String,
    },
    CreateDirectory {
        path: PathBuf,
        name: String,
    },
    Rename {
        from: PathBuf,
        to: PathBuf,
        from_name: String,
        to_name: String,
        kind: DirectoryBufferEntryKind,
    },
    Delete {
        path: PathBuf,
        name: String,
        kind: DirectoryBufferEntryKind,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DirectoryBufferOperationKind {
    CreateFile,
    CreateDirectory,
    Rename,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DirectoryBufferOperationRisk {
    Low,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryBufferPreviewOperation {
    pub kind: DirectoryBufferOperationKind,
    pub source_path: Option<PathBuf>,
    pub target_path: Option<PathBuf>,
    pub risk: DirectoryBufferOperationRisk,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryBufferOperationPreview {
    pub id: String,
    pub root_path: PathBuf,
    pub operation_count: usize,
    pub high_risk_count: usize,
    pub operations: Vec<DirectoryBufferPreviewOperation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryBufferOperationPrompt {
    pub preview_id: String,
    pub status_line: String,
    pub detail_lines: Vec<String>,
    pub confirm_command: String,
    pub cancel_command: String,
    pub recovery_hint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MessagePagerAction {
    Enter,
    ForwardLine,
    ForwardHalfPage,
    ForwardPage,
    BackwardLine,
    BackwardHalfPage,
    BackwardPage,
    Top,
    Bottom,
    Dismiss,
}

impl MessagePagerAction {
    fn from_key(key: &KeyInput) -> Option<Self> {
        match key {
            KeyInput::Enter => Some(Self::Enter),
            KeyInput::Char('j') | KeyInput::Down => Some(Self::ForwardLine),
            KeyInput::Char('d') => Some(Self::ForwardHalfPage),
            KeyInput::Char(' ')
            | KeyInput::Char('f')
            | KeyInput::PageDown
            | KeyInput::Ctrl('f')
            | KeyInput::Ctrl('F') => Some(Self::ForwardPage),
            KeyInput::Char('k') | KeyInput::Up => Some(Self::BackwardLine),
            KeyInput::Char('u') => Some(Self::BackwardHalfPage),
            KeyInput::Char('b') | KeyInput::PageUp | KeyInput::Ctrl('b') | KeyInput::Ctrl('B') => {
                Some(Self::BackwardPage)
            }
            KeyInput::Char('g') => Some(Self::Top),
            KeyInput::Char('G') => Some(Self::Bottom),
            KeyInput::Escape | KeyInput::Ctrl('[') | KeyInput::Char('q') => Some(Self::Dismiss),
            _ => None,
        }
    }
}

fn normalize_message_pager_key(message: &str) -> String {
    message
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectoryBufferPreviewConfirmationError {
    Validation(Vec<DirectoryBufferPlanValidationError>),
    MissingPreview,
    StalePreview {
        expected_preview_id: String,
        actual_preview_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectoryBufferPlanValidationError {
    NoDirectoryBuffer,
    EmptyLine {
        line_number: usize,
    },
    EmptyName {
        line_number: usize,
    },
    DuplicateName {
        line_number: usize,
        first_line_number: usize,
        name: String,
    },
    ParentDirectoryEscape {
        line_number: usize,
        name: String,
    },
    PathSeparator {
        line_number: usize,
        name: String,
    },
    UnsupportedDecoration {
        line_number: usize,
        line: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MessagePagerState {
    message_key: String,
    max_offset: u16,
    dismissed: bool,
}

/// エディタセッションの状態。保存と終了の判定に使用する。
#[derive(Debug)]
pub struct EditorSessionState {
    /// 対象ファイルパス（新規バッファの場合は None）
    target_path: Option<PathBuf>,
    /// 描画時のタブ幅
    tab_size: u16,
    /// 行番号表示の初期状態
    line_numbers: bool,
    /// 行番号欄の最小幅
    number_width: u16,
    relative_number: bool,
    cursorline: bool,
    scrolloff: u16,
    sidescrolloff: u16,
    wrap: bool,
    laststatus: u8,
    message_area_height: u16,
    message_scroll_offset: u16,
    message_pager: Option<MessagePagerState>,
    list: bool,
    listchars: String,
    markdown_render: bool,
    foldmethod: String,
    foldlevel: u16,
    resolved_theme: ResolvedTheme,
    /// read-only 起動かどうか
    read_only: bool,
    /// 現在 dirty 状態かどうか
    dirty: bool,
    /// 直近の保存失敗メッセージ
    last_save_error: Option<String>,
    /// directory buffer の表示と操作対象 metadata。
    directory_buffer: Option<DirectoryBufferState>,
    directory_marked_paths: BTreeSet<PathBuf>,
    pending_directory_operation_preview: Option<(
        DirectoryBufferOperationPreview,
        DirectoryBufferOperationPlan,
    )>,
    directory_operation_confirmation_dialog_active: bool,
}

impl EditorSessionState {
    /// 新しいセッション状態を作成する。
    pub fn new(target_path: Option<PathBuf>) -> Self {
        Self::new_with_tab_size_and_line_numbers_and_number_width(target_path, 8, false, 4)
    }

    /// タブ幅を指定して新しいセッション状態を作成する。
    pub fn new_with_tab_size(target_path: Option<PathBuf>, tab_size: u16) -> Self {
        Self::new_with_tab_size_and_line_numbers_and_number_width(target_path, tab_size, false, 4)
    }

    /// タブ幅と行番号表示を指定して新しいセッション状態を作成する。
    pub fn new_with_tab_size_and_line_numbers(
        target_path: Option<PathBuf>,
        tab_size: u16,
        line_numbers: bool,
    ) -> Self {
        Self::new_with_tab_size_and_line_numbers_and_number_width(
            target_path,
            tab_size,
            line_numbers,
            4,
        )
    }

    /// タブ幅、行番号表示、行番号欄幅を指定して新しいセッション状態を作成する。
    pub fn new_with_tab_size_and_line_numbers_and_number_width(
        target_path: Option<PathBuf>,
        tab_size: u16,
        line_numbers: bool,
        number_width: u16,
    ) -> Self {
        Self::new_with_options(target_path, tab_size, line_numbers, number_width, false)
    }

    /// タブ幅、行番号表示、行番号欄幅、read-only を指定して新しいセッション状態を作成する。
    pub fn new_with_options(
        target_path: Option<PathBuf>,
        tab_size: u16,
        line_numbers: bool,
        number_width: u16,
        read_only: bool,
    ) -> Self {
        let tab_size = tab_size.max(1);
        let number_width = number_width.max(1);
        log::debug!(
            "[editor_session] new session state: target_path={:?}, tab_size={}, line_numbers={}, number_width={}, read_only={}",
            target_path,
            tab_size,
            line_numbers,
            number_width,
            read_only
        );
        let mut state = Self {
            target_path,
            tab_size,
            line_numbers,
            number_width,
            relative_number: false,
            cursorline: false,
            scrolloff: 0,
            sidescrolloff: 0,
            wrap: true,
            laststatus: 2,
            message_area_height: 5,
            message_scroll_offset: 0,
            message_pager: None,
            list: false,
            listchars: "tab:>-,trail:-".to_string(),
            markdown_render: true,
            foldmethod: "manual".to_string(),
            foldlevel: 0,
            resolved_theme: ResolvedTheme::default(),
            read_only,
            dirty: false,
            last_save_error: None,
            directory_buffer: None,
            directory_marked_paths: BTreeSet::new(),
            pending_directory_operation_preview: None,
            directory_operation_confirmation_dialog_active: false,
        };
        if let Some(path) = state.target_path.clone() {
            if let Err(error) = state.refresh_directory_buffer_for_path(&path) {
                log::debug!(
                    "[editor_session] failed to initialize directory buffer metadata: path={}, error={}",
                    path.display(),
                    error
                );
            }
        }
        state
    }

    /// 現在の buffer 内容から保存要求を生成する。
    /// target_path が未設定の場合は SaveRequestError::NoTargetPath を返す。
    pub fn build_save_request(
        &self,
        buffer_contents: &str,
    ) -> Result<SaveRequest, SaveRequestError> {
        log::debug!(
            "[editor_session] building save request: target_path={:?}, contents_len={}, read_only={}",
            self.target_path,
            buffer_contents.len(),
            self.read_only
        );
        if self.read_only {
            log::debug!("[editor_session] save request failed: session is read-only");
            return Err(SaveRequestError::ReadOnly);
        }
        if let Some(directory_buffer) = &self.directory_buffer {
            log::debug!(
                "[editor_session] save request failed: active directory buffer is not writable: root_path={}, entries={}",
                directory_buffer.root_path.display(),
                directory_buffer.entries.len()
            );
            return Err(SaveRequestError::DirectoryBuffer);
        }
        match &self.target_path {
            Some(path) => {
                if path.is_dir() {
                    log::debug!(
                        "[editor_session] save request failed: directory buffer is not writable: path={}",
                        path.display()
                    );
                    return Err(SaveRequestError::DirectoryBuffer);
                }
                let request = SaveRequest {
                    path: path.clone(),
                    contents: buffer_contents.to_string(),
                };
                log::debug!(
                    "[editor_session] save request built: path={}",
                    path.display()
                );
                Ok(request)
            }
            None => {
                log::debug!("[editor_session] save request failed: no target path");
                Err(SaveRequestError::NoTargetPath)
            }
        }
    }

    /// dirty 状態を更新する（CoreBridge の snapshot から反映する想定）。
    pub fn update_dirty(&mut self, dirty: bool) {
        if let Some(directory_buffer) = &self.directory_buffer {
            log::debug!(
                "[editor_session][dired] dirty update projected for directory buffer: previous={}, requested={}, root_path={}, entries={}",
                self.dirty,
                dirty,
                directory_buffer.root_path.display(),
                directory_buffer.entries.len()
            );
            self.dirty = dirty;
            return;
        }
        log::debug!(
            "[editor_session] dirty state updated: {} -> {}",
            self.dirty,
            dirty
        );
        self.dirty = dirty;
    }

    /// 現在の dirty 状態を返す。
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// 直近の保存失敗メッセージを返す。
    pub fn last_save_error(&self) -> Option<&str> {
        self.last_save_error.as_deref()
    }

    /// 保存成功を記録し、dirty 状態を解除する。
    pub fn record_save_success(&mut self) {
        log::debug!("[editor_session] save success recorded: dirty -> false");
        self.dirty = false;
        self.last_save_error = None;
    }

    /// 保存失敗を記録する。dirty 状態は維持される。
    pub fn record_save_failure(&mut self, message: String) {
        log::debug!(
            "[editor_session] save failure recorded: message={}, dirty={}",
            message,
            self.dirty
        );
        self.last_save_error = Some(message);
    }

    /// 終了要求を判定する。
    pub fn evaluate_quit(&self, force: bool) -> QuitDecision {
        log::debug!(
            "[editor_session] evaluating quit: force={}, dirty={}",
            force,
            self.dirty
        );
        if force {
            log::debug!("[editor_session] quit decision: ForceQuit");
            QuitDecision::ForceQuit
        } else if self.dirty {
            log::debug!("[editor_session] quit decision: WarnUnsaved");
            QuitDecision::WarnUnsaved
        } else {
            log::debug!("[editor_session] quit decision: Allow");
            QuitDecision::Allow
        }
    }

    /// 対象パスの参照を返す。
    pub fn target_path(&self) -> Option<&PathBuf> {
        self.target_path.as_ref()
    }

    /// active buffer の対象パスを host application 側の保存状態へ反映する。
    pub fn replace_target_path(&mut self, target_path: PathBuf) {
        log::debug!(
            "[editor_session] replacing target path from runtime host action: old={:?}, new={}",
            self.target_path,
            target_path.display()
        );
        let directory_buffer_is_current = target_path.is_dir()
            && self
                .directory_buffer
                .as_ref()
                .is_some_and(|directory_buffer| {
                    paths_refer_to_same_location(&directory_buffer.root_path, &target_path)
                });
        if directory_buffer_is_current {
            self.rebase_directory_buffer_root(&target_path);
            log::debug!(
                "[editor_session][dired] reusing already refreshed directory buffer metadata during target replacement: path={}",
                target_path.display()
            );
        } else if let Err(error) = self.refresh_directory_buffer_for_path(&target_path) {
            log::debug!(
                "[editor_session] failed to refresh directory buffer metadata during target replacement: path={}, error={}",
                target_path.display(),
                error
            );
            self.directory_buffer = None;
        }
        self.target_path = Some(target_path);
        self.dirty = false;
        self.last_save_error = None;
    }

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

    fn rebase_directory_buffer_root(&mut self, target_path: &Path) {
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

    fn refresh_directory_buffer_for_path(&mut self, path: &Path) -> std::io::Result<()> {
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

    /// 描画時のタブ幅を返す。
    pub fn tab_size(&self) -> u16 {
        self.tab_size
    }

    /// 行番号表示が有効かを返す。
    pub fn line_numbers(&self) -> bool {
        self.line_numbers
    }

    /// 行番号欄の最小幅を返す。
    pub fn number_width(&self) -> u16 {
        self.number_width
    }

    pub fn relative_number(&self) -> bool {
        self.relative_number
    }

    pub fn cursorline(&self) -> bool {
        self.cursorline
    }

    pub fn scrolloff(&self) -> u16 {
        self.scrolloff
    }

    pub fn sidescrolloff(&self) -> u16 {
        self.sidescrolloff
    }

    pub fn wrap(&self) -> bool {
        self.wrap
    }

    pub fn laststatus(&self) -> u8 {
        self.laststatus
    }

    pub fn message_area_height(&self) -> u16 {
        self.message_area_height
    }

    pub fn message_scroll_offset(&self) -> u16 {
        self.message_scroll_offset
    }

    pub fn sync_message_pager(&mut self, message: &str, visible_height: u16) -> bool {
        let normalized = normalize_message_pager_key(message);
        let line_count = normalized.lines().count();
        let visible_height = visible_height.max(1);
        let max_offset = u16::try_from(line_count.saturating_sub(usize::from(visible_height)))
            .unwrap_or(u16::MAX);
        let before_active = self.message_pager_active();
        let before_offset = self.message_scroll_offset;

        if normalized.is_empty() || line_count <= 1 {
            self.message_pager = None;
            self.message_scroll_offset = 0;
            log::debug!(
                "[editor_session] message pager cleared: reason=no_multiline_message, line_count={}, visible_height={}",
                line_count,
                visible_height
            );
            return before_active || before_offset != 0;
        }

        match self.message_pager.as_mut() {
            Some(pager) if pager.message_key == normalized => {
                pager.max_offset = max_offset;
                self.message_scroll_offset = self.message_scroll_offset.min(max_offset);
            }
            _ => {
                log::debug!(
                    "[editor_session] message pager activated: line_count={}, visible_height={}, max_offset={}",
                    line_count,
                    visible_height,
                    max_offset
                );
                self.message_scroll_offset = 0;
                self.message_pager = Some(MessagePagerState {
                    message_key: normalized,
                    max_offset,
                    dismissed: false,
                });
            }
        }

        before_active != self.message_pager_active() || before_offset != self.message_scroll_offset
    }

    pub fn message_pager_active(&self) -> bool {
        self.message_pager
            .as_ref()
            .is_some_and(|pager| !pager.dismissed)
    }

    pub fn message_pager_prompt_kind(&self) -> Option<CorePagerPromptKind> {
        let pager = self.message_pager.as_ref()?;
        if pager.dismissed {
            return None;
        }
        if self.message_scroll_offset >= pager.max_offset {
            Some(CorePagerPromptKind::HitReturn)
        } else {
            Some(CorePagerPromptKind::More)
        }
    }

    pub fn message_pager_hides_message(&self, message: &str) -> bool {
        let normalized = normalize_message_pager_key(message);
        self.message_pager.as_ref().is_some_and(|pager| {
            pager.dismissed && !normalized.is_empty() && pager.message_key == normalized
        })
    }

    pub fn reopen_message_pager(&mut self) -> Option<String> {
        let pager = self.message_pager.as_mut()?;
        pager.dismissed = false;
        self.message_scroll_offset = 0;
        log::debug!(
            "[editor_session] message pager reopened: max_offset={}, message_len={}",
            pager.max_offset,
            pager.message_key.len()
        );
        Some(pager.message_key.clone())
    }

    pub fn handle_message_pager_key(&mut self, key: &KeyInput) -> bool {
        let Some(action) = MessagePagerAction::from_key(key) else {
            return false;
        };
        if !self.message_pager_active() {
            return false;
        }
        let Some(pager) = self.message_pager.as_mut() else {
            return false;
        };

        let before = self.message_scroll_offset;
        match action {
            MessagePagerAction::Enter => {
                if self.message_scroll_offset >= pager.max_offset {
                    pager.dismissed = true;
                } else {
                    self.message_scroll_offset = self
                        .message_scroll_offset
                        .saturating_add(1)
                        .min(pager.max_offset);
                }
            }
            MessagePagerAction::ForwardLine => {
                self.message_scroll_offset = self
                    .message_scroll_offset
                    .saturating_add(1)
                    .min(pager.max_offset);
            }
            MessagePagerAction::ForwardHalfPage => {
                let delta = (self.message_area_height / 2).max(1);
                self.message_scroll_offset = self
                    .message_scroll_offset
                    .saturating_add(delta)
                    .min(pager.max_offset);
            }
            MessagePagerAction::ForwardPage => {
                self.message_scroll_offset = self
                    .message_scroll_offset
                    .saturating_add(self.message_area_height.max(1))
                    .min(pager.max_offset);
            }
            MessagePagerAction::BackwardLine => {
                self.message_scroll_offset = self.message_scroll_offset.saturating_sub(1);
            }
            MessagePagerAction::BackwardHalfPage => {
                let delta = (self.message_area_height / 2).max(1);
                self.message_scroll_offset = self.message_scroll_offset.saturating_sub(delta);
            }
            MessagePagerAction::BackwardPage => {
                self.message_scroll_offset = self
                    .message_scroll_offset
                    .saturating_sub(self.message_area_height.max(1));
            }
            MessagePagerAction::Top => {
                self.message_scroll_offset = 0;
            }
            MessagePagerAction::Bottom => {
                self.message_scroll_offset = pager.max_offset;
            }
            MessagePagerAction::Dismiss => {
                pager.dismissed = true;
            }
        }
        log::debug!(
            "[editor_session] message pager key handled: key={:?}, action={:?}, before={}, after={}, max_offset={}, active={}",
            key,
            action,
            before,
            self.message_scroll_offset,
            pager.max_offset,
            !pager.dismissed
        );
        true
    }

    pub fn scroll_message_area_by(&mut self, delta: i16, max_offset: u16) -> bool {
        let before = self.message_scroll_offset.min(max_offset);
        let after = if delta < 0 {
            before.saturating_sub(delta.unsigned_abs())
        } else {
            before.saturating_add(delta as u16).min(max_offset)
        };
        self.message_scroll_offset = after;
        log::debug!(
            "[editor_session] message area scroll: before={}, after={}, delta={}, max_offset={}",
            before,
            after,
            delta,
            max_offset
        );
        before != after
    }

    pub fn list(&self) -> bool {
        self.list
    }

    pub fn listchars(&self) -> &str {
        &self.listchars
    }

    pub fn markdown_render(&self) -> bool {
        self.markdown_render
    }

    pub fn foldmethod(&self) -> &str {
        &self.foldmethod
    }

    pub fn foldlevel(&self) -> u16 {
        self.foldlevel
    }

    pub fn resolved_theme(&self) -> &ResolvedTheme {
        &self.resolved_theme
    }

    pub fn set_resolved_theme(&mut self, resolved_theme: ResolvedTheme) {
        log::debug!("[editor_session] resolved theme updated for session");
        self.resolved_theme = resolved_theme;
    }

    /// 行番号表示の有効/無効を更新する。
    pub fn set_line_numbers(&mut self, enabled: bool) {
        log::debug!(
            "[editor_session] line number visibility updated: {} -> {}",
            self.line_numbers,
            enabled
        );
        self.line_numbers = enabled;
    }

    /// 行番号欄の最小幅を更新する。
    pub fn set_number_width(&mut self, width: u16) {
        let width = width.max(1);
        log::debug!(
            "[editor_session] number width updated: {} -> {}",
            self.number_width,
            width
        );
        self.number_width = width;
    }

    pub fn apply_presentation_option(
        &mut self,
        name: SayaOptionName,
        value: SayaOptionValue,
    ) -> Result<(), String> {
        log::debug!(
            "[editor_session] applying presentation option: name={}, value={:?}",
            name,
            value
        );
        match (name, value) {
            (SayaOptionName::LineNumbers, SayaOptionValue::Boolean(value)) => {
                self.set_line_numbers(value);
                Ok(())
            }
            (SayaOptionName::RelativeNumber, SayaOptionValue::Boolean(value)) => {
                self.relative_number = value;
                Ok(())
            }
            (SayaOptionName::CursorLine, SayaOptionValue::Boolean(value)) => {
                self.cursorline = value;
                Ok(())
            }
            (SayaOptionName::ScrollOff, SayaOptionValue::Number(value)) => {
                self.scrolloff = u16::try_from(value.max(0)).unwrap_or(u16::MAX);
                Ok(())
            }
            (SayaOptionName::SidescrollOff, SayaOptionValue::Number(value)) => {
                self.sidescrolloff = u16::try_from(value.max(0)).unwrap_or(u16::MAX);
                Ok(())
            }
            (SayaOptionName::Wrap, SayaOptionValue::Boolean(value)) => {
                self.wrap = value;
                Ok(())
            }
            (SayaOptionName::NumberWidth, SayaOptionValue::Number(value)) => {
                self.set_number_width(u16::try_from(value.max(1)).unwrap_or(u16::MAX));
                Ok(())
            }
            (SayaOptionName::LastStatus, SayaOptionValue::Number(value)) => {
                self.laststatus = u8::try_from(value.clamp(0, 3)).unwrap_or(2);
                Ok(())
            }
            (SayaOptionName::MessageHeight, SayaOptionValue::Number(value)) => {
                let next = u16::try_from(value.max(1)).unwrap_or(u16::MAX);
                log::debug!(
                    "[editor_session] message area height updated: {} -> {}",
                    self.message_area_height,
                    next
                );
                self.message_area_height = next;
                self.message_scroll_offset = 0;
                self.message_pager = None;
                Ok(())
            }
            (SayaOptionName::List, SayaOptionValue::Boolean(value)) => {
                self.list = value;
                Ok(())
            }
            (SayaOptionName::ListChars, SayaOptionValue::String(value)) => {
                self.listchars = value;
                Ok(())
            }
            (SayaOptionName::MarkdownRender, SayaOptionValue::Boolean(value)) => {
                log::debug!(
                    "[editor_session] markdown render projection updated: {} -> {}",
                    self.markdown_render,
                    value
                );
                self.markdown_render = value;
                Ok(())
            }
            (SayaOptionName::FoldMethod, SayaOptionValue::String(value)) => {
                self.foldmethod = value;
                Ok(())
            }
            (SayaOptionName::FoldLevel, SayaOptionValue::Number(value)) => {
                self.foldlevel = u16::try_from(value.max(0)).unwrap_or(u16::MAX);
                Ok(())
            }
            (name, value) => Err(format!(
                "presentation option type mismatch: name={name}, value={value:?}"
            )),
        }
    }

    pub fn read_only(&self) -> bool {
        self.read_only
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

fn directory_buffer_compare_entries(
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

fn directory_buffer_directory_group_rank(kind: DirectoryBufferEntryKind) -> usize {
    match kind {
        DirectoryBufferEntryKind::Directory => 0,
        DirectoryBufferEntryKind::File
        | DirectoryBufferEntryKind::Symlink
        | DirectoryBufferEntryKind::Other => 1,
    }
}

fn directory_buffer_entry_id(root_path: &Path, name: &str, kind: DirectoryBufferEntryKind) -> u64 {
    let mut hasher = DefaultHasher::new();
    root_path.hash(&mut hasher);
    name.hash(&mut hasher);
    kind.hash(&mut hasher);
    hasher.finish()
}

fn directory_buffer_operation_preview(
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

fn directory_buffer_operation_prompt(
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

fn directory_buffer_prompt_operation_line(operation: &DirectoryBufferPreviewOperation) -> String {
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

fn display_optional_path(path: Option<&Path>) -> String {
    path.map(|path| path.display().to_string())
        .unwrap_or_else(|| "<none>".to_string())
}

fn directory_buffer_preview_operation(
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

fn validate_directory_buffer_plan_name(
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn unique_test_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "saya-editor-session-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time went backwards")
                .as_nanos()
        ))
    }

    // ---- タスク 5.1: 保存要求の生成テスト ----

    #[test]
    fn build_save_request_returns_request_with_path_and_contents() {
        let state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
        let buffer_contents = "hello world\n";

        let request = state
            .build_save_request(buffer_contents)
            .expect("保存要求の生成に成功すること");

        assert_eq!(request.path, PathBuf::from("/tmp/test.txt"));
        assert_eq!(request.contents, "hello world\n");
    }

    #[test]
    fn build_save_request_fails_when_no_target_path() {
        let state = EditorSessionState::new(None);

        let result = state.build_save_request("data");

        assert_eq!(
            result,
            Err(SaveRequestError::NoTargetPath),
            "target_path が未設定の場合は NoTargetPath エラーになること"
        );
    }

    #[test]
    fn build_save_request_extracts_current_buffer_contents() {
        let state = EditorSessionState::new(Some(PathBuf::from("/tmp/file.txt")));
        let contents = "line1\nline2\nline3\n";

        let request = state.build_save_request(contents).unwrap();

        assert_eq!(
            request.contents, contents,
            "buffer の現在内容がそのまま保存要求に含まれること"
        );
    }

    // ---- タスク 5.2: 保存成功時の clean 状態テスト ----

    #[test]
    fn record_save_success_clears_dirty_state() {
        let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
        state.update_dirty(true);
        assert!(state.is_dirty(), "保存前は dirty であること");

        state.record_save_success();

        assert!(!state.is_dirty(), "保存成功後は dirty が解除されること");
    }

    #[test]
    fn record_save_success_clears_last_save_error() {
        let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
        state.record_save_failure("previous error".to_string());
        assert!(state.last_save_error().is_some());

        state.record_save_success();

        assert_eq!(
            state.last_save_error(),
            None,
            "保存成功後はエラーメッセージがクリアされること"
        );
    }

    #[test]
    fn quit_evaluates_to_allow_after_save_success() {
        let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
        state.update_dirty(true);
        state.record_save_success();

        let decision = state.evaluate_quit(false);

        assert_eq!(
            decision,
            QuitDecision::Allow,
            "保存成功後の通常終了は Allow であること"
        );
    }

    // ---- タスク 5.3: 保存失敗時の編集継続テスト ----

    #[test]
    fn record_save_failure_preserves_dirty_state() {
        let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
        state.update_dirty(true);

        state.record_save_failure("disk full".to_string());

        assert!(state.is_dirty(), "保存失敗後も dirty 状態が維持されること");
    }

    #[test]
    fn record_save_failure_stores_error_message_for_display() {
        let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));

        state.record_save_failure("permission denied".to_string());

        assert_eq!(
            state.last_save_error(),
            Some("permission denied"),
            "保存失敗メッセージが表示用に保持されること"
        );
    }

    #[test]
    fn save_failure_does_not_prevent_further_editing() {
        let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
        state.update_dirty(true);
        state.record_save_failure("write error".to_string());

        // 保存失敗後も dirty 更新が可能であること
        state.update_dirty(true);
        assert!(
            state.is_dirty(),
            "保存失敗後も編集状態の更新が可能であること"
        );

        // 再度保存要求を生成できること
        let request = state.build_save_request("updated content");
        assert!(request.is_ok(), "保存失敗後も保存要求を再生成できること");
    }

    // ---- タスク 5.4: 通常終了と強制終了の分岐テスト ----

    #[test]
    fn evaluate_quit_allows_when_clean() {
        let state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));

        let decision = state.evaluate_quit(false);

        assert_eq!(
            decision,
            QuitDecision::Allow,
            "clean 状態での通常終了は Allow であること"
        );
    }

    #[test]
    fn evaluate_quit_warns_when_dirty_and_not_forced() {
        let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
        state.update_dirty(true);

        let decision = state.evaluate_quit(false);

        assert_eq!(
            decision,
            QuitDecision::WarnUnsaved,
            "dirty 状態での通常終了は WarnUnsaved であること"
        );
    }

    #[test]
    fn evaluate_quit_force_quits_even_when_dirty() {
        let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
        state.update_dirty(true);

        let decision = state.evaluate_quit(true);

        assert_eq!(
            decision,
            QuitDecision::ForceQuit,
            "dirty 状態でも force=true なら ForceQuit であること"
        );
    }

    #[test]
    fn evaluate_quit_force_quits_when_clean() {
        let state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));

        let decision = state.evaluate_quit(true);

        assert_eq!(
            decision,
            QuitDecision::ForceQuit,
            "clean 状態でも force=true なら ForceQuit であること"
        );
    }

    // ---- タスク 5.5: 未保存状態での終了警告テスト ----

    #[test]
    fn dirty_state_triggers_warn_unsaved_on_normal_quit() {
        let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
        state.update_dirty(true);

        let decision = state.evaluate_quit(false);

        assert_eq!(
            decision,
            QuitDecision::WarnUnsaved,
            "dirty 状態の通常終了は警告に切り替わること"
        );
    }

    #[test]
    fn warn_unsaved_does_not_terminate_session() {
        let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
        state.update_dirty(true);

        let decision = state.evaluate_quit(false);
        assert_eq!(decision, QuitDecision::WarnUnsaved);

        // 警告後もセッション状態は維持される
        assert!(
            state.is_dirty(),
            "警告後も dirty 状態が維持されること（即時終了しない）"
        );

        // 警告後も保存要求を生成できる
        let request = state.build_save_request("content");
        assert!(request.is_ok(), "警告後も保存操作が可能であること");
    }

    #[test]
    fn force_quit_bypasses_unsaved_warning() {
        let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
        state.update_dirty(true);

        // 通常終了では警告になる
        assert_eq!(state.evaluate_quit(false), QuitDecision::WarnUnsaved);

        // 強制終了では即時終了
        assert_eq!(
            state.evaluate_quit(true),
            QuitDecision::ForceQuit,
            "強制終了は未保存警告をバイパスすること"
        );
    }

    #[test]
    fn clean_state_after_save_allows_normal_quit() {
        let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
        state.update_dirty(true);

        // 保存前は警告
        assert_eq!(state.evaluate_quit(false), QuitDecision::WarnUnsaved);

        // 保存成功
        state.record_save_success();

        // 保存後は通常終了可能
        assert_eq!(
            state.evaluate_quit(false),
            QuitDecision::Allow,
            "保存成功後は通常終了が許可されること"
        );
    }

    #[test]
    fn new_with_tab_size_preserves_requested_value() {
        let state = EditorSessionState::new_with_tab_size(None, 4);

        assert_eq!(state.tab_size(), 4);
    }

    #[test]
    fn new_with_tab_size_clamps_zero_to_one() {
        let state = EditorSessionState::new_with_tab_size(None, 0);

        assert_eq!(state.tab_size(), 1);
    }

    #[test]
    fn new_with_number_width_defaults_to_four() {
        let state = EditorSessionState::new(None);

        assert_eq!(state.number_width(), 4);
    }

    #[test]
    fn set_line_numbers_enables_projection_flag() {
        let mut state = EditorSessionState::new(None);
        assert!(!state.line_numbers(), "既定値は false であること");

        state.set_line_numbers(true);

        assert!(state.line_numbers(), "行番号表示が有効になること");
    }

    #[test]
    fn set_line_numbers_disables_projection_flag() {
        let mut state = EditorSessionState::new_with_tab_size_and_line_numbers(None, 8, true);
        assert!(state.line_numbers(), "初期状態は true であること");

        state.set_line_numbers(false);

        assert!(!state.line_numbers(), "行番号表示が無効になること");
    }

    #[test]
    fn set_number_width_updates_projection_width() {
        let mut state = EditorSessionState::new(None);

        state.set_number_width(6);

        assert_eq!(state.number_width(), 6);
    }

    #[test]
    fn set_number_width_clamps_zero_to_one() {
        let mut state = EditorSessionState::new(None);

        state.set_number_width(0);

        assert_eq!(state.number_width(), 1);
    }

    #[test]
    fn message_area_height_defaults_to_five() {
        let state = EditorSessionState::new(None);

        assert_eq!(state.message_area_height(), 5);
        assert_eq!(state.message_scroll_offset(), 0);
    }

    #[test]
    fn scroll_message_area_by_clamps_to_available_range() {
        let mut state = EditorSessionState::new(None);

        assert!(state.scroll_message_area_by(2, 3));
        assert_eq!(state.message_scroll_offset(), 2);
        assert!(state.scroll_message_area_by(2, 3));
        assert_eq!(state.message_scroll_offset(), 3);
        assert!(state.scroll_message_area_by(-1, 3));
        assert_eq!(state.message_scroll_offset(), 2);
        assert!(state.scroll_message_area_by(-9, 3));
        assert_eq!(state.message_scroll_offset(), 0);
    }

    #[test]
    fn message_pager_enters_more_state_for_overflowing_message_and_uses_vim_keys() {
        let mut state = EditorSessionState::new(None);

        assert!(state.sync_message_pager("one\ntwo\nthree\nfour", 2));
        assert!(state.message_pager_active());
        assert_eq!(
            state.message_pager_prompt_kind(),
            Some(vim_core_rs::CorePagerPromptKind::More)
        );

        assert!(state.handle_message_pager_key(&crate::input::router::KeyInput::Enter));
        assert_eq!(state.message_scroll_offset(), 1);
        assert!(state.handle_message_pager_key(&crate::input::router::KeyInput::Char(' ')));
        assert_eq!(state.message_scroll_offset(), 2);
        assert_eq!(
            state.message_pager_prompt_kind(),
            Some(vim_core_rs::CorePagerPromptKind::HitReturn)
        );
        assert!(state.handle_message_pager_key(&crate::input::router::KeyInput::Char('j')));
        assert!(state.message_pager_active());
        assert_eq!(state.message_scroll_offset(), 2);
        assert!(state.handle_message_pager_key(&crate::input::router::KeyInput::Enter));
        assert!(!state.message_pager_active());
        assert!(state.message_pager_hides_message("one\ntwo\nthree\nfour"));
    }

    #[test]
    fn message_pager_supports_backward_keys_without_leaving_message_mode() {
        let mut state = EditorSessionState::new(None);
        state
            .apply_presentation_option(SayaOptionName::MessageHeight, SayaOptionValue::Number(2))
            .expect("message height option should apply");

        assert!(state.sync_message_pager("one\ntwo\nthree\nfour\nfive\nsix", 2));
        assert!(state.handle_message_pager_key(&crate::input::router::KeyInput::Char('G')));
        assert_eq!(state.message_scroll_offset(), 4);
        assert!(state.handle_message_pager_key(&crate::input::router::KeyInput::Char('k')));
        assert_eq!(state.message_scroll_offset(), 3);
        assert!(state.handle_message_pager_key(&crate::input::router::KeyInput::Char('b')));
        assert_eq!(state.message_scroll_offset(), 1);
        assert!(state.handle_message_pager_key(&crate::input::router::KeyInput::Char('g')));
        assert_eq!(state.message_scroll_offset(), 0);
        assert!(state.message_pager_active());
    }

    #[test]
    fn message_pager_can_be_reopened_after_hit_return_dismisses_it() {
        let mut state = EditorSessionState::new(None);
        assert!(state.sync_message_pager("one\ntwo\nthree\nfour", 2));
        assert!(state.handle_message_pager_key(&crate::input::router::KeyInput::Char('G')));
        assert_eq!(
            state.message_pager_prompt_kind(),
            Some(vim_core_rs::CorePagerPromptKind::HitReturn)
        );
        assert!(state.handle_message_pager_key(&crate::input::router::KeyInput::Enter));
        assert!(!state.message_pager_active());

        let message = state
            .reopen_message_pager()
            .expect("dismissed message pager should be reopenable");

        assert_eq!(message, "one\ntwo\nthree\nfour");
        assert_eq!(state.message_scroll_offset(), 0);
        assert_eq!(
            state.message_pager_prompt_kind(),
            Some(vim_core_rs::CorePagerPromptKind::More)
        );
    }

    #[test]
    fn message_pager_escape_dismisses_and_marks_current_message_hidden() {
        let mut state = EditorSessionState::new(None);
        assert!(state.sync_message_pager("one\ntwo\nthree\nfour", 2));

        assert!(state.handle_message_pager_key(&crate::input::router::KeyInput::Escape));

        assert!(!state.message_pager_active());
        assert!(state.message_pager_hides_message("one\ntwo\nthree\nfour"));
    }

    #[test]
    fn message_pager_enter_dismisses_multiline_message_that_fits_visible_height() {
        let mut state = EditorSessionState::new(None);
        assert!(state.sync_message_pager("one\ntwo\nthree\nfour\nfive", 5));
        assert_eq!(
            state.message_pager_prompt_kind(),
            Some(vim_core_rs::CorePagerPromptKind::HitReturn)
        );

        assert!(state.handle_message_pager_key(&crate::input::router::KeyInput::Enter));

        assert!(!state.message_pager_active());
        assert!(state.message_pager_hides_message("one\ntwo\nthree\nfour\nfive"));
    }

    #[test]
    fn message_pager_ctrl_left_bracket_dismisses_like_escape() {
        let mut state = EditorSessionState::new(None);
        assert!(state.sync_message_pager("one\ntwo\nthree\nfour", 2));

        assert!(state.handle_message_pager_key(&crate::input::router::KeyInput::Ctrl('[')));

        assert!(!state.message_pager_active());
        assert!(state.message_pager_hides_message("one\ntwo\nthree\nfour"));
    }

    #[test]
    fn build_save_request_fails_when_session_is_read_only() {
        let state = EditorSessionState::new_with_options(None, 8, false, 4, true);

        let result = state.build_save_request("content");

        assert_eq!(result, Err(SaveRequestError::ReadOnly));
        assert!(state.read_only());
    }

    #[test]
    fn build_save_request_fails_when_target_path_is_directory() {
        let dir_path = std::env::temp_dir().join(format!(
            "saya-editor-session-directory-buffer-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time went backwards")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir_path).expect("test directory");
        let state = EditorSessionState::new(Some(dir_path.clone()));

        let result = state.build_save_request("README.md\n");

        assert_eq!(result, Err(SaveRequestError::DirectoryBuffer));
        std::fs::remove_dir(dir_path).expect("cleanup directory");
    }

    #[test]
    fn directory_buffer_metadata_tracks_display_root_entries_and_ids() {
        let root_path = std::env::temp_dir().join(format!(
            "saya-editor-session-directory-metadata-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time went backwards")
                .as_nanos()
        ));
        let nested_path = root_path.join("src");
        let readme_path = root_path.join("README.md");
        std::fs::create_dir_all(&nested_path).expect("nested directory");
        std::fs::write(&readme_path, "hello\n").expect("test file");

        let state = EditorSessionState::new(Some(root_path.clone()));
        let directory_buffer = state
            .directory_buffer()
            .expect("directory target should initialize directory buffer metadata");

        assert_eq!(directory_buffer.root_path, root_path);
        assert_eq!(directory_buffer.display_text, "src/\nREADME.md\n");
        assert_eq!(directory_buffer.entries.len(), 2);
        assert_eq!(directory_buffer.entries[0].name, "src");
        assert_eq!(
            directory_buffer.entries[0].kind,
            DirectoryBufferEntryKind::Directory
        );
        assert_eq!(directory_buffer.entries[0].display_text, "src/");
        assert_ne!(
            directory_buffer.entries[0].id, directory_buffer.entries[1].id,
            "entry ids should distinguish entries in the same directory"
        );
        assert_eq!(directory_buffer.entries[1].name, "README.md");
        assert_eq!(directory_buffer.entries[1].path, readme_path);
        assert_eq!(
            directory_buffer.entries[1].kind,
            DirectoryBufferEntryKind::File
        );
        assert_eq!(directory_buffer.entries[1].display_text, "README.md");

        std::fs::remove_dir_all(directory_buffer.root_path.clone()).expect("cleanup directory");
    }

    #[cfg(unix)]
    #[test]
    fn directory_buffer_kind_sort_groups_directories_then_sorts_other_entries_by_name() {
        let root_path = std::env::temp_dir().join(format!(
            "saya-editor-session-directory-kind-sort-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time went backwards")
                .as_nanos()
        ));
        let directory_path = root_path.join("middle-dir");
        let file_path = root_path.join("z-file.txt");
        let target_path = root_path.join("target.md");
        let link_path = root_path.join("a-link.md");
        std::fs::create_dir_all(&directory_path).expect("nested directory");
        std::fs::write(&file_path, "file\n").expect("file entry");
        std::fs::write(&target_path, "target\n").expect("symlink target");
        std::os::unix::fs::symlink(&target_path, &link_path).expect("symlink");

        let directory_buffer = read_directory_buffer_state_with_options(
            &root_path,
            DirectoryBufferListingOptions {
                show_hidden: true,
                sort_by: DirectoryBufferSortKey::Kind,
                filter: None,
            },
        )
        .expect("directory listing");

        assert_eq!(
            directory_buffer
                .entries
                .iter()
                .map(|entry| entry.display_text.as_str())
                .collect::<Vec<_>>(),
            vec!["middle-dir/", "a-link.md@", "target.md", "z-file.txt"]
        );

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }

    #[cfg(unix)]
    #[test]
    fn directory_buffer_display_marks_symlink_entries() {
        let root_path = std::env::temp_dir().join(format!(
            "saya-editor-session-directory-symlink-display-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time went backwards")
                .as_nanos()
        ));
        let target_path = root_path.join("target.md");
        let link_path = root_path.join("linked.md");
        std::fs::create_dir_all(&root_path).expect("test directory");
        std::fs::write(&target_path, "target\n").expect("target file");
        std::os::unix::fs::symlink(&target_path, &link_path).expect("symlink");

        let state = EditorSessionState::new(Some(root_path.clone()));
        let directory_buffer = state
            .directory_buffer()
            .expect("directory target should initialize directory buffer metadata");

        let link = directory_buffer
            .entries
            .iter()
            .find(|entry| entry.name == "linked.md")
            .expect("symlink entry should exist");
        assert_eq!(link.kind, DirectoryBufferEntryKind::Symlink);
        assert_eq!(link.display_text, "linked.md@");
        assert!(
            directory_buffer.display_text.contains("linked.md@\n"),
            "rendered dired listing should identify symlinks: {:?}",
            directory_buffer.display_text
        );

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }

    #[test]
    fn directory_buffer_dirty_update_is_projected_but_kept_out_of_regular_save_flow() {
        let dir_path = std::env::temp_dir().join(format!(
            "saya-editor-session-directory-dirty-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time went backwards")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir_path).expect("test directory");
        let mut state = EditorSessionState::new(Some(dir_path.clone()));

        state.update_dirty(true);

        assert!(
            state.is_dirty(),
            "directory buffer edits should still project a modified state"
        );
        assert_eq!(
            state.build_save_request("mutated listing"),
            Err(SaveRequestError::DirectoryBuffer)
        );

        std::fs::remove_dir(dir_path).expect("cleanup directory");
    }

    #[test]
    fn directory_buffer_preview_prompt_emphasizes_risky_operations_for_headless_ui() {
        let root_path = unique_test_dir("preview-prompt");
        let alpha_path = root_path.join("alpha.md");
        let beta_path = root_path.join("beta.md");
        std::fs::create_dir_all(&root_path).expect("test directory");
        std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
        std::fs::write(&beta_path, "beta\n").expect("beta file");
        let mut state = EditorSessionState::new(Some(root_path.clone()));

        let preview = state
            .prepare_directory_buffer_operation_preview("beta.md\n")
            .expect("deleted listing line should prepare a preview");
        let prompt = state
            .directory_buffer_operation_prompt()
            .expect("prepared preview should expose a prompt state");

        assert_eq!(prompt.preview_id, preview.id);
        assert!(prompt.status_line.contains("Apply 1 dired operation"));
        assert!(prompt.status_line.contains("1 high-risk"));
        assert!(prompt.status_line.contains("y/Enter=OK"));
        assert!(prompt.status_line.contains("n/Esc=Cancel"));
        assert!(
            prompt.detail_lines.iter().any(|line| {
                line.contains("HIGH RISK") && line.contains("delete") && line.contains("alpha.md")
            }),
            "delete preview should be clearly marked as high risk: {:?}",
            prompt.detail_lines
        );
        assert_eq!(prompt.confirm_command, "OK");
        assert_eq!(prompt.cancel_command, "Cancel");
        assert!(prompt.recovery_hint.contains("No filesystem changes"));
        assert!(
            alpha_path.exists(),
            "preview must not mutate the filesystem"
        );

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }

    #[test]
    fn directory_buffer_preview_cancel_clears_state_without_filesystem_mutation() {
        let root_path = unique_test_dir("preview-cancel");
        let alpha_path = root_path.join("alpha.md");
        let beta_path = root_path.join("beta.md");
        std::fs::create_dir_all(&root_path).expect("test directory");
        std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
        std::fs::write(&beta_path, "beta\n").expect("beta file");
        let mut state = EditorSessionState::new(Some(root_path.clone()));

        state
            .prepare_directory_buffer_operation_preview("beta.md\n")
            .expect("deleted listing line should prepare a preview");
        let cancelled = state
            .cancel_directory_buffer_operation_preview()
            .expect("pending preview should be cancellable");

        assert_eq!(cancelled.operation_count, 1);
        assert!(state.pending_directory_operation_preview().is_none());
        assert!(state.directory_buffer_operation_prompt().is_none());
        assert!(alpha_path.exists(), "cancel must not delete files");
        assert!(beta_path.exists());

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }

    #[test]
    fn directory_buffer_mark_state_survives_refresh_and_tracks_rename() {
        let root_path = std::env::temp_dir().join(format!(
            "saya-editor-session-directory-mark-refresh-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time went backwards")
                .as_nanos()
        ));
        let alpha_path = root_path.join("alpha.md");
        let beta_path = root_path.join("beta.md");
        let renamed_path = root_path.join("renamed.md");
        std::fs::create_dir_all(&root_path).expect("test directory");
        std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
        std::fs::write(&beta_path, "beta\n").expect("beta file");
        let mut state = EditorSessionState::new(Some(root_path.clone()));

        let alpha = state
            .current_directory_entry(0)
            .expect("alpha should be first")
            .clone();
        state.mark_directory_entry(&alpha);
        state
            .refresh_directory_buffer_for_path(&root_path)
            .expect("refresh should keep directory metadata");

        assert_eq!(
            state.marked_directory_entries(),
            vec![alpha.clone()],
            "marked entry should remain selected after refresh"
        );

        std::fs::rename(&alpha_path, &renamed_path).expect("rename file");
        state.record_directory_entry_rename(&alpha_path, &renamed_path);
        state
            .refresh_directory_buffer_for_path(&root_path)
            .expect("refresh after rename");

        let marked = state.marked_directory_entries();
        assert_eq!(marked.len(), 1);
        assert_eq!(marked[0].name, "renamed.md");
        assert_eq!(marked[0].path, renamed_path);

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }

    #[test]
    fn directory_buffer_mark_state_clears_when_moving_to_another_directory() {
        let root_path = std::env::temp_dir().join(format!(
            "saya-editor-session-directory-mark-move-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time went backwards")
                .as_nanos()
        ));
        let first_path = root_path.join("first");
        let second_path = root_path.join("second");
        let first_file_path = first_path.join("alpha.md");
        let second_file_path = second_path.join("beta.md");
        std::fs::create_dir_all(&first_path).expect("first directory");
        std::fs::create_dir_all(&second_path).expect("second directory");
        std::fs::write(&first_file_path, "alpha\n").expect("first file");
        std::fs::write(&second_file_path, "beta\n").expect("second file");
        let mut state = EditorSessionState::new(Some(first_path.clone()));

        let entry = state
            .current_directory_entry(0)
            .expect("entry should exist")
            .clone();
        state.mark_directory_entry(&entry);
        assert_eq!(state.marked_directory_entries().len(), 1);

        state.replace_target_path(second_path);

        assert!(
            state.marked_directory_entries().is_empty(),
            "marks from the previous directory must not leak into the next directory"
        );

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }

    #[test]
    fn directory_buffer_unmark_and_clear_all_update_mark_state() {
        let root_path = std::env::temp_dir().join(format!(
            "saya-editor-session-directory-mark-clear-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time went backwards")
                .as_nanos()
        ));
        let alpha_path = root_path.join("alpha.md");
        let beta_path = root_path.join("beta.md");
        std::fs::create_dir_all(&root_path).expect("test directory");
        std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
        std::fs::write(&beta_path, "beta\n").expect("beta file");
        let mut state = EditorSessionState::new(Some(root_path.clone()));
        let alpha = state
            .current_directory_entry(0)
            .expect("alpha should exist")
            .clone();
        let beta = state
            .current_directory_entry(1)
            .expect("beta should exist")
            .clone();

        state.mark_directory_entry(&alpha);
        state.mark_directory_entry(&beta);
        assert_eq!(state.marked_directory_entries().len(), 2);

        state.unmark_directory_entry(&alpha);
        assert_eq!(state.marked_directory_entries(), vec![beta]);

        state.clear_directory_marks();
        assert!(state.marked_directory_entries().is_empty());

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }

    #[test]
    fn writable_directory_buffer_deleted_line_builds_delete_plan_without_touching_filesystem() {
        let root_path = unique_test_dir("writable-delete-plan");
        let alpha_path = root_path.join("alpha.md");
        let beta_path = root_path.join("beta.md");
        std::fs::create_dir_all(&root_path).expect("test directory");
        std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
        std::fs::write(&beta_path, "beta\n").expect("beta file");
        let state = EditorSessionState::new(Some(root_path.clone()));

        let plan = state
            .build_directory_buffer_operation_plan("beta.md\n")
            .expect("deleted listing line should produce a plan");

        assert_eq!(
            plan.operations,
            vec![DirectoryBufferPlannedOperation::Delete {
                path: alpha_path.clone(),
                name: "alpha.md".to_string(),
                kind: DirectoryBufferEntryKind::File,
            }]
        );
        assert!(
            alpha_path.exists(),
            "phase 7 only plans deletion and must not mutate the filesystem"
        );
        assert!(beta_path.exists());

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }

    #[test]
    fn writable_directory_buffer_empty_edited_text_deletes_every_entry_in_plan_only() {
        let root_path = unique_test_dir("writable-delete-all-plan");
        let alpha_path = root_path.join("alpha.md");
        std::fs::create_dir_all(&root_path).expect("test directory");
        std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
        let state = EditorSessionState::new(Some(root_path.clone()));

        let plan = state
            .build_directory_buffer_operation_plan("")
            .expect("empty edited listing should mean every entry was deleted");

        assert_eq!(
            plan.operations,
            vec![DirectoryBufferPlannedOperation::Delete {
                path: alpha_path.clone(),
                name: "alpha.md".to_string(),
                kind: DirectoryBufferEntryKind::File,
            }]
        );
        assert!(
            alpha_path.exists(),
            "empty edited listing must still only prepare a delete plan"
        );

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }

    #[test]
    fn writable_directory_buffer_rename_builds_rename_plan_and_reorder_is_noop() {
        let root_path = unique_test_dir("writable-rename-plan");
        let alpha_path = root_path.join("alpha.md");
        let beta_path = root_path.join("beta.md");
        let renamed_path = root_path.join("renamed.md");
        std::fs::create_dir_all(&root_path).expect("test directory");
        std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
        std::fs::write(&beta_path, "beta\n").expect("beta file");
        let state = EditorSessionState::new(Some(root_path.clone()));

        let rename_plan = state
            .build_directory_buffer_operation_plan("renamed.md\nbeta.md\n")
            .expect("changed existing line should produce a rename plan");

        assert_eq!(
            rename_plan.operations,
            vec![DirectoryBufferPlannedOperation::Rename {
                from: alpha_path.clone(),
                to: renamed_path,
                from_name: "alpha.md".to_string(),
                to_name: "renamed.md".to_string(),
                kind: DirectoryBufferEntryKind::File,
            }]
        );

        let reorder_plan = state
            .build_directory_buffer_operation_plan("beta.md\nalpha.md\n")
            .expect("pure reorder should still be a valid plan");

        assert!(
            reorder_plan.operations.is_empty(),
            "line reorder alone must not produce filesystem operations"
        );
        assert!(alpha_path.exists());
        assert!(beta_path.exists());

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }

    #[test]
    fn writable_directory_buffer_added_lines_build_file_and_directory_create_plan() {
        let root_path = unique_test_dir("writable-create-plan");
        let alpha_path = root_path.join("alpha.md");
        std::fs::create_dir_all(&root_path).expect("test directory");
        std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
        let state = EditorSessionState::new(Some(root_path.clone()));

        let plan = state
            .build_directory_buffer_operation_plan("alpha.md\nnotes.md\nsrc/\n")
            .expect("new listing lines should produce create operations");

        assert_eq!(
            plan.operations,
            vec![
                DirectoryBufferPlannedOperation::CreateFile {
                    path: root_path.join("notes.md"),
                    name: "notes.md".to_string(),
                },
                DirectoryBufferPlannedOperation::CreateDirectory {
                    path: root_path.join("src"),
                    name: "src".to_string(),
                },
            ]
        );
        assert!(
            !root_path.join("notes.md").exists() && !root_path.join("src").exists(),
            "phase 7 create operations must remain plans only"
        );

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }

    #[test]
    fn writable_directory_buffer_invalid_diff_returns_validation_errors_without_filesystem_changes()
    {
        let root_path = unique_test_dir("writable-validation");
        let alpha_path = root_path.join("alpha.md");
        std::fs::create_dir_all(&root_path).expect("test directory");
        std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
        let state = EditorSessionState::new(Some(root_path.clone()));

        let errors = state
            .build_directory_buffer_operation_plan(
                "alpha.md\n\n../escape.md\nalpha.md\nbad/name\nlink@\n",
            )
            .expect_err("invalid edited listing should fail validation");

        assert!(errors.iter().any(|error| matches!(
            error,
            DirectoryBufferPlanValidationError::EmptyLine { line_number: 2 }
        )));
        assert!(errors.iter().any(|error| matches!(
            error,
            DirectoryBufferPlanValidationError::ParentDirectoryEscape { line_number: 3, .. }
        )));
        assert!(errors.iter().any(|error| matches!(
            error,
            DirectoryBufferPlanValidationError::DuplicateName { name, .. } if name == "alpha.md"
        )));
        assert!(errors.iter().any(|error| matches!(
            error,
            DirectoryBufferPlanValidationError::PathSeparator { line_number: 5, .. }
        )));
        assert!(errors.iter().any(|error| matches!(
            error,
            DirectoryBufferPlanValidationError::UnsupportedDecoration { line_number: 6, .. }
        )));
        assert!(
            alpha_path.exists(),
            "validation must happen before any filesystem mutation"
        );

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }
}
