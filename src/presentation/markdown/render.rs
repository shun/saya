//! Markdown → Float ウィンドウ表示用整形レンダリングサービス。
//!
//! `markdown_structure` は構文解析（ブロック / インラインの位置情報）
//! までを担い、本モジュールはその結果から「float に表示する行リスト」
//! と「ハイライトに使える inline スタイル範囲」を構築する。
//!
//! LSP hover / signature help / completion ドキュメント等、float に
//! markdown を表示する全機能で共通利用することを想定する。
//! saya コアは markdown の構造を知っていても、特定プロトコル（LSP 等）
//! 固有の知識は持たない。
//!
//! 変換ルール（Phase D 受け入れ条件準拠）:
//! - 見出し: `#` プレフィクスはそのまま残す（視覚階層の保持）
//! - リスト項目: 行頭の `- ` / `* ` / `+ ` / `1. ` を `• ` に正規化
//! - フェンスコード: 開始・終了の `` ``` `` 行を除去し、本文だけ残す
//! - インラインコード: バッククォートは残し、`InlineStyleKind::Code` の
//!   範囲を記録する（描画層でモノスペース強調できるように）
//! - 強調マーカー: `*` / `_` はリテラルに残し、範囲を記録する
//! - リンク: `[text](url)` を `text (url)` に展開し、`LinkText` と
//!   `LinkUrl` の範囲を別々に記録する
//! - 末尾の連続空行は trim、内側の空行は段落区切りとして保持する

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::presentation::markdown::structure::{
    MarkdownBlockKind, MarkdownDocumentMap, MarkdownInlineKind, MarkdownTextRange,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineStyleKind {
    Code,
    Emphasis,
    LinkText,
    LinkUrl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InlineStyle {
    pub kind: InlineStyleKind,
    pub line: usize,
    pub column_start: usize,
    pub column_end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RenderedFloatContent {
    pub lines: Vec<String>,
    pub inline_styles: Vec<InlineStyle>,
}

/// Markdown ソースを float に表示可能な行リストへ変換する。
/// 変換ルールはモジュールヘッダの説明に従う。
pub fn render_markdown_to_float_content(source: &str) -> RenderedFloatContent {
    let map = MarkdownDocumentMap::parse(source);
    let source_lines: Vec<&str> = source.lines().collect();

    log::debug!(
        "[markdown_render] rendering markdown: source_lines={}, blocks={}, inlines={}",
        source_lines.len(),
        map.blocks.len(),
        map.inlines.len()
    );

    let mut rendered_lines: Vec<String> = Vec::with_capacity(source_lines.len());
    let mut inline_styles: Vec<InlineStyle> = Vec::new();

    // fence ブロックの「ヘッダ行 / フッタ行」を識別するための index 集合。
    // 内部のコード本文は通常 line として出力する。
    let mut fence_marker_lines: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for block in &map.blocks {
        if let MarkdownBlockKind::FencedCodeBlock { .. } = block.kind {
            fence_marker_lines.insert(block.range.start.line);
            fence_marker_lines.insert(block.range.end.line);
        }
    }

    // 各 source 行ごとに変換結果を out 配列に push し、出力 line index と
    // 元 line index の対応関係を作る。inline スタイルは出力行ベースで
    // 構築するため、列のオフセット変換も追跡する。
    let mut source_to_rendered: Vec<Option<RenderedLineMap>> = vec![None; source_lines.len()];

    for (source_line_idx, source_line) in source_lines.iter().enumerate() {
        if fence_marker_lines.contains(&source_line_idx) {
            log::debug!(
                "[markdown_render] dropped fence marker line: index={}, content={:?}",
                source_line_idx,
                source_line
            );
            continue;
        }

        let mut line_render = render_line(source_line);
        // List marker 正規化（行頭の `- ` 等 → `• `）はブロック判定に依存
        if let Some(list_render) =
            transform_list_marker_if_applicable(&line_render, &map, source_line_idx)
        {
            line_render = list_render;
        }

        let rendered_index = rendered_lines.len();
        rendered_lines.push(line_render.text);
        source_to_rendered[source_line_idx] = Some(RenderedLineMap {
            rendered_index,
            column_offsets: line_render.column_offsets,
        });
    }

    // 末尾連続空行を trim（内部の空行は段落区切りとして保持）
    while rendered_lines
        .last()
        .map(|line| line.trim().is_empty())
        .unwrap_or(false)
    {
        rendered_lines.pop();
    }

    // inline スタイルの構築は MarkdownDocumentMap.inlines を走査し、
    // 出力行 index + 出力列に変換する。fence 内部の inline は元の
    // markdown_structure 側で除外されているため、ここでは fence マーカー行
    // をスキップした座標写像をそのまま用いる。
    for inline in &map.inlines {
        let Some(map_entry) = source_to_rendered
            .get(inline.range.start.line)
            .and_then(Option::as_ref)
        else {
            continue;
        };
        let mapped = match map_inline_columns(&inline.range, map_entry) {
            Some(mapped) => mapped,
            None => continue,
        };
        match &inline.kind {
            MarkdownInlineKind::InlineCode => inline_styles.push(InlineStyle {
                kind: InlineStyleKind::Code,
                line: map_entry.rendered_index,
                column_start: mapped.column_start,
                column_end: mapped.column_end,
            }),
            MarkdownInlineKind::EmphasisMarker { .. } => inline_styles.push(InlineStyle {
                kind: InlineStyleKind::Emphasis,
                line: map_entry.rendered_index,
                column_start: mapped.column_start,
                column_end: mapped.column_end,
            }),
            MarkdownInlineKind::Link {
                text,
                destination: _,
            } => {
                // Phase D の Link 表示は可視テキストのみで URL を隠す方針。
                // 構造化情報として `LinkText` 範囲だけを記録し、`LinkUrl` は
                // 出力しない。OSC8 hyperlink 等の対応が来た時にここに
                // ハイパーリンクメタデータを足す形で拡張する。
                if let Some(text_map) = map_inline_columns(text, map_entry) {
                    inline_styles.push(InlineStyle {
                        kind: InlineStyleKind::LinkText,
                        line: map_entry.rendered_index,
                        column_start: text_map.column_start,
                        column_end: text_map.column_end,
                    });
                }
            }
        }
    }

    log::debug!(
        "[markdown_render] markdown render finished: rendered_lines={}, inline_styles={}",
        rendered_lines.len(),
        inline_styles.len()
    );

    RenderedFloatContent {
        lines: rendered_lines,
        inline_styles,
    }
}

/// `RenderedFloatContent` を表示幅 `max_width` セルで折り返し、
/// 各行の長さがその上限を超えないよう正規化する。折り返しが発生した
/// 元 line の `inline_styles` は破棄（列再マップが意味を失うため）し、
/// 折り返しが起きなかった line の `inline_styles` は新しい line index
/// に再マップして保持する。
pub fn wrap_rendered_content_to_width(
    content: RenderedFloatContent,
    max_width: usize,
) -> RenderedFloatContent {
    if max_width == 0 {
        return content;
    }
    let RenderedFloatContent {
        lines,
        inline_styles,
    } = content;
    let mut wrapped_lines: Vec<String> = Vec::with_capacity(lines.len());
    let mut source_to_rendered_start: Vec<usize> = Vec::with_capacity(lines.len());
    let mut source_was_wrapped: Vec<bool> = Vec::with_capacity(lines.len());

    for line in lines {
        source_to_rendered_start.push(wrapped_lines.len());
        if line.is_empty() {
            wrapped_lines.push(String::new());
            source_was_wrapped.push(false);
            continue;
        }
        let line_width = UnicodeWidthStr::width(line.as_str());
        if line_width <= max_width {
            wrapped_lines.push(line);
            source_was_wrapped.push(false);
            continue;
        }
        wrapped_lines.extend(wrap_line_to_width_preserving_words(&line, max_width));
        source_was_wrapped.push(true);
    }

    let mapped_inline_styles: Vec<InlineStyle> = inline_styles
        .into_iter()
        .filter_map(|style| {
            if style.line >= source_was_wrapped.len() {
                return None;
            }
            if source_was_wrapped[style.line] {
                return None;
            }
            Some(InlineStyle {
                kind: style.kind,
                line: source_to_rendered_start[style.line],
                column_start: style.column_start,
                column_end: style.column_end,
            })
        })
        .collect();

    // 末尾の空行を trim（render_markdown_to_float_content と同じ規約）
    while wrapped_lines
        .last()
        .map(|line| line.trim().is_empty())
        .unwrap_or(false)
    {
        wrapped_lines.pop();
    }
    let truncated_styles: Vec<InlineStyle> = mapped_inline_styles
        .into_iter()
        .filter(|style| style.line < wrapped_lines.len())
        .collect();

    log::debug!(
        "[markdown_render] wrap_rendered_content_to_width: rendered_lines={}, inline_styles={}, max_width={}",
        wrapped_lines.len(),
        truncated_styles.len(),
        max_width
    );

    RenderedFloatContent {
        lines: wrapped_lines,
        inline_styles: truncated_styles,
    }
}

fn wrap_line_to_width_preserving_words(line: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 || UnicodeWidthStr::width(line) <= max_width {
        return vec![line.to_string()];
    }

    let mut wrapped = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;

    for segment in line.split_inclusive(char::is_whitespace) {
        let segment_width = UnicodeWidthStr::width(segment);
        if current_width > 0 && current_width + segment_width > max_width {
            wrapped.push(trim_trailing_whitespace(std::mem::take(&mut current)));
            current_width = 0;
        }

        if segment_width > max_width {
            if !current.is_empty() {
                wrapped.push(trim_trailing_whitespace(std::mem::take(&mut current)));
                current_width = 0;
            }
            wrapped.extend(wrap_long_word_to_width(segment.trim_end(), max_width));
            continue;
        }

        current.push_str(segment);
        current_width += segment_width;
    }

    if !current.is_empty() {
        wrapped.push(trim_trailing_whitespace(current));
    }
    wrapped
}

fn wrap_long_word_to_width(word: &str, max_width: usize) -> Vec<String> {
    let mut wrapped = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;
    for ch in word.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if current_width + ch_width > max_width && !current.is_empty() {
            wrapped.push(std::mem::take(&mut current));
            current_width = 0;
        }
        current.push(ch);
        current_width += ch_width;
    }
    if !current.is_empty() {
        wrapped.push(current);
    }
    wrapped
}

fn trim_trailing_whitespace(mut line: String) -> String {
    while line.ends_with(char::is_whitespace) {
        line.pop();
    }
    line
}

/// プレーンテキストを float に表示可能な行リストへ変換する。
/// markdown の変換ルールは一切適用せず、行で分割するだけ。
pub fn render_plaintext_to_float_content(source: &str) -> RenderedFloatContent {
    let lines: Vec<String> = source.lines().map(ToString::to_string).collect();
    log::debug!(
        "[markdown_render] rendering plaintext: lines={}",
        lines.len()
    );
    RenderedFloatContent {
        lines,
        inline_styles: Vec::new(),
    }
}

#[derive(Debug, Clone)]
struct RenderedLineMap {
    rendered_index: usize,
    column_offsets: Vec<ColumnMapping>,
}

#[derive(Debug, Clone, Copy)]
struct ColumnMapping {
    /// 元 source 行の列開始位置（バイト）
    source_column_start: usize,
    /// 元 source 行の列終了位置（バイト exclusive）
    source_column_end: usize,
    /// 対応する出力行の列開始位置
    rendered_column_start: usize,
    /// 対応する出力行の列終了位置
    rendered_column_end: usize,
}

struct LineRenderResult {
    text: String,
    column_offsets: Vec<ColumnMapping>,
}

#[derive(Debug, Clone, Copy)]
struct MappedRange {
    column_start: usize,
    column_end: usize,
}

/// 行頭インデントの TAB を等価 space 数に展開するセル数。markdown の
/// fenced code block 内の Go ソースなどがタブインデントで来た場合、
/// ratatui の Cell に TAB をそのまま渡すと表示が崩れるため正規化する。
const TAB_EXPANSION_SPACES: usize = 4;

/// markdown 仕様に従い、`\X` でエスケープ可能な ASCII 句読点集合。
/// gopls 等は hover 内の backtick や `*` を `\` でエスケープしてくるため、
/// レンダリング時にエスケープを剥がしてリテラル文字に戻す。
fn is_markdown_escapable_punctuation(byte: u8) -> bool {
    matches!(
        byte,
        b'\\'
            | b'`'
            | b'*'
            | b'_'
            | b'{'
            | b'}'
            | b'['
            | b']'
            | b'('
            | b')'
            | b'#'
            | b'+'
            | b'-'
            | b'.'
            | b'!'
            | b'<'
            | b'>'
            | b'|'
            | b'~'
            | b'='
            | b'"'
    )
}

fn render_line(source_line: &str) -> LineRenderResult {
    // Link 展開・TAB 展開・バックスラッシュエスケープ解釈を行う。
    // 列対応は元 source 列 → rendered 列にマッピングして保存。
    let mut text = String::with_capacity(source_line.len());
    let mut column_offsets: Vec<ColumnMapping> = Vec::new();
    let bytes = source_line.as_bytes();
    let mut cursor = 0;

    while cursor < bytes.len() {
        // 1) TAB は 4 space に展開（visual-safe rendering）
        if bytes[cursor] == b'\t' {
            let rendered_start = text.len();
            for _ in 0..TAB_EXPANSION_SPACES {
                text.push(' ');
            }
            column_offsets.push(ColumnMapping {
                source_column_start: cursor,
                source_column_end: cursor + 1,
                rendered_column_start: rendered_start,
                rendered_column_end: text.len(),
            });
            cursor += 1;
            continue;
        }
        // 2) markdown のバックスラッシュエスケープ: `\X` (X が句読点) → X
        if bytes[cursor] == b'\\'
            && cursor + 1 < bytes.len()
            && is_markdown_escapable_punctuation(bytes[cursor + 1])
        {
            let escaped = bytes[cursor + 1];
            let rendered_start = text.len();
            text.push(escaped as char);
            column_offsets.push(ColumnMapping {
                source_column_start: cursor,
                source_column_end: cursor + 2,
                rendered_column_start: rendered_start,
                rendered_column_end: text.len(),
            });
            cursor += 2;
            continue;
        }
        // 3) Link `[text](url)` を可視テキストのみに変換し、URL は隠す。
        //    将来 OSC8 hyperlink などで URL を「クリック先メタデータ」として
        //    持たせる場合は別レイヤーで扱う（Phase D ではテキストのみ表示）。
        if bytes[cursor] == b'['
            && let Some(text_end_relative) = source_line[cursor + 1..].find(']')
        {
            let text_end = cursor + 1 + text_end_relative;
            if bytes.get(text_end + 1) == Some(&b'(')
                && let Some(dest_end_relative) = source_line[text_end + 2..].find(')')
            {
                let dest_end = text_end + 2 + dest_end_relative;
                let full_end = dest_end + 1;
                let label = &source_line[cursor + 1..text_end];

                let rendered_start = text.len();
                text.push_str(label);
                let rendered_end = text.len();

                // 元 source の `[text]` 部分（cursor+1 .. text_end）が
                // 描画後の `text` 領域に対応する。markdown_structure 由来の
                // LinkText 範囲はこの対応を使って rendered 列に変換される。
                column_offsets.push(ColumnMapping {
                    source_column_start: cursor + 1,
                    source_column_end: text_end,
                    rendered_column_start: rendered_start,
                    rendered_column_end: rendered_end,
                });

                // 元 source の "[text](url)" 全体も column 対応として記録
                // （markdown_structure の Link の `range` 全体がこの範囲）。
                column_offsets.push(ColumnMapping {
                    source_column_start: cursor,
                    source_column_end: full_end,
                    rendered_column_start: rendered_start,
                    rendered_column_end: rendered_end,
                });

                cursor = full_end;
                continue;
            }
        }
        // 4) その他のバイト（multi-byte char 単位で 1 文字進める）
        let byte_start = cursor;
        let char_len = source_line[cursor..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(1);
        let byte_end = cursor + char_len;
        text.push_str(&source_line[byte_start..byte_end]);
        column_offsets.push(ColumnMapping {
            source_column_start: byte_start,
            source_column_end: byte_end,
            rendered_column_start: byte_start,
            rendered_column_end: byte_end,
        });
        cursor = byte_end;
    }

    LineRenderResult {
        text,
        column_offsets,
    }
}

fn transform_list_marker_if_applicable(
    line: &LineRenderResult,
    map: &MarkdownDocumentMap,
    source_line_idx: usize,
) -> Option<LineRenderResult> {
    let is_list_item = map.blocks.iter().any(|block| match block.kind {
        MarkdownBlockKind::ListItem { .. } => block.range.start.line == source_line_idx,
        _ => false,
    });
    if !is_list_item {
        return None;
    }

    let original = &line.text;
    let leading_whitespace = original
        .chars()
        .take_while(|c| c.is_ascii_whitespace())
        .count();
    let rest = &original[leading_whitespace..];

    let (marker_len, after_marker_idx) = if let Some(stripped) = rest.strip_prefix("- ") {
        (2, leading_whitespace + (rest.len() - stripped.len()))
    } else if let Some(stripped) = rest.strip_prefix("* ") {
        (2, leading_whitespace + (rest.len() - stripped.len()))
    } else if let Some(stripped) = rest.strip_prefix("+ ") {
        (2, leading_whitespace + (rest.len() - stripped.len()))
    } else {
        // 数字付きリスト `1. ` 等
        let digit_count = rest.bytes().take_while(|b| b.is_ascii_digit()).count();
        if digit_count > 0
            && rest.as_bytes().get(digit_count) == Some(&b'.')
            && rest.as_bytes().get(digit_count + 1) == Some(&b' ')
        {
            (digit_count + 2, leading_whitespace + digit_count + 2)
        } else {
            return None;
        }
    };

    let mut new_text = String::with_capacity(original.len());
    new_text.push_str(&original[..leading_whitespace]);
    new_text.push_str("• ");
    new_text.push_str(&original[after_marker_idx..]);

    // 列マッピングを「marker 部分は • + 半角空白に圧縮」した形に
    // 再構築する。bullet 後の文字は src marker 長さの差だけ rendered 列が
    // ずれる。
    let bullet_byte_len = "• ".len();
    let rendered_shift_for_after_marker = bullet_byte_len as isize - marker_len as isize;

    let column_offsets = line
        .column_offsets
        .iter()
        .filter_map(|mapping| {
            if mapping.source_column_end <= leading_whitespace + marker_len {
                if mapping.source_column_start >= leading_whitespace {
                    // marker 内部の cell はスタイル対象から除外
                    return None;
                }
                return Some(*mapping);
            }
            let rendered_column_start = adjusted_column(
                mapping.rendered_column_start,
                leading_whitespace,
                marker_len,
                rendered_shift_for_after_marker,
            );
            let rendered_column_end = adjusted_column(
                mapping.rendered_column_end,
                leading_whitespace,
                marker_len,
                rendered_shift_for_after_marker,
            );
            Some(ColumnMapping {
                source_column_start: mapping.source_column_start,
                source_column_end: mapping.source_column_end,
                rendered_column_start,
                rendered_column_end,
            })
        })
        .collect();

    Some(LineRenderResult {
        text: new_text,
        column_offsets,
    })
}

fn adjusted_column(
    original: usize,
    leading_whitespace: usize,
    marker_len: usize,
    shift: isize,
) -> usize {
    if original < leading_whitespace + marker_len {
        leading_whitespace
    } else {
        let value = original as isize + shift;
        if value < 0 { 0 } else { value as usize }
    }
}

fn map_inline_columns(
    source_range: &MarkdownTextRange,
    line_map: &RenderedLineMap,
) -> Option<MappedRange> {
    if source_range.start.line != source_range.end.line {
        return None;
    }
    let mut rendered_start: Option<usize> = None;
    let mut rendered_end: Option<usize> = None;
    for mapping in &line_map.column_offsets {
        if mapping.source_column_start >= source_range.start.column
            && mapping.source_column_end <= source_range.end.column
        {
            if rendered_start.is_none() {
                rendered_start = Some(mapping.rendered_column_start);
            }
            rendered_end = Some(mapping.rendered_column_end);
        }
    }
    Some(MappedRange {
        column_start: rendered_start?,
        column_end: rendered_end?,
    })
}
