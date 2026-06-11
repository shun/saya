//! Markdown WYSIWYG 表示投影。

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MarkdownDisplayProjection {
    pub(super) lines: Vec<String>,
    pub(super) line_projections: Vec<ScreenLineProjection>,
}

pub(super) fn project_markdown_display_lines(
    input: &ProjectionInput<'_>,
) -> MarkdownDisplayProjection {
    let fallback_lines = project_visible_input_text_lines(input);
    let line_projections = project_markdown_line_projections(input);
    let has_expanded_source_row = has_expanded_markdown_source_row(&line_projections);
    if !has_expanded_source_row
        && !line_projections
            .iter()
            .any(projection_is_synthetic_display_line)
    {
        return MarkdownDisplayProjection {
            lines: fallback_lines,
            line_projections,
        };
    }

    let fallback_by_absolute_row = fallback_lines
        .iter()
        .zip(input_visible_rows(input))
        .map(|(line, (absolute_row, _))| (absolute_row, line.clone()))
        .collect::<BTreeMap<_, _>>();
    let lines = line_projections
        .iter()
        .map(|projection| {
            if projection_is_synthetic_display_line(projection) {
                return " ".repeat(usize::from(projection.line_start_col));
            }
            fallback_by_absolute_row
                .get(&projection.absolute_row)
                .cloned()
                .unwrap_or_else(|| projection.raw_text.clone())
        })
        .collect::<Vec<_>>();

    log::debug!(
        "[screen_model] markdown display lines expanded: window_id={}, fallback_lines={}, display_lines={}, line_projections={}",
        input.window_id,
        fallback_lines.len(),
        lines.len(),
        line_projections.len()
    );

    MarkdownDisplayProjection {
        lines,
        line_projections,
    }
}

pub(super) fn has_expanded_markdown_source_row(line_projections: &[ScreenLineProjection]) -> bool {
    let mut seen = BTreeSet::new();
    line_projections
        .iter()
        .any(|projection| !seen.insert(projection.absolute_row))
}

pub(super) fn projection_is_synthetic_display_line(projection: &ScreenLineProjection) -> bool {
    projection.raw_text.is_empty()
        && !projection.display_text.is_empty()
        && projection.cells.is_empty()
}

pub(super) fn markdown_projection_source_text(input: &ProjectionInput<'_>) -> String {
    if !input.snapshot.text.is_empty() {
        return input.snapshot.text.clone();
    }
    let Some(range) = input.line_range else {
        return String::new();
    };
    let mut source = "\n".repeat(range.start_row);
    source.push_str(&range.lines.join("\n"));
    log::debug!(
        "[screen_model] markdown projection source reconstructed from line_range: window_id={}, start_row={}, lines={}, byte_len={}",
        input.window_id,
        range.start_row,
        range.lines.len(),
        source.len()
    );
    source
}

pub(super) fn project_markdown_table_block_projections(
    map: Option<&MarkdownDocumentMap>,
    source_text: &str,
    absolute_row: usize,
    line_start_col: u16,
    available_width: usize,
) -> Option<Vec<ScreenLineProjection>> {
    let map = map?;
    let block = map.blocks.iter().find(|block| {
        matches!(block.kind, MarkdownBlockKind::Table)
            && (block.range.start.line..=block.range.end.line).contains(&absolute_row)
    })?;
    let rendered_rows = render_markdown_table_block(
        source_text,
        block.range.start.line,
        block.range.end.line,
        available_width,
    )?;
    let source_lines = source_text.lines().collect::<Vec<_>>();
    let projections = rendered_rows
        .into_iter()
        .enumerate()
        .filter(|(index, rendered)| {
            if let Some(source_line) = rendered.source_line {
                return source_line >= absolute_row;
            }
            *index == 0 && absolute_row == block.range.start.line
                || *index > block.range.end.line.saturating_sub(block.range.start.line)
        })
        .map(|(_, rendered)| {
            let raw_text = rendered
                .source_line
                .and_then(|line| source_lines.get(line).copied())
                .unwrap_or_default();
            log::debug!(
                "[screen_model] markdown table display row rendered: table_start={}, table_end={}, source_line={:?}, raw_len={}, rendered_width={}, text={:?}",
                block.range.start.line,
                block.range.end.line,
                rendered.source_line,
                raw_text.len(),
                display_width(&rendered.text, 1),
                rendered.text
            );
            project_rendered_markdown_table_line(
                rendered.source_line.unwrap_or(block.range.start.line),
                raw_text,
                &rendered.text,
                line_start_col,
            )
        })
        .collect::<Vec<_>>();
    Some(projections)
}

pub(super) fn project_markdown_line_projections(
    input: &ProjectionInput<'_>,
) -> Vec<ScreenLineProjection> {
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
    let number_width = projected_input_line_number_width(input);
    let line_start_col = if line_number_enabled {
        u16::try_from(number_width + 1).unwrap_or(u16::MAX)
    } else {
        0
    };
    let raw_expansion = resolve_markdown_raw_expansion(input);
    // Width available to the rendered table body: the pane minus the line-number
    // gutter. `0` means the pane width is unknown, leaving the table unconstrained.
    let table_available_width =
        usize::from(input.rect.width).saturating_sub(usize::from(line_start_col));

    let visible_rows = if input.line_range.is_some() {
        input_visible_rows(input)
    } else {
        input
            .snapshot
            .text
            .split('\n')
            .enumerate()
            .skip(input.viewport_top)
            .take(input.body_height.max(1))
            .collect::<Vec<_>>()
    };
    let source_text = markdown_projection_source_text(input);
    let tab_size = usize::from(input.session_state.tab_size().max(1));
    let mut projections = Vec::new();
    let mut visible_iter = visible_rows.into_iter().peekable();
    while let Some((absolute_row, raw_text)) = visible_iter.next() {
        let keep_raw = raw_expansion.contains_row(absolute_row);
        if !keep_raw
            && let Some(table_projections) = project_markdown_table_block_projections(
                markdown_document_map,
                source_text.as_str(),
                absolute_row,
                line_start_col,
                table_available_width,
            )
        {
            let table_end = table_projections
                .iter()
                .filter(|projection| !projection_is_synthetic_display_line(projection))
                .map(|projection| projection.absolute_row)
                .max()
                .unwrap_or(absolute_row);
            log::debug!(
                "[screen_model] markdown table display block projected: start_row={}, end_row={}, display_rows={}",
                absolute_row,
                table_end,
                table_projections.len()
            );
            projections.extend(table_projections);
            while visible_iter
                .peek()
                .is_some_and(|(row, _)| *row <= table_end)
            {
                visible_iter.next();
            }
            continue;
        }

        projections.push(project_markdown_line_projection(
            absolute_row,
            raw_text,
            markdown_document_map,
            source_text.as_str(),
            keep_raw,
            tab_size,
            line_start_col,
        ));
    }

    log::debug!(
        "[screen_model] markdown line projections built: window_id={}, visible_rows={}, viewport_top={}, line_start_col={}, markdown_metadata_present={}, raw_expansion={:?}",
        input.window_id,
        projections.len(),
        input.viewport_top,
        line_start_col,
        markdown_document_map.is_some(),
        raw_expansion
    );
    if std::env::var_os("SAYA_TRACE_RENDER").is_some() {
        log::debug!(
            "[saya-trace][screen_model][markdown] window_id={} cursor_row={} active={} metadata={} raw_expansion={:?}",
            input.window_id,
            input.cursor_row,
            input.is_active,
            markdown_document_map.is_some(),
            raw_expansion
        );
    }

    projections
}

pub(super) fn project_markdown_line_projection(
    absolute_row: usize,
    raw_text: &str,
    markdown_document_map: Option<&MarkdownDocumentMap>,
    source_text: &str,
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
            .map(|map| markdown_conceal_ranges_for_line(map, source_text, absolute_row, raw_text))
            .unwrap_or_default()
    };
    let mut display_text = String::new();
    let mut spans = Vec::new();
    let mut cells = Vec::new();
    let mut display_col = usize::from(line_start_col);
    let mut raw_col = 0usize;
    // 行全体のレイアウトを 1 度だけ構築し、raw 区間ごとに同じ写像を共有する。
    // タブ stop は content_col 起算で計算され、ガターはレンダリング時に
    // line_start_col として加算されるだけ。
    let layout = VisualLineLayout::build(
        raw_text,
        u16::try_from(tab_size.max(1)).unwrap_or(u16::MAX),
        line_start_col,
    );

    for operation in conceal_ranges {
        if operation.raw_start_col > raw_col {
            append_raw_projection_segment(
                absolute_row,
                &layout,
                raw_col,
                operation.raw_start_col,
                &mut display_col,
                &mut display_text,
                &mut spans,
                &mut cells,
            );
        }
        let operation_raw_end_col = operation.raw_end_col;
        append_replacement_projection_segment(
            absolute_row,
            raw_text,
            operation,
            &mut display_col,
            &mut display_text,
            &mut spans,
            &mut cells,
        );
        raw_col = raw_col.max(operation_raw_end_col);
    }

    if raw_col < raw_text.len() {
        append_raw_projection_segment(
            absolute_row,
            &layout,
            raw_col,
            raw_text.len(),
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
pub(super) enum MarkdownRawExpansion {
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

pub(super) fn resolve_markdown_raw_expansion(input: &ProjectionInput<'_>) -> MarkdownRawExpansion {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MarkdownProjectionOperation {
    raw_start_col: usize,
    raw_end_col: usize,
    replacement: Option<String>,
}

pub(super) fn markdown_conceal_ranges_for_line(
    map: &MarkdownDocumentMap,
    _source_text: &str,
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

pub(super) fn heading_marker_range(
    raw_text: &str,
    level: u8,
) -> Option<MarkdownProjectionOperation> {
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

pub(super) fn checkbox_marker_range(
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
        replacement: Some(marker.1.to_string()),
    })
}

pub(super) fn list_marker_range(
    raw_text: &str,
    ordered: bool,
) -> Option<MarkdownProjectionOperation> {
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
        replacement: Some("• ".to_string()),
    })
}

pub(super) fn append_raw_projection_segment(
    absolute_row: usize,
    layout: &VisualLineLayout,
    raw_start_col: usize,
    raw_end_col: usize,
    display_col: &mut usize,
    display_text: &mut String,
    spans: &mut Vec<ScreenDisplaySpan>,
    cells: &mut Vec<ScreenCellMapping>,
) {
    let raw_text = layout.raw_text();
    let raw_start_col = clamp_to_char_boundary(raw_text, raw_start_col.min(raw_text.len()));
    let raw_end_col = clamp_to_char_boundary(raw_text, raw_end_col.min(raw_text.len()));
    if raw_end_col <= raw_start_col {
        return;
    }
    let display_start_col = *display_col;
    for cell in layout.cells() {
        let cell_raw_start = cell.raw_start().get();
        let cell_raw_end = cell.raw_end().get();
        if cell_raw_end <= raw_start_col {
            continue;
        }
        if cell_raw_start >= raw_end_col {
            break;
        }
        let width = usize::from(cell.content_width());
        let ch = raw_text[cell_raw_start..cell_raw_end]
            .chars()
            .next()
            .expect("layout cell must cover at least one char");
        if ch == '\t' {
            display_text.extend(std::iter::repeat_n(' ', width));
        } else {
            display_text.push(ch);
        }
        cells.push(ScreenCellMapping {
            display_col: u16::try_from(*display_col).unwrap_or(u16::MAX),
            display_end_col_exclusive: u16::try_from(display_col.saturating_add(width))
                .unwrap_or(u16::MAX),
            raw_start_col: cell_raw_start,
            raw_end_col: cell_raw_end,
        });
        *display_col = display_col.saturating_add(width);
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

pub(super) fn append_replacement_projection_segment(
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
    if let Some(replacement) = operation.replacement.as_deref() {
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
