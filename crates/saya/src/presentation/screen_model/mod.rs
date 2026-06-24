//! 描画専用モデルと投影ロジック。
//!
//! CoreSnapshot と EditorSessionState から描画に必要な情報だけを
//! 抽出し、ScreenModel として TuiRenderer に渡す。
//! 描画側は ScreenModel だけを入力とし、CoreSnapshot に直接依存しない。

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use unicode_width::UnicodeWidthChar;
use vim_core_rs::{
    CoreBufferInfo, CoreBufferLineRange, CoreLightSnapshot, CoreMode, CoreSnapshot,
    CoreSyntaxChunk, CoreWindowInfo,
};

use crate::app::session::{DirectoryBufferEntryKind, EditorSessionState};
use crate::core::bridge::VisualSelection;
use crate::core::notification_prompt::{
    BellIndication, InputPromptView, MessageLineCandidate, MessageLineSource, PagerPromptView,
    SuppressedPromptHint, WorkspaceMessageLineState, WorkspaceNotificationPromptView,
    resolve_workspace_message_line,
};
use crate::features::search::query::{SearchMatchKind, SearchQueryMode, SearchVisibleState};
use crate::presentation::floating_window::FloatingScreenModel;
use crate::presentation::markdown::structure::{
    MarkdownBlockKind, MarkdownCheckboxState, MarkdownDocumentMap, MarkdownInlineKind,
};
use crate::presentation::theme::{
    FilerSemanticStyleKey, MarkdownSemanticStyleKey, ResolvedTextStyle, ResolvedTheme,
    normalize_language_id,
};
use crate::presentation::viewport::WindowViewportStore;
use crate::presentation::visual_line_layout::{RawByteCol, VisualLineLayout};

/// 描画専用 view model。
///
/// TuiRenderer はこの型だけを入力とし、CoreSnapshot や
/// EditorSessionState を直接参照しない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenModel {
    pub window_id: i32,
    pub buffer_id: i32,
    pub rect: PaneRect,
    /// 現在のファイル名（未設定なら "[新規]"）
    pub file_name: String,
    /// 現在のモードラベル（例: "NORMAL", "INSERT"）
    pub mode_label: String,
    /// Renderer-ready status line text for this pane.
    pub status_line: String,
    /// 表示用カーソル形状。
    pub cursor_style: ScreenCursorStyle,
    /// バッファが変更済みかどうか
    pub dirty: bool,
    /// 表示用の行データ
    pub lines: Vec<String>,
    /// raw buffer line と display line の対応表。
    pub line_projections: Vec<ScreenLineProjection>,
    /// カーソル行（0-indexed）
    pub cursor_row: u16,
    /// カーソル列（0-indexed）
    pub cursor_col: u16,
    /// Visual mode の選択範囲（表示セル座標）
    pub visual_selection: Option<ScreenSelection>,
    /// 検索ハイライトの表示用 overlay
    pub search_overlays: Vec<ScreenSearchOverlay>,
    /// 構文ハイライトの表示用 chunk（表示セル座標）
    pub syntax_chunks: Vec<ScreenSyntaxChunk>,
    /// Markdown semantic presentation style ranges.
    pub markdown_style_ranges: Vec<ScreenMarkdownStyleRange>,
    /// Filer/dired presentation style ranges projected from host metadata.
    pub filer_style_ranges: Vec<ScreenFilerStyleRange>,
    /// Renderer-ready startup theme for base UI and syntax styling.
    pub resolved_theme: ResolvedTheme,
    /// メッセージ欄に表示する通知（エラーやガイダンス）
    pub message_line: Option<String>,
    pub command_cursor_col: Option<u16>,
    pub is_active: bool,
}

pub type PaneScreenModel = ScreenModel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenCursorStyle {
    Block,
    SteadyBar,
    UnderScore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PaneRect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLineModel {
    pub text: String,
    pub cursor_col: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceScreenModel {
    pub panes: Vec<PaneScreenModel>,
    pub floats: Vec<FloatingScreenModel>,
    pub active_window_id: i32,
    pub message_line: WorkspaceMessageLineState,
    pub message_area_height: u16,
    pub message_scroll_offset: u16,
    pub prompt_line: Option<InputPromptView>,
    pub pager_prompt: Option<PagerPromptView>,
    pub suppressed_prompt_hints: Vec<SuppressedPromptHint>,
    pub bell: Option<BellIndication>,
    pub command_line: Option<CommandLineModel>,
}

impl WorkspaceScreenModel {
    pub fn active_cursor_style(&self) -> ScreenCursorStyle {
        if self.command_line.is_some() {
            log::debug!(
                "[screen_model] active cursor style resolved from command line overlay: style={:?}",
                ScreenCursorStyle::SteadyBar
            );
            return ScreenCursorStyle::SteadyBar;
        }

        let style = self
            .panes
            .iter()
            .find(|pane| pane.window_id == self.active_window_id)
            .map(|pane| pane.cursor_style)
            .unwrap_or(ScreenCursorStyle::Block);
        log::debug!(
            "[screen_model] active cursor style resolved from active pane: active_window_id={}, style={style:?}",
            self.active_window_id
        );
        style
    }

    pub fn projection_summary(&self) -> WorkspaceProjectionSummary {
        let window_ids = self
            .panes
            .iter()
            .map(|pane| pane.window_id)
            .collect::<Vec<_>>();
        let pane_geometry = self
            .panes
            .iter()
            .map(|pane| PaneProjectionGeometry {
                window_id: pane.window_id,
                rect: pane.rect,
            })
            .collect::<Vec<_>>();
        let visible_buffer_ids = self
            .panes
            .iter()
            .map(|pane| pane.buffer_id)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();

        log::debug!(
            "[screen_model] workspace projection summary built: windows={}, active_window_id={}, pane_geometry={}, visible_buffers={}",
            window_ids.len(),
            self.active_window_id,
            pane_geometry.len(),
            visible_buffer_ids.len(),
        );

        WorkspaceProjectionSummary {
            window_ids,
            active_window_id: self.active_window_id,
            pane_geometry,
            visible_buffer_ids,
        }
    }

    pub fn visible_message_text(&self) -> Option<&str> {
        self.message_line.visible_text()
    }

    pub fn visible_message_source(&self) -> Option<MessageLineSource> {
        self.message_line.visible_source()
    }

    pub fn suppressed_message_sources(&self) -> Vec<MessageLineSource> {
        self.message_line.suppressed_sources()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceProjectionSummary {
    pub window_ids: Vec<i32>,
    pub active_window_id: i32,
    pub pane_geometry: Vec<PaneProjectionGeometry>,
    pub visible_buffer_ids: Vec<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneProjectionGeometry {
    pub window_id: i32,
    pub rect: PaneRect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceProjectionError {
    ActiveWindowMissing,
    WindowNotFound { window_id: i32 },
}

impl fmt::Display for WorkspaceProjectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorkspaceProjectionError::ActiveWindowMissing => {
                write!(f, "active window could not be resolved")
            }
            WorkspaceProjectionError::WindowNotFound { window_id } => {
                write!(f, "window not found: window_id={window_id}")
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenSelection {
    pub start_row: u16,
    pub start_col: u16,
    pub line_start_col: u16,
    pub end_row: u16,
    pub end_col_exclusive: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenSearchOverlay {
    pub row: u16,
    pub start_col: u16,
    pub end_col_exclusive: u16,
    pub kind: SearchMatchKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenSyntaxChunk {
    pub row: u16,
    pub start_col: u16,
    pub end_col_exclusive: u16,
    pub syn_id: i32,
    pub name: Option<String>,
    pub language: Option<String>,
    pub tree_sitter: Option<ScreenTreeSitterSyntax>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenMarkdownStyleRange {
    pub row: u16,
    pub start_col: u16,
    pub end_col_exclusive: u16,
    pub style: ResolvedTextStyle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenFilerStyleRange {
    pub row: u16,
    pub start_col: u16,
    pub end_col_exclusive: u16,
    pub key: FilerSemanticStyleKey,
    pub style: ResolvedTextStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenSyntaxCategory {
    Attribute,
    Comment,
    Constant,
    Constructor,
    Function,
    Keyword,
    Label,
    Markup,
    Module,
    Number,
    Operator,
    Property,
    Punctuation,
    String,
    Tag,
    Text,
    Type,
    Variable,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenSyntaxModifier {
    Async,
    Declaration,
    Definition,
    Deprecated,
    Documentation,
    Mutable,
    Readonly,
    Static,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenTreeSitterSyntax {
    pub category: ScreenSyntaxCategory,
    pub modifiers: Vec<ScreenSyntaxModifier>,
    pub capture_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenLineProjection {
    pub absolute_row: usize,
    pub raw_text: String,
    pub display_text: String,
    pub spans: Vec<ScreenDisplaySpan>,
    pub cells: Vec<ScreenCellMapping>,
    pub line_start_col: u16,
}

impl ScreenLineProjection {
    pub fn logical_to_display_col(&self, raw_col: usize) -> u16 {
        let raw_col = clamp_to_char_boundary(&self.raw_text, raw_col.min(self.raw_text.len()));
        if raw_col == 0 {
            return self.line_start_col;
        }

        if let Some(span) = self.spans.iter().find(|span| {
            matches!(
                span.kind,
                ScreenDisplaySpanKind::ConcealedMarkdownMarker
                    | ScreenDisplaySpanKind::MarkdownReplacement { .. }
            ) && raw_col >= span.raw_start_col
                && raw_col < span.raw_end_col
        }) {
            log::debug!(
                "[screen_model] markdown logical->display concealed/replacement: row={}, raw_col={}, display_col={}",
                self.absolute_row,
                raw_col,
                span.display_start_col
            );
            return span.display_start_col;
        }

        if let Some(cell) = self.cells.iter().find(|cell| cell.raw_start_col == raw_col) {
            log::debug!(
                "[screen_model] markdown logical->display cell start: row={}, raw_col={}, display_col={}",
                self.absolute_row,
                raw_col,
                cell.display_col
            );
            return cell.display_col;
        }

        let display_col = self
            .cells
            .iter()
            .filter(|cell| cell.raw_end_col <= raw_col)
            .map(|cell| cell.display_end_col_exclusive)
            .last()
            .or_else(|| {
                self.spans
                    .iter()
                    .filter(|span| span.raw_end_col <= raw_col)
                    .map(|span| span.display_end_col_exclusive)
                    .last()
            })
            .unwrap_or(self.line_start_col);
        log::debug!(
            "[screen_model] markdown logical->display: row={}, raw_col={}, display_col={}",
            self.absolute_row,
            raw_col,
            display_col
        );
        display_col
    }

    pub fn display_to_logical_col(&self, display_col: u16) -> Option<usize> {
        if display_col < self.line_start_col {
            return None;
        }

        if let Some(cell) = self.cells.iter().find(|cell| {
            display_col >= cell.display_col && display_col < cell.display_end_col_exclusive
        }) {
            log::debug!(
                "[screen_model] markdown display->logical cell hit: row={}, display_col={}, raw_col={}",
                self.absolute_row,
                display_col,
                cell.raw_start_col
            );
            return Some(cell.raw_start_col);
        }

        let raw_col = self
            .spans
            .iter()
            .filter(|span| span.display_end_col_exclusive <= display_col)
            .map(|span| span.raw_end_col)
            .last()
            .unwrap_or(0);
        log::debug!(
            "[screen_model] markdown display->logical line end: row={}, display_col={}, raw_col={}",
            self.absolute_row,
            display_col,
            raw_col
        );
        Some(raw_col)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenDisplaySpan {
    pub raw_start_col: usize,
    pub raw_end_col: usize,
    pub display_start_col: u16,
    pub display_end_col_exclusive: u16,
    pub kind: ScreenDisplaySpanKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenDisplaySpanKind {
    RawText,
    ConcealedMarkdownMarker,
    MarkdownReplacement { text: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenCellMapping {
    pub display_col: u16,
    pub display_end_col_exclusive: u16,
    pub raw_start_col: usize,
    pub raw_end_col: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenMessageKind {
    CommandPreview,
    CoreMessage,
    SystemWarning,
    TransientInfo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenMessageState {
    pub kind: ScreenMessageKind,
    pub text: String,
}

impl ScreenMessageState {
    fn new(kind: ScreenMessageKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
        }
    }
}

/// 投影の入力をまとめた構造体。
///
/// CoreSnapshot と EditorSessionState から描画に必要な情報を選択して渡す。
pub struct ProjectionInput<'a> {
    pub snapshot: &'a CoreSnapshot,
    pub line_range: Option<&'a CoreBufferLineRange>,
    pub session_state: &'a EditorSessionState,
    pub visual_selection: Option<&'a VisualSelection>,
    pub search_state: Option<&'a SearchVisibleState>,
    pub syntax_lines: Option<&'a BTreeMap<usize, Vec<CoreSyntaxChunk>>>,
    #[cfg(feature = "tree-sitter-syntax")]
    pub tree_sitter_syntax: Option<&'a vim_core_rs::CoreTreeSitterRangeSyntax>,
    pub markdown_document_map: Option<&'a MarkdownDocumentMap>,
    pub command_preview: Option<&'a str>,
    pub core_message: Option<&'a str>,
    pub system_warning: Option<&'a str>,
    pub transient_info: Option<&'a str>,
    pub window_id: i32,
    pub buffer_id: i32,
    pub rect: PaneRect,
    pub is_active: bool,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub viewport_top: usize,
    pub body_height: usize,
    pub dirty_override: Option<bool>,
}

impl<'a> ProjectionInput<'a> {
    pub fn new(
        snapshot: &'a CoreSnapshot,
        session_state: &'a EditorSessionState,
        transient_message: Option<&'a str>,
    ) -> Self {
        let active_window = resolve_projection_active_window(snapshot);
        let active_buffer_id = active_window.map(|window| window.buf_id).unwrap_or(0);
        Self {
            snapshot,
            line_range: None,
            session_state,
            visual_selection: None,
            search_state: None,
            syntax_lines: None,
            #[cfg(feature = "tree-sitter-syntax")]
            tree_sitter_syntax: None,
            markdown_document_map: None,
            command_preview: None,
            core_message: None,
            system_warning: None,
            transient_info: transient_message,
            window_id: active_window.map(|window| window.id).unwrap_or(0),
            buffer_id: active_buffer_id,
            rect: active_window
                .map(PaneRect::from_core_window)
                .unwrap_or_default(),
            is_active: active_window.is_some(),
            cursor_row: active_window
                .map(|window| window.cursor_row)
                .unwrap_or(snapshot.cursor_row),
            cursor_col: active_window
                .map(|window| window.cursor_col)
                .unwrap_or(snapshot.cursor_col),
            viewport_top: 0,
            body_height: usize::MAX,
            dirty_override: None,
        }
    }

    pub fn with_viewport(mut self, viewport_top: usize, body_height: usize) -> Self {
        self.viewport_top = viewport_top;
        self.body_height = body_height.max(1);
        self
    }

    pub fn with_line_range(mut self, line_range: Option<&'a CoreBufferLineRange>) -> Self {
        self.line_range = line_range;
        self
    }

    pub fn with_visual_selection(mut self, visual_selection: Option<&'a VisualSelection>) -> Self {
        self.visual_selection = visual_selection;
        self
    }

    pub fn with_search_state(mut self, search_state: Option<&'a SearchVisibleState>) -> Self {
        self.search_state = search_state;
        self
    }

    pub fn with_syntax_lines(
        mut self,
        syntax_lines: Option<&'a BTreeMap<usize, Vec<CoreSyntaxChunk>>>,
    ) -> Self {
        self.syntax_lines = syntax_lines;
        self
    }

    #[cfg(feature = "tree-sitter-syntax")]
    pub fn with_tree_sitter_syntax(
        mut self,
        tree_sitter_syntax: Option<&'a vim_core_rs::CoreTreeSitterRangeSyntax>,
    ) -> Self {
        self.tree_sitter_syntax = tree_sitter_syntax;
        self
    }

    pub fn with_markdown_document_map(
        mut self,
        markdown_document_map: Option<&'a MarkdownDocumentMap>,
    ) -> Self {
        self.markdown_document_map = markdown_document_map;
        self
    }

    pub fn with_command_preview(mut self, command_preview: Option<&'a str>) -> Self {
        self.command_preview = command_preview;
        self
    }

    pub fn with_core_message(mut self, core_message: Option<&'a str>) -> Self {
        self.core_message = core_message;
        self
    }

    pub fn with_system_warning(mut self, system_warning: Option<&'a str>) -> Self {
        self.system_warning = system_warning;
        self
    }

    pub fn with_transient_info(mut self, transient_info: Option<&'a str>) -> Self {
        self.transient_info = transient_info;
        self
    }

    pub fn with_window(mut self, window: &CoreWindowInfo, rect: PaneRect, is_active: bool) -> Self {
        self.window_id = window.id;
        self.buffer_id = window.buf_id;
        self.rect = rect;
        self.is_active = is_active;
        self.cursor_row = window.cursor_row;
        self.cursor_col = window.cursor_col;
        self
    }

    pub fn with_dirty_override(mut self, dirty: Option<bool>) -> Self {
        self.dirty_override = dirty;
        self
    }
}

pub struct WorkspaceProjectionInput<'a> {
    pub snapshot: &'a CoreSnapshot,
    pub light_snapshot: Option<&'a CoreLightSnapshot>,
    pub line_ranges: &'a BTreeMap<i32, CoreBufferLineRange>,
    pub session_state: &'a EditorSessionState,
    pub visual_selection: Option<&'a VisualSelection>,
    pub search_states: &'a BTreeMap<i32, SearchVisibleState>,
    pub syntax_lines: &'a BTreeMap<i32, BTreeMap<usize, Vec<CoreSyntaxChunk>>>,
    #[cfg(feature = "tree-sitter-syntax")]
    pub tree_sitter_syntax: &'a BTreeMap<i32, vim_core_rs::CoreTreeSitterRangeSyntax>,
    pub markdown_document_maps: &'a BTreeMap<i32, Arc<MarkdownDocumentMap>>,
    pub command_preview: Option<&'a str>,
    pub core_message: Option<&'a str>,
    pub notification_prompt: Option<&'a WorkspaceNotificationPromptView>,
    pub system_warning: Option<&'a str>,
    pub transient_info: Option<&'a str>,
    pub viewport_store: &'a WindowViewportStore,
    pub terminal_width: u16,
    pub terminal_height: u16,
}

impl PaneRect {
    pub fn from_core_window(window: &CoreWindowInfo) -> Self {
        Self {
            x: u16::try_from(window.col).unwrap_or(u16::MAX),
            y: u16::try_from(window.row).unwrap_or(u16::MAX),
            width: u16::try_from(window.width).unwrap_or(u16::MAX),
            height: u16::try_from(window.height).unwrap_or(u16::MAX),
        }
    }
}

/// CoreSnapshot と EditorSessionState から ScreenModel を生成する。
///
/// 描画側はこの関数の戻り値だけを使い、CoreSnapshot に直接依存しない。
pub fn project(input: &ProjectionInput<'_>) -> ScreenModel {
    let started_at = Instant::now();
    log::debug!(
        "[screen_model] projecting: mode={:?}, dirty={}, cursor=({},{}), command_preview={:?}, core_message={:?}, system_warning={:?}, transient_info={:?}",
        input.snapshot.mode,
        input.snapshot.dirty,
        input.cursor_row,
        input.cursor_col,
        input.command_preview,
        input.core_message,
        input.system_warning,
        input.transient_info,
    );

    let file_name = resolve_file_name(input.snapshot, input.session_state);
    let mode_label = mode_to_label(input.snapshot.mode);
    let cursor_style = mode_to_cursor_style(input.snapshot.mode);
    let dirty = input.dirty_override.unwrap_or(input.snapshot.dirty);
    let status_line = input
        .session_state
        .render_status_line(&file_name, &mode_label, dirty);
    let markdown_display = project_markdown_display_lines(input);
    let lines = markdown_display.lines;
    let line_projections = markdown_display.line_projections;
    trace_projection_lines("visible", &lines, input.viewport_top);
    let cursor_row = resolve_projected_cursor_row(input, &line_projections);
    let cursor_col = resolve_input_cursor_col(input, input.cursor_row, input.cursor_col);
    let visual_selection = resolve_visual_selection(input);
    let search_overlays = project_search_overlays(input, &line_projections);
    let markdown_style_ranges = project_markdown_style_ranges(input, &line_projections);
    let filer_style_ranges = project_filer_style_ranges(input, &line_projections);
    let mut syntax_chunks = project_syntax_chunks(input, &line_projections);
    #[cfg(feature = "tree-sitter-syntax")]
    syntax_chunks.extend(project_tree_sitter_syntax_chunks(input, &line_projections));
    syntax_chunks.sort_by_key(|chunk| (chunk.row, chunk.start_col, chunk.end_col_exclusive));
    let message_state = resolve_message_state(input);
    let message_line = message_state.as_ref().map(|state| state.text.clone());

    log::debug!(
        "[screen_model] projected: file_name={:?}, mode_label={:?}, cursor_style={:?}, dirty={}, snapshot_dirty={}, dirty_override={:?}, lines_count={}, cursor=({},{}), search_overlays={}, markdown_style_ranges={}, syntax_chunks={}, message_state_kind={:?}, message_line={:?}",
        file_name,
        mode_label,
        cursor_style,
        dirty,
        input.snapshot.dirty,
        input.dirty_override,
        lines.len(),
        cursor_row,
        cursor_col,
        search_overlays.len(),
        markdown_style_ranges.len(),
        syntax_chunks.len(),
        message_state.as_ref().map(|state| state.kind),
        message_line,
    );
    log::debug!(
        "[PERF][screen_model] project text_len={} viewport_top={} body_height={} visible_lines={} line_projections={} elapsed_ms={}",
        input
            .line_range
            .map(|range| range.lines.iter().map(String::len).sum())
            .unwrap_or_else(|| input.snapshot.text.len()),
        input.viewport_top,
        input.body_height,
        lines.len(),
        line_projections.len(),
        started_at.elapsed().as_millis()
    );

    ScreenModel {
        window_id: input.window_id,
        buffer_id: input.buffer_id,
        rect: input.rect,
        file_name,
        mode_label,
        status_line,
        cursor_style,
        dirty,
        lines,
        line_projections,
        cursor_row,
        cursor_col,
        visual_selection,
        search_overlays,
        syntax_chunks,
        markdown_style_ranges,
        filer_style_ranges,
        resolved_theme: input.session_state.resolved_theme().clone(),
        message_line,
        command_cursor_col: None,
        is_active: input.is_active,
    }
}

pub fn project_workspace(
    input: &WorkspaceProjectionInput<'_>,
) -> Result<WorkspaceScreenModel, WorkspaceProjectionError> {
    let active_window_id = input
        .light_snapshot
        .and_then(CoreLightSnapshot::active_window_id)
        .or_else(|| input.snapshot.active_window_id());
    let Some(active_window_id) = active_window_id else {
        log::debug!("[screen_model] workspace projection aborted: active window missing");
        return Err(WorkspaceProjectionError::ActiveWindowMissing);
    };
    let command_line = input.command_preview.map(|preview| CommandLineModel {
        text: preview.to_string(),
        cursor_col: u16::try_from(display_width(
            preview,
            usize::from(input.session_state.tab_size().max(1)),
        ))
        .unwrap_or(u16::MAX),
    });
    let workspace_message_line = resolve_workspace_message_line_state(input);
    let reserved_rows = if command_line.is_some() {
        0
    } else {
        u16::from(workspace_message_line.visible_text().is_some())
    };
    let workspace_height = input.terminal_height.saturating_sub(reserved_rows).max(1);
    let pane_window_ids = input
        .snapshot
        .windows
        .iter()
        .map(|window| window.id)
        .collect::<Vec<_>>();

    let panes = pane_window_ids
        .into_iter()
        .map(|window_id| {
            let Some(window) = input.snapshot.window(window_id) else {
                log::debug!(
                    "[screen_model] workspace projection aborted: snapshot.window(window_id) returned None: window_id={}, active_window_id={:?}",
                    window_id,
                    active_window_id,
                );
                return Err(WorkspaceProjectionError::WindowNotFound { window_id });
            };
            let rect = map_window_rect(window, input.terminal_width, workspace_height);
            let body_height = usize::from(rect.height.saturating_sub(1).max(1));
            let viewport_top = input
                .viewport_store
                .get(window.id)
                .map(|viewport| viewport.top_line())
                .unwrap_or_else(|| window.topline.saturating_sub(1));
            let is_active = active_window_id == window_id;
            let pane_dirty = if is_active {
                Some(input.session_state.is_dirty())
            } else {
                input
                    .snapshot
                    .buffers
                    .iter()
                    .find(|buffer| buffer.id == window.buf_id)
                    .map(|buffer| buffer.dirty)
            };
            let pane_input = ProjectionInput::new(input.snapshot, input.session_state, None)
                .with_window(window, rect, is_active)
                .with_dirty_override(pane_dirty)
                .with_line_range(input.line_ranges.get(&window.id))
                .with_visual_selection(if is_active {
                    input.visual_selection
                } else {
                    None
                })
                .with_search_state(input.search_states.get(&window.id))
                .with_syntax_lines(input.syntax_lines.get(&window.id));
            #[cfg(feature = "tree-sitter-syntax")]
            let pane_input =
                pane_input.with_tree_sitter_syntax(input.tree_sitter_syntax.get(&window.id));
            let mut pane_input = pane_input
                .with_markdown_document_map(
                    input.markdown_document_maps.get(&window.id).map(Arc::as_ref),
                )
                .with_viewport(viewport_top, body_height);
            if is_active {
                log::debug!(
                    "[screen_model] active pane cursor projected from snapshot: window_id={}, snapshot_cursor=({},{}), window_cursor=({},{}), rect=({},{},{},{})",
                    window.id,
                    input.snapshot.cursor_row,
                    input.snapshot.cursor_col,
                    window.cursor_row,
                    window.cursor_col,
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height
                );
                pane_input.cursor_row = input.snapshot.cursor_row;
                pane_input.cursor_col = input.snapshot.cursor_col;
            } else {
                log::debug!(
                    "[screen_model] inactive pane cursor projected from window metadata: window_id={}, cursor=({},{}), rect=({},{},{},{})",
                    window.id,
                    window.cursor_row,
                    window.cursor_col,
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height
                );
                pane_input.cursor_row = window.cursor_row;
                pane_input.cursor_col = window.cursor_col;
            }
            Ok(project(&pane_input))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let model_message_line = if command_line.is_some()
        && workspace_message_line.visible_source() == Some(MessageLineSource::CommandPreview)
    {
        WorkspaceMessageLineState {
            visible: None,
            suppressed: workspace_message_line.suppressed.clone(),
        }
    } else {
        workspace_message_line
    };

    let core_pager_prompt = input
        .notification_prompt
        .and_then(|prompt| prompt.pager_prompt);
    let message_pager_prompt =
        input
            .session_state
            .message_pager_prompt_kind()
            .map(|kind| PagerPromptView {
                kind,
                one_shot: false,
            });
    let message_scroll_offset = if model_message_line.visible_text().is_some() {
        input.session_state.message_scroll_offset()
    } else {
        0
    };

    Ok(WorkspaceScreenModel {
        panes,
        floats: Vec::new(),
        active_window_id,
        message_line: model_message_line,
        message_area_height: input.session_state.message_area_height(),
        message_scroll_offset,
        prompt_line: input
            .notification_prompt
            .and_then(|prompt| prompt.input_prompt.clone()),
        pager_prompt: core_pager_prompt.or(message_pager_prompt),
        suppressed_prompt_hints: input
            .notification_prompt
            .map(|prompt| prompt.suppressed_prompt_hints.clone())
            .unwrap_or_default(),
        bell: input.notification_prompt.and_then(|prompt| prompt.bell),
        command_line,
    })
}

pub(crate) fn resolve_workspace_message_line_state(
    input: &WorkspaceProjectionInput<'_>,
) -> WorkspaceMessageLineState {
    let mut candidates = Vec::new();

    if let Some(preview) = input.command_preview {
        candidates.push(MessageLineCandidate::legacy(
            MessageLineSource::CommandPreview,
            preview,
        ));
    }
    let projected_core_message = input
        .notification_prompt
        .and_then(|prompt| prompt.notification_message.as_ref())
        .map(|message| message.text.as_str())
        .or(input.core_message);
    if let Some(message) = projected_core_message {
        push_message_candidate_unless_dismissed(
            &mut candidates,
            input.session_state,
            MessageLineSource::CoreNotification,
            message,
        );
    }
    if let Some(message) = input.system_warning {
        push_message_candidate_unless_dismissed(
            &mut candidates,
            input.session_state,
            MessageLineSource::SystemWarning,
            message,
        );
    }
    if let Some(message) = input.transient_info {
        push_message_candidate_unless_dismissed(
            &mut candidates,
            input.session_state,
            MessageLineSource::TransientInfo,
            message,
        );
    }
    if let Some(error) = input.session_state.last_save_error() {
        let message = format!("保存失敗: {error}");
        push_message_candidate_unless_dismissed(
            &mut candidates,
            input.session_state,
            MessageLineSource::TransientInfo,
            message,
        );
    }

    let state = resolve_workspace_message_line(candidates);
    log::debug!(
        "[screen_model] workspace message line resolved: visible_source={:?}, suppressed_sources={:?}",
        state.visible_source(),
        state.suppressed_sources()
    );
    state
}

fn push_message_candidate_unless_dismissed(
    candidates: &mut Vec<MessageLineCandidate>,
    session_state: &EditorSessionState,
    source: MessageLineSource,
    message: impl AsRef<str>,
) {
    let message = message.as_ref();
    if session_state.message_pager_hides_message(message) {
        log::debug!(
            "[screen_model] suppressed dismissed message pager candidate: source={:?}, message_lines={}",
            source,
            message.lines().count()
        );
        return;
    }
    candidates.push(MessageLineCandidate::legacy(source, message));
}

fn resolve_projection_active_window(snapshot: &CoreSnapshot) -> Option<&CoreWindowInfo> {
    let active_window_id = snapshot.active_window_id();
    let active_window = active_window_id.and_then(|window_id| snapshot.window(window_id));
    log::debug!(
        "[screen_model] resolve projection active window: snapshot_active_window_id={:?}, chosen_window_id={:?}",
        active_window_id,
        active_window.map(|window| window.id),
    );
    active_window
}

fn trace_projection_lines(phase: &str, lines: &[String], viewport_top: usize) {
    if std::env::var_os("SAYA_TRACE_RENDER").is_none() {
        return;
    }

    let visible_row = 0usize;
    let absolute_row = viewport_top.saturating_add(visible_row);
    let line = lines.get(visible_row).map(String::as_str).unwrap_or("");

    log::debug!(
        "[saya-trace][screen_model][{phase}] viewport_top={viewport_top} abs_row={} line={line:?}",
        absolute_row + 1
    );
}

fn resolve_visual_selection(input: &ProjectionInput<'_>) -> Option<ScreenSelection> {
    let selection = input.visual_selection?;
    let viewport_bottom = input
        .viewport_top
        .saturating_add(input.body_height.max(1))
        .saturating_sub(1);
    if selection.end_row < input.viewport_top || selection.start_row > viewport_bottom {
        return None;
    }

    let start_row = selection.start_row.max(input.viewport_top);
    let end_row = selection.end_row.min(viewport_bottom);
    let line_start_col = line_number_offset_for_input(input, input.session_state.line_numbers());
    let start_col = if selection.mode == CoreMode::VisualLine {
        line_start_col
    } else if start_row == selection.start_row {
        resolve_input_display_col_for_position(input, start_row, selection.start_col)
    } else {
        line_start_col
    };
    let end_col_exclusive = if selection.mode == CoreMode::VisualLine {
        input_visible_line_end_col_exclusive(
            input,
            end_row,
            input.session_state.line_numbers() || input.session_state.relative_number(),
        )
    } else if end_row == selection.end_row {
        resolve_input_display_col_after_inclusive_position(input, end_row, selection.end_col)
    } else {
        u16::MAX
    };

    Some(ScreenSelection {
        start_row: resolve_cursor_row(start_row, input.viewport_top, input.body_height),
        start_col,
        line_start_col,
        end_row: resolve_cursor_row(end_row, input.viewport_top, input.body_height),
        end_col_exclusive,
    })
}

/// アクティブバッファのファイル名を解決する。
///
/// session_state に target_path がある場合はそのファイル名部分を使い、
/// ない場合はデフォルトの "[新規]" を返す。
fn resolve_file_name(_snapshot: &CoreSnapshot, session_state: &EditorSessionState) -> String {
    // session_state の target_path を唯一のソースとする。
    // snapshot のバッファ名はセッション再利用時に汚染されうるため参照しない。
    if let Some(path) = session_state.target_path() {
        log::debug!(
            "[screen_model] file name from session target path: {}",
            path.display()
        );
        return path.display().to_string();
    }

    log::debug!("[screen_model] file name defaulting to [新規]");
    "[新規]".to_string()
}

/// CoreMode をユーザー向けのラベル文字列に変換する。
fn mode_to_label(mode: CoreMode) -> String {
    let label = match mode {
        CoreMode::Normal => "NORMAL",
        CoreMode::Insert => "INSERT",
        CoreMode::Visual => "VISUAL",
        CoreMode::VisualLine => "V-LINE",
        CoreMode::VisualBlock => "V-BLOCK",
        CoreMode::Replace => "REPLACE",
        CoreMode::Select => "SELECT",
        CoreMode::SelectLine => "S-LINE",
        CoreMode::SelectBlock => "S-BLOCK",
        CoreMode::CommandLine => "COMMAND",
        CoreMode::OperatorPending => "OP PENDING",
    };
    log::debug!("[screen_model] mode {:?} -> label {:?}", mode, label);
    label.to_string()
}

fn mode_to_cursor_style(mode: CoreMode) -> ScreenCursorStyle {
    let style = match mode {
        CoreMode::Normal
        | CoreMode::Visual
        | CoreMode::VisualLine
        | CoreMode::VisualBlock
        | CoreMode::Select
        | CoreMode::SelectLine
        | CoreMode::SelectBlock
        | CoreMode::OperatorPending => ScreenCursorStyle::Block,
        CoreMode::Insert | CoreMode::CommandLine => ScreenCursorStyle::SteadyBar,
        CoreMode::Replace => ScreenCursorStyle::UnderScore,
    };
    log::debug!("[screen_model] mode {:?} -> cursor style {:?}", mode, style);
    style
}

fn text_line_count(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    let newline_count = text.bytes().filter(|byte| *byte == b'\n').count();
    if text.ends_with('\n') {
        newline_count
    } else {
        newline_count.saturating_add(1)
    }
}

/// メッセージ欄の内容を解決する。
///
/// command preview が最優先で、その次に core message、
/// system warning、transient info を適用する。
fn resolve_message_state(input: &ProjectionInput<'_>) -> Option<ScreenMessageState> {
    if let Some(message) = resolve_message_text(input.command_preview) {
        log::debug!(
            "[screen_model] message line from command preview: {:?}",
            message
        );
        return Some(ScreenMessageState::new(
            ScreenMessageKind::CommandPreview,
            message,
        ));
    }

    if let Some(message) = resolve_message_text(input.core_message) {
        log::debug!(
            "[screen_model] message line from core message: {:?}",
            message
        );
        return Some(ScreenMessageState::new(
            ScreenMessageKind::CoreMessage,
            message,
        ));
    }

    if let Some(message) = resolve_message_text(input.system_warning) {
        log::debug!(
            "[screen_model] message line from system warning: {:?}",
            message
        );
        return Some(ScreenMessageState::new(
            ScreenMessageKind::SystemWarning,
            message,
        ));
    }

    if let Some(message) = resolve_message_text(input.transient_info) {
        log::debug!(
            "[screen_model] message line from transient info: {:?}",
            message
        );
        return Some(ScreenMessageState::new(
            ScreenMessageKind::TransientInfo,
            message,
        ));
    }

    if let Some(error) = input.session_state.last_save_error() {
        log::debug!(
            "[screen_model] message line from save error fallback: {:?}",
            error
        );
        return Some(ScreenMessageState::new(
            ScreenMessageKind::TransientInfo,
            format!("保存失敗: {}", error),
        ));
    }

    log::debug!("[screen_model] no message line");
    None
}

fn resolve_message_text(message: Option<&str>) -> Option<String> {
    let message = message?.trim();
    if message.is_empty() {
        None
    } else {
        Some(message.to_string())
    }
}

fn map_window_rect(
    window: &CoreWindowInfo,
    terminal_width: u16,
    workspace_height: u16,
) -> PaneRect {
    let x = u16::try_from(window.col)
        .unwrap_or(u16::MAX)
        .min(terminal_width);
    let y = u16::try_from(window.row)
        .unwrap_or(u16::MAX)
        .min(workspace_height);
    let width = u16::try_from(window.width)
        .unwrap_or(u16::MAX)
        .min(terminal_width.saturating_sub(x))
        .max(1);
    let height = u16::try_from(window.height)
        .unwrap_or(u16::MAX)
        .min(workspace_height.saturating_sub(y))
        .max(1);
    PaneRect {
        x,
        y,
        width,
        height,
    }
}

mod markdown_projection;
mod markdown_table;
mod search_projection;
mod style_ranges;
mod syntax_projection;
mod text_layout;

use markdown_projection::*;
use markdown_table::*;
use search_projection::*;
use style_ranges::*;
use syntax_projection::*;
use text_layout::*;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
