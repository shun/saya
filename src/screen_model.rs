//! 描画専用モデルと投影ロジック。
//!
//! CoreSnapshot と EditorSessionState から描画に必要な情報だけを
//! 抽出し、ScreenModel として TuiRenderer に渡す。
//! 描画側は ScreenModel だけを入力とし、CoreSnapshot に直接依存しない。

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use unicode_width::UnicodeWidthChar;
use vim_core_rs::{CoreMode, CoreSnapshot, CoreSyntaxChunk, CoreWindowInfo};

use crate::core_bridge::VisualSelection;
use crate::core_notification_prompt::{
    BellIndication, InputPromptView, MessageLineCandidate, MessageLineSource, PagerPromptView,
    SuppressedPromptHint, WorkspaceMessageLineState, WorkspaceNotificationPromptView,
    resolve_workspace_message_line,
};
use crate::editor_session::EditorSessionState;
use crate::markdown_structure::{
    MarkdownBlockKind, MarkdownCheckboxState, MarkdownDocumentMap, MarkdownInlineKind,
};
use crate::search_query::{SearchMatchKind, SearchQueryMode, SearchVisibleState};
use crate::theme::{MarkdownSemanticStyleKey, ResolvedTextStyle};
use crate::viewport::WindowViewportStore;

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
    pub tree_sitter: Option<ScreenTreeSitterSyntax>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenMarkdownStyleRange {
    pub row: u16,
    pub start_col: u16,
    pub end_col_exclusive: u16,
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
        }
    }

    pub fn with_viewport(mut self, viewport_top: usize, body_height: usize) -> Self {
        self.viewport_top = viewport_top;
        self.body_height = body_height.max(1);
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
}

pub struct WorkspaceProjectionInput<'a> {
    pub snapshot: &'a CoreSnapshot,
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
    let dirty = input.snapshot.dirty;
    let rendered_lines = apply_list_projection(
        split_text_to_lines(&input.snapshot.text, input.session_state.tab_size()),
        input.session_state.list(),
        input.session_state.listchars(),
    );
    let full_lines = apply_line_number_prefix(
        rendered_lines,
        input.session_state.line_numbers() || input.session_state.relative_number(),
        input.session_state.relative_number(),
        input.session_state.number_width(),
        input.cursor_row,
    );
    trace_projection_lines("full", &full_lines, 0);
    let lines = slice_visible_lines(&full_lines, input.viewport_top, input.body_height);
    let line_projections = project_markdown_line_projections(input);
    trace_projection_lines("visible", &lines, input.viewport_top);
    let cursor_row = resolve_cursor_row(input.cursor_row, input.viewport_top, input.body_height);
    let cursor_col = resolve_cursor_col(
        &input.snapshot.text,
        input.cursor_row,
        input.cursor_col,
        input.session_state.tab_size(),
        input.session_state.line_numbers() || input.session_state.relative_number(),
        input.session_state.number_width(),
    );
    let visual_selection = resolve_visual_selection(input);
    let search_overlays = project_search_overlays(input, &line_projections);
    let markdown_style_ranges = project_markdown_style_ranges(input, &line_projections);
    let mut syntax_chunks = project_syntax_chunks(input, &line_projections);
    #[cfg(feature = "tree-sitter-syntax")]
    syntax_chunks.extend(project_tree_sitter_syntax_chunks(input, &line_projections));
    syntax_chunks.sort_by_key(|chunk| (chunk.row, chunk.start_col, chunk.end_col_exclusive));
    let message_state = resolve_message_state(input);
    let message_line = message_state.as_ref().map(|state| state.text.clone());

    log::debug!(
        "[screen_model] projected: file_name={:?}, mode_label={:?}, cursor_style={:?}, dirty={}, lines_count={}, cursor=({},{}), search_overlays={}, markdown_style_ranges={}, syntax_chunks={}, message_state_kind={:?}, message_line={:?}",
        file_name,
        mode_label,
        cursor_style,
        dirty,
        lines.len(),
        cursor_row,
        cursor_col,
        search_overlays.len(),
        markdown_style_ranges.len(),
        syntax_chunks.len(),
        message_state.as_ref().map(|state| state.kind),
        message_line,
    );

    ScreenModel {
        window_id: input.window_id,
        buffer_id: input.buffer_id,
        rect: input.rect,
        file_name,
        mode_label,
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
        message_line,
        command_cursor_col: None,
        is_active: input.is_active,
    }
}

pub fn project_workspace(
    input: &WorkspaceProjectionInput<'_>,
) -> Result<WorkspaceScreenModel, WorkspaceProjectionError> {
    let active_window_id = input.snapshot.active_window_id();
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
            let pane_input = ProjectionInput::new(input.snapshot, input.session_state, None)
                .with_window(window, rect, is_active)
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

    let absolute_row = 6usize;
    let line = absolute_row
        .checked_sub(viewport_top)
        .and_then(|row| lines.get(row))
        .map(String::as_str)
        .unwrap_or("");

    eprintln!(
        "[saya-trace][screen_model][{phase}] viewport_top={viewport_top} abs_row=7 line={line:?}"
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
    let line_start_col = line_number_offset(
        &input.snapshot.text,
        input.session_state.line_numbers(),
        input.session_state.number_width(),
    );
    let start_col = if selection.mode == CoreMode::VisualLine {
        line_start_col
    } else if start_row == selection.start_row {
        resolve_display_col_for_position(
            &input.snapshot.text,
            start_row,
            selection.start_col,
            input.session_state.tab_size(),
            input.session_state.line_numbers() || input.session_state.relative_number(),
            input.session_state.number_width(),
        )
    } else {
        line_start_col
    };
    let end_col_exclusive = if selection.mode == CoreMode::VisualLine {
        visible_line_end_col_exclusive(
            &input.snapshot.text,
            end_row,
            input.session_state.tab_size(),
            input.session_state.line_numbers() || input.session_state.relative_number(),
            input.session_state.number_width(),
        )
    } else if end_row == selection.end_row {
        resolve_display_col_after_inclusive_position(
            &input.snapshot.text,
            end_row,
            selection.end_col,
            input.session_state.tab_size(),
            input.session_state.line_numbers() || input.session_state.relative_number(),
            input.session_state.number_width(),
        )
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

fn slice_visible_lines(lines: &[String], viewport_top: usize, body_height: usize) -> Vec<String> {
    let body_height = body_height.max(1);
    let start = viewport_top.min(lines.len());
    let end = start.saturating_add(body_height).min(lines.len());
    let visible = lines[start..end].to_vec();

    log::debug!(
        "[screen_model] sliced visible lines: viewport_top={}, body_height={}, total_lines={}, visible_lines={}",
        viewport_top,
        body_height,
        lines.len(),
        visible.len()
    );

    visible
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

/// テキストを行に分割する。
fn split_text_to_lines(text: &str, tab_size: u16) -> Vec<String> {
    let lines: Vec<String> = text
        .lines()
        .map(|line| expand_tabs(line, usize::from(tab_size.max(1))))
        .collect();
    log::debug!("[screen_model] split text to {} lines", lines.len());
    lines
}

fn apply_line_number_prefix(
    lines: Vec<String>,
    enabled: bool,
    relative_number: bool,
    configured_width: u16,
    cursor_row: usize,
) -> Vec<String> {
    if !enabled && !relative_number {
        return lines;
    }

    let width = line_number_width(lines.len(), configured_width);
    let numbered_lines = lines
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            let number = if relative_number && index != cursor_row {
                index.abs_diff(cursor_row)
            } else {
                index + 1
            };
            format!("{:>width$} {}", number, line, width = width)
        })
        .collect();
    log::debug!("[screen_model] applied line number prefix");
    numbered_lines
}

fn apply_list_projection(lines: Vec<String>, enabled: bool, listchars: &str) -> Vec<String> {
    if !enabled {
        return lines;
    }
    let trail = parse_listchars_trail(listchars).unwrap_or('-');
    lines
        .into_iter()
        .map(|line| render_list_line(&line, trail))
        .collect()
}

fn render_list_line(line: &str, trail: char) -> String {
    let trimmed_len = line.trim_end_matches(' ').len();
    let mut rendered = String::with_capacity(line.len());
    rendered.push_str(&line[..trimmed_len]);
    rendered.extend(std::iter::repeat_n(
        trail,
        line.len().saturating_sub(trimmed_len),
    ));
    rendered
}

fn parse_listchars_trail(listchars: &str) -> Option<char> {
    listchars.split(',').find_map(|part| {
        part.strip_prefix("trail:")
            .and_then(|value| value.chars().next())
    })
}

/// vim-core-rs のバイト列ベースカーソル位置を terminal の表示セル列へ変換する。
fn resolve_cursor_col(
    text: &str,
    cursor_row: usize,
    cursor_col: usize,
    tab_size: u16,
    line_numbers: bool,
    number_width: u16,
) -> u16 {
    let line = text.split('\n').nth(cursor_row).unwrap_or("");
    let clamped_col = cursor_col.min(line.len());
    let boundary_col = clamp_to_char_boundary(line, clamped_col);
    let base_display_col = display_width(&line[..boundary_col], usize::from(tab_size.max(1)));
    let line_number_offset = usize::from(line_number_offset(text, line_numbers, number_width));
    let display_col = base_display_col.saturating_add(line_number_offset);
    let display_col = u16::try_from(display_col).unwrap_or(u16::MAX);

    log::debug!(
        "[screen_model] resolved cursor col: row={}, raw_col={}, boundary_col={}, base_display_col={}, line_number_offset={}, display_col={}",
        cursor_row,
        cursor_col,
        boundary_col,
        base_display_col,
        line_number_offset,
        display_col
    );

    display_col
}

fn resolve_display_col_for_position(
    text: &str,
    cursor_row: usize,
    cursor_col: usize,
    tab_size: u16,
    line_numbers: bool,
    number_width: u16,
) -> u16 {
    resolve_cursor_col(
        text,
        cursor_row,
        cursor_col,
        tab_size,
        line_numbers,
        number_width,
    )
}

fn resolve_display_col_after_inclusive_position(
    text: &str,
    cursor_row: usize,
    cursor_col: usize,
    tab_size: u16,
    line_numbers: bool,
    number_width: u16,
) -> u16 {
    let line = text.split('\n').nth(cursor_row).unwrap_or("");
    if line.is_empty() {
        return resolve_display_col_for_position(
            text,
            cursor_row,
            cursor_col,
            tab_size,
            line_numbers,
            number_width,
        );
    }
    let clamped_col = clamp_to_char_boundary(line, cursor_col.min(line.len()));
    let next_col = line[clamped_col..]
        .chars()
        .next()
        .map(|ch| clamped_col + ch.len_utf8())
        .unwrap_or(clamped_col);
    resolve_display_col_for_position(
        text,
        cursor_row,
        next_col,
        tab_size,
        line_numbers,
        number_width,
    )
}

fn line_number_offset(text: &str, line_numbers: bool, number_width: u16) -> u16 {
    if line_numbers {
        u16::try_from(line_number_width(text.lines().count(), number_width) + 1).unwrap_or(u16::MAX)
    } else {
        0
    }
}

fn visible_line_end_col_exclusive(
    text: &str,
    row: usize,
    tab_size: u16,
    line_numbers: bool,
    number_width: u16,
) -> u16 {
    let line = text.split('\n').nth(row).unwrap_or("");
    resolve_display_col_for_position(text, row, line.len(), tab_size, line_numbers, number_width)
}

fn line_number_width(line_count: usize, configured_width: u16) -> usize {
    line_count
        .max(1)
        .to_string()
        .len()
        .max(usize::from(configured_width.max(1)))
}

fn resolve_cursor_row(cursor_row: usize, viewport_top: usize, body_height: usize) -> u16 {
    let body_height = body_height.max(1);
    let relative_row = cursor_row.saturating_sub(viewport_top).min(body_height - 1);
    let relative_row = u16::try_from(relative_row).unwrap_or(u16::MAX);

    log::debug!(
        "[screen_model] resolved cursor row: absolute_row={}, viewport_top={}, body_height={}, relative_row={}",
        cursor_row,
        viewport_top,
        body_height,
        relative_row
    );

    relative_row
}

fn clamp_to_char_boundary(text: &str, col: usize) -> usize {
    let mut boundary = col.min(text.len());
    while boundary > 0 && !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    boundary
}

fn expand_tabs(line: &str, tab_size: usize) -> String {
    let mut expanded = String::new();
    let mut display_col = 0usize;

    for ch in line.chars() {
        if ch == '\t' {
            let spaces = next_tab_stop(display_col, tab_size) - display_col;
            expanded.push_str(&" ".repeat(spaces));
            display_col += spaces;
            continue;
        }

        expanded.push(ch);
        display_col += char_display_width(ch);
    }

    expanded
}

fn display_width(text: &str, tab_size: usize) -> usize {
    let mut display_col = 0usize;

    for ch in text.chars() {
        if ch == '\t' {
            display_col = next_tab_stop(display_col, tab_size);
        } else {
            display_col += char_display_width(ch);
        }
    }

    display_col
}

fn next_tab_stop(display_col: usize, tab_size: usize) -> usize {
    let tab_size = tab_size.max(1);
    display_col + (tab_size - (display_col % tab_size)).min(tab_size)
}

fn char_display_width(ch: char) -> usize {
    UnicodeWidthChar::width(ch).unwrap_or(0)
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

fn project_search_overlays(
    input: &ProjectionInput<'_>,
    line_projections: &[ScreenLineProjection],
) -> Vec<ScreenSearchOverlay> {
    let Some(search_state) = input.search_state else {
        log::debug!("[screen_model] no search state provided");
        return Vec::new();
    };

    if search_state.window_id != input.window_id {
        log::debug!(
            "[screen_model] search state window mismatch: input_window_id={}, search_window_id={}",
            input.window_id,
            search_state.window_id
        );
        return Vec::new();
    }

    if search_state.matches.is_empty() {
        log::debug!("[screen_model] search state has no matches");
        return Vec::new();
    }

    if matches!(search_state.mode, SearchQueryMode::Hlsearch)
        && (!search_state.hlsearch_enabled || search_state.hlsearch_suspended)
    {
        log::debug!(
            "[screen_model] hlsearch overlay suppressed: enabled={}, suspended={}",
            search_state.hlsearch_enabled,
            search_state.hlsearch_suspended
        );
        return Vec::new();
    }

    let viewport_bottom = input
        .viewport_top
        .saturating_add(input.body_height.max(1))
        .saturating_sub(1);
    let visible_start_row = input.viewport_top.saturating_add(1);
    let visible_end_row = viewport_bottom.saturating_add(1);
    let start_row = search_state.visible_rows.start_row.max(visible_start_row);
    let end_row = search_state.visible_rows.end_row.min(visible_end_row);
    if start_row > end_row {
        log::debug!(
            "[screen_model] search overlays outside visible rows: visible=({}, {}), state=({}, {})",
            visible_start_row,
            visible_end_row,
            search_state.visible_rows.start_row,
            search_state.visible_rows.end_row
        );
        return Vec::new();
    }

    let mut overlays = Vec::new();
    for search_match in &search_state.matches {
        overlays.extend(project_search_match_overlays(
            input,
            line_projections,
            search_match,
            start_row,
            end_row,
        ));
    }

    overlays.sort_by_key(|overlay| {
        let kind_rank = match overlay.kind {
            SearchMatchKind::Current => 0usize,
            SearchMatchKind::Incremental => 1usize,
            SearchMatchKind::Regular => 2usize,
        };
        (
            overlay.row,
            overlay.start_col,
            kind_rank,
            overlay.end_col_exclusive,
        )
    });

    log::debug!(
        "[screen_model] projected search overlays: count={}, rows={:?}",
        overlays.len(),
        overlays
            .iter()
            .map(|overlay| overlay.row)
            .collect::<Vec<_>>()
    );

    overlays
}

fn project_search_match_overlays(
    input: &ProjectionInput<'_>,
    line_projections: &[ScreenLineProjection],
    search_match: &crate::search_query::SearchMatch,
    visible_start_row: usize,
    visible_end_row: usize,
) -> Vec<ScreenSearchOverlay> {
    let match_start_row = search_match.start_row.max(visible_start_row);
    let match_end_row = search_match.end_row.min(visible_end_row);
    if match_start_row > match_end_row {
        return Vec::new();
    }

    let mut overlays = Vec::new();
    for row in match_start_row..=match_end_row {
        let Some((start_col, end_col_exclusive)) =
            resolve_search_overlay_display_bounds(input, line_projections, search_match, row)
        else {
            continue;
        };
        let relative_row = row.saturating_sub(1).saturating_sub(input.viewport_top);
        overlays.push(ScreenSearchOverlay {
            row: u16::try_from(relative_row).unwrap_or(u16::MAX),
            start_col,
            end_col_exclusive,
            kind: search_match.kind,
        });
    }

    overlays
}

fn resolve_search_overlay_display_bounds(
    input: &ProjectionInput<'_>,
    line_projections: &[ScreenLineProjection],
    search_match: &crate::search_query::SearchMatch,
    row: usize,
) -> Option<(u16, u16)> {
    let absolute_row = row.saturating_sub(1);
    let projection = input.markdown_document_map.and_then(|_| {
        line_projections
            .iter()
            .find(|projection| projection.absolute_row == absolute_row)
    });
    let start_col = if row == search_match.start_row {
        projection.map_or_else(
            || {
                resolve_display_col_for_position(
                    &input.snapshot.text,
                    search_match.start_row - 1,
                    search_match.start_col,
                    input.session_state.tab_size(),
                    input.session_state.line_numbers() || input.session_state.relative_number(),
                    input.session_state.number_width(),
                )
            },
            |projection| projection.logical_to_display_col(search_match.start_col),
        )
    } else {
        projection.map_or_else(
            || {
                line_number_offset(
                    &input.snapshot.text,
                    input.session_state.line_numbers() || input.session_state.relative_number(),
                    input.session_state.number_width(),
                )
            },
            |projection| projection.line_start_col,
        )
    };
    let end_col_exclusive = if row == search_match.end_row {
        projection.map_or_else(
            || {
                resolve_display_col_for_position(
                    &input.snapshot.text,
                    search_match.end_row - 1,
                    search_match.end_col,
                    input.session_state.tab_size(),
                    input.session_state.line_numbers() || input.session_state.relative_number(),
                    input.session_state.number_width(),
                )
            },
            |projection| projection.logical_to_display_col(search_match.end_col),
        )
    } else {
        projection.map_or_else(
            || {
                visible_line_end_col_exclusive(
                    &input.snapshot.text,
                    row - 1,
                    input.session_state.tab_size(),
                    input.session_state.line_numbers() || input.session_state.relative_number(),
                    input.session_state.number_width(),
                )
            },
            |projection| projection.logical_to_display_col(projection.raw_text.len()),
        )
    };

    if end_col_exclusive <= start_col {
        log::debug!(
            "[screen_model] ignoring search overlay with non-positive width: window_id={}, row={}, start_col={}, end_col_exclusive={}",
            input.window_id,
            row,
            start_col,
            end_col_exclusive
        );
        return None;
    }

    Some((start_col, end_col_exclusive))
}

fn project_markdown_style_ranges(
    input: &ProjectionInput<'_>,
    line_projections: &[ScreenLineProjection],
) -> Vec<ScreenMarkdownStyleRange> {
    let Some(markdown_document_map) = input.markdown_document_map else {
        return Vec::new();
    };
    if !input.session_state.markdown_render() {
        return Vec::new();
    }

    let theme = input.session_state.resolved_theme();
    let viewport_bottom = input
        .viewport_top
        .saturating_add(input.body_height.max(1))
        .saturating_sub(1);
    let mut ranges = Vec::new();

    for block in &markdown_document_map.blocks {
        if block.range.end.line < input.viewport_top || block.range.start.line > viewport_bottom {
            continue;
        }
        match block.kind {
            MarkdownBlockKind::Heading { level } => {
                let Some(style) = theme.heading_style(level) else {
                    continue;
                };
                if let Some((row, start_col, end_col_exclusive)) =
                    project_markdown_range_display_bounds(
                        line_projections,
                        block.range.start.line,
                        block.range.start.column,
                        block.range.end.column,
                        input.viewport_top,
                    )
                {
                    ranges.push(ScreenMarkdownStyleRange {
                        row,
                        start_col,
                        end_col_exclusive,
                        style,
                    });
                }
            }
            MarkdownBlockKind::FencedCodeBlock { .. } => {
                append_block_style_ranges(
                    &mut ranges,
                    line_projections,
                    block.range,
                    input.viewport_top,
                    theme.markdown_style(MarkdownSemanticStyleKey::FencedCodeBlock),
                );
            }
            MarkdownBlockKind::Table => {
                append_block_style_ranges(
                    &mut ranges,
                    line_projections,
                    block.range,
                    input.viewport_top,
                    theme.markdown_style(MarkdownSemanticStyleKey::Table),
                );
            }
            MarkdownBlockKind::ListItem { .. } => {}
        }
    }

    for inline in &markdown_document_map.inlines {
        if inline.range.start.line < input.viewport_top || inline.range.start.line > viewport_bottom
        {
            continue;
        }
        let style = match &inline.kind {
            MarkdownInlineKind::InlineCode => {
                theme.markdown_style(MarkdownSemanticStyleKey::InlineCode)
            }
            MarkdownInlineKind::Link { .. } => theme.markdown_style(MarkdownSemanticStyleKey::Link),
            MarkdownInlineKind::EmphasisMarker { .. } => None,
        };
        let Some(style) = style.cloned().filter(|style| !style.is_empty()) else {
            continue;
        };
        if let Some((row, start_col, end_col_exclusive)) = project_markdown_range_display_bounds(
            line_projections,
            inline.range.start.line,
            inline.range.start.column,
            inline.range.end.column,
            input.viewport_top,
        ) {
            ranges.push(ScreenMarkdownStyleRange {
                row,
                start_col,
                end_col_exclusive,
                style,
            });
        }
    }

    ranges.sort_by_key(|range| (range.row, range.start_col, range.end_col_exclusive));
    log::debug!(
        "[screen_model] markdown semantic style ranges projected: window_id={}, ranges={}",
        input.window_id,
        ranges.len()
    );
    ranges
}

fn append_block_style_ranges(
    ranges: &mut Vec<ScreenMarkdownStyleRange>,
    line_projections: &[ScreenLineProjection],
    range: crate::markdown_structure::MarkdownTextRange,
    viewport_top: usize,
    style: Option<&ResolvedTextStyle>,
) {
    let Some(style) = style.cloned().filter(|style| !style.is_empty()) else {
        return;
    };
    for absolute_row in range.start.line..=range.end.line {
        let Some(projection) = line_projections
            .iter()
            .find(|projection| projection.absolute_row == absolute_row)
        else {
            continue;
        };
        let start = if absolute_row == range.start.line {
            range.start.column
        } else {
            0
        };
        let end = if absolute_row == range.end.line {
            range.end.column
        } else {
            projection.raw_text.len()
        };
        if let Some((row, start_col, end_col_exclusive)) = project_markdown_range_display_bounds(
            line_projections,
            absolute_row,
            start,
            end,
            viewport_top,
        ) {
            ranges.push(ScreenMarkdownStyleRange {
                row,
                start_col,
                end_col_exclusive,
                style: style.clone(),
            });
        }
    }
}

fn project_markdown_range_display_bounds(
    line_projections: &[ScreenLineProjection],
    absolute_row: usize,
    raw_start_col: usize,
    raw_end_col: usize,
    viewport_top: usize,
) -> Option<(u16, u16, u16)> {
    let projection = line_projections
        .iter()
        .find(|projection| projection.absolute_row == absolute_row)?;
    let row = u16::try_from(absolute_row.saturating_sub(viewport_top)).unwrap_or(u16::MAX);
    let start_col = projection.logical_to_display_col(raw_start_col);
    let end_col_exclusive = projection.logical_to_display_col(raw_end_col);
    (end_col_exclusive > start_col).then_some((row, start_col, end_col_exclusive))
}

fn project_syntax_chunks(
    input: &ProjectionInput<'_>,
    line_projections: &[ScreenLineProjection],
) -> Vec<ScreenSyntaxChunk> {
    let Some(syntax_lines) = input.syntax_lines else {
        log::debug!("[screen_model] no syntax lines provided");
        return Vec::new();
    };
    if syntax_lines.is_empty() {
        log::debug!("[screen_model] syntax lines are empty");
        return Vec::new();
    }

    let viewport_bottom = input
        .viewport_top
        .saturating_add(input.body_height.max(1))
        .saturating_sub(1);
    let line_numbers = input.session_state.line_numbers() || input.session_state.relative_number();
    let mut projected = Vec::new();

    for (absolute_row, chunks) in syntax_lines {
        if *absolute_row < input.viewport_top || *absolute_row > viewport_bottom {
            continue;
        }
        let row =
            u16::try_from(absolute_row.saturating_sub(input.viewport_top)).unwrap_or(u16::MAX);
        for chunk in chunks {
            if chunk.syn_id == 0 || chunk.end_col <= chunk.start_col {
                continue;
            }
            let markdown_projection = input.markdown_document_map.and_then(|_| {
                line_projections
                    .iter()
                    .find(|projection| projection.absolute_row == *absolute_row)
            });
            let start_col = markdown_projection.map_or_else(
                || {
                    resolve_display_col_for_position(
                        &input.snapshot.text,
                        *absolute_row,
                        chunk.start_col,
                        input.session_state.tab_size(),
                        line_numbers,
                        input.session_state.number_width(),
                    )
                },
                |projection| projection.logical_to_display_col(chunk.start_col),
            );
            let end_col_exclusive = markdown_projection.map_or_else(
                || {
                    resolve_display_col_for_position(
                        &input.snapshot.text,
                        *absolute_row,
                        chunk.end_col,
                        input.session_state.tab_size(),
                        line_numbers,
                        input.session_state.number_width(),
                    )
                },
                |projection| projection.logical_to_display_col(chunk.end_col),
            );
            if end_col_exclusive <= start_col {
                log::debug!(
                    "[screen_model] ignoring syntax chunk with non-positive display width: window_id={}, row={}, syn_id={}, raw=({},{}), display=({},{})",
                    input.window_id,
                    absolute_row,
                    chunk.syn_id,
                    chunk.start_col,
                    chunk.end_col,
                    start_col,
                    end_col_exclusive
                );
                continue;
            }
            projected.push(ScreenSyntaxChunk {
                row,
                start_col,
                end_col_exclusive,
                syn_id: chunk.syn_id,
                name: chunk.name.clone(),
                tree_sitter: None,
            });
        }
    }

    projected.sort_by_key(|chunk| (chunk.row, chunk.start_col, chunk.end_col_exclusive));
    log::debug!(
        "[screen_model] projected syntax chunks: window_id={}, chunks={}, rows={:?}",
        input.window_id,
        projected.len(),
        projected.iter().map(|chunk| chunk.row).collect::<Vec<_>>()
    );
    projected
}

#[cfg(feature = "tree-sitter-syntax")]
fn project_tree_sitter_syntax_chunks(
    input: &ProjectionInput<'_>,
    line_projections: &[ScreenLineProjection],
) -> Vec<ScreenSyntaxChunk> {
    use vim_core_rs::{CoreTextPosition, CoreTreeSitterStatus};

    let Some(syntax) = input.tree_sitter_syntax else {
        log::debug!("[screen_model] no Tree-sitter syntax provided");
        return Vec::new();
    };
    if syntax.buffer_id != input.buffer_id {
        log::debug!(
            "[screen_model] ignoring Tree-sitter syntax for different buffer: window_id={}, model_buffer_id={}, syntax_buffer_id={}",
            input.window_id,
            input.buffer_id,
            syntax.buffer_id
        );
        return Vec::new();
    }
    let Some(buffer) = input
        .snapshot
        .buffers
        .iter()
        .find(|buffer| buffer.id == input.buffer_id)
    else {
        log::debug!(
            "[screen_model] ignoring Tree-sitter syntax because buffer is missing: window_id={}, buffer_id={}",
            input.window_id,
            input.buffer_id
        );
        return Vec::new();
    };
    let raw_lines = input.snapshot.text.split('\n').collect::<Vec<_>>();
    let coverage_line_count = input.snapshot.text.lines().count().max(1);
    let viewport_bottom = input
        .viewport_top
        .saturating_add(input.body_height.max(1))
        .saturating_sub(1)
        .min(coverage_line_count.saturating_sub(1));
    let visible_range = vim_core_rs::CoreTextRange {
        start: vim_core_rs::CoreTextPosition {
            row: input.viewport_top,
            col: 0,
        },
        end: vim_core_rs::CoreTextPosition {
            row: viewport_bottom.saturating_add(1),
            col: 0,
        },
    };
    if syntax.source_revision != buffer.source_revision
        || !matches!(syntax.status, CoreTreeSitterStatus::Prepared)
        || syntax.has_error
        || !syntax.error_ranges.is_empty()
        || !matches!(
            syntax.budget_status,
            vim_core_rs::CoreTreeSitterBudgetStatus::WithinBudget
        )
        || !tree_sitter_coverage_contains_range(&syntax.covered_ranges, visible_range)
    {
        log::debug!(
            "[screen_model] ignoring non-fresh Tree-sitter syntax: window_id={}, buffer_id={}, syntax_revision={:?}, buffer_revision={:?}, status={:?}, has_error={}, error_ranges={}, covered_ranges={}, budget_status={:?}",
            input.window_id,
            input.buffer_id,
            syntax.source_revision,
            buffer.source_revision,
            syntax.status,
            syntax.has_error,
            syntax.error_ranges.len(),
            syntax.covered_ranges.len(),
            syntax.budget_status
        );
        return Vec::new();
    }

    let line_numbers = input.session_state.line_numbers() || input.session_state.relative_number();
    let mut projected = Vec::new();

    for chunk in &syntax.chunks {
        let start_row = chunk.range.start.row.max(input.viewport_top);
        let end_row_exclusive = if chunk.range.end.col == 0 {
            chunk.range.end.row
        } else {
            chunk.range.end.row.saturating_add(1)
        };
        let end_row_inclusive = end_row_exclusive
            .saturating_sub(1)
            .min(viewport_bottom)
            .min(raw_lines.len().saturating_sub(1));
        if start_row > end_row_inclusive {
            continue;
        }

        for absolute_row in start_row..=end_row_inclusive {
            let raw_line_len = raw_lines.get(absolute_row).map_or(0, |line| line.len());
            let raw_start_col = if absolute_row == chunk.range.start.row {
                chunk.range.start.col
            } else {
                0
            };
            let raw_end_col = if absolute_row == chunk.range.end.row {
                chunk.range.end.col
            } else {
                raw_line_len
            };
            if raw_end_col <= raw_start_col {
                continue;
            }
            let Some((start_col, end_col_exclusive)) = project_tree_sitter_chunk_display_range(
                input,
                line_projections,
                absolute_row,
                CoreTextPosition {
                    row: absolute_row,
                    col: raw_start_col,
                },
                CoreTextPosition {
                    row: absolute_row,
                    col: raw_end_col,
                },
                line_numbers,
            ) else {
                continue;
            };
            projected.push(ScreenSyntaxChunk {
                row: u16::try_from(absolute_row.saturating_sub(input.viewport_top))
                    .unwrap_or(u16::MAX),
                start_col,
                end_col_exclusive,
                syn_id: 0,
                name: None,
                tree_sitter: Some(ScreenTreeSitterSyntax {
                    category: map_tree_sitter_category(chunk.category),
                    modifiers: chunk
                        .modifiers
                        .iter()
                        .copied()
                        .map(map_tree_sitter_modifier)
                        .collect(),
                    capture_name: chunk.capture_name.clone(),
                }),
            });
        }
    }

    log::debug!(
        "[screen_model] projected Tree-sitter syntax chunks: window_id={}, chunks={}, rows={:?}",
        input.window_id,
        projected.len(),
        projected.iter().map(|chunk| chunk.row).collect::<Vec<_>>()
    );
    projected
}

#[cfg(feature = "tree-sitter-syntax")]
fn tree_sitter_coverage_contains_range(
    covered_ranges: &[vim_core_rs::CoreTextRange],
    range: vim_core_rs::CoreTextRange,
) -> bool {
    covered_ranges
        .iter()
        .any(|covered| covered.start <= range.start && range.end <= covered.end)
}

#[cfg(feature = "tree-sitter-syntax")]
fn project_tree_sitter_chunk_display_range(
    input: &ProjectionInput<'_>,
    line_projections: &[ScreenLineProjection],
    absolute_row: usize,
    start: vim_core_rs::CoreTextPosition,
    end: vim_core_rs::CoreTextPosition,
    line_numbers: bool,
) -> Option<(u16, u16)> {
    let markdown_projection = input.markdown_document_map.and_then(|_| {
        line_projections
            .iter()
            .find(|projection| projection.absolute_row == absolute_row)
    });
    let start_col = markdown_projection.map_or_else(
        || {
            resolve_display_col_for_position(
                &input.snapshot.text,
                absolute_row,
                start.col,
                input.session_state.tab_size(),
                line_numbers,
                input.session_state.number_width(),
            )
        },
        |projection| projection.logical_to_display_col(start.col),
    );
    let end_col_exclusive = markdown_projection.map_or_else(
        || {
            resolve_display_col_for_position(
                &input.snapshot.text,
                absolute_row,
                end.col,
                input.session_state.tab_size(),
                line_numbers,
                input.session_state.number_width(),
            )
        },
        |projection| projection.logical_to_display_col(end.col),
    );
    if end_col_exclusive <= start_col {
        log::debug!(
            "[screen_model] ignoring Tree-sitter syntax chunk with non-positive display width: window_id={}, row={}, raw=({},{}), display=({},{})",
            input.window_id,
            absolute_row,
            start.col,
            end.col,
            start_col,
            end_col_exclusive
        );
        return None;
    }
    Some((start_col, end_col_exclusive))
}

#[cfg(feature = "tree-sitter-syntax")]
fn map_tree_sitter_category(category: vim_core_rs::CoreSyntaxCategory) -> ScreenSyntaxCategory {
    match category {
        vim_core_rs::CoreSyntaxCategory::Attribute => ScreenSyntaxCategory::Attribute,
        vim_core_rs::CoreSyntaxCategory::Comment => ScreenSyntaxCategory::Comment,
        vim_core_rs::CoreSyntaxCategory::Constant => ScreenSyntaxCategory::Constant,
        vim_core_rs::CoreSyntaxCategory::Constructor => ScreenSyntaxCategory::Constructor,
        vim_core_rs::CoreSyntaxCategory::Function => ScreenSyntaxCategory::Function,
        vim_core_rs::CoreSyntaxCategory::Keyword => ScreenSyntaxCategory::Keyword,
        vim_core_rs::CoreSyntaxCategory::Label => ScreenSyntaxCategory::Label,
        vim_core_rs::CoreSyntaxCategory::Markup => ScreenSyntaxCategory::Markup,
        vim_core_rs::CoreSyntaxCategory::Module => ScreenSyntaxCategory::Module,
        vim_core_rs::CoreSyntaxCategory::Number => ScreenSyntaxCategory::Number,
        vim_core_rs::CoreSyntaxCategory::Operator => ScreenSyntaxCategory::Operator,
        vim_core_rs::CoreSyntaxCategory::Property => ScreenSyntaxCategory::Property,
        vim_core_rs::CoreSyntaxCategory::Punctuation => ScreenSyntaxCategory::Punctuation,
        vim_core_rs::CoreSyntaxCategory::String => ScreenSyntaxCategory::String,
        vim_core_rs::CoreSyntaxCategory::Tag => ScreenSyntaxCategory::Tag,
        vim_core_rs::CoreSyntaxCategory::Text => ScreenSyntaxCategory::Text,
        vim_core_rs::CoreSyntaxCategory::Type => ScreenSyntaxCategory::Type,
        vim_core_rs::CoreSyntaxCategory::Variable => ScreenSyntaxCategory::Variable,
        vim_core_rs::CoreSyntaxCategory::Unknown => ScreenSyntaxCategory::Unknown,
    }
}

#[cfg(feature = "tree-sitter-syntax")]
fn map_tree_sitter_modifier(modifier: vim_core_rs::CoreSyntaxModifier) -> ScreenSyntaxModifier {
    match modifier {
        vim_core_rs::CoreSyntaxModifier::Async => ScreenSyntaxModifier::Async,
        vim_core_rs::CoreSyntaxModifier::Declaration => ScreenSyntaxModifier::Declaration,
        vim_core_rs::CoreSyntaxModifier::Definition => ScreenSyntaxModifier::Definition,
        vim_core_rs::CoreSyntaxModifier::Deprecated => ScreenSyntaxModifier::Deprecated,
        vim_core_rs::CoreSyntaxModifier::Documentation => ScreenSyntaxModifier::Documentation,
        vim_core_rs::CoreSyntaxModifier::Mutable => ScreenSyntaxModifier::Mutable,
        vim_core_rs::CoreSyntaxModifier::Readonly => ScreenSyntaxModifier::Readonly,
        vim_core_rs::CoreSyntaxModifier::Static => ScreenSyntaxModifier::Static,
    }
}

fn project_markdown_line_projections(input: &ProjectionInput<'_>) -> Vec<ScreenLineProjection> {
    let raw_lines = input.snapshot.text.split('\n').collect::<Vec<_>>();
    let markdown_document_map = if input.session_state.markdown_render() {
        input.markdown_document_map
    } else {
        log::debug!(
            "[screen_model] markdown render projection disabled by session option: window_id={}, cursor_row={}",
            input.window_id,
            input.cursor_row
        );
        None
    };
    let line_number_enabled =
        input.session_state.line_numbers() || input.session_state.relative_number();
    let number_width = line_number_width(raw_lines.len(), input.session_state.number_width());
    let line_start_col = if line_number_enabled {
        u16::try_from(number_width + 1).unwrap_or(u16::MAX)
    } else {
        0
    };
    let viewport_start = input.viewport_top.min(raw_lines.len());
    let viewport_end = viewport_start
        .saturating_add(input.body_height.max(1))
        .min(raw_lines.len());
    let raw_expansion = resolve_markdown_raw_expansion(input);

    let projections = (viewport_start..viewport_end)
        .map(|absolute_row| {
            let raw_text = raw_lines.get(absolute_row).copied().unwrap_or("");
            let keep_raw = raw_expansion.contains_row(absolute_row);
            project_markdown_line_projection(
                absolute_row,
                raw_text,
                markdown_document_map,
                keep_raw,
                usize::from(input.session_state.tab_size().max(1)),
                line_start_col,
            )
        })
        .collect::<Vec<_>>();

    log::debug!(
        "[screen_model] markdown line projections built: window_id={}, visible_rows={}, viewport_top={}, line_start_col={}, markdown_metadata_present={}, raw_expansion={:?}",
        input.window_id,
        projections.len(),
        input.viewport_top,
        line_start_col,
        markdown_document_map.is_some(),
        raw_expansion
    );

    projections
}

fn project_markdown_line_projection(
    absolute_row: usize,
    raw_text: &str,
    markdown_document_map: Option<&MarkdownDocumentMap>,
    keep_raw: bool,
    tab_size: usize,
    line_start_col: u16,
) -> ScreenLineProjection {
    let conceal_ranges = if keep_raw {
        log::debug!(
            "[screen_model] markdown raw line selected: row={}, raw_len={}, reason=active_cursor_raw_expansion",
            absolute_row,
            raw_text.len()
        );
        Vec::new()
    } else {
        markdown_document_map
            .map(|map| markdown_conceal_ranges_for_line(map, absolute_row, raw_text))
            .unwrap_or_default()
    };
    let mut display_text = String::new();
    let mut spans = Vec::new();
    let mut cells = Vec::new();
    let mut display_col = usize::from(line_start_col);
    let mut raw_col = 0usize;

    for operation in conceal_ranges {
        if operation.raw_start_col > raw_col {
            append_raw_projection_segment(
                absolute_row,
                raw_text,
                raw_col,
                operation.raw_start_col,
                tab_size,
                &mut display_col,
                &mut display_text,
                &mut spans,
                &mut cells,
            );
        }
        append_replacement_projection_segment(
            absolute_row,
            raw_text,
            operation,
            &mut display_col,
            &mut display_text,
            &mut spans,
            &mut cells,
        );
        raw_col = raw_col.max(operation.raw_end_col);
    }

    if raw_col < raw_text.len() {
        append_raw_projection_segment(
            absolute_row,
            raw_text,
            raw_col,
            raw_text.len(),
            tab_size,
            &mut display_col,
            &mut display_text,
            &mut spans,
            &mut cells,
        );
    }

    log::debug!(
        "[screen_model] markdown line projection built: row={}, raw_len={}, display_width={}, spans={}, cells={}",
        absolute_row,
        raw_text.len(),
        display_col.saturating_sub(usize::from(line_start_col)),
        spans.len(),
        cells.len()
    );

    ScreenLineProjection {
        absolute_row,
        raw_text: raw_text.to_string(),
        display_text,
        spans,
        cells,
        line_start_col,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MarkdownRawExpansion {
    None,
    CursorBlock {
        start_row: usize,
        end_row: usize,
        kind: MarkdownBlockKind,
    },
    CursorRow {
        row: usize,
    },
}

impl MarkdownRawExpansion {
    fn contains_row(&self, row: usize) -> bool {
        match self {
            MarkdownRawExpansion::None => false,
            MarkdownRawExpansion::CursorBlock {
                start_row, end_row, ..
            } => (*start_row..=*end_row).contains(&row),
            MarkdownRawExpansion::CursorRow { row: cursor_row } => *cursor_row == row,
        }
    }
}

fn resolve_markdown_raw_expansion(input: &ProjectionInput<'_>) -> MarkdownRawExpansion {
    if !input.session_state.markdown_render() {
        log::debug!(
            "[screen_model] markdown raw expansion disabled: window_id={}, active={}, cursor_row={}, reason=markdown_render_option_off",
            input.window_id,
            input.is_active,
            input.cursor_row
        );
        return MarkdownRawExpansion::None;
    }

    let Some(map) = input.markdown_document_map else {
        log::debug!(
            "[screen_model] markdown raw expansion disabled: window_id={}, active={}, cursor_row={}, reason=no_markdown_metadata",
            input.window_id,
            input.is_active,
            input.cursor_row
        );
        return MarkdownRawExpansion::None;
    };

    if !input.is_active {
        log::debug!(
            "[screen_model] markdown raw expansion disabled: window_id={}, active={}, cursor_row={}, block_count={}, reason=inactive_pane",
            input.window_id,
            input.is_active,
            input.cursor_row,
            map.blocks.len()
        );
        return MarkdownRawExpansion::None;
    }

    if let Some(block) = map
        .blocks
        .iter()
        .find(|block| (block.range.start.line..=block.range.end.line).contains(&input.cursor_row))
    {
        let expansion = MarkdownRawExpansion::CursorBlock {
            start_row: block.range.start.line,
            end_row: block.range.end.line,
            kind: block.kind.clone(),
        };
        log::debug!(
            "[screen_model] markdown raw expansion resolved: window_id={}, cursor_row={}, start_row={}, end_row={}, kind={:?}, reason=cursor_inside_block",
            input.window_id,
            input.cursor_row,
            block.range.start.line,
            block.range.end.line,
            block.kind
        );
        return expansion;
    }

    log::debug!(
        "[screen_model] markdown raw expansion resolved: window_id={}, cursor_row={}, block_count={}, reason=no_block_contains_cursor_row_fallback_to_cursor_row",
        input.window_id,
        input.cursor_row,
        map.blocks.len()
    );
    MarkdownRawExpansion::CursorRow {
        row: input.cursor_row,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MarkdownProjectionOperation {
    raw_start_col: usize,
    raw_end_col: usize,
    replacement: Option<&'static str>,
}

fn markdown_conceal_ranges_for_line(
    map: &MarkdownDocumentMap,
    absolute_row: usize,
    raw_text: &str,
) -> Vec<MarkdownProjectionOperation> {
    let mut operations = Vec::new();
    for block in &map.blocks {
        match &block.kind {
            MarkdownBlockKind::Heading { level } if block.range.start.line == absolute_row => {
                if let Some(operation) = heading_marker_range(raw_text, *level) {
                    operations.push(operation);
                }
            }
            MarkdownBlockKind::ListItem {
                ordered,
                checkbox: Some(state),
                ..
            } if block.range.start.line == absolute_row => {
                if let Some(range) = list_marker_range(raw_text, *ordered) {
                    operations.push(range);
                }
                if let Some(range) = checkbox_marker_range(raw_text, *state) {
                    operations.push(range);
                }
            }
            MarkdownBlockKind::ListItem {
                ordered,
                checkbox: None,
                ..
            } if block.range.start.line == absolute_row => {
                if let Some(range) = list_marker_range(raw_text, *ordered) {
                    operations.push(range);
                }
            }
            _ => {}
        }
    }
    for inline in &map.inlines {
        match &inline.kind {
            MarkdownInlineKind::EmphasisMarker { .. }
                if inline.range.start.line == absolute_row =>
            {
                operations.push(MarkdownProjectionOperation {
                    raw_start_col: inline.range.start.column,
                    raw_end_col: inline.range.end.column,
                    replacement: None,
                });
            }
            MarkdownInlineKind::InlineCode if inline.range.start.line == absolute_row => {
                operations.push(MarkdownProjectionOperation {
                    raw_start_col: inline.range.start.column,
                    raw_end_col: inline.range.start.column + 1,
                    replacement: None,
                });
                operations.push(MarkdownProjectionOperation {
                    raw_start_col: inline.range.end.column.saturating_sub(1),
                    raw_end_col: inline.range.end.column,
                    replacement: None,
                });
            }
            MarkdownInlineKind::Link { text, destination }
                if inline.range.start.line == absolute_row =>
            {
                operations.push(MarkdownProjectionOperation {
                    raw_start_col: inline.range.start.column,
                    raw_end_col: text.start.column,
                    replacement: None,
                });
                operations.push(MarkdownProjectionOperation {
                    raw_start_col: text.end.column,
                    raw_end_col: destination.end.column.saturating_add(1),
                    replacement: None,
                });
            }
            _ => {}
        }
    }

    operations.sort_by_key(|operation| (operation.raw_start_col, operation.raw_end_col));
    let mut normalized = Vec::new();
    for operation in operations {
        if operation.raw_end_col <= operation.raw_start_col {
            continue;
        }
        if normalized
            .last()
            .is_some_and(|last: &MarkdownProjectionOperation| {
                last.raw_end_col > operation.raw_start_col
            })
        {
            log::debug!(
                "[screen_model] skipping overlapping markdown projection operation: row={}, raw=({}, {})",
                absolute_row,
                operation.raw_start_col,
                operation.raw_end_col
            );
            continue;
        }
        normalized.push(operation);
    }
    normalized
}

fn heading_marker_range(raw_text: &str, level: u8) -> Option<MarkdownProjectionOperation> {
    let marker_start = raw_text
        .char_indices()
        .find_map(|(index, ch)| (!ch.is_whitespace()).then_some(index))?;
    let marker_end = marker_start
        .saturating_add(usize::from(level))
        .saturating_add(1);
    (marker_end <= raw_text.len()).then_some(MarkdownProjectionOperation {
        raw_start_col: marker_start,
        raw_end_col: marker_end,
        replacement: None,
    })
}

fn checkbox_marker_range(
    raw_text: &str,
    state: MarkdownCheckboxState,
) -> Option<MarkdownProjectionOperation> {
    let marker = match state {
        MarkdownCheckboxState::Checked => raw_text
            .find("[x]")
            .or_else(|| raw_text.find("[X]"))
            .map(|start| (start, "✅")),
        MarkdownCheckboxState::Unchecked => raw_text.find("[ ]").map(|start| (start, "☐")),
    }?;
    let marker_start = marker.0;
    let marker_end = marker_start.saturating_add(3);
    if marker_end > raw_text.len() {
        return None;
    }
    Some(MarkdownProjectionOperation {
        raw_start_col: marker_start,
        raw_end_col: marker_end,
        replacement: Some(marker.1),
    })
}

fn list_marker_range(raw_text: &str, ordered: bool) -> Option<MarkdownProjectionOperation> {
    if ordered {
        return None;
    }
    let marker_start = raw_text
        .char_indices()
        .find_map(|(index, ch)| (!ch.is_whitespace()).then_some(index))?;
    let marker_end = marker_start.saturating_add(2);
    let marker = raw_text.get(marker_start..marker_end)?;
    matches!(marker, "- " | "+ " | "* ").then_some(MarkdownProjectionOperation {
        raw_start_col: marker_start,
        raw_end_col: marker_end,
        replacement: Some("• "),
    })
}

fn append_raw_projection_segment(
    absolute_row: usize,
    raw_text: &str,
    raw_start_col: usize,
    raw_end_col: usize,
    tab_size: usize,
    display_col: &mut usize,
    display_text: &mut String,
    spans: &mut Vec<ScreenDisplaySpan>,
    cells: &mut Vec<ScreenCellMapping>,
) {
    let raw_start_col = clamp_to_char_boundary(raw_text, raw_start_col.min(raw_text.len()));
    let raw_end_col = clamp_to_char_boundary(raw_text, raw_end_col.min(raw_text.len()));
    if raw_end_col <= raw_start_col {
        return;
    }
    let display_start_col = *display_col;
    let mut raw_col = raw_start_col;
    for ch in raw_text[raw_start_col..raw_end_col].chars() {
        let next_raw_col = raw_col + ch.len_utf8();
        let rendered = if ch == '\t' {
            let width = next_tab_stop(*display_col, tab_size).saturating_sub(*display_col);
            " ".repeat(width)
        } else {
            ch.to_string()
        };
        let width = display_width(&rendered, tab_size);
        display_text.push_str(&rendered);
        cells.push(ScreenCellMapping {
            display_col: u16::try_from(*display_col).unwrap_or(u16::MAX),
            display_end_col_exclusive: u16::try_from(display_col.saturating_add(width))
                .unwrap_or(u16::MAX),
            raw_start_col: raw_col,
            raw_end_col: next_raw_col,
        });
        *display_col = display_col.saturating_add(width);
        raw_col = next_raw_col;
    }
    spans.push(ScreenDisplaySpan {
        raw_start_col,
        raw_end_col,
        display_start_col: u16::try_from(display_start_col).unwrap_or(u16::MAX),
        display_end_col_exclusive: u16::try_from(*display_col).unwrap_or(u16::MAX),
        kind: ScreenDisplaySpanKind::RawText,
    });
    log::debug!(
        "[screen_model] markdown raw segment projected: row={}, raw=({},{}), display=({}, {})",
        absolute_row,
        raw_start_col,
        raw_end_col,
        display_start_col,
        *display_col
    );
}

fn append_replacement_projection_segment(
    absolute_row: usize,
    raw_text: &str,
    operation: MarkdownProjectionOperation,
    display_col: &mut usize,
    display_text: &mut String,
    spans: &mut Vec<ScreenDisplaySpan>,
    cells: &mut Vec<ScreenCellMapping>,
) {
    let raw_start_col =
        clamp_to_char_boundary(raw_text, operation.raw_start_col.min(raw_text.len()));
    let raw_end_col = clamp_to_char_boundary(raw_text, operation.raw_end_col.min(raw_text.len()));
    let display_start_col = *display_col;
    if let Some(replacement) = operation.replacement {
        display_text.push_str(replacement);
        let width = display_width(replacement, 1);
        *display_col = display_col.saturating_add(width);
        cells.push(ScreenCellMapping {
            display_col: u16::try_from(display_start_col).unwrap_or(u16::MAX),
            display_end_col_exclusive: u16::try_from(*display_col).unwrap_or(u16::MAX),
            raw_start_col,
            raw_end_col,
        });
        spans.push(ScreenDisplaySpan {
            raw_start_col,
            raw_end_col,
            display_start_col: u16::try_from(display_start_col).unwrap_or(u16::MAX),
            display_end_col_exclusive: u16::try_from(*display_col).unwrap_or(u16::MAX),
            kind: ScreenDisplaySpanKind::MarkdownReplacement {
                text: replacement.to_string(),
            },
        });
    } else {
        spans.push(ScreenDisplaySpan {
            raw_start_col,
            raw_end_col,
            display_start_col: u16::try_from(display_start_col).unwrap_or(u16::MAX),
            display_end_col_exclusive: u16::try_from(display_start_col).unwrap_or(u16::MAX),
            kind: ScreenDisplaySpanKind::ConcealedMarkdownMarker,
        });
    }
    log::debug!(
        "[screen_model] markdown conceal/replacement segment projected: row={}, raw=({},{}), display=({},{}), replacement={:?}",
        absolute_row,
        raw_start_col,
        raw_end_col,
        display_start_col,
        *display_col,
        operation.replacement
    );
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use vim_core_rs::{CoreInputRequestKind, CoreMode, CoreSyntaxChunk};

    use super::*;
    use crate::core_bridge::CoreBridge;
    use crate::core_notification_prompt::InputPromptStatus;
    use crate::search_capability::SearchCapabilityContract;
    use crate::search_query::{
        SearchMatch, SearchMatchKind, SearchQueryMode, SearchVisibleRows, SearchVisibleState,
    };

    use crate::session_guard::test_lock as session_test_lock;

    // ---- タスク 6.1: file name と mode を描画モデルへ投影するテスト ----

    #[test]
    fn projects_file_name_from_session_target_path() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("hello\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(Some(PathBuf::from("/tmp/hello.txt")));

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(
            model.file_name, "/tmp/hello.txt",
            "session の target_path がファイル名として投影されること"
        );
    }

    #[test]
    fn projects_default_file_name_when_no_target_path() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(
            model.file_name, "[新規]",
            "ターゲットパスなしの場合はデフォルト名が使われること"
        );
    }

    #[test]
    fn projects_normal_mode_label() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("text\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        assert_eq!(snapshot.mode, CoreMode::Normal);

        let session_state = EditorSessionState::new(None);
        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(
            model.mode_label, "NORMAL",
            "ノーマルモードのラベルが NORMAL であること"
        );
    }

    #[test]
    fn projects_insert_mode_label() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("text\n").expect("core bridge");
        bridge.dispatch_key("i").expect("insert mode");
        let snapshot = bridge.snapshot();
        assert_eq!(snapshot.mode, CoreMode::Insert);

        let session_state = EditorSessionState::new(None);
        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(
            model.mode_label, "INSERT",
            "インサートモードのラベルが INSERT であること"
        );
    }

    #[test]
    fn projects_cursor_style_from_core_mode() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("text\n").expect("core bridge");
        let base_snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let cases = [
            (CoreMode::Normal, ScreenCursorStyle::Block),
            (CoreMode::Insert, ScreenCursorStyle::SteadyBar),
            (CoreMode::Replace, ScreenCursorStyle::UnderScore),
            (CoreMode::Visual, ScreenCursorStyle::Block),
            (CoreMode::CommandLine, ScreenCursorStyle::SteadyBar),
        ];

        for (mode, expected_style) in cases {
            let mut snapshot = base_snapshot.clone();
            snapshot.mode = mode;
            let model = project(&ProjectionInput::new(&snapshot, &session_state, None));
            assert_eq!(
                model.cursor_style, expected_style,
                "mode {mode:?} should project cursor style {expected_style:?}"
            );
        }
    }

    #[test]
    fn active_cursor_style_prefers_command_line_overlay() {
        let pane = ScreenModel {
            window_id: 1,
            buffer_id: 1,
            rect: PaneRect {
                x: 0,
                y: 0,
                width: 20,
                height: 4,
            },
            file_name: "sample.txt".to_string(),
            mode_label: "NORMAL".to_string(),
            cursor_style: ScreenCursorStyle::Block,
            dirty: false,
            lines: vec!["alpha".to_string()],
            line_projections: vec![],
            cursor_row: 0,
            cursor_col: 0,
            visual_selection: None,
            search_overlays: vec![],
            syntax_chunks: vec![],
            markdown_style_ranges: vec![],
            message_line: None,
            command_cursor_col: None,
            is_active: true,
        };
        let mut workspace = WorkspaceScreenModel {
            panes: vec![pane],
            active_window_id: 1,
            message_line: resolve_workspace_message_line(Vec::<MessageLineCandidate>::new()),
            message_area_height: 5,
            message_scroll_offset: 0,
            prompt_line: None,
            pager_prompt: None,
            suppressed_prompt_hints: vec![],
            bell: None,
            command_line: None,
        };

        assert_eq!(workspace.active_cursor_style(), ScreenCursorStyle::Block);

        workspace.command_line = Some(CommandLineModel {
            text: ":write".to_string(),
            cursor_col: 6,
        });

        assert_eq!(
            workspace.active_cursor_style(),
            ScreenCursorStyle::SteadyBar
        );
    }

    #[test]
    fn file_name_and_mode_are_never_empty_at_startup() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert!(
            !model.file_name.is_empty(),
            "起動直後でもファイル名は空でないこと"
        );
        assert!(
            !model.mode_label.is_empty(),
            "起動直後でもモードラベルは空でないこと"
        );
    }

    // ---- タスク 6.2: dirty 状態とカーソル位置を描画モデルへ投影するテスト ----

    #[test]
    fn projects_dirty_false_for_clean_buffer() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("clean\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert!(!model.dirty, "未編集バッファは dirty=false であること");
    }

    #[test]
    fn projects_dirty_true_after_edit() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("text\n").expect("core bridge");
        bridge.dispatch_key("i").expect("insert mode");
        bridge.dispatch_key("X").expect("insert X");
        bridge.dispatch_key("\x1b").expect("normal mode");
        let snapshot = bridge.snapshot();
        assert!(snapshot.dirty);

        let session_state = EditorSessionState::new(None);
        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert!(model.dirty, "編集後のバッファは dirty=true であること");
    }

    #[test]
    fn projects_cursor_position_at_origin() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("abc\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(model.cursor_row, 0, "初期カーソル行は 0");
        assert_eq!(model.cursor_col, 0, "初期カーソル列は 0");
    }

    #[test]
    fn projects_cursor_position_after_movement() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("abcde\nfghij\n").expect("core bridge");
        bridge.dispatch_key("jll").expect("j, ll for movement");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(model.cursor_row, 1, "カーソル行が移動後に反映されること");
        assert_eq!(model.cursor_col, 2, "カーソル列が移動後に反映されること");
    }

    #[test]
    fn projects_visible_slice_and_relative_cursor_row_when_viewport_applied() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge =
            CoreBridge::new("line1\nline2\nline3\nline4\nline5\n").expect("core bridge");
        bridge.dispatch_key("jjj").expect("move to fourth line");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let model =
            project(&ProjectionInput::new(&snapshot, &session_state, None).with_viewport(2, 2));

        assert_eq!(model.lines, vec!["line3", "line4"]);
        assert_eq!(model.cursor_row, 1, "viewport 内の相対行へ変換されること");
    }

    #[test]
    fn projects_syntax_chunks_to_visible_display_columns_without_changing_line_text() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("fn\tmain\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new_with_tab_size_and_line_numbers(None, 4, true);
        let mut syntax_lines = BTreeMap::new();
        syntax_lines.insert(
            0,
            vec![CoreSyntaxChunk {
                start_col: 3,
                end_col: 7,
                syn_id: 11,
                name: Some("Identifier".to_string()),
            }],
        );

        let model = project(
            &ProjectionInput::new(&snapshot, &session_state, None)
                .with_syntax_lines(Some(&syntax_lines)),
        );

        assert_eq!(
            model.lines,
            vec!["   1 fn  main"],
            "syntax projection must not change rendered text"
        );
        assert_eq!(
            model.syntax_chunks,
            vec![ScreenSyntaxChunk {
                row: 0,
                start_col: 9,
                end_col_exclusive: 13,
                syn_id: 11,
                name: Some("Identifier".to_string()),
                tree_sitter: None,
            }]
        );
    }

    #[test]
    fn projects_syntax_chunks_through_markdown_rich_display_mapping() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let source = "# Title\n";
        let bridge = CoreBridge::new(source).expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);
        let markdown_map = MarkdownDocumentMap::parse(source);
        let mut syntax_lines = BTreeMap::new();
        syntax_lines.insert(
            0,
            vec![CoreSyntaxChunk {
                start_col: 2,
                end_col: 7,
                syn_id: 11,
                name: Some("Title".to_string()),
            }],
        );
        let mut input = ProjectionInput::new(&snapshot, &session_state, None)
            .with_markdown_document_map(Some(&markdown_map))
            .with_syntax_lines(Some(&syntax_lines));
        input.is_active = false;

        let model = project(&input);

        assert_eq!(model.line_projections[0].display_text, "Title");
        assert_eq!(
            model.syntax_chunks,
            vec![ScreenSyntaxChunk {
                row: 0,
                start_col: 0,
                end_col_exclusive: 5,
                syn_id: 11,
                name: Some("Title".to_string()),
                tree_sitter: None,
            }],
            "syntax chunks should be projected through Markdown rich display-space"
        );
    }

    #[test]
    fn projects_syntax_chunks_against_raw_active_markdown_rows() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let source = "# Title\n";
        let bridge = CoreBridge::new(source).expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);
        let markdown_map = MarkdownDocumentMap::parse(source);
        let mut syntax_lines = BTreeMap::new();
        syntax_lines.insert(
            0,
            vec![CoreSyntaxChunk {
                start_col: 2,
                end_col: 7,
                syn_id: 11,
                name: Some("Title".to_string()),
            }],
        );

        let model = project(
            &ProjectionInput::new(&snapshot, &session_state, None)
                .with_markdown_document_map(Some(&markdown_map))
                .with_syntax_lines(Some(&syntax_lines)),
        );

        assert_eq!(model.line_projections[0].display_text, "# Title");
        assert_eq!(
            model.syntax_chunks,
            vec![ScreenSyntaxChunk {
                row: 0,
                start_col: 2,
                end_col_exclusive: 7,
                syn_id: 11,
                name: Some("Title".to_string()),
                tree_sitter: None,
            }],
            "active Markdown rows should keep syntax chunks aligned with raw text"
        );
    }

    #[cfg(feature = "tree-sitter-syntax")]
    #[test]
    fn projects_prepared_tree_sitter_chunks_without_core_syntax_chunks() {
        use vim_core_rs::{
            CoreSyntaxCategory, CoreSyntaxModifier, CoreTextPosition, CoreTextRange,
            CoreTreeSitterBudgetStatus, CoreTreeSitterChunk, CoreTreeSitterProvenance,
            CoreTreeSitterRangeSyntax, CoreTreeSitterStatus,
        };

        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("fn main() {}\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let active_buffer = snapshot
            .buffers
            .iter()
            .find(|buffer| buffer.is_active)
            .expect("active buffer");
        let session_state = EditorSessionState::new(None);
        let covered_range = CoreTextRange {
            start: CoreTextPosition { row: 0, col: 0 },
            end: CoreTextPosition {
                row: usize::MAX,
                col: usize::MAX,
            },
        };
        let syntax = CoreTreeSitterRangeSyntax {
            buffer_id: active_buffer.id,
            source_revision: active_buffer.source_revision,
            provenance: CoreTreeSitterProvenance {
                language_id: "rust".to_string(),
                package_id: "tree-sitter-rust".to_string(),
                package_version: "0.24.2".to_string(),
                parser_version: "14".to_string(),
                query_version: "saya-test".to_string(),
            },
            status: CoreTreeSitterStatus::Prepared,
            has_error: false,
            covered_ranges: vec![covered_range],
            error_ranges: vec![],
            budget_status: CoreTreeSitterBudgetStatus::WithinBudget,
            chunks: vec![CoreTreeSitterChunk {
                range: CoreTextRange {
                    start: CoreTextPosition { row: 0, col: 0 },
                    end: CoreTextPosition { row: 0, col: 2 },
                },
                capture_name: "keyword".to_string(),
                category: CoreSyntaxCategory::Keyword,
                modifiers: vec![CoreSyntaxModifier::Definition],
            }],
            embedded_regions: vec![],
        };

        let model = project(
            &ProjectionInput::new(&snapshot, &session_state, None)
                .with_tree_sitter_syntax(Some(&syntax)),
        );

        assert_eq!(
            model.syntax_chunks,
            vec![ScreenSyntaxChunk {
                row: 0,
                start_col: 0,
                end_col_exclusive: 2,
                syn_id: 0,
                name: None,
                tree_sitter: Some(ScreenTreeSitterSyntax {
                    category: ScreenSyntaxCategory::Keyword,
                    modifiers: vec![ScreenSyntaxModifier::Definition],
                    capture_name: "keyword".to_string(),
                }),
            }],
            "Tree-sitter render data must stay separate from Vim CoreSyntaxChunk"
        );
    }

    #[cfg(feature = "tree-sitter-syntax")]
    #[test]
    fn skips_tree_sitter_chunks_when_result_is_not_fresh_prepared_data() {
        use vim_core_rs::{
            CoreBufferRevision, CoreSyntaxCategory, CoreTextPosition, CoreTextRange,
            CoreTreeSitterBudgetStatus, CoreTreeSitterChunk, CoreTreeSitterProvenance,
            CoreTreeSitterRangeSyntax, CoreTreeSitterStatus,
        };

        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("fn main() {}\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let active_buffer = snapshot
            .buffers
            .iter()
            .find(|buffer| buffer.is_active)
            .expect("active buffer");
        let session_state = EditorSessionState::new(None);
        let covered_range = CoreTextRange {
            start: CoreTextPosition { row: 0, col: 0 },
            end: CoreTextPosition {
                row: usize::MAX,
                col: usize::MAX,
            },
        };
        let base_syntax = CoreTreeSitterRangeSyntax {
            buffer_id: active_buffer.id,
            source_revision: active_buffer.source_revision,
            provenance: CoreTreeSitterProvenance {
                language_id: "rust".to_string(),
                package_id: "tree-sitter-rust".to_string(),
                package_version: "0.24.2".to_string(),
                parser_version: "14".to_string(),
                query_version: "saya-test".to_string(),
            },
            status: CoreTreeSitterStatus::Prepared,
            has_error: false,
            covered_ranges: vec![covered_range],
            error_ranges: vec![],
            budget_status: CoreTreeSitterBudgetStatus::WithinBudget,
            chunks: vec![CoreTreeSitterChunk {
                range: CoreTextRange {
                    start: CoreTextPosition { row: 0, col: 0 },
                    end: CoreTextPosition { row: 0, col: 2 },
                },
                capture_name: "keyword".to_string(),
                category: CoreSyntaxCategory::Keyword,
                modifiers: vec![],
            }],
            embedded_regions: vec![],
        };

        let stale_revision = {
            let mut syntax = base_syntax.clone();
            syntax.source_revision = CoreBufferRevision {
                value: active_buffer.source_revision.value.saturating_sub(1),
            };
            syntax
        };
        let stale_status = {
            let mut syntax = base_syntax.clone();
            syntax.status = CoreTreeSitterStatus::Stale;
            syntax
        };
        let parser_error = {
            let mut syntax = base_syntax.clone();
            syntax.has_error = true;
            syntax
        };
        let error_range = {
            let mut syntax = base_syntax.clone();
            syntax.error_ranges = vec![CoreTextRange {
                start: CoreTextPosition { row: 0, col: 0 },
                end: CoreTextPosition { row: 0, col: 2 },
            }];
            syntax
        };
        let budget_exceeded = {
            let mut syntax = base_syntax.clone();
            syntax.budget_status = CoreTreeSitterBudgetStatus::GlobalBudgetExceeded;
            syntax
        };
        let uncovered = {
            let mut syntax = base_syntax;
            syntax.covered_ranges.clear();
            syntax
        };

        for (case, syntax) in [
            ("stale revision", stale_revision),
            ("stale status", stale_status),
            ("parser error", parser_error),
            ("error range", error_range),
            ("budget exceeded", budget_exceeded),
            ("uncovered visible range", uncovered),
        ] {
            let model = project(
                &ProjectionInput::new(&snapshot, &session_state, None)
                    .with_tree_sitter_syntax(Some(&syntax)),
            );

            assert!(
                model.syntax_chunks.is_empty(),
                "{case} Tree-sitter data must not be drawn as fresh highlight"
            );
        }
    }

    #[test]
    fn markdown_projection_preserves_raw_offsets_for_multibyte_and_concealed_markers() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let source = "# あ*強*\n";
        let bridge = CoreBridge::new(source).expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);
        let markdown_map = MarkdownDocumentMap::parse(source);
        let mut input = ProjectionInput::new(&snapshot, &session_state, None)
            .with_markdown_document_map(Some(&markdown_map));
        input.is_active = false;

        let model = project(&input);
        let row = &model.line_projections[0];

        assert_eq!(row.raw_text, "# あ*強*");
        assert_eq!(row.display_text, "あ強");
        assert_eq!(row.logical_to_display_col(0), 0);
        assert_eq!(row.logical_to_display_col(2), 0);
        assert_eq!(row.logical_to_display_col(5), 2);
        assert_eq!(row.logical_to_display_col(9), 4);
        assert_eq!(row.display_to_logical_col(0), Some(2));
        assert_eq!(row.display_to_logical_col(2), Some(6));
        assert_eq!(
            snapshot.text, source,
            "Markdown projection must not mutate the raw buffer text"
        );
    }

    #[test]
    fn markdown_projection_maps_tabs_from_raw_byte_to_display_cells() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let source = "# a\tb\n";
        let bridge = CoreBridge::new(source).expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new_with_tab_size(None, 4);
        let markdown_map = MarkdownDocumentMap::parse(source);
        let mut input = ProjectionInput::new(&snapshot, &session_state, None)
            .with_markdown_document_map(Some(&markdown_map));
        input.is_active = false;

        let model = project(&input);
        let row = &model.line_projections[0];

        assert_eq!(row.raw_text, "# a\tb");
        assert_eq!(row.display_text, "a   b");
        assert_eq!(row.logical_to_display_col(3), 1);
        assert_eq!(row.logical_to_display_col(4), 4);
        assert_eq!(row.display_to_logical_col(1), Some(3));
        assert_eq!(row.display_to_logical_col(3), Some(3));
        assert_eq!(row.display_to_logical_col(4), Some(4));
    }

    #[test]
    fn markdown_projection_keeps_line_number_gutter_outside_raw_mapping() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let source = "# Title\n";
        let bridge = CoreBridge::new(source).expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new_with_tab_size_and_line_numbers_and_number_width(
            None, 8, true, 4,
        );
        let markdown_map = MarkdownDocumentMap::parse(source);
        let mut input = ProjectionInput::new(&snapshot, &session_state, None)
            .with_markdown_document_map(Some(&markdown_map));
        input.is_active = false;

        let model = project(&input);
        let row = &model.line_projections[0];

        assert_eq!(model.lines[0], "   1 # Title");
        assert_eq!(row.line_start_col, 5);
        assert_eq!(row.display_text, "Title");
        assert_eq!(row.logical_to_display_col(2), 5);
        assert_eq!(row.display_to_logical_col(4), None);
        assert_eq!(row.display_to_logical_col(5), Some(2));
    }

    #[test]
    fn markdown_projection_replaces_checkbox_marker_with_wide_display_glyph() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let source = "- [x] done\n";
        let bridge = CoreBridge::new(source).expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);
        let markdown_map = MarkdownDocumentMap::parse(source);
        let mut input = ProjectionInput::new(&snapshot, &session_state, None)
            .with_markdown_document_map(Some(&markdown_map));
        input.is_active = false;

        let model = project(&input);
        let row = &model.line_projections[0];

        assert_eq!(row.raw_text, "- [x] done");
        assert_eq!(row.display_text, "• ✅ done");
        assert_eq!(row.logical_to_display_col(2), 2);
        assert_eq!(row.logical_to_display_col(5), 4);
        assert_eq!(row.display_to_logical_col(2), Some(2));
        assert_eq!(row.display_to_logical_col(3), Some(2));
        assert!(
            row.spans.iter().any(|span| matches!(
                span.kind,
                ScreenDisplaySpanKind::MarkdownReplacement { ref text } if text == "✅"
            )),
            "checkbox marker should be represented as an explicit replacement span"
        );
    }

    #[test]
    fn markdown_projection_replaces_unordered_list_marker_with_bullet() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let source = "- item\n";
        let bridge = CoreBridge::new(source).expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);
        let markdown_map = MarkdownDocumentMap::parse(source);
        let mut input = ProjectionInput::new(&snapshot, &session_state, None)
            .with_markdown_document_map(Some(&markdown_map));
        input.is_active = false;

        let model = project(&input);
        let row = &model.line_projections[0];

        assert_eq!(row.raw_text, "- item");
        assert_eq!(row.display_text, "• item");
        assert_eq!(row.logical_to_display_col(0), 0);
        assert_eq!(row.logical_to_display_col(2), 2);
        assert!(
            row.spans.iter().any(|span| matches!(
                span.kind,
                ScreenDisplaySpanKind::MarkdownReplacement { ref text } if text == "• "
            )),
            "unordered list marker should be represented as an explicit replacement span"
        );
    }

    #[test]
    fn markdown_projection_respects_viewport_absolute_rows() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let source = "alpha\n# Beta\n*gamma*\n";
        let bridge = CoreBridge::new(source).expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);
        let markdown_map = MarkdownDocumentMap::parse(source);

        let model = project(
            &ProjectionInput::new(&snapshot, &session_state, None)
                .with_markdown_document_map(Some(&markdown_map))
                .with_viewport(1, 2),
        );

        assert_eq!(model.lines, vec!["# Beta", "*gamma*"]);
        assert_eq!(
            model
                .line_projections
                .iter()
                .map(|row| (row.absolute_row, row.display_text.as_str()))
                .collect::<Vec<_>>(),
            vec![(1, "Beta"), (2, "gamma")]
        );
    }

    #[test]
    fn active_markdown_projection_keeps_cursor_heading_block_raw_and_other_rows_rich() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let source = "# Title\n*body*\n";
        let bridge = CoreBridge::new(source).expect("core bridge");
        let mut snapshot = bridge.snapshot();
        snapshot.cursor_row = 0;
        let session_state = EditorSessionState::new(None);
        let markdown_map = MarkdownDocumentMap::parse(source);
        let mut input = ProjectionInput::new(&snapshot, &session_state, None)
            .with_markdown_document_map(Some(&markdown_map));
        input.cursor_row = 0;

        let model = project(&input);

        assert_eq!(model.line_projections[0].display_text, "# Title");
        assert_eq!(model.line_projections[1].display_text, "body");
        assert_eq!(
            snapshot.text, source,
            "raw block expansion must not mutate the raw buffer text"
        );
    }

    #[test]
    fn active_markdown_projection_keeps_cursor_list_block_raw() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let source = "- [x] done\n# Next\n";
        let bridge = CoreBridge::new(source).expect("core bridge");
        let mut snapshot = bridge.snapshot();
        snapshot.cursor_row = 0;
        let session_state = EditorSessionState::new(None);
        let markdown_map = MarkdownDocumentMap::parse(source);
        let mut input = ProjectionInput::new(&snapshot, &session_state, None)
            .with_markdown_document_map(Some(&markdown_map));
        input.cursor_row = 0;

        let model = project(&input);

        assert_eq!(model.line_projections[0].display_text, "- [x] done");
        assert_eq!(model.line_projections[1].display_text, "Next");
    }

    #[test]
    fn active_markdown_projection_keeps_entire_cursor_fenced_block_raw() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let source = "# Before\n```rust\n*raw*\n```\n# After\n";
        let bridge = CoreBridge::new(source).expect("core bridge");
        let mut snapshot = bridge.snapshot();
        snapshot.cursor_row = 2;
        let session_state = EditorSessionState::new(None);
        let markdown_map = MarkdownDocumentMap::parse(source);
        let mut input = ProjectionInput::new(&snapshot, &session_state, None)
            .with_markdown_document_map(Some(&markdown_map));
        input.cursor_row = 2;

        let model = project(&input);

        assert_eq!(model.line_projections[0].display_text, "Before");
        assert_eq!(model.line_projections[1].display_text, "```rust");
        assert_eq!(model.line_projections[2].display_text, "*raw*");
        assert_eq!(model.line_projections[3].display_text, "```");
        assert_eq!(model.line_projections[4].display_text, "After");
    }

    #[test]
    fn active_markdown_projection_keeps_entire_cursor_table_block_raw() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let source = "| A | B |\n|---|---|\n| *x* | y |\n# After\n";
        let bridge = CoreBridge::new(source).expect("core bridge");
        let mut snapshot = bridge.snapshot();
        snapshot.cursor_row = 2;
        let session_state = EditorSessionState::new(None);
        let markdown_map = MarkdownDocumentMap::parse(source);
        let mut input = ProjectionInput::new(&snapshot, &session_state, None)
            .with_markdown_document_map(Some(&markdown_map));
        input.cursor_row = 2;

        let model = project(&input);

        assert_eq!(model.line_projections[0].display_text, "| A | B |");
        assert_eq!(model.line_projections[1].display_text, "|---|---|");
        assert_eq!(model.line_projections[2].display_text, "| *x* | y |");
        assert_eq!(model.line_projections[3].display_text, "After");
    }

    #[test]
    fn active_markdown_projection_falls_back_to_cursor_row_raw_for_inline_only_markdown() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let source = "*active*\n*rich*\n";
        let bridge = CoreBridge::new(source).expect("core bridge");
        let mut snapshot = bridge.snapshot();
        snapshot.cursor_row = 0;
        let session_state = EditorSessionState::new(None);
        let markdown_map = MarkdownDocumentMap::parse(source);

        let model = project(
            &ProjectionInput::new(&snapshot, &session_state, None)
                .with_markdown_document_map(Some(&markdown_map)),
        );

        assert_eq!(model.line_projections[0].display_text, "*active*");
        assert_eq!(model.line_projections[1].display_text, "rich");
    }

    #[test]
    fn active_markdown_projection_tracks_cursor_movement_between_raw_rows() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let source = "# Title\n*body*\n# After\n";
        let mut bridge = CoreBridge::new(source).expect("core bridge");
        let session_state = EditorSessionState::new(None);
        let markdown_map = MarkdownDocumentMap::parse(source);

        let initial_snapshot = bridge.snapshot();
        assert_eq!(initial_snapshot.cursor_row, 0);
        let initial_model = project(
            &ProjectionInput::new(&initial_snapshot, &session_state, None)
                .with_markdown_document_map(Some(&markdown_map)),
        );

        assert_eq!(initial_model.line_projections[0].display_text, "# Title");
        assert_eq!(initial_model.line_projections[1].display_text, "body");
        assert_eq!(initial_model.line_projections[2].display_text, "After");

        bridge
            .dispatch_key("j")
            .expect("move cursor to inline Markdown row");
        let moved_snapshot = bridge.snapshot();
        assert_eq!(moved_snapshot.cursor_row, 1);
        let moved_model = project(
            &ProjectionInput::new(&moved_snapshot, &session_state, None)
                .with_markdown_document_map(Some(&markdown_map)),
        );

        assert_eq!(moved_model.line_projections[0].display_text, "Title");
        assert_eq!(moved_model.line_projections[1].display_text, "*body*");
        assert_eq!(moved_model.line_projections[2].display_text, "After");
    }

    #[test]
    fn inactive_markdown_projection_keeps_all_rows_rich_even_at_cursor_block() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let source = "# Title\n*body*\n";
        let bridge = CoreBridge::new(source).expect("core bridge");
        let mut snapshot = bridge.snapshot();
        snapshot.cursor_row = 0;
        let session_state = EditorSessionState::new(None);
        let markdown_map = MarkdownDocumentMap::parse(source);
        let mut input = ProjectionInput::new(&snapshot, &session_state, None)
            .with_markdown_document_map(Some(&markdown_map));
        input.is_active = false;

        let model = project(&input);

        assert_eq!(model.line_projections[0].display_text, "Title");
        assert_eq!(model.line_projections[1].display_text, "body");
    }

    #[test]
    fn inactive_markdown_projection_keeps_list_item_rich_even_at_cursor_row() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let source = "- [x] done\n# Next\n";
        let bridge = CoreBridge::new(source).expect("core bridge");
        let mut snapshot = bridge.snapshot();
        snapshot.cursor_row = 0;
        let session_state = EditorSessionState::new(None);
        let markdown_map = MarkdownDocumentMap::parse(source);
        let mut input = ProjectionInput::new(&snapshot, &session_state, None)
            .with_markdown_document_map(Some(&markdown_map));
        input.is_active = false;

        let model = project(&input);

        assert_eq!(model.line_projections[0].display_text, "• ✅ done");
        assert_eq!(model.line_projections[1].display_text, "Next");
    }

    #[test]
    fn markdown_projection_keeps_all_rows_raw_when_markdown_render_is_disabled() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let source = "# Title\n- [x] done\n*tail*\n";
        let bridge = CoreBridge::new(source).expect("core bridge");
        let snapshot = bridge.snapshot();
        let mut session_state = EditorSessionState::new(None);
        session_state
            .apply_presentation_option(
                crate::option_registry::SayaOptionName::MarkdownRender,
                crate::option_registry::SayaOptionValue::Boolean(false),
            )
            .expect("markdownrender option should apply");
        let markdown_map = MarkdownDocumentMap::parse(source);

        let model = project(
            &ProjectionInput::new(&snapshot, &session_state, None)
                .with_markdown_document_map(Some(&markdown_map)),
        );

        assert_eq!(
            model
                .line_projections
                .iter()
                .map(|line| line.display_text.as_str())
                .collect::<Vec<_>>(),
            vec!["# Title", "- [x] done", "*tail*", ""],
            "disabled Markdown rendering should keep raw Markdown even when metadata is present"
        );
    }

    #[test]
    fn dirty_and_cursor_update_on_redraw() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("line1\nline2\n").expect("core bridge");

        // 初回投影
        let snapshot1 = bridge.snapshot();
        let session_state = EditorSessionState::new(None);
        let model1 = project(&ProjectionInput::new(&snapshot1, &session_state, None));
        assert!(!model1.dirty);
        assert_eq!(model1.cursor_row, 0);

        // 編集操作後に再投影
        bridge.dispatch_key("j").expect("move down");
        bridge.dispatch_key("i").expect("insert mode");
        bridge.dispatch_key("Z").expect("insert Z");
        bridge.dispatch_key("\x1b").expect("normal mode");

        let snapshot2 = bridge.snapshot();
        let model2 = project(&ProjectionInput::new(&snapshot2, &session_state, None));

        assert!(model2.dirty, "編集後の再描画では dirty=true");
        assert_eq!(model2.cursor_row, 1, "カーソル行が再描画で追随すること");
    }

    // ---- タスク 6.3: message line を描画モデルへ取り込むテスト ----

    #[test]
    fn projects_no_message_line_in_normal_state() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("text\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(model.message_line, None, "通常状態ではメッセージ欄は空");
    }

    #[test]
    fn projects_save_failure_as_message_line() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("text\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let mut session_state = EditorSessionState::new(None);
        session_state.record_save_failure("disk full".to_string());

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(
            model.message_line,
            Some("保存失敗: disk full".to_string()),
            "保存失敗メッセージが message_line に反映されること"
        );
    }

    #[test]
    fn projects_transient_message_over_save_error_in_message_line() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("text\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let mut session_state = EditorSessionState::new(None);
        session_state.record_save_failure("old error".to_string());

        let model = project(&ProjectionInput::new(
            &snapshot,
            &session_state,
            Some("未保存の変更があります"),
        ));

        assert_eq!(
            model.message_line,
            Some("未保存の変更があります".to_string()),
            "transient_message が save error より優先されること"
        );
    }

    #[test]
    fn projects_unsaved_warning_as_transient_message() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("text\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let model = project(&ProjectionInput::new(
            &snapshot,
            &session_state,
            Some("未保存の変更があります。:q! で強制終了できます"),
        ));

        assert_eq!(
            model.message_line,
            Some("未保存の変更があります。:q! で強制終了できます".to_string()),
            "未保存警告が transient_message として投影されること"
        );
    }

    #[test]
    fn projects_config_failure_as_transient_message() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("text\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let model = project(&ProjectionInput::new(
            &snapshot,
            &session_state,
            Some("設定の読み込みに失敗しました"),
        ));

        assert_eq!(
            model.message_line,
            Some("設定の読み込みに失敗しました".to_string()),
            "設定失敗メッセージが投影されること"
        );
    }

    #[test]
    fn resolves_message_state_by_fixed_priority_order() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("text\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let priority_input = ProjectionInput::new(&snapshot, &session_state, Some("transient"))
            .with_command_preview(Some("/pattern"))
            .with_core_message(Some("core warning"))
            .with_system_warning(Some("system warning"));
        let resolved = resolve_message_state(&priority_input)
            .expect("command preview should win over all other messages");
        assert_eq!(resolved.kind, ScreenMessageKind::CommandPreview);
        assert_eq!(resolved.text, "/pattern");

        let core_first = ProjectionInput::new(&snapshot, &session_state, Some("transient"))
            .with_core_message(Some("core warning"))
            .with_system_warning(Some("system warning"));
        let resolved = resolve_message_state(&core_first)
            .expect("core message should win when no command preview exists");
        assert_eq!(resolved.kind, ScreenMessageKind::CoreMessage);
        assert_eq!(resolved.text, "core warning");

        let system_first = ProjectionInput::new(&snapshot, &session_state, Some("transient"))
            .with_system_warning(Some("system warning"));
        let resolved = resolve_message_state(&system_first)
            .expect("system warning should win when no higher-priority message exists");
        assert_eq!(resolved.kind, ScreenMessageKind::SystemWarning);
        assert_eq!(resolved.text, "system warning");

        let transient_only = ProjectionInput::new(&snapshot, &session_state, Some("transient"));
        let resolved = resolve_message_state(&transient_only)
            .expect("transient info should be used as the fallback");
        assert_eq!(resolved.kind, ScreenMessageKind::TransientInfo);
        assert_eq!(resolved.text, "transient");
    }

    #[test]
    fn clears_message_line_after_save_success() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("text\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let mut session_state = EditorSessionState::new(None);

        // 保存失敗を記録
        session_state.record_save_failure("error".to_string());
        let model1 = project(&ProjectionInput::new(&snapshot, &session_state, None));
        assert!(model1.message_line.is_some());

        // 保存成功を記録
        session_state.record_save_success();
        let model2 = project(&ProjectionInput::new(&snapshot, &session_state, None));
        assert_eq!(
            model2.message_line, None,
            "保存成功後はメッセージ欄がクリアされること"
        );
    }

    #[test]
    fn success_message_takes_priority_when_provided_as_transient() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("text\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let model = project(&ProjectionInput::new(
            &snapshot,
            &session_state,
            Some("保存しました"),
        ));

        assert_eq!(
            model.message_line,
            Some("保存しました".to_string()),
            "成功メッセージが transient として投影されること"
        );
    }

    // ---- タスク 6.4: 行データとカーソルを terminal 描画へ流せるようにするテスト ----

    #[test]
    fn projects_text_lines_from_snapshot() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("line1\nline2\nline3\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(
            model.lines,
            vec!["line1", "line2", "line3"],
            "行データが snapshot から正しく分割されること"
        );
    }

    #[test]
    fn project_limits_visible_lines_to_body_height() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("line1\nline2\nline3\nline4\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let model =
            project(&ProjectionInput::new(&snapshot, &session_state, None).with_viewport(1, 2));

        assert_eq!(model.lines, vec!["line2", "line3"]);
    }

    #[test]
    fn projects_cursor_coordinates_as_u16() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("abcdef\nghijkl\n").expect("core bridge");
        bridge.dispatch_key("jlll").expect("move to row=1, col=3");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(model.cursor_row, 1, "カーソル行が u16 として正しく変換");
        assert_eq!(model.cursor_col, 3, "カーソル列が u16 として正しく変換");
    }

    #[test]
    fn projects_display_cursor_col_for_multibyte_character() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("あa\n").expect("core bridge");
        bridge
            .dispatch_key("l")
            .expect("move right over multibyte char");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        assert_eq!(
            snapshot.cursor_col, 3,
            "vim-core-rs の cursor_col は UTF-8 バイト位置で進むこと"
        );

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(model.cursor_row, 0, "行位置はそのまま反映されること");
        assert_eq!(
            model.cursor_col, 2,
            "全角 1 文字ぶんは terminal 上で 2 セルとして描画されること"
        );
    }

    #[test]
    fn projects_cursor_col_with_line_number_prefix() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("alpha\nbeta\n").expect("core bridge");
        bridge.dispatch_key("jll").expect("move to row=1, col=2");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new_with_tab_size_and_line_numbers_and_number_width(
            None, 8, true, 4,
        );

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(model.lines[1], "   2 beta");
        assert_eq!(model.cursor_row, 1);
        assert_eq!(
            model.cursor_col, 7,
            "行番号と区切り分だけ右へ補正されること"
        );
    }

    #[test]
    fn projects_cursor_col_with_line_numbers_and_multibyte_text() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("あa\n").expect("core bridge");
        bridge
            .dispatch_key("l")
            .expect("move right over multibyte char");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new_with_tab_size_and_line_numbers_and_number_width(
            None, 8, true, 4,
        );

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(model.lines[0], "   1 あa");
        assert_eq!(
            model.cursor_col, 7,
            "全角表示幅に行番号オフセットが加算されること"
        );
    }

    #[test]
    fn projects_line_numbers_using_configured_minimum_width() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("alpha\nbeta\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new_with_tab_size_and_line_numbers_and_number_width(
            None, 8, true, 4,
        );

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(model.lines[0], "   1 alpha");
        assert_eq!(model.lines[1], "   2 beta");
    }

    #[test]
    fn projects_visual_selection_with_line_number_gutter_offset() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("alpha\nbeta\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new_with_tab_size_and_line_numbers_and_number_width(
            None, 8, true, 4,
        );
        let visual_selection = VisualSelection {
            mode: CoreMode::VisualLine,
            start_row: 0,
            start_col: 0,
            end_row: 1,
            end_col: 3,
        };

        let model = project(
            &ProjectionInput::new(&snapshot, &session_state, None)
                .with_visual_selection(Some(&visual_selection)),
        );
        let selection = model
            .visual_selection
            .expect("visual selection should be projected");

        assert_eq!(selection.start_col, 5);
        assert_eq!(selection.line_start_col, 5);
        assert_eq!(selection.end_col_exclusive, 9);
    }

    #[test]
    fn projects_visual_line_selection_as_full_lines() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("aaa\nbbbb\ncc\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);
        let visual_selection = VisualSelection {
            mode: CoreMode::VisualLine,
            start_row: 1,
            start_col: 3,
            end_row: 2,
            end_col: 0,
        };

        let model = project(
            &ProjectionInput::new(&snapshot, &session_state, None)
                .with_visual_selection(Some(&visual_selection)),
        );
        let selection = model
            .visual_selection
            .expect("visual selection should be projected");

        assert_eq!(selection.start_row, 1);
        assert_eq!(selection.start_col, 0);
        assert_eq!(selection.line_start_col, 0);
        assert_eq!(selection.end_row, 2);
        assert_eq!(selection.end_col_exclusive, 2);
    }

    #[test]
    fn projects_tab_as_spaces_using_default_tab_size() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("\ta\n").expect("core bridge");
        bridge.dispatch_key("l").expect("move right over tab");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(model.lines[0], "        a");
        assert_eq!(model.cursor_col, 8);
    }

    #[test]
    fn projects_tab_using_configured_tab_size() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("\ta\n").expect("core bridge");
        bridge.dispatch_key("l").expect("move right over tab");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new_with_tab_size(None, 4);

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(model.lines[0], "    a");
        assert_eq!(model.cursor_col, 4);
    }

    #[test]
    fn screen_model_contains_all_draw_fields() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("first\nsecond\n").expect("core bridge");
        bridge.dispatch_key("i").expect("insert mode");
        bridge.dispatch_key("X").expect("insert X");
        bridge.dispatch_key("\x1b").expect("normal mode");
        let snapshot = bridge.snapshot();

        let session_state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));

        let model = project(&ProjectionInput::new(
            &snapshot,
            &session_state,
            Some("テストメッセージ"),
        ));

        // 全フィールドがまとめて draw に必要なデータを持つこと
        assert!(!model.file_name.is_empty(), "ファイル名は空でないこと");
        assert!(!model.mode_label.is_empty(), "モードラベルは空でないこと");
        assert!(model.dirty, "編集後は dirty=true であること");
        assert!(!model.lines.is_empty(), "行データは空でないこと");
        assert!(model.message_line.is_some(), "メッセージ欄が存在すること");
    }

    #[test]
    fn redraw_input_limited_to_screen_model_only() {
        // ScreenModel だけで描画に必要な全情報が揃うことを型レベルで検証
        let model = ScreenModel {
            window_id: 1,
            buffer_id: 1,
            rect: PaneRect {
                x: 0,
                y: 0,
                width: 1,
                height: 2,
            },
            file_name: "test.txt".to_string(),
            mode_label: "NORMAL".to_string(),
            cursor_style: ScreenCursorStyle::Block,
            dirty: false,
            lines: vec!["hello".to_string()],
            line_projections: vec![],
            cursor_row: 0,
            cursor_col: 0,
            visual_selection: None,
            search_overlays: vec![],
            syntax_chunks: vec![],
            markdown_style_ranges: vec![],
            message_line: None,
            command_cursor_col: None,
            is_active: true,
        };

        // ScreenModel の各フィールドにアクセスできること（コンパイル時検証）
        let _ = &model.file_name;
        let _ = &model.mode_label;
        let _ = model.dirty;
        let _ = &model.lines;
        let _ = model.cursor_row;
        let _ = model.cursor_col;
        let _ = &model.visual_selection;
        let _ = &model.message_line;

        // CoreSnapshot への直接参照は不要（型の独立性）
        assert_eq!(
            model.mode_label, "NORMAL",
            "ScreenModel だけで描画に必要な全情報が揃うこと"
        );
    }

    #[test]
    fn projects_search_overlay_with_tabs_wide_glyphs_and_gutter_offset() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("\tあx\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let active_window_id = snapshot
            .active_window_id()
            .expect("active window should exist");
        let session_state = EditorSessionState::new_with_tab_size_and_line_numbers_and_number_width(
            None, 4, true, 4,
        );
        let search_state = SearchVisibleState {
            capability: SearchCapabilityContract::baseline_ready_contract(),
            window_id: active_window_id,
            visible_rows: SearchVisibleRows {
                start_row: 1,
                end_row: 1,
            },
            mode: SearchQueryMode::Hlsearch,
            pattern: Some("あ".to_string()),
            input_pattern: None,
            hlsearch_enabled: true,
            hlsearch_suspended: false,
            incsearch_active: false,
            matches: vec![SearchMatch {
                kind: SearchMatchKind::Current,
                start_row: 1,
                start_col: 1,
                end_row: 1,
                end_col: 4,
            }],
        };

        let model = project(
            &ProjectionInput::new(&snapshot, &session_state, None)
                .with_search_state(Some(&search_state)),
        );

        assert_eq!(
            model.search_overlays,
            vec![ScreenSearchOverlay {
                row: 0,
                start_col: 9,
                end_col_exclusive: 11,
                kind: SearchMatchKind::Current,
            }],
            "tab と全角文字と行番号オフセットを display-space に正しく投影すること"
        );
    }

    #[test]
    fn projects_search_overlay_against_markdown_rich_projection_with_gutter_offset() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let source = "# Title\n";
        let bridge = CoreBridge::new(source).expect("core bridge");
        let snapshot = bridge.snapshot();
        let active_window_id = snapshot
            .active_window_id()
            .expect("active window should exist");
        let session_state = EditorSessionState::new_with_tab_size_and_line_numbers_and_number_width(
            None, 8, true, 4,
        );
        let markdown_map = MarkdownDocumentMap::parse(source);
        let search_state = SearchVisibleState {
            capability: SearchCapabilityContract::baseline_ready_contract(),
            window_id: active_window_id,
            visible_rows: SearchVisibleRows {
                start_row: 1,
                end_row: 1,
            },
            mode: SearchQueryMode::Hlsearch,
            pattern: Some("Title".to_string()),
            input_pattern: None,
            hlsearch_enabled: true,
            hlsearch_suspended: false,
            incsearch_active: false,
            matches: vec![SearchMatch {
                kind: SearchMatchKind::Current,
                start_row: 1,
                start_col: 2,
                end_row: 1,
                end_col: 7,
            }],
        };
        let mut input = ProjectionInput::new(&snapshot, &session_state, None)
            .with_markdown_document_map(Some(&markdown_map))
            .with_search_state(Some(&search_state));
        input.is_active = false;

        let model = project(&input);

        assert_eq!(model.lines[0], "   1 # Title");
        assert_eq!(model.line_projections[0].line_start_col, 5);
        assert_eq!(model.line_projections[0].display_text, "Title");
        assert_eq!(
            model.search_overlays,
            vec![ScreenSearchOverlay {
                row: 0,
                start_col: 5,
                end_col_exclusive: 10,
                kind: SearchMatchKind::Current,
            }],
            "search overlays should use Markdown projection display-space, not raw marker columns"
        );
    }

    #[test]
    fn projects_search_overlay_clips_to_visible_rows_and_keeps_match_kinds() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("zero\nalpha\nbeta\nomega\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let active_window_id = snapshot
            .active_window_id()
            .expect("active window should exist");
        let session_state = EditorSessionState::new(None);
        let search_state = SearchVisibleState {
            capability: SearchCapabilityContract::baseline_ready_contract(),
            window_id: active_window_id,
            visible_rows: SearchVisibleRows {
                start_row: 2,
                end_row: 3,
            },
            hlsearch_enabled: true,
            hlsearch_suspended: false,
            incsearch_active: false,
            mode: SearchQueryMode::Hlsearch,
            pattern: Some("a".to_string()),
            input_pattern: None,
            matches: vec![
                SearchMatch {
                    kind: SearchMatchKind::Regular,
                    start_row: 2,
                    start_col: 0,
                    end_row: 2,
                    end_col: 5,
                },
                SearchMatch {
                    kind: SearchMatchKind::Current,
                    start_row: 3,
                    start_col: 1,
                    end_row: 3,
                    end_col: 4,
                },
                SearchMatch {
                    kind: SearchMatchKind::Regular,
                    start_row: 4,
                    start_col: 0,
                    end_row: 4,
                    end_col: 5,
                },
            ],
        };

        let model = project(
            &ProjectionInput::new(&snapshot, &session_state, None)
                .with_search_state(Some(&search_state))
                .with_viewport(1, 2),
        );

        assert_eq!(
            model.search_overlays,
            vec![
                ScreenSearchOverlay {
                    row: 0,
                    start_col: 0,
                    end_col_exclusive: 5,
                    kind: SearchMatchKind::Regular,
                },
                ScreenSearchOverlay {
                    row: 1,
                    start_col: 1,
                    end_col_exclusive: 4,
                    kind: SearchMatchKind::Current,
                }
            ],
            "visible rows のみが投影され、current match が区別されること"
        );
    }

    #[test]
    fn ignores_search_overlay_for_different_window_id() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("alpha\nbeta\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);
        let search_state = SearchVisibleState {
            capability: SearchCapabilityContract::baseline_ready_contract(),
            window_id: 999_999,
            visible_rows: SearchVisibleRows {
                start_row: 1,
                end_row: 1,
            },
            hlsearch_enabled: true,
            hlsearch_suspended: false,
            incsearch_active: false,
            mode: SearchQueryMode::Hlsearch,
            pattern: Some("alpha".to_string()),
            input_pattern: None,
            matches: vec![SearchMatch {
                kind: SearchMatchKind::Current,
                start_row: 1,
                start_col: 0,
                end_row: 1,
                end_col: 5,
            }],
        };

        let model = project(
            &ProjectionInput::new(&snapshot, &session_state, None)
                .with_search_state(Some(&search_state)),
        );

        assert!(
            model.search_overlays.is_empty(),
            "別 window の search overlay は投影しないこと"
        );
    }

    #[test]
    fn projection_input_keeps_explicit_failure_when_snapshot_has_no_active_window() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("alpha\nbeta\ngamma\n").expect("core bridge");
        bridge
            .apply_ex_command(":split")
            .expect("split should succeed");
        let mut snapshot = bridge.snapshot();
        for (index, window) in snapshot.windows.iter_mut().enumerate() {
            window.id = 41 + i32::try_from(index).expect("window index fits in i32");
            window.is_active = false;
        }
        snapshot.cursor_row = 7;
        snapshot.cursor_col = 11;
        let session_state = EditorSessionState::new(None);

        let input = ProjectionInput::new(&snapshot, &session_state, None);

        assert_eq!(
            input.window_id, 0,
            "active_window_id() が取れない snapshot では first window を採用せず explicit failure を保つこと"
        );
        assert_eq!(
            input.buffer_id, 0,
            "active window が解決できない場合は first window の buffer を流用しないこと"
        );
        assert_eq!(
            input.rect,
            PaneRect::default(),
            "active window が解決できない場合は geometry fallback を作らないこと"
        );
        assert_eq!(
            input.cursor_row, snapshot.cursor_row,
            "global cursor は snapshot の active cursor contract をそのまま使うこと"
        );
        assert_eq!(input.cursor_col, snapshot.cursor_col);
    }

    #[test]
    fn workspace_projection_does_not_infer_active_pane_from_windows_scan() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("alpha\nbeta\ngamma\n").expect("core bridge");
        bridge
            .apply_ex_command(":split")
            .expect("split should succeed");
        let mut snapshot = bridge.snapshot();
        for (index, window) in snapshot.windows.iter_mut().enumerate() {
            window.id = 71 + i32::try_from(index).expect("window index fits in i32");
            window.is_active = false;
        }
        let session_state = EditorSessionState::new(None);
        let viewport_store = WindowViewportStore::new();
        let search_states = BTreeMap::new();
        let syntax_lines = BTreeMap::new();
        let markdown_document_maps = BTreeMap::new();

        let result = project_workspace(&WorkspaceProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            visual_selection: None,
            search_states: &search_states,
            syntax_lines: &syntax_lines,
            #[cfg(feature = "tree-sitter-syntax")]
            tree_sitter_syntax: &BTreeMap::new(),
            markdown_document_maps: &markdown_document_maps,
            command_preview: None,
            core_message: None,
            notification_prompt: None,
            system_warning: None,
            transient_info: None,
            viewport_store: &viewport_store,
            terminal_width: 80,
            terminal_height: 24,
        });

        assert_eq!(
            result,
            Err(WorkspaceProjectionError::ActiveWindowMissing),
            "active_window_id() が None のときは windows 走査で active pane を推測しないこと"
        );
    }

    #[test]
    fn workspace_projection_uses_snapshot_cursor_for_active_pane_and_window_cursor_for_inactive_pane()
     {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("alpha\nbeta\ngamma\ndelta\n").expect("core bridge");
        bridge
            .apply_ex_command(":split")
            .expect("split should succeed");
        let mut snapshot = bridge.snapshot();
        let active_window_id = snapshot
            .active_window_id()
            .expect("split snapshot should have an active window");
        let mut active_window_snapshot = snapshot
            .window(active_window_id)
            .expect("active window should exist")
            .clone();
        let mut inactive_window_snapshot = snapshot
            .windows
            .iter()
            .find(|window| window.id != active_window_id)
            .expect("inactive window should exist")
            .clone();
        snapshot.cursor_row = 2;
        snapshot.cursor_col = 2;
        active_window_snapshot.cursor_row = 4;
        active_window_snapshot.cursor_col = 1;
        inactive_window_snapshot.cursor_row = 3;
        inactive_window_snapshot.cursor_col = 3;
        snapshot.windows = vec![active_window_snapshot, inactive_window_snapshot];
        let session_state = EditorSessionState::new(None);
        let viewport_store = WindowViewportStore::new();
        let search_states = BTreeMap::new();
        let syntax_lines = BTreeMap::new();
        let markdown_document_maps = BTreeMap::new();

        let model = project_workspace(&WorkspaceProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            visual_selection: None,
            search_states: &search_states,
            syntax_lines: &syntax_lines,
            #[cfg(feature = "tree-sitter-syntax")]
            tree_sitter_syntax: &BTreeMap::new(),
            markdown_document_maps: &markdown_document_maps,
            command_preview: None,
            core_message: None,
            notification_prompt: None,
            system_warning: None,
            transient_info: None,
            viewport_store: &viewport_store,
            terminal_width: 80,
            terminal_height: 24,
        })
        .expect("workspace projection should still build for split snapshots");

        let active_pane = model
            .panes
            .iter()
            .find(|pane| pane.window_id == active_window_id)
            .expect("active pane should exist");
        let inactive_pane = model
            .panes
            .iter()
            .find(|pane| pane.window_id != active_window_id)
            .expect("inactive pane should exist");

        assert_eq!(
            active_pane.cursor_row, 2,
            "active pane は snapshot 全体の cursor_row を使うこと"
        );
        assert_eq!(
            active_pane.cursor_col, 2,
            "active pane は snapshot 全体の cursor_col を使うこと"
        );
        assert_eq!(
            inactive_pane.cursor_row, 3,
            "inactive pane は window metadata の cursor_row を使うこと"
        );
        assert_eq!(
            inactive_pane.cursor_col, 3,
            "inactive pane は window metadata の cursor_col を使うこと"
        );
    }

    #[test]
    fn workspace_projection_keeps_full_height_when_global_rows_are_empty() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("alpha\nbeta\ngamma\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);
        let viewport_store = WindowViewportStore::new();
        let search_states = BTreeMap::new();
        let syntax_lines = BTreeMap::new();
        let markdown_document_maps = BTreeMap::new();

        let model = project_workspace(&WorkspaceProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            visual_selection: None,
            search_states: &search_states,
            syntax_lines: &syntax_lines,
            #[cfg(feature = "tree-sitter-syntax")]
            tree_sitter_syntax: &BTreeMap::new(),
            markdown_document_maps: &markdown_document_maps,
            command_preview: None,
            core_message: None,
            notification_prompt: None,
            system_warning: None,
            transient_info: None,
            viewport_store: &viewport_store,
            terminal_width: 80,
            terminal_height: 24,
        })
        .expect("workspace projection should succeed");

        let expected_height = snapshot.windows[0].height as u16;
        assert_eq!(
            model.panes[0].rect.height, expected_height,
            "message/command が空なら host が pane height を余計に削らないこと"
        );
    }

    #[test]
    fn workspace_projection_passes_markdown_maps_into_pane_line_projections() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let source = "# Title\n";
        let bridge = CoreBridge::new(source).expect("core bridge");
        let snapshot = bridge.snapshot();
        let window_id = snapshot.windows[0].id;
        let session_state = EditorSessionState::new(None);
        let viewport_store = WindowViewportStore::new();
        let search_states = BTreeMap::new();
        let syntax_lines = BTreeMap::new();
        let mut markdown_document_maps = BTreeMap::new();
        markdown_document_maps.insert(window_id, Arc::new(MarkdownDocumentMap::parse(source)));

        let model = project_workspace(&WorkspaceProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            visual_selection: None,
            search_states: &search_states,
            syntax_lines: &syntax_lines,
            #[cfg(feature = "tree-sitter-syntax")]
            tree_sitter_syntax: &BTreeMap::new(),
            markdown_document_maps: &markdown_document_maps,
            command_preview: None,
            core_message: None,
            notification_prompt: None,
            system_warning: None,
            transient_info: None,
            viewport_store: &viewport_store,
            terminal_width: 80,
            terminal_height: 24,
        })
        .expect("workspace projection should succeed");

        let pane = model
            .panes
            .iter()
            .find(|pane| pane.window_id == window_id)
            .expect("pane should exist");
        assert_eq!(pane.line_projections[0].raw_text, "# Title");
        assert_eq!(
            pane.line_projections[0].display_text, "# Title",
            "workspace projection should pass the per-window markdown map and active cursor block should remain raw"
        );
    }

    #[test]
    fn workspace_projection_keeps_active_markdown_raw_expansion_out_of_inactive_panes() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let source = "# Title\n*body*\n";
        let mut bridge = CoreBridge::new(source).expect("core bridge");
        bridge
            .apply_ex_command(":split")
            .expect("split should succeed");
        let mut snapshot = bridge.snapshot();
        snapshot.cursor_row = 0;
        let active_window_id = snapshot
            .active_window_id()
            .expect("split snapshot should have an active window");
        for window in &mut snapshot.windows {
            window.cursor_row = 0;
        }
        let session_state = EditorSessionState::new(None);
        let viewport_store = WindowViewportStore::new();
        let search_states = BTreeMap::new();
        let syntax_lines = BTreeMap::new();
        let markdown_map = Arc::new(MarkdownDocumentMap::parse(source));
        let markdown_document_maps = snapshot
            .windows
            .iter()
            .map(|window| (window.id, Arc::clone(&markdown_map)))
            .collect::<BTreeMap<_, _>>();

        let model = project_workspace(&WorkspaceProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            visual_selection: None,
            search_states: &search_states,
            syntax_lines: &syntax_lines,
            #[cfg(feature = "tree-sitter-syntax")]
            tree_sitter_syntax: &BTreeMap::new(),
            markdown_document_maps: &markdown_document_maps,
            command_preview: None,
            core_message: None,
            notification_prompt: None,
            system_warning: None,
            transient_info: None,
            viewport_store: &viewport_store,
            terminal_width: 80,
            terminal_height: 24,
        })
        .expect("workspace projection should succeed");

        let active_pane = model
            .panes
            .iter()
            .find(|pane| pane.window_id == active_window_id)
            .expect("active pane should exist");
        let inactive_pane = model
            .panes
            .iter()
            .find(|pane| pane.window_id != active_window_id)
            .expect("inactive pane should exist");

        assert_eq!(active_pane.line_projections[0].display_text, "# Title");
        assert_eq!(inactive_pane.line_projections[0].display_text, "Title");
    }

    #[test]
    fn workspace_projection_without_markdown_map_keeps_raw_display_projection() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let source = "# Title\n";
        let bridge = CoreBridge::new(source).expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);
        let viewport_store = WindowViewportStore::new();
        let search_states = BTreeMap::new();
        let syntax_lines = BTreeMap::new();
        let markdown_document_maps = BTreeMap::new();

        let model = project_workspace(&WorkspaceProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            visual_selection: None,
            search_states: &search_states,
            syntax_lines: &syntax_lines,
            #[cfg(feature = "tree-sitter-syntax")]
            tree_sitter_syntax: &BTreeMap::new(),
            markdown_document_maps: &markdown_document_maps,
            command_preview: None,
            core_message: None,
            notification_prompt: None,
            system_warning: None,
            transient_info: None,
            viewport_store: &viewport_store,
            terminal_width: 80,
            terminal_height: 24,
        })
        .expect("workspace projection should succeed");

        assert_eq!(model.panes[0].line_projections[0].raw_text, "# Title");
        assert_eq!(
            model.panes[0].line_projections[0].display_text, "# Title",
            "without a markdown map, display projection should remain raw text"
        );
    }

    #[test]
    fn workspace_projection_reserves_only_one_row_for_command_line_without_message() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("alpha\nbeta\ngamma\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);
        let viewport_store = WindowViewportStore::new();
        let search_states = BTreeMap::new();
        let syntax_lines = BTreeMap::new();
        let markdown_document_maps = BTreeMap::new();

        let model = project_workspace(&WorkspaceProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            visual_selection: None,
            search_states: &search_states,
            syntax_lines: &syntax_lines,
            #[cfg(feature = "tree-sitter-syntax")]
            tree_sitter_syntax: &BTreeMap::new(),
            markdown_document_maps: &markdown_document_maps,
            command_preview: Some(":w"),
            core_message: None,
            notification_prompt: None,
            system_warning: None,
            transient_info: None,
            viewport_store: &viewport_store,
            terminal_width: 80,
            terminal_height: 24,
        })
        .expect("workspace projection should succeed");

        let expected_height = snapshot.windows[0].height as u16;
        assert_eq!(
            model.panes[0].rect.height, expected_height,
            "command line だけの時も message row を重複予約せず core の pane height を保つこと"
        );
        assert_eq!(model.visible_message_text(), None);
    }

    #[test]
    fn workspace_message_line_state_preserves_suppressed_notifications_while_command_preview_is_active()
     {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("alpha\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);
        let viewport_store = WindowViewportStore::new();
        let search_states = BTreeMap::new();
        let syntax_lines = BTreeMap::new();
        let markdown_document_maps = BTreeMap::new();
        let input = WorkspaceProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            visual_selection: None,
            search_states: &search_states,
            syntax_lines: &syntax_lines,
            #[cfg(feature = "tree-sitter-syntax")]
            tree_sitter_syntax: &BTreeMap::new(),
            markdown_document_maps: &markdown_document_maps,
            command_preview: Some(":%s/foo/bar"),
            core_message: Some("core note"),
            notification_prompt: None,
            system_warning: Some("system warning"),
            transient_info: Some("saved"),
            viewport_store: &viewport_store,
            terminal_width: 80,
            terminal_height: 24,
        };

        let state = resolve_workspace_message_line_state(&input);
        assert_eq!(
            state.visible_source(),
            Some(MessageLineSource::CommandPreview)
        );
        assert_eq!(state.visible_text(), Some(":%s/foo/bar"));
        assert_eq!(
            state.suppressed_sources(),
            vec![
                MessageLineSource::SystemWarning,
                MessageLineSource::CoreNotification,
                MessageLineSource::TransientInfo,
            ]
        );

        let model = project_workspace(&input).expect("workspace projection should succeed");
        assert_eq!(
            model.command_line.as_ref().map(|line| line.text.as_str()),
            Some(":%s/foo/bar")
        );
        assert_eq!(model.visible_message_text(), None);
    }

    #[test]
    fn workspace_message_line_state_distinguishes_system_warning_from_core_notification() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("alpha\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);
        let viewport_store = WindowViewportStore::new();
        let search_states = BTreeMap::new();
        let syntax_lines = BTreeMap::new();
        let markdown_document_maps = BTreeMap::new();
        let input = WorkspaceProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            visual_selection: None,
            search_states: &search_states,
            syntax_lines: &syntax_lines,
            #[cfg(feature = "tree-sitter-syntax")]
            tree_sitter_syntax: &BTreeMap::new(),
            markdown_document_maps: &markdown_document_maps,
            command_preview: None,
            core_message: Some("shared text"),
            notification_prompt: None,
            system_warning: Some("shared text"),
            transient_info: None,
            viewport_store: &viewport_store,
            terminal_width: 80,
            terminal_height: 24,
        };

        let state = resolve_workspace_message_line_state(&input);
        assert_eq!(
            state.visible_source(),
            Some(MessageLineSource::SystemWarning)
        );
        assert_eq!(state.visible_text(), Some("shared text"));
        assert_eq!(
            state.suppressed_sources(),
            vec![MessageLineSource::CoreNotification]
        );

        let model = project_workspace(&input).expect("workspace projection should succeed");
        assert_eq!(model.visible_message_text(), Some("shared text"));
        assert_eq!(model.command_line, None);
    }

    #[test]
    fn workspace_projection_summary_reports_windows_active_pane_geometry_and_visible_buffers() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("alpha\nbeta\ngamma\ndelta\n").expect("core bridge");
        bridge
            .apply_ex_command(":split")
            .expect("split should succeed");
        let snapshot = bridge.snapshot();
        let active_window_id = snapshot
            .active_window_id()
            .expect("split snapshot should have an active window");
        let session_state = EditorSessionState::new(None);
        let viewport_store = WindowViewportStore::new();
        let search_states = BTreeMap::new();
        let syntax_lines = BTreeMap::new();
        let markdown_document_maps = BTreeMap::new();

        let model = project_workspace(&WorkspaceProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            visual_selection: None,
            search_states: &search_states,
            syntax_lines: &syntax_lines,
            #[cfg(feature = "tree-sitter-syntax")]
            tree_sitter_syntax: &BTreeMap::new(),
            markdown_document_maps: &markdown_document_maps,
            command_preview: None,
            core_message: None,
            notification_prompt: None,
            system_warning: None,
            transient_info: None,
            viewport_store: &viewport_store,
            terminal_width: 80,
            terminal_height: 24,
        })
        .expect("workspace projection should succeed before summary is built");

        let summary = model.projection_summary();
        let expected_window_ids = model
            .panes
            .iter()
            .map(|pane| pane.window_id)
            .collect::<Vec<_>>();
        let expected_geometry = model
            .panes
            .iter()
            .map(|pane| PaneProjectionGeometry {
                window_id: pane.window_id,
                rect: pane.rect,
            })
            .collect::<Vec<_>>();
        let expected_visible_buffers = model
            .panes
            .iter()
            .map(|pane| pane.buffer_id)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();

        assert_eq!(summary.window_ids, expected_window_ids);
        assert_eq!(summary.active_window_id, active_window_id);
        assert_eq!(summary.pane_geometry, expected_geometry);
        assert_eq!(summary.visible_buffer_ids, expected_visible_buffers);
    }

    #[test]
    fn projection_summary_ignores_message_prompt_and_rollback_lifecycle_state() {
        let pane = ScreenModel {
            window_id: 11,
            buffer_id: 21,
            rect: PaneRect {
                x: 1,
                y: 2,
                width: 30,
                height: 10,
            },
            file_name: "summary.txt".to_string(),
            mode_label: "NORMAL".to_string(),
            cursor_style: ScreenCursorStyle::Block,
            dirty: false,
            lines: vec!["alpha".to_string()],
            line_projections: vec![],
            cursor_row: 0,
            cursor_col: 0,
            visual_selection: None,
            search_overlays: vec![],
            syntax_chunks: vec![],
            markdown_style_ranges: vec![],
            message_line: None,
            command_cursor_col: None,
            is_active: true,
        };
        let base = WorkspaceScreenModel {
            panes: vec![pane.clone()],
            active_window_id: 11,
            message_line: WorkspaceMessageLineState::default(),
            message_area_height: 5,
            message_scroll_offset: 0,
            prompt_line: None,
            pager_prompt: None,
            suppressed_prompt_hints: vec![],
            bell: None,
            command_line: None,
        };
        let with_prompt_and_messages = WorkspaceScreenModel {
            panes: vec![pane],
            active_window_id: 11,
            message_line: WorkspaceMessageLineState {
                visible: Some(MessageLineCandidate::legacy(
                    MessageLineSource::CoreNotification,
                    "visible message",
                )),
                suppressed: vec![MessageLineCandidate::legacy(
                    MessageLineSource::TransientInfo,
                    "hidden message",
                )],
            },
            message_area_height: 5,
            message_scroll_offset: 0,
            prompt_line: Some(InputPromptView {
                prompt: "prompt".to_string(),
                input: "typed".to_string(),
                correlation_id: 42,
                input_kind: CoreInputRequestKind::CommandLine,
                status: InputPromptStatus::Active,
            }),
            pager_prompt: None,
            suppressed_prompt_hints: vec![],
            bell: Some(BellIndication { count: 1 }),
            command_line: Some(CommandLineModel {
                text: ":write".to_string(),
                cursor_col: 6,
            }),
        };

        assert_eq!(
            base.projection_summary(),
            with_prompt_and_messages.projection_summary(),
            "projection summary は構造診断用なので message/prompt/rollback lifecycle を判断材料にしない"
        );
    }
}
