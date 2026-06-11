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
use crate::runtime::config::{StatusLineConfig, StatusLineSegment};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MermaidPreviewZoom {
    Fit,
    Percent(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MermaidPreviewViewState {
    pub focused: bool,
    pub zoom: MermaidPreviewZoom,
    pub pan_x_px: u32,
    pub pan_y_px: u32,
}

impl Default for MermaidPreviewViewState {
    fn default() -> Self {
        Self {
            focused: false,
            zoom: MermaidPreviewZoom::Fit,
            pan_x_px: 0,
            pan_y_px: 0,
        }
    }
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

fn apply_signed_delta_u32(value: u32, delta: i32) -> u32 {
    if delta.is_negative() {
        value.saturating_sub(delta.unsigned_abs())
    } else {
        value.saturating_add(delta as u32)
    }
}

fn normalize_mermaid_preview_background(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "transparent".to_string()
    } else {
        trimmed.to_string()
    }
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
    mermaid_preview_auto: bool,
    mermaid_preview_background: String,
    mermaid_preview_width_percent: u16,
    mermaid_preview_height_percent: u16,
    mermaid_preview_manual_active: bool,
    mermaid_preview_view: MermaidPreviewViewState,
    mermaid_preview_closed: bool,
    foldmethod: String,
    foldlevel: u16,
    resolved_theme: ResolvedTheme,
    filetype: Option<String>,
    status_line_config: StatusLineConfig,
    /// read-only 起動かどうか
    read_only: bool,
    /// 現在 dirty 状態かどうか
    dirty: bool,
    /// 最後に保存成功として扱った CoreBridge revision。
    last_clean_core_revision: Option<u64>,
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
    pending_directory_save_then_quit_force: Option<bool>,
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
            mermaid_preview_auto: true,
            mermaid_preview_background: "transparent".to_string(),
            mermaid_preview_width_percent: 55,
            mermaid_preview_height_percent: 55,
            mermaid_preview_manual_active: false,
            mermaid_preview_view: MermaidPreviewViewState::default(),
            mermaid_preview_closed: false,
            foldmethod: "manual".to_string(),
            foldlevel: 0,
            resolved_theme: ResolvedTheme::default(),
            filetype: None,
            status_line_config: StatusLineConfig::default(),
            read_only,
            dirty: false,
            last_clean_core_revision: None,
            last_save_error: None,
            directory_buffer: None,
            directory_marked_paths: BTreeSet::new(),
            pending_directory_operation_preview: None,
            directory_operation_confirmation_dialog_active: false,
            pending_directory_save_then_quit_force: None,
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
        self.update_dirty_at_revision(dirty, None);
    }

    /// dirty 状態を CoreBridge の revision と合わせて更新する。
    pub fn update_dirty_at_revision(&mut self, dirty: bool, core_revision: Option<u64>) {
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
        let dirty = if dirty
            && core_revision
                .zip(self.last_clean_core_revision)
                .is_some_and(|(revision, clean_revision)| revision <= clean_revision)
        {
            log::debug!(
                "[editor_session] ignoring stale core dirty at saved revision: revision={:?}, last_clean_core_revision={:?}",
                core_revision,
                self.last_clean_core_revision
            );
            false
        } else {
            dirty
        };
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
        self.record_save_success_at_revision(None);
    }

    /// 保存成功を CoreBridge の revision と合わせて記録する。
    pub fn record_save_success_at_revision(&mut self, core_revision: Option<u64>) {
        log::debug!("[editor_session] save success recorded: dirty -> false");
        self.dirty = false;
        self.last_clean_core_revision = core_revision;
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

    pub fn filetype(&self) -> Option<&str> {
        self.filetype.as_deref()
    }

    pub fn set_filetype(&mut self, filetype: Option<String>) {
        log::debug!(
            "[editor_session] filetype updated for session statusline: {:?}",
            filetype
        );
        self.filetype = filetype;
    }

    pub fn set_status_line_config(&mut self, config: StatusLineConfig) {
        log::debug!(
            "[editor_session] statusline config updated: left_segments={}, right_segments={}",
            config.left.len(),
            config.right.len()
        );
        self.status_line_config = config;
    }

    pub fn render_status_line(&self, file_name: &str, mode_label: &str, dirty: bool) -> String {
        let render_side = |segments: &[StatusLineSegment]| {
            segments
                .iter()
                .filter_map(|segment| match segment {
                    StatusLineSegment::FileName => Some(file_name.to_string()),
                    StatusLineSegment::Mode => Some(mode_label.to_string()),
                    StatusLineSegment::FileType => self
                        .filetype()
                        .filter(|filetype| !filetype.trim().is_empty())
                        .map(ToString::to_string),
                    StatusLineSegment::Modified => dirty.then(|| "[+]!".to_string()),
                })
                .filter(|component| !component.trim().is_empty())
                .collect::<Vec<_>>()
                .join(" | ")
        };
        let left = render_side(&self.status_line_config.left);
        let right = render_side(&self.status_line_config.right);
        match (left.is_empty(), right.is_empty()) {
            (true, true) => String::new(),
            (false, true) => left,
            (true, false) => right,
            (false, false) => format!("{left} || {right}"),
        }
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
            (SayaOptionName::MermaidPreview, SayaOptionValue::Boolean(value)) => {
                log::debug!(
                    "[editor_session][mermaid_preview] auto preview option updated: {} -> {}",
                    self.mermaid_preview_auto,
                    value
                );
                self.mermaid_preview_auto = value;
                Ok(())
            }
            (SayaOptionName::MermaidPreviewBackground, SayaOptionValue::String(value)) => {
                let next = normalize_mermaid_preview_background(&value);
                log::debug!(
                    "[editor_session][mermaid_preview] preview background updated: {:?} -> {:?}",
                    self.mermaid_preview_background,
                    next
                );
                self.mermaid_preview_background = next;
                Ok(())
            }
            (SayaOptionName::MermaidPreviewWidth, SayaOptionValue::Number(value)) => {
                let next = u16::try_from(value.clamp(1, 100)).unwrap_or(100);
                log::debug!(
                    "[editor_session][mermaid_preview] preview width percent updated: {} -> {}",
                    self.mermaid_preview_width_percent,
                    next
                );
                self.mermaid_preview_width_percent = next;
                Ok(())
            }
            (SayaOptionName::MermaidPreviewHeight, SayaOptionValue::Number(value)) => {
                let next = u16::try_from(value.clamp(1, 100)).unwrap_or(100);
                log::debug!(
                    "[editor_session][mermaid_preview] preview height percent updated: {} -> {}",
                    self.mermaid_preview_height_percent,
                    next
                );
                self.mermaid_preview_height_percent = next;
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

mod directory;
mod mermaid_preview;
mod message_pager;

pub(crate) use directory::read_directory_buffer_state_with_options;
use message_pager::MessagePagerState;

#[cfg(test)]
mod tests;
