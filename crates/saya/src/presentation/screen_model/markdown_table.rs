//! Markdown テーブルブロックのレンダリング。

use super::*;

/// Minimum width kept for a single table column when the natural layout has to
/// be shrunk to fit the pane. Three display cells leave room for at least a
/// short word fragment plus the wrap to continue on the next line.
pub(super) const MIN_MARKDOWN_TABLE_COLUMN_WIDTH: usize = 3;

pub(super) fn render_markdown_table_block(
    source_text: &str,
    start_line: usize,
    end_line: usize,
    available_width: usize,
) -> Option<Vec<RenderedMarkdownTableLine>> {
    let raw_rows = (start_line..=end_line)
        .map(|line| source_text.split('\n').nth(line).unwrap_or_default())
        .collect::<Vec<_>>();
    let parsed_rows = raw_rows
        .iter()
        .map(|row| parse_markdown_table_cells(row))
        .collect::<Vec<_>>();
    let alignments = raw_rows
        .get(1)
        .and_then(|row| parse_markdown_table_delimiter(row))?;
    let column_count = alignments.len();
    if column_count == 0 || parsed_rows.first().map(Vec::len) != Some(column_count) {
        return None;
    }

    let mut natural_widths = vec![0usize; column_count];
    for (row_index, cells) in parsed_rows.iter().enumerate() {
        if row_index == 1 {
            continue;
        }
        for (column_index, cell) in cells.iter().take(column_count).enumerate() {
            for display_line in markdown_table_cell_display_lines(cell) {
                natural_widths[column_index] =
                    natural_widths[column_index].max(display_width(display_line, 1));
            }
        }
    }

    let column_widths = fit_markdown_table_column_widths(&natural_widths, available_width);
    if column_widths != natural_widths {
        log::debug!(
            "[screen_model] markdown table columns shrunk to fit pane: available_width={}, natural={:?}, fitted={:?}",
            available_width,
            natural_widths,
            column_widths
        );
    }

    let mut rendered = Vec::new();
    for (row_index, cells) in parsed_rows.iter().enumerate() {
        if row_index == 1 {
            rendered.push(RenderedMarkdownTableLine {
                source_line: Some(start_line + row_index),
                text: render_markdown_table_separator_row(&column_widths),
            });
            continue;
        }
        for text in render_markdown_table_content_rows(cells, &column_widths, &alignments) {
            rendered.push(RenderedMarkdownTableLine {
                source_line: Some(start_line + row_index),
                text,
            });
        }
    }

    log::debug!(
        "[screen_model] markdown table block rendered: start_line={}, end_line={}, rows={}, columns={}, widths={:?}",
        start_line,
        end_line,
        rendered.len(),
        column_count,
        column_widths
    );

    Some(rendered)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RenderedMarkdownTableLine {
    pub(super) source_line: Option<usize>,
    pub(super) text: String,
}

pub(super) fn project_rendered_markdown_table_line(
    absolute_row: usize,
    raw_text: &str,
    display_text: &str,
    line_start_col: u16,
) -> ScreenLineProjection {
    let mut cells = Vec::new();
    let mut spans = Vec::new();
    if !raw_text.is_empty() {
        let display_width = display_width(display_text, 1);
        cells.push(ScreenCellMapping {
            display_col: line_start_col,
            display_end_col_exclusive: u16::try_from(
                usize::from(line_start_col).saturating_add(display_width),
            )
            .unwrap_or(u16::MAX),
            raw_start_col: 0,
            raw_end_col: raw_text.len(),
        });
        spans.push(ScreenDisplaySpan {
            raw_start_col: 0,
            raw_end_col: raw_text.len(),
            display_start_col: line_start_col,
            display_end_col_exclusive: u16::try_from(
                usize::from(line_start_col).saturating_add(display_width),
            )
            .unwrap_or(u16::MAX),
            kind: ScreenDisplaySpanKind::MarkdownReplacement {
                text: display_text.to_string(),
            },
        });
    }
    ScreenLineProjection {
        absolute_row,
        raw_text: raw_text.to_string(),
        display_text: display_text.to_string(),
        spans,
        cells,
        line_start_col,
    }
}

pub(super) fn parse_markdown_table_cells(row: &str) -> Vec<String> {
    let mut row = row.trim_start();
    if let Some(stripped) = row.strip_prefix('|') {
        row = stripped;
    }
    if row.ends_with('|') && !row.ends_with("\\|") {
        row = &row[..row.len().saturating_sub(1)];
    }

    let mut cells = Vec::new();
    let mut current = String::new();
    let mut chars = row.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' && chars.peek() == Some(&'|') {
            current.push('|');
            chars.next();
            continue;
        }
        if ch == '|' {
            cells.push(render_markdown_table_cell(current.trim()));
            current.clear();
            continue;
        }
        current.push(ch);
    }
    cells.push(render_markdown_table_cell(current.trim()));
    cells
}

pub(super) fn render_markdown_table_cell(cell: &str) -> String {
    let mut rendered = String::new();
    let mut cursor = 0usize;
    while cursor < cell.len() {
        if let Some(link) = parse_inline_link_at(cell, cursor) {
            rendered.push_str(link.text);
            cursor = link.end;
            continue;
        }
        if let Some(code) = parse_inline_code_at(cell, cursor) {
            rendered.push_str(code.text);
            cursor = code.end;
            continue;
        }
        if let Some(end) = parse_html_break_at(cell, cursor) {
            rendered.push('\n');
            cursor = end;
            continue;
        }
        let Some(ch) = cell[cursor..].chars().next() else {
            break;
        };
        if !matches!(ch, '*' | '_') {
            rendered.push(ch);
        }
        cursor += ch.len_utf8();
    }
    rendered
}

pub(super) fn parse_html_break_at(cell: &str, cursor: usize) -> Option<usize> {
    let remaining = cell.get(cursor..)?;
    ["<br>", "<br/>", "<br />"]
        .iter()
        .find_map(|tag| remaining.starts_with(tag).then_some(cursor + tag.len()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct InlineTableFragment<'a> {
    text: &'a str,
    end: usize,
}

pub(super) fn parse_inline_link_at(cell: &str, cursor: usize) -> Option<InlineTableFragment<'_>> {
    if cell.as_bytes().get(cursor) != Some(&b'[') {
        return None;
    }
    let text_end_relative = cell.get(cursor + 1..)?.find(']')?;
    let text_end = cursor + 1 + text_end_relative;
    if cell.as_bytes().get(text_end + 1) != Some(&b'(') {
        return None;
    }
    let destination_end_relative = cell.get(text_end + 2..)?.find(')')?;
    Some(InlineTableFragment {
        text: cell.get(cursor + 1..text_end)?,
        end: text_end + 2 + destination_end_relative + 1,
    })
}

pub(super) fn parse_inline_code_at(cell: &str, cursor: usize) -> Option<InlineTableFragment<'_>> {
    if cell.as_bytes().get(cursor) != Some(&b'`') {
        return None;
    }
    let end_relative = cell.get(cursor + 1..)?.find('`')?;
    let end = cursor + 1 + end_relative + 1;
    Some(InlineTableFragment {
        text: cell.get(cursor + 1..end.saturating_sub(1))?,
        end,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MarkdownTableAlignment {
    Left,
    Center,
    Right,
}

pub(super) fn parse_markdown_table_delimiter(row: &str) -> Option<Vec<MarkdownTableAlignment>> {
    let cells = row
        .trim()
        .trim_matches('|')
        .split('|')
        .map(str::trim)
        .collect::<Vec<_>>();
    if cells.len() < 2 {
        return None;
    }
    let mut alignments = Vec::new();
    for cell in cells {
        let left = cell.starts_with(':');
        let right = cell.ends_with(':');
        let core = cell.trim_matches(':');
        if core.is_empty() || !core.bytes().all(|byte| byte == b'-') {
            return None;
        }
        alignments.push(match (left, right) {
            (true, true) => MarkdownTableAlignment::Center,
            (false, true) => MarkdownTableAlignment::Right,
            _ => MarkdownTableAlignment::Left,
        });
    }
    Some(alignments)
}

pub(super) fn markdown_table_cell_display_lines(cell: &str) -> Vec<&str> {
    let lines = cell.split('\n').collect::<Vec<_>>();
    if lines.is_empty() { vec![""] } else { lines }
}

/// Shrink the natural per-column widths so the rendered table fits within
/// `available_width`. Columns are reduced one display cell at a time, always
/// trimming the widest column first, so the available room is shared evenly.
/// An `available_width` of `0` means the pane width is unknown and the natural
/// layout is kept untouched.
pub(super) fn fit_markdown_table_column_widths(
    natural_widths: &[usize],
    available_width: usize,
) -> Vec<usize> {
    let column_count = natural_widths.len();
    if column_count == 0 || available_width == 0 {
        return natural_widths.to_vec();
    }

    // Each column renders as `│ <content> ` (border + two pad spaces) plus one
    // trailing `│`, so the non-content chrome is `3 * columns + 1`.
    let chrome = column_count.saturating_mul(3).saturating_add(1);
    let budget = available_width.saturating_sub(chrome);
    let natural_total = natural_widths.iter().sum::<usize>();
    if natural_total <= budget {
        return natural_widths.to_vec();
    }

    let mut widths = natural_widths.to_vec();
    loop {
        if widths.iter().sum::<usize>() <= budget {
            break;
        }
        let Some((index, _)) = widths
            .iter()
            .enumerate()
            .filter(|(_, width)| **width > MIN_MARKDOWN_TABLE_COLUMN_WIDTH)
            .max_by_key(|(_, width)| **width)
        else {
            // Every column is already at the floor; the pane is too narrow to
            // shrink further without dropping columns entirely.
            break;
        };
        widths[index] -= 1;
    }
    widths
}

/// Wrap a single rendered cell to `width` display cells, returning one entry per
/// display line. Existing line breaks (from `<br>`) are preserved, words are
/// kept whole when they fit, and any word wider than the column is hard-broken
/// by display width so multi-byte characters never split mid-cell incorrectly.
pub(super) fn wrap_markdown_table_cell(cell: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    for segment in cell.split('\n') {
        wrap_markdown_table_segment(segment, width, &mut lines);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

pub(super) fn wrap_markdown_table_segment(segment: &str, width: usize, lines: &mut Vec<String>) {
    let mut current = String::new();
    let mut current_width = 0usize;
    for word in segment.split(' ') {
        let word_width = display_width(word, 1);
        if !current.is_empty() && current_width.saturating_add(1).saturating_add(word_width) > width
        {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
        }
        if word_width <= width {
            if current.is_empty() {
                current.push_str(word);
                current_width = word_width;
            } else {
                current.push(' ');
                current.push_str(word);
                current_width += 1 + word_width;
            }
            continue;
        }
        if !current.is_empty() {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
        }
        for ch in word.chars() {
            let ch_width = char_display_width(ch);
            if current_width + ch_width > width && current_width > 0 {
                lines.push(std::mem::take(&mut current));
                current_width = 0;
            }
            current.push(ch);
            current_width += ch_width;
        }
    }
    lines.push(current);
}

pub(super) fn render_markdown_table_content_rows(
    cells: &[String],
    column_widths: &[usize],
    alignments: &[MarkdownTableAlignment],
) -> Vec<String> {
    let cell_lines = column_widths
        .iter()
        .enumerate()
        .map(|(column_index, width)| {
            cells
                .get(column_index)
                .map(|cell| wrap_markdown_table_cell(cell, *width))
                .unwrap_or_else(|| vec![String::new()])
        })
        .collect::<Vec<_>>();
    let row_height = cell_lines.iter().map(Vec::len).max().unwrap_or(1);
    let mut rendered_rows = Vec::new();
    for display_row in 0..row_height {
        let mut rendered = String::new();
        for (column_index, width) in column_widths.iter().enumerate() {
            let cell = cell_lines
                .get(column_index)
                .and_then(|lines| lines.get(display_row))
                .map(String::as_str)
                .unwrap_or_default();
            let padded = pad_markdown_table_cell(
                cell,
                *width,
                alignments
                    .get(column_index)
                    .copied()
                    .unwrap_or(MarkdownTableAlignment::Left),
            );
            rendered.push('│');
            rendered.push(' ');
            rendered.push_str(&padded);
            rendered.push(' ');
        }
        rendered.push('│');
        rendered_rows.push(rendered);
    }
    rendered_rows
}

pub(super) fn pad_markdown_table_cell(
    cell: &str,
    width: usize,
    alignment: MarkdownTableAlignment,
) -> String {
    let cell_width = display_width(cell, 1);
    let total_padding = width.saturating_sub(cell_width);
    match alignment {
        MarkdownTableAlignment::Right => format!("{}{}", " ".repeat(total_padding), cell),
        MarkdownTableAlignment::Center => {
            let left = total_padding / 2;
            let right = total_padding.saturating_sub(left);
            format!("{}{}{}", " ".repeat(left), cell, " ".repeat(right))
        }
        MarkdownTableAlignment::Left => format!("{}{}", cell, " ".repeat(total_padding)),
    }
}

pub(super) fn render_markdown_table_separator_row(column_widths: &[usize]) -> String {
    let mut rendered = String::new();
    for width in column_widths {
        rendered.push('│');
        rendered.push_str(&"─".repeat(width.saturating_add(2)));
    }
    rendered.push('│');
    rendered
}
