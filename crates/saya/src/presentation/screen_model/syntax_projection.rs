//! syntax chunk と tree-sitter 構文の投影。

use super::*;

pub(super) fn project_syntax_chunks(
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
    let mut projected = Vec::new();
    let buffer_language = buffer_language_id(input);

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
                || resolve_input_display_col_for_position(input, *absolute_row, chunk.start_col),
                |projection| projection.logical_to_display_col(chunk.start_col),
            );
            let end_col_exclusive = markdown_projection.map_or_else(
                || resolve_input_display_col_for_position(input, *absolute_row, chunk.end_col),
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
                language: markdown_embedded_language_id(input.markdown_document_map, *absolute_row)
                    .or_else(|| buffer_language.clone()),
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

pub(super) fn markdown_embedded_language_id(
    markdown_document_map: Option<&MarkdownDocumentMap>,
    absolute_row: usize,
) -> Option<String> {
    let map = markdown_document_map?;
    map.blocks.iter().find_map(|block| {
        let MarkdownBlockKind::FencedCodeBlock { info, .. } = &block.kind else {
            return None;
        };
        if absolute_row <= block.range.start.line || absolute_row >= block.range.end.line {
            return None;
        }
        let language = info
            .as_deref()
            .and_then(|info| info.split_whitespace().next())
            .and_then(normalize_language_id);
        if let Some(language) = &language {
            log::debug!(
                "[screen_model][syntax] markdown fenced code language resolved: row={}, language={}",
                absolute_row,
                language
            );
        }
        language
    })
}

pub(super) fn buffer_language_id(input: &ProjectionInput<'_>) -> Option<String> {
    let buffer = input
        .snapshot
        .buffers
        .iter()
        .find(|buffer| buffer.id == input.buffer_id)?;
    let path_hint = buffer_path_hint(buffer);
    let language = language_id_from_path_hint(path_hint);
    if path_hint != buffer.name {
        log::debug!(
            "[screen_model] using buffer document identity for syntax language: window_id={}, buffer_id={}, buffer_name={:?}, document_id={:?}, path_hint={:?}, language={:?}",
            input.window_id,
            input.buffer_id,
            buffer.name,
            buffer.document_id,
            path_hint,
            language
        );
    }
    language
}

pub(super) fn buffer_path_hint(buffer: &CoreBufferInfo) -> &str {
    buffer
        .document_id
        .as_deref()
        .and_then(|document_id| document_id.strip_prefix("file://"))
        .filter(|document_id| !document_id.is_empty())
        .unwrap_or(&buffer.name)
}

pub(super) fn language_id_from_path_hint(path: &str) -> Option<String> {
    let extension = path
        .rsplit('.')
        .next()
        .filter(|extension| *extension != path)?;
    match extension.trim().to_ascii_lowercase().as_str() {
        "go" | "rs" | "ts" | "tsx" | "md" => normalize_language_id(extension),
        _ => None,
    }
}

#[cfg(feature = "tree-sitter-syntax")]
pub(super) fn project_tree_sitter_syntax_chunks(
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
    let coverage_line_count = input_line_count(input);
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
            .min(coverage_line_count.saturating_sub(1));
        if start_row > end_row_inclusive {
            continue;
        }

        for absolute_row in start_row..=end_row_inclusive {
            let raw_line_len = input_line_at(input, absolute_row).len();
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
                language: tree_sitter_chunk_language_id(syntax, chunk),
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
pub(super) fn tree_sitter_chunk_language_id(
    syntax: &vim_core_rs::CoreTreeSitterRangeSyntax,
    chunk: &vim_core_rs::CoreTreeSitterChunk,
) -> Option<String> {
    syntax
        .embedded_regions
        .iter()
        .find_map(|region| {
            if !matches!(
                region.normalized_kind,
                vim_core_rs::CoreEmbeddedBlockKind::Syntax
            ) || chunk.range.start < region.content_range.start
                || chunk.range.end > region.content_range.end
            {
                return None;
            }
            let resolved = region.resolved_language.as_ref()?;
            if !matches!(
                resolved.status,
                vim_core_rs::CoreLanguageResolutionStatus::Resolved
            ) || !matches!(resolved.kind, vim_core_rs::CoreEmbeddedBlockKind::Syntax)
            {
                return None;
            }
            resolved
                .language_id
                .as_deref()
                .and_then(normalize_language_id)
        })
        .or_else(|| normalize_language_id(&syntax.provenance.language_id))
}

#[cfg(feature = "tree-sitter-syntax")]
pub(super) fn tree_sitter_coverage_contains_range(
    covered_ranges: &[vim_core_rs::CoreTextRange],
    range: vim_core_rs::CoreTextRange,
) -> bool {
    covered_ranges
        .iter()
        .any(|covered| covered.start <= range.start && range.end <= covered.end)
}

#[cfg(feature = "tree-sitter-syntax")]
pub(super) fn project_tree_sitter_chunk_display_range(
    input: &ProjectionInput<'_>,
    line_projections: &[ScreenLineProjection],
    absolute_row: usize,
    start: vim_core_rs::CoreTextPosition,
    end: vim_core_rs::CoreTextPosition,
    _line_numbers: bool,
) -> Option<(u16, u16)> {
    let markdown_projection = input.markdown_document_map.and_then(|_| {
        line_projections
            .iter()
            .find(|projection| projection.absolute_row == absolute_row)
    });
    let start_col = markdown_projection.map_or_else(
        || resolve_input_display_col_for_position(input, absolute_row, start.col),
        |projection| projection.logical_to_display_col(start.col),
    );
    let end_col_exclusive = markdown_projection.map_or_else(
        || resolve_input_display_col_for_position(input, absolute_row, end.col),
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
pub(super) fn map_tree_sitter_category(
    category: vim_core_rs::CoreSyntaxCategory,
) -> ScreenSyntaxCategory {
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
pub(super) fn map_tree_sitter_modifier(
    modifier: vim_core_rs::CoreSyntaxModifier,
) -> ScreenSyntaxModifier {
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
