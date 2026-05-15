use serde_json::Value;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::floating_window::{
    FloatingAnchor, FloatingAnchorSignature, FloatingBorder, FloatingChrome, FloatingCloseEvents,
    FloatingFit, FloatingFocusId, FloatingInlineStyle, FloatingInlineStyleKind, FloatingLifecycle,
    FloatingOpenWithFocusOutcome, FloatingPlacement, FloatingRelativeTo, FloatingSize,
    FloatingWindowId, FloatingWindowManager, FloatingZIndex,
};
use crate::input_router::KeyInput;
use crate::markdown_render::{
    InlineStyle as MarkdownInlineStyle, InlineStyleKind as MarkdownInlineStyleKind,
    RenderedFloatContent, render_markdown_to_float_content, render_plaintext_to_float_content,
    wrap_rendered_content_to_width,
};

const LSP_HOVER_FOCUS_ID: &str = "lsp:hover";
const LSP_DIAGNOSTIC_GROUP: &str = "lsp:diagnostic";
const LSP_LOCATION_GROUP: &str = "lsp:locations";
const LSP_SYMBOL_GROUP: &str = "lsp:symbols";
const MAX_FLOAT_WIDTH: u16 = 72;
const MAX_FLOAT_HEIGHT: u16 = 12;

/// LSP hover float の close 触発イベント集合。Neovim の
/// `close_events = { "CursorMoved", "ModeChanged", "BufLeave" }` 相当。
fn lsp_hover_close_events() -> FloatingCloseEvents {
    FloatingCloseEvents::none()
        .with_cursor_move()
        .with_mode_change()
        .with_window_leave()
}

/// LSP hover float の close キー集合（Neovim の `vim.lsp.buf.hover` の
/// "q で閉じる" 慣習に倣う）。focus 中の float でだけ有効化される。
fn lsp_hover_close_keys() -> Vec<KeyInput> {
    vec![KeyInput::Escape, KeyInput::Ctrl('['), KeyInput::Char('q')]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LspHoverOpenOutcome {
    /// 新規 hover float を開いた場合。`id` は新 float の識別子。
    Opened { id: FloatingWindowId },
    /// 同じカーソル位置で既に開いていた hover float に focus を移した
    /// 場合（Neovim 流の "2 回目 K で float に focus" 動作）。
    FocusedExisting { id: FloatingWindowId },
}

impl LspHoverOpenOutcome {
    pub fn id(&self) -> FloatingWindowId {
        match self {
            Self::Opened { id } | Self::FocusedExisting { id } => *id,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LspHoverFloatRequest {
    pub window_id: i32,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub response: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LspDiagnosticFloatRequest {
    pub window_id: i32,
    pub line: usize,
    pub column: usize,
    pub diagnostics: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LspLocationListRequest {
    pub window_id: i32,
    pub title: String,
    pub response: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LspSymbolOutlineRequest {
    pub window_id: i32,
    pub response: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspDiagnosticEntry {
    pub uri: Option<String>,
    pub line: usize,
    pub column: usize,
    pub message: String,
    pub severity: Option<u64>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LspDiagnosticStore {
    diagnostics: Vec<LspDiagnosticEntry>,
    cursor: Option<usize>,
}

impl LspDiagnosticStore {
    pub fn replace_from_lsp_value(&mut self, value: &Value) {
        self.diagnostics = diagnostic_entries_from_lsp_value(value);
        self.cursor = None;
        log::debug!(
            "[lsp_float] diagnostic store replaced: diagnostics={}",
            self.diagnostics.len()
        );
    }

    pub fn next_diagnostic(&mut self) -> Option<&LspDiagnosticEntry> {
        if self.diagnostics.is_empty() {
            log::debug!("[lsp_float] next diagnostic requested with empty store");
            return None;
        }
        let index = self
            .cursor
            .map(|index| (index + 1) % self.diagnostics.len())
            .unwrap_or(0);
        self.cursor = Some(index);
        log::debug!("[lsp_float] selected next diagnostic: index={index}");
        self.diagnostics.get(index)
    }

    pub fn previous_diagnostic(&mut self) -> Option<&LspDiagnosticEntry> {
        if self.diagnostics.is_empty() {
            log::debug!("[lsp_float] previous diagnostic requested with empty store");
            return None;
        }
        let index = self
            .cursor
            .map(|index| {
                if index == 0 {
                    self.diagnostics.len() - 1
                } else {
                    index - 1
                }
            })
            .unwrap_or_else(|| self.diagnostics.len() - 1);
        self.cursor = Some(index);
        log::debug!("[lsp_float] selected previous diagnostic: index={index}");
        self.diagnostics.get(index)
    }

    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

/// `markdown_render::InlineStyle` を float 層の `FloatingInlineStyle` に
/// 変換する。`markdown_render` は LSP / float の詳細を知らないため、
/// kind の対応付けはこの hover 専用ヘルパに閉じ込めて、saya コアと
/// markdown 表現の責務分離を守る。
fn floating_inline_styles_from_markdown(
    styles: Vec<MarkdownInlineStyle>,
) -> Vec<FloatingInlineStyle> {
    styles
        .into_iter()
        .map(|style| FloatingInlineStyle {
            kind: match style.kind {
                MarkdownInlineStyleKind::Code => FloatingInlineStyleKind::Code,
                MarkdownInlineStyleKind::Emphasis => FloatingInlineStyleKind::Emphasis,
                MarkdownInlineStyleKind::LinkText => FloatingInlineStyleKind::LinkText,
                MarkdownInlineStyleKind::LinkUrl => FloatingInlineStyleKind::LinkUrl,
            },
            line: style.line,
            column_start: style.column_start,
            column_end: style.column_end,
        })
        .collect()
}

/// hover float の内部表示幅（border 込み `MAX_FLOAT_WIDTH` から左右枠 2 列を引いた値）。
pub fn hover_float_inner_width() -> usize {
    usize::from(MAX_FLOAT_WIDTH.saturating_sub(2))
}

pub fn open_lsp_hover_float(
    manager: &mut FloatingWindowManager,
    request: LspHoverFloatRequest,
) -> Option<LspHoverOpenOutcome> {
    let rendered = wrap_rendered_content_to_width(
        render_lsp_hover_response(&request.response),
        hover_float_inner_width(),
    );
    if rendered.lines.is_empty() {
        log::debug!(
            "[lsp_float] hover response did not produce a float: window_id={}, cursor=({},{}), response={}",
            request.window_id,
            request.cursor_row,
            request.cursor_col,
            request.response
        );
        return None;
    }

    let size = size_for_lines(&rendered.lines);
    let RenderedFloatContent {
        lines: rendered_lines,
        inline_styles: markdown_styles,
    } = rendered;
    let floating_styles = floating_inline_styles_from_markdown(markdown_styles);
    log::debug!(
        "[lsp_float] opening hover float via focus toggle: window_id={}, cursor=({},{}), lines={}, inline_styles={}, size=({},{})",
        request.window_id,
        request.cursor_row,
        request.cursor_col,
        rendered_lines.len(),
        floating_styles.len(),
        size.width,
        size.height
    );
    let outcome = manager.open_static_lines_with_focus_toggle(
        rendered_lines,
        FloatingFocusId::new(LSP_HOVER_FOCUS_ID),
        FloatingAnchorSignature::cursor(request.window_id, request.cursor_row, request.cursor_col),
        FloatingLifecycle::CloseOnEvents(lsp_hover_close_events()),
        FloatingPlacement {
            relative_to: FloatingRelativeTo::Cursor {
                window_id: request.window_id,
            },
            anchor: FloatingAnchor::NorthWest,
            row: 1,
            col: 0,
            fit: FloatingFit::TruncateToGrid,
        },
        size,
        FloatingChrome {
            border: FloatingBorder::Single,
        },
        FloatingZIndex::Hover,
        true,
    );
    let hover_outcome = match outcome {
        FloatingOpenWithFocusOutcome::Opened { id } => {
            // 新規 hover float のみ close_keys / inline_styles を上書きする。
            // FocusedExisting の場合は既存 float の状態を維持する。
            manager.set_close_keys(id, lsp_hover_close_keys());
            manager.set_inline_styles(id, floating_styles);
            LspHoverOpenOutcome::Opened { id }
        }
        FloatingOpenWithFocusOutcome::FocusedExisting { id } => {
            LspHoverOpenOutcome::FocusedExisting { id }
        }
    };
    log::debug!("[lsp_float] hover float open outcome: {:?}", hover_outcome);
    Some(hover_outcome)
}

/// LSP hover response（plaintext / markdown / MarkedString / 配列形式）を
/// float 表示用の `RenderedFloatContent` に変換する。markdown 形式は
/// `markdown_render` を経由してレンダリングし、plaintext は素通しする。
pub fn render_lsp_hover_response(value: &Value) -> RenderedFloatContent {
    let contents = value
        .pointer("/result/contents")
        .or_else(|| value.get("contents"))
        .or_else(|| value.get("result"))
        .unwrap_or(value);
    render_hover_contents(contents)
}

fn render_hover_contents(value: &Value) -> RenderedFloatContent {
    match value {
        Value::Null => RenderedFloatContent::default(),
        Value::String(text) => render_plaintext_to_float_content(text),
        Value::Array(items) => {
            let mut combined = RenderedFloatContent::default();
            for item in items {
                let part = render_hover_contents(item);
                if part.lines.is_empty() {
                    continue;
                }
                if !combined.lines.is_empty() {
                    combined.lines.push(String::new());
                }
                let line_offset = combined.lines.len();
                combined.lines.extend(part.lines);
                for style in part.inline_styles {
                    combined
                        .inline_styles
                        .push(crate::markdown_render::InlineStyle {
                            kind: style.kind,
                            line: style.line + line_offset,
                            column_start: style.column_start,
                            column_end: style.column_end,
                        });
                }
            }
            combined
        }
        Value::Object(object) => {
            if let Some(text) = object.get("value").and_then(Value::as_str) {
                let kind = object.get("kind").and_then(Value::as_str);
                let language = object.get("language").and_then(Value::as_str);
                if kind == Some("markdown") || language.is_some() {
                    render_markdown_to_float_content(text)
                } else {
                    render_plaintext_to_float_content(text)
                }
            } else if let Some(contents) = object.get("contents") {
                render_hover_contents(contents)
            } else {
                RenderedFloatContent::default()
            }
        }
        _ => RenderedFloatContent::default(),
    }
}

pub fn open_lsp_diagnostic_float(
    manager: &mut FloatingWindowManager,
    request: LspDiagnosticFloatRequest,
) -> Option<FloatingWindowId> {
    let lines = diagnostic_lines_from_lsp_value(&request.diagnostics);
    if lines.is_empty() {
        log::debug!(
            "[lsp_float] diagnostics did not produce a float: window_id={}, position=({},{}), diagnostics={}",
            request.window_id,
            request.line,
            request.column,
            request.diagnostics
        );
        return None;
    }

    let size = size_for_lines(&lines);
    log::debug!(
        "[lsp_float] opening diagnostic float: window_id={}, position=({},{}), lines={}, size=({},{})",
        request.window_id,
        request.line,
        request.column,
        lines.len(),
        size.width,
        size.height
    );
    Some(
        manager.open_static_lines_with_lifecycle_and_replacement_group(
            lines,
            FloatingLifecycle::CloseOnCursorMove,
            Some(LSP_DIAGNOSTIC_GROUP.to_string()),
            FloatingPlacement {
                relative_to: FloatingRelativeTo::BufferPosition {
                    window_id: request.window_id,
                    line: request.line,
                    column: request.column,
                },
                anchor: FloatingAnchor::NorthWest,
                row: 1,
                col: 0,
                fit: FloatingFit::TruncateToGrid,
            },
            size,
            FloatingChrome {
                border: FloatingBorder::Single,
            },
            FloatingZIndex::Hover,
            true,
        ),
    )
}

pub fn open_lsp_location_list_float(
    manager: &mut FloatingWindowManager,
    request: LspLocationListRequest,
) -> Option<FloatingWindowId> {
    let mut lines = location_lines_from_lsp_value(&request.response);
    if lines.is_empty() {
        log::debug!(
            "[lsp_float] location list did not produce a float: window_id={}, title={}, response={}",
            request.window_id,
            request.title,
            request.response
        );
        return None;
    }
    if !request.title.trim().is_empty() {
        lines.insert(0, request.title.trim().to_string());
    }
    open_static_lsp_list_float(
        manager,
        request.window_id,
        lines,
        LSP_LOCATION_GROUP,
        "locations",
    )
}

pub fn open_lsp_symbol_outline_float(
    manager: &mut FloatingWindowManager,
    request: LspSymbolOutlineRequest,
) -> Option<FloatingWindowId> {
    let lines = symbol_lines_from_lsp_value(&request.response);
    if lines.is_empty() {
        log::debug!(
            "[lsp_float] symbol outline did not produce a float: window_id={}, response={}",
            request.window_id,
            request.response
        );
        return None;
    }
    open_static_lsp_list_float(
        manager,
        request.window_id,
        lines,
        LSP_SYMBOL_GROUP,
        "symbols",
    )
}

fn open_static_lsp_list_float(
    manager: &mut FloatingWindowManager,
    window_id: i32,
    lines: Vec<String>,
    replacement_group: &'static str,
    label: &str,
) -> Option<FloatingWindowId> {
    let size = size_for_lines(&lines);
    log::debug!(
        "[lsp_float] opening {label} float: window_id={}, lines={}, size=({},{})",
        window_id,
        lines.len(),
        size.width,
        size.height
    );
    Some(
        manager.open_static_lines_with_lifecycle_and_replacement_group(
            lines,
            FloatingLifecycle::ReplaceByGroup(replacement_group),
            Some(replacement_group.to_string()),
            FloatingPlacement::editor_at(1, 1),
            size,
            FloatingChrome {
                border: FloatingBorder::Single,
            },
            FloatingZIndex::Hover,
            true,
        ),
    )
}

/// 旧 API 互換。`render_lsp_hover_response` で得た rendered lines を
/// 返すラッパ。Phase D 以降は内部で markdown_render を経由するため、
/// 旧来の "MarkedString の language を行頭にラベルとして付与する" 挙動は
/// 廃止され、markdown レンダリング規約に沿った行が返る。
pub fn hover_lines_from_lsp_value(value: &Value) -> Vec<String> {
    render_lsp_hover_response(value).lines
}

pub fn location_lines_from_lsp_value(value: &Value) -> Vec<String> {
    let result = value
        .pointer("/result")
        .or_else(|| value.get("result"))
        .unwrap_or(value);
    let locations = match result {
        Value::Array(items) => items.clone(),
        Value::Object(_) => vec![result.clone()],
        _ => Vec::new(),
    };
    locations
        .iter()
        .filter_map(|location| {
            let uri = location
                .get("uri")
                .or_else(|| location.pointer("/targetUri"))
                .and_then(Value::as_str)?;
            let start = location
                .pointer("/range/start")
                .or_else(|| location.pointer("/targetSelectionRange/start"))
                .or_else(|| location.pointer("/targetRange/start"))?;
            let line = start.get("line").and_then(Value::as_u64).unwrap_or(0) + 1;
            let column = start.get("character").and_then(Value::as_u64).unwrap_or(0) + 1;
            Some(format!("{}:{}:{}", display_uri(uri), line, column))
        })
        .collect()
}

pub fn symbol_lines_from_lsp_value(value: &Value) -> Vec<String> {
    let result = value
        .pointer("/result")
        .or_else(|| value.get("result"))
        .unwrap_or(value);
    let Some(items) = result.as_array() else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    collect_symbol_lines(items, 0, &mut lines);
    lines
}

fn diagnostic_lines_from_lsp_value(value: &Value) -> Vec<String> {
    let diagnostics = value
        .pointer("/params/diagnostics")
        .or_else(|| value.get("diagnostics"))
        .unwrap_or(value);
    let Some(items) = diagnostics.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let message = item.get("message").and_then(Value::as_str)?.trim();
            if message.is_empty() {
                return None;
            }
            Some(match item.get("severity").and_then(Value::as_u64) {
                Some(1) => format!("Error: {message}"),
                Some(2) => format!("Warning: {message}"),
                Some(3) => format!("Info: {message}"),
                Some(4) => format!("Hint: {message}"),
                _ => message.to_string(),
            })
        })
        .collect()
}

fn diagnostic_entries_from_lsp_value(value: &Value) -> Vec<LspDiagnosticEntry> {
    let uri = value
        .pointer("/params/uri")
        .or_else(|| value.get("uri"))
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let diagnostics = value
        .pointer("/params/diagnostics")
        .or_else(|| value.get("diagnostics"))
        .unwrap_or(value);
    let Some(items) = diagnostics.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let message = item.get("message").and_then(Value::as_str)?.trim();
            if message.is_empty() {
                return None;
            }
            let start = item.pointer("/range/start");
            Some(LspDiagnosticEntry {
                uri: uri.clone(),
                line: start
                    .and_then(|start| start.get("line"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as usize,
                column: start
                    .and_then(|start| start.get("character"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as usize,
                message: message.to_string(),
                severity: item.get("severity").and_then(Value::as_u64),
            })
        })
        .collect()
}

fn collect_symbol_lines(items: &[Value], depth: usize, lines: &mut Vec<String>) {
    for item in items {
        let Some(name) = item.get("name").and_then(Value::as_str) else {
            continue;
        };
        let kind = item
            .get("kind")
            .and_then(Value::as_u64)
            .map(symbol_kind_name)
            .unwrap_or("Symbol");
        let start = item
            .pointer("/range/start")
            .or_else(|| item.pointer("/selectionRange/start"));
        let line = start
            .and_then(|start| start.get("line"))
            .and_then(Value::as_u64)
            .unwrap_or(0)
            + 1;
        let column = start
            .and_then(|start| start.get("character"))
            .and_then(Value::as_u64)
            .unwrap_or(0)
            + 1;
        lines.push(format!(
            "{}{} [{}] {}:{}",
            "  ".repeat(depth),
            name,
            kind,
            line,
            column
        ));
        if let Some(children) = item.get("children").and_then(Value::as_array) {
            collect_symbol_lines(children, depth + 1, lines);
        }
    }
}

fn symbol_kind_name(kind: u64) -> &'static str {
    match kind {
        1 => "File",
        2 => "Module",
        3 => "Namespace",
        4 => "Package",
        5 => "Class",
        6 => "Method",
        7 => "Property",
        8 => "Field",
        9 => "Constructor",
        10 => "Enum",
        11 => "Interface",
        12 => "Function",
        13 => "Variable",
        14 => "Constant",
        15 => "String",
        16 => "Number",
        17 => "Boolean",
        18 => "Array",
        19 => "Object",
        20 => "Key",
        21 => "Null",
        22 => "EnumMember",
        23 => "Struct",
        24 => "Event",
        25 => "Operator",
        26 => "TypeParameter",
        _ => "Symbol",
    }
}

fn display_uri(uri: &str) -> String {
    uri.strip_prefix("file://")
        .map(percent_decode_path)
        .unwrap_or_else(|| uri.to_string())
}

pub fn file_uri_to_path(uri: &str) -> Option<std::path::PathBuf> {
    let path = uri.strip_prefix("file://")?;
    Some(std::path::PathBuf::from(percent_decode_path(path)))
}

fn percent_decode_path(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
        {
            output.push((high << 4) | low);
            index += 3;
            continue;
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// 表示幅 (UnicodeWidthChar) ベースで各行を `max_width` セルに折り返す。
/// 単語境界の解析は行わず、表示幅で安全に切るだけのシンプル実装。
/// 空行は段落区切りとして維持される。
pub fn wrap_lines_to_width(lines: Vec<String>, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return lines;
    }
    let mut wrapped: Vec<String> = Vec::with_capacity(lines.len());
    let mut wrap_count = 0usize;
    for line in lines {
        if line.is_empty() {
            wrapped.push(String::new());
            continue;
        }
        let line_width = UnicodeWidthStr::width(line.as_str());
        if line_width <= max_width {
            wrapped.push(line);
            continue;
        }
        let mut current = String::new();
        let mut current_width = 0usize;
        for ch in line.chars() {
            let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if current_width + ch_width > max_width && !current.is_empty() {
                wrapped.push(std::mem::take(&mut current));
                current_width = 0;
                wrap_count += 1;
            }
            current.push(ch);
            current_width += ch_width;
        }
        if !current.is_empty() {
            wrapped.push(current);
        }
    }
    if wrap_count > 0 {
        log::debug!(
            "[lsp_float] wrapped {} long line(s) at width {}",
            wrap_count,
            max_width
        );
    }
    wrapped
}

fn size_for_lines(lines: &[String]) -> FloatingSize {
    let content_width = lines
        .iter()
        .map(|line| UnicodeWidthStr::width(line.as_str()))
        .max()
        .unwrap_or(1)
        .clamp(1, usize::from(MAX_FLOAT_WIDTH.saturating_sub(2)));
    let content_height = lines
        .len()
        .clamp(1, usize::from(MAX_FLOAT_HEIGHT.saturating_sub(2)));
    FloatingSize {
        width: u16::try_from(content_width + 2).unwrap_or(MAX_FLOAT_WIDTH),
        height: u16::try_from(content_height + 2).unwrap_or(MAX_FLOAT_HEIGHT),
    }
}
