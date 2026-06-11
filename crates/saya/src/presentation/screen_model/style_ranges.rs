//! Markdown / filer スタイルレンジの投影。

use super::*;

pub(super) fn project_markdown_style_ranges(
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

pub(super) fn append_block_style_ranges(
    ranges: &mut Vec<ScreenMarkdownStyleRange>,
    line_projections: &[ScreenLineProjection],
    range: crate::presentation::markdown::structure::MarkdownTextRange,
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

pub(super) fn project_filer_style_ranges(
    input: &ProjectionInput<'_>,
    line_projections: &[ScreenLineProjection],
) -> Vec<ScreenFilerStyleRange> {
    let Some(directory_buffer) = input.session_state.directory_buffer() else {
        return Vec::new();
    };
    let Some(buffer) = input
        .snapshot
        .buffers
        .iter()
        .find(|buffer| buffer.id == input.buffer_id)
    else {
        return Vec::new();
    };
    let buffer_path = std::path::Path::new(&buffer.name);
    if buffer_path != directory_buffer.root_path
        && !line_range_matches_directory_buffer(input.line_range, directory_buffer)
        && !(input.is_active && buffer_path.is_dir())
    {
        return Vec::new();
    }
    let theme = input.session_state.resolved_theme();
    let viewport_bottom = input
        .viewport_top
        .saturating_add(input.body_height.max(1))
        .saturating_sub(1);
    let mut ranges = Vec::new();
    for (entry_index, entry) in directory_buffer.entries.iter().enumerate() {
        if entry_index < input.viewport_top || entry_index > viewport_bottom {
            continue;
        }
        let Some(projection) = line_projections
            .iter()
            .find(|projection| projection.absolute_row == entry_index)
        else {
            continue;
        };
        let row = u16::try_from(entry_index.saturating_sub(input.viewport_top)).unwrap_or(u16::MAX);
        let end_col_exclusive = projection.logical_to_display_col(projection.raw_text.len());
        let key = filer_key_for_entry_kind(entry.kind);
        ranges.push(ScreenFilerStyleRange {
            row,
            start_col: projection.line_start_col,
            end_col_exclusive,
            key,
            style: theme.filer_style(key).cloned().unwrap_or_default(),
        });
        if input.session_state.is_directory_entry_marked(entry) {
            ranges.push(ScreenFilerStyleRange {
                row,
                start_col: projection.line_start_col,
                end_col_exclusive,
                key: FilerSemanticStyleKey::Marked,
                style: theme
                    .filer_style(FilerSemanticStyleKey::Marked)
                    .cloned()
                    .unwrap_or_default(),
            });
        }
    }
    log::debug!(
        "[screen_model][filer] projected filer style ranges: window_id={}, root_path={}, ranges={}",
        input.window_id,
        directory_buffer.root_path.display(),
        ranges.len()
    );
    ranges
}

pub(super) fn line_range_matches_directory_buffer(
    line_range: Option<&CoreBufferLineRange>,
    directory_buffer: &crate::app::session::DirectoryBufferState,
) -> bool {
    let Some(line_range) = line_range else {
        return false;
    };
    let directory_lines = directory_buffer.display_text.lines().collect::<Vec<_>>();
    if line_range.start_row >= directory_lines.len()
        || line_range.start_row.saturating_add(line_range.lines.len()) > directory_lines.len()
    {
        return false;
    }
    line_range
        .lines
        .iter()
        .zip(directory_lines.iter().skip(line_range.start_row))
        .all(|(line, directory_line)| line == directory_line)
}

pub(super) fn filer_key_for_entry_kind(kind: DirectoryBufferEntryKind) -> FilerSemanticStyleKey {
    match kind {
        DirectoryBufferEntryKind::Directory => FilerSemanticStyleKey::Directory,
        DirectoryBufferEntryKind::File => FilerSemanticStyleKey::File,
        DirectoryBufferEntryKind::Symlink => FilerSemanticStyleKey::Symlink,
        DirectoryBufferEntryKind::Other => FilerSemanticStyleKey::Other,
    }
}

pub(super) fn project_markdown_range_display_bounds(
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
