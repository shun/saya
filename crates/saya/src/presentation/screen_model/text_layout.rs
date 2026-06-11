//! 入力テキストの行レイアウト・表示幅計算。

use super::*;

pub(super) fn input_line_count(input: &ProjectionInput<'_>) -> usize {
    input
        .line_range
        .map(|range| range.total_line_count.max(1))
        .unwrap_or_else(|| text_line_count(&input.snapshot.text).max(1))
}

pub(super) fn input_line_at<'a>(input: &'a ProjectionInput<'_>, row: usize) -> &'a str {
    if let Some(range) = input.line_range {
        if row >= range.start_row {
            let relative_row = row - range.start_row;
            if let Some(line) = range.lines.get(relative_row) {
                return line;
            }
        }
        return "";
    }
    input.snapshot.text.split('\n').nth(row).unwrap_or("")
}

pub(super) fn input_visible_rows<'a>(input: &'a ProjectionInput<'_>) -> Vec<(usize, &'a str)> {
    let body_height = input.body_height.max(1);
    if let Some(range) = input.line_range {
        return range
            .lines
            .iter()
            .enumerate()
            .skip(input.viewport_top.saturating_sub(range.start_row))
            .take(body_height)
            .map(|(index, line)| (range.start_row.saturating_add(index), line.as_str()))
            .collect();
    }
    input
        .snapshot
        .text
        .lines()
        .enumerate()
        .skip(input.viewport_top)
        .take(body_height)
        .collect()
}

pub(super) fn projected_input_line_number_width(input: &ProjectionInput<'_>) -> usize {
    if input.line_range.is_none() {
        if input.body_height == usize::MAX {
            return line_number_width(
                text_line_count(&input.snapshot.text),
                input.session_state.number_width(),
            );
        }
        let visible_line_upper_bound = input.viewport_top.saturating_add(input.body_height.max(1));
        let relevant_line_count = visible_line_upper_bound.max(input.cursor_row.saturating_add(1));
        return line_number_width(relevant_line_count, input.session_state.number_width());
    }
    let visible_line_upper_bound = input.viewport_top.saturating_add(input.body_height.max(1));
    let relevant_line_count = visible_line_upper_bound
        .max(input.cursor_row.saturating_add(1))
        .max(input_line_count(input));
    line_number_width(relevant_line_count, input.session_state.number_width())
}

pub(super) fn project_visible_input_text_lines(input: &ProjectionInput<'_>) -> Vec<String> {
    let body_height = input.body_height.max(1);
    let tab_size = input.session_state.tab_size().max(1);
    let number_width = projected_input_line_number_width(input);
    let line_numbers = input.session_state.line_numbers() || input.session_state.relative_number();
    let trail = input
        .session_state
        .list()
        .then(|| parse_listchars_trail(input.session_state.listchars()).unwrap_or('-'));
    let visible = input_visible_rows(input)
        .into_iter()
        .take(body_height)
        .map(|(index, line)| {
            // VisualLineLayout を用いて Vim 互換の content-col 起算でタブを展開する。
            // ガター(行番号)はレイアウト計算後に prefix として連結するだけなので、
            // layout 構築時の gutter_width は 0 を渡してコンテンツ表示テキストのみ得る。
            let layout = VisualLineLayout::build(line, tab_size, 0);
            let mut rendered = layout.display_text().to_string();
            if let Some(trail) = trail {
                rendered = render_list_line(&rendered, trail);
            }
            if line_numbers {
                let number = if input.session_state.relative_number() && index != input.cursor_row {
                    index.abs_diff(input.cursor_row)
                } else {
                    index + 1
                };
                rendered = format!("{:>width$} {}", number, rendered, width = number_width);
            }
            rendered
        })
        .collect::<Vec<_>>();

    log::debug!(
        "[screen_model] projected visible input text lines: window_id={}, buffer_id={}, viewport_top={}, body_height={}, number_width={}, visible_lines={}, source={}",
        input.window_id,
        input.buffer_id,
        input.viewport_top,
        body_height,
        number_width,
        visible.len(),
        if input.line_range.is_some() {
            "line_range"
        } else {
            "full_snapshot"
        }
    );

    visible
}

pub(super) fn render_list_line(line: &str, trail: char) -> String {
    let trimmed_len = line.trim_end_matches(' ').len();
    let mut rendered = String::with_capacity(line.len());
    rendered.push_str(&line[..trimmed_len]);
    rendered.extend(std::iter::repeat_n(
        trail,
        line.len().saturating_sub(trimmed_len),
    ));
    rendered
}

pub(super) fn parse_listchars_trail(listchars: &str) -> Option<char> {
    listchars.split(',').find_map(|part| {
        part.strip_prefix("trail:")
            .and_then(|value| value.chars().next())
    })
}

pub(super) fn resolve_input_cursor_col(
    input: &ProjectionInput<'_>,
    cursor_row: usize,
    cursor_col: usize,
) -> u16 {
    let line = input_line_at(input, cursor_row);
    let clamped_col = cursor_col.min(line.len());
    let boundary_col = clamp_to_char_boundary(line, clamped_col);
    let line_number_offset = line_number_offset_for_input(
        input,
        input.session_state.line_numbers() || input.session_state.relative_number(),
    );
    // VisualLineLayout を単一の真実として参照し、レンダリング側と
    // 完全に同じ raw↔display 写像でカーソル列を解決する。
    let layout = VisualLineLayout::build(
        line,
        input.session_state.tab_size().max(1),
        line_number_offset,
    );
    let display_col = layout.raw_to_screen(RawByteCol(boundary_col)).get();

    log::debug!(
        "[screen_model] resolved input cursor col: window_id={}, row={}, raw_col={}, boundary_col={}, content_width={}, line_number_offset={}, display_col={}, source={}",
        input.window_id,
        cursor_row,
        cursor_col,
        boundary_col,
        layout.content_width().get(),
        line_number_offset,
        display_col,
        if input.line_range.is_some() {
            "line_range"
        } else {
            "full_snapshot"
        }
    );

    display_col
}

pub(super) fn resolve_input_display_col_for_position(
    input: &ProjectionInput<'_>,
    cursor_row: usize,
    cursor_col: usize,
) -> u16 {
    resolve_input_cursor_col(input, cursor_row, cursor_col)
}

pub(super) fn resolve_input_display_col_after_inclusive_position(
    input: &ProjectionInput<'_>,
    cursor_row: usize,
    cursor_col: usize,
) -> u16 {
    let line = input_line_at(input, cursor_row);
    if line.is_empty() {
        return resolve_input_display_col_for_position(input, cursor_row, cursor_col);
    }
    let clamped_col = clamp_to_char_boundary(line, cursor_col.min(line.len()));
    let next_col = line[clamped_col..]
        .chars()
        .next()
        .map(|ch| clamped_col + ch.len_utf8())
        .unwrap_or(clamped_col);
    resolve_input_display_col_for_position(input, cursor_row, next_col)
}

pub(super) fn line_number_offset_for_input(input: &ProjectionInput<'_>, line_numbers: bool) -> u16 {
    if line_numbers {
        u16::try_from(projected_input_line_number_width(input).saturating_add(1))
            .unwrap_or(u16::MAX)
    } else {
        0
    }
}

pub(super) fn input_visible_line_end_col_exclusive(
    input: &ProjectionInput<'_>,
    row: usize,
    line_numbers: bool,
) -> u16 {
    let line = input_line_at(input, row);
    resolve_input_display_col_for_position(input, row, line.len())
        .max(line_number_offset_for_input(input, line_numbers))
}

pub(super) fn line_number_width(line_count: usize, configured_width: u16) -> usize {
    line_count
        .max(1)
        .to_string()
        .len()
        .max(usize::from(configured_width.max(1)))
}

pub(super) fn resolve_cursor_row(
    cursor_row: usize,
    viewport_top: usize,
    body_height: usize,
) -> u16 {
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

pub(super) fn resolve_projected_cursor_row(
    input: &ProjectionInput<'_>,
    line_projections: &[ScreenLineProjection],
) -> u16 {
    if let Some((display_row, _)) = line_projections.iter().enumerate().find(|(_, projection)| {
        projection.absolute_row == input.cursor_row
            && !projection_is_synthetic_display_line(projection)
    }) {
        let display_row = display_row.min(input.body_height.max(1).saturating_sub(1));
        let display_row = u16::try_from(display_row).unwrap_or(u16::MAX);
        log::debug!(
            "[screen_model] resolved projected cursor row: absolute_row={}, viewport_top={}, display_row={}, line_projections={}",
            input.cursor_row,
            input.viewport_top,
            display_row,
            line_projections.len()
        );
        return display_row;
    }

    resolve_cursor_row(input.cursor_row, input.viewport_top, input.body_height)
}

pub(super) fn clamp_to_char_boundary(text: &str, col: usize) -> usize {
    let mut boundary = col.min(text.len());
    while boundary > 0 && !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    boundary
}

pub(super) fn display_width(text: &str, tab_size: usize) -> usize {
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

pub(super) fn next_tab_stop(display_col: usize, tab_size: usize) -> usize {
    let tab_size = tab_size.max(1);
    display_col + (tab_size - (display_col % tab_size)).min(tab_size)
}

pub(super) fn char_display_width(ch: char) -> usize {
    UnicodeWidthChar::width(ch).unwrap_or(0)
}
