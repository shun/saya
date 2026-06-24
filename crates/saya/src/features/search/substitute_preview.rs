use vim_core_rs::{CoreBufferLineRange, CoreWindowInfo};

use crate::features::search::capability::SearchCapabilityContract;
use crate::features::search::query::{
    SearchMatch, SearchMatchKind, SearchQueryMode, SearchVisibleRows, SearchVisibleState,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct SubstitutePreviewCommand {
    pattern: String,
    /// 2 個目の区切り（置換フィールド）が未入力なら `None`。
    /// その場合はテキストを変更せず、パターンのマッチだけをハイライトする。
    replacement: Option<String>,
    global: bool,
    scope: SubstitutePreviewScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubstitutePreviewScope {
    CurrentLine,
    VisibleLines,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubstitutePreviewRender {
    pub line_range: CoreBufferLineRange,
    pub search_state: SearchVisibleState,
}

pub fn build_substitute_preview_state(
    window: &CoreWindowInfo,
    line_range: &CoreBufferLineRange,
    command_line_prompt: Option<char>,
    command_line_buffer: &str,
) -> Option<SearchVisibleState> {
    build_substitute_preview_render(window, line_range, command_line_prompt, command_line_buffer)
        .map(|preview| preview.search_state)
}

pub fn build_substitute_preview_render(
    window: &CoreWindowInfo,
    line_range: &CoreBufferLineRange,
    command_line_prompt: Option<char>,
    command_line_buffer: &str,
) -> Option<SubstitutePreviewRender> {
    if command_line_prompt != Some(':') {
        return None;
    }
    let command = parse_substitute_preview_command(command_line_buffer)?;
    let (preview_lines, matches) = collect_literal_preview_lines(window, line_range, &command);
    if matches.is_empty() {
        log::debug!(
            "[substitute_preview] no literal preview matches: window_id={}, buffer_id={}, pattern={:?}, replacement={:?}, global={}, scope={:?}, visible_start_row={}, visible_lines={}",
            window.id,
            window.buf_id,
            command.pattern,
            command.replacement,
            command.global,
            command.scope,
            line_range.start_row,
            line_range.lines.len()
        );
        return None;
    }

    log::debug!(
        "[substitute_preview] built substitute live preview: window_id={}, buffer_id={}, pattern={:?}, replacement={:?}, global={}, scope={:?}, matches={}, visible_start_row={}, visible_lines={}",
        window.id,
        window.buf_id,
        command.pattern,
        command.replacement,
        command.global,
        command.scope,
        matches.len(),
        line_range.start_row,
        line_range.lines.len()
    );

    let mut preview_line_range = line_range.clone();
    preview_line_range.lines = preview_lines;
    Some(SubstitutePreviewRender {
        line_range: preview_line_range,
        search_state: SearchVisibleState {
            capability: SearchCapabilityContract::baseline_ready_contract(),
            window_id: window.id,
            visible_rows: SearchVisibleRows {
                start_row: line_range.start_row.saturating_add(1),
                end_row: line_range
                    .start_row
                    .saturating_add(line_range.lines.len())
                    .max(line_range.start_row.saturating_add(1)),
            },
            mode: SearchQueryMode::IncsearchPreview,
            pattern: Some(command.pattern.clone()),
            input_pattern: Some(command.pattern),
            hlsearch_enabled: true,
            hlsearch_suspended: false,
            incsearch_active: true,
            matches,
        },
    })
}

fn collect_literal_preview_lines(
    window: &CoreWindowInfo,
    line_range: &CoreBufferLineRange,
    command: &SubstitutePreviewCommand,
) -> (Vec<String>, Vec<SearchMatch>) {
    let mut preview_lines = Vec::with_capacity(line_range.lines.len());
    let mut matches = Vec::new();
    for (line_offset, line) in line_range.lines.iter().enumerate() {
        let row_zero_based = line_range.start_row.saturating_add(line_offset);
        if command.scope == SubstitutePreviewScope::CurrentLine
            && row_zero_based != window.cursor_row
        {
            preview_lines.push(line.clone());
            continue;
        }

        // 置換が未確定（パターン入力中）の場合はテキストを変えずパターン自体を描画する。
        // ハイライト対象は実際の substitute と同じく g フラグに従い、g が無ければ行ごとに
        // 最初のマッチだけを対象にする（neovim の inccommand 準拠）。
        let replacement_text = command
            .replacement
            .as_deref()
            .unwrap_or(command.pattern.as_str());
        let mut preview_line = String::with_capacity(line.len());
        let mut search_from = 0usize;
        let mut replaced_on_line = false;
        while search_from <= line.len() {
            let Some(relative_start) = line[search_from..].find(&command.pattern) else {
                break;
            };
            let start_col = search_from.saturating_add(relative_start);
            let end_col = start_col.saturating_add(command.pattern.len());
            preview_line.push_str(&line[search_from..start_col]);
            let preview_start_col = preview_line.len();
            preview_line.push_str(replacement_text);
            let preview_end_col = preview_line.len();
            matches.push(SearchMatch {
                kind: SearchMatchKind::Incremental,
                start_row: row_zero_based.saturating_add(1),
                start_col: preview_start_col,
                end_row: row_zero_based.saturating_add(1),
                end_col: preview_end_col,
            });
            replaced_on_line = true;
            search_from = end_col.max(start_col.saturating_add(1));
            if !command.global {
                break;
            }
        }
        preview_line.push_str(&line[search_from..]);
        if replaced_on_line {
            preview_lines.push(preview_line);
        } else {
            preview_lines.push(line.clone());
        }
    }
    (preview_lines, matches)
}

fn parse_substitute_preview_command(input: &str) -> Option<SubstitutePreviewCommand> {
    let input = input
        .trim_start()
        .strip_prefix(':')
        .unwrap_or(input.trim_start());
    let (scope, after_range) = parse_substitute_preview_range(input);
    let after_command = strip_substitute_command_name(after_range.trim_start())?;
    let after_command = after_command.trim_start();
    let delimiter = after_command.bytes().next()?;
    if !is_substitute_preview_delimiter(delimiter) {
        return None;
    }
    let payload = &after_command[1..];
    let (raw_pattern, rest, pattern_closed) = split_substitute_preview_field(payload, delimiter);
    let pattern = unescape_substitute_preview_pattern(&raw_pattern, delimiter);
    if pattern.is_empty() {
        return None;
    }
    if !pattern_closed {
        // 置換フィールドの区切りがまだ入力されていない＝パターン入力中。
        // 置換は確定していないのでテキストは変えず、マッチのハイライトだけ行う。
        return Some(SubstitutePreviewCommand {
            pattern,
            replacement: None,
            global: false,
            scope,
        });
    }
    let (raw_replacement, flags, _) = split_substitute_preview_field(rest, delimiter);
    let replacement = unescape_substitute_preview_pattern(&raw_replacement, delimiter);
    let global = flags.contains('g');
    Some(SubstitutePreviewCommand {
        pattern,
        replacement: Some(replacement),
        global,
        scope,
    })
}

fn parse_substitute_preview_range(input: &str) -> (SubstitutePreviewScope, &str) {
    let trimmed = input.trim_start();
    if let Some(rest) = trimmed.strip_prefix('%') {
        return (SubstitutePreviewScope::VisibleLines, rest);
    }

    let range_len = trimmed
        .char_indices()
        .take_while(|(_, ch)| matches!(ch, '0'..='9' | '.' | '$' | ',' | ';' | '+' | '-'))
        .last()
        .map(|(index, ch)| index + ch.len_utf8())
        .unwrap_or(0);
    if range_len > 0 {
        (SubstitutePreviewScope::VisibleLines, &trimmed[range_len..])
    } else {
        (SubstitutePreviewScope::CurrentLine, trimmed)
    }
}

fn strip_substitute_command_name(input: &str) -> Option<&str> {
    for name in ["substitute", "s"] {
        if let Some(rest) = input.strip_prefix(name)
            && rest.bytes().next().is_none_or(|byte| {
                is_substitute_preview_delimiter(byte) || byte.is_ascii_whitespace()
            })
        {
            return Some(rest);
        }
    }
    None
}

fn split_substitute_preview_field(input: &str, delimiter: u8) -> (String, &str, bool) {
    let mut escaped = false;
    for (index, byte) in input.bytes().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        if byte == b'\\' {
            escaped = true;
            continue;
        }
        if byte == delimiter {
            return (input[..index].to_string(), &input[index + 1..], true);
        }
    }
    (input.to_string(), "", false)
}

fn unescape_substitute_preview_pattern(input: &str, delimiter: u8) -> String {
    let mut output = String::with_capacity(input.len());
    let mut escaped = false;
    for byte in input.bytes() {
        if escaped {
            if byte == delimiter || byte == b'\\' {
                output.push(byte as char);
            } else {
                output.push('\\');
                output.push(byte as char);
            }
            escaped = false;
            continue;
        }
        if byte == b'\\' {
            escaped = true;
        } else {
            output.push(byte as char);
        }
    }
    if escaped {
        output.push('\\');
    }
    output
}

fn is_substitute_preview_delimiter(byte: u8) -> bool {
    byte.is_ascii_punctuation() && byte != b'\\' && byte != b'"'
}

#[cfg(test)]
#[path = "substitute_preview_test.rs"]
mod tests;
