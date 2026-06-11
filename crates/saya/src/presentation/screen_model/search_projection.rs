//! 検索オーバーレイの投影。

use super::*;

pub(super) fn project_search_overlays(
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

pub(super) fn project_search_match_overlays(
    input: &ProjectionInput<'_>,
    line_projections: &[ScreenLineProjection],
    search_match: &crate::features::search::query::SearchMatch,
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

pub(super) fn resolve_search_overlay_display_bounds(
    input: &ProjectionInput<'_>,
    line_projections: &[ScreenLineProjection],
    search_match: &crate::features::search::query::SearchMatch,
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
                resolve_input_display_col_for_position(
                    input,
                    search_match.start_row - 1,
                    search_match.start_col,
                )
            },
            |projection| projection.logical_to_display_col(search_match.start_col),
        )
    } else {
        projection.map_or_else(
            || {
                line_number_offset_for_input(
                    input,
                    input.session_state.line_numbers() || input.session_state.relative_number(),
                )
            },
            |projection| projection.line_start_col,
        )
    };
    let end_col_exclusive = if row == search_match.end_row {
        projection.map_or_else(
            || {
                resolve_input_display_col_for_position(
                    input,
                    search_match.end_row - 1,
                    search_match.end_col,
                )
            },
            |projection| projection.logical_to_display_col(search_match.end_col),
        )
    } else {
        projection.map_or_else(
            || {
                input_visible_line_end_col_exclusive(
                    input,
                    row - 1,
                    input.session_state.line_numbers() || input.session_state.relative_number(),
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
