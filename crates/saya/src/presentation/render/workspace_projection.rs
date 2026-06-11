//! ワークスペース描画の前段で必要となる投影データの収集。
//!
//! バッファの行範囲・行数、検索ハイライト状態、置換プレビュー、
//! シンタックスハイライト（tree-sitter 含む）、Markdown ドキュメント
//! マップなどを `CoreSnapshot` から集約する純粋関数群。描画本体
//! （`build_workspace_render_output`）から呼ばれる。

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::features::search::query::SearchVisibleState;
use crate::features::search::substitute_preview::build_substitute_preview_render;
use crate::presentation::markdown::structure::{
    MarkdownCacheStatus, MarkdownDocumentMap, MarkdownMetadataCache, MarkdownMetadataKey,
};
use crate::presentation::viewport::WindowViewportStore;
use vim_core_rs::CoreBufferLineRange;

/// バッファテキストの行数（最低 1）。シンタックス収集の総行数フォールバック。
pub(crate) fn buffer_line_count(text: &str) -> usize {
    text.lines().count().max(1)
}

pub fn collect_workspace_line_ranges(
    core_bridge: &crate::core::bridge::CoreBridge,
    snapshot: &vim_core_rs::CoreSnapshot,
    viewport_store: &WindowViewportStore,
) -> BTreeMap<i32, CoreBufferLineRange> {
    let mut line_ranges = BTreeMap::new();
    for window in &snapshot.windows {
        let body_height = window.height.saturating_sub(1).max(1);
        let viewport_top = viewport_store
            .get(window.id)
            .map(|viewport| viewport.top_line())
            .unwrap_or_else(|| window.topline.saturating_sub(1));
        let requested_lines = body_height.max(1);
        match core_bridge.buffer_line_range(window.buf_id, viewport_top, requested_lines) {
            Some(range) => {
                log::debug!(
                    "[main] workspace line range collected: window_id={}, buffer_id={}, viewport_top={}, requested_lines={}, returned_lines={}, total_line_count={}, source_revision={:?}",
                    window.id,
                    window.buf_id,
                    viewport_top,
                    requested_lines,
                    range.lines.len(),
                    range.total_line_count,
                    range.source_revision
                );
                line_ranges.insert(window.id, range);
            }
            None => {
                log::debug!(
                    "[main] workspace line range missing: window_id={}, buffer_id={}, viewport_top={}, requested_lines={}",
                    window.id,
                    window.buf_id,
                    viewport_top,
                    requested_lines
                );
            }
        }
    }
    line_ranges
}

pub fn collect_workspace_buffer_line_counts(
    core_bridge: &crate::core::bridge::CoreBridge,
    snapshot: &vim_core_rs::CoreSnapshot,
) -> BTreeMap<i32, usize> {
    let mut line_counts = BTreeMap::new();
    let mut seen_buffers = BTreeSet::new();
    for window in &snapshot.windows {
        if !seen_buffers.insert(window.buf_id) {
            continue;
        }
        match core_bridge.buffer_line_range(window.buf_id, 0, 0) {
            Some(range) => {
                log::debug!(
                    "[main] workspace buffer line count collected: buffer_id={}, total_line_count={}, source_revision={:?}",
                    window.buf_id,
                    range.total_line_count,
                    range.source_revision
                );
                line_counts.insert(window.buf_id, range.total_line_count);
            }
            None => {
                log::debug!(
                    "[main] workspace buffer line count missing: buffer_id={}",
                    window.buf_id
                );
            }
        }
    }
    line_counts
}

pub fn trace_workspace_render_pipeline(
    phase: &str,
    snapshot_text: &str,
    workspace_model: &crate::presentation::screen_model::WorkspaceScreenModel,
) {
    if std::env::var_os("SAYA_TRACE_RENDER").is_none() {
        return;
    }

    let Some(active_pane) = workspace_model
        .panes
        .iter()
        .find(|pane| pane.window_id == workspace_model.active_window_id)
    else {
        return;
    };
    let absolute_row = 6usize;
    let snapshot_line = snapshot_text.lines().nth(absolute_row).unwrap_or("");
    let viewport_top = usize::from(active_pane.rect.y);
    let visible_row = absolute_row.checked_sub(viewport_top);
    let projected_line = visible_row
        .and_then(|row| active_pane.lines.get(row))
        .map(String::as_str)
        .unwrap_or("");
    let projected_display = visible_row
        .and_then(|row| active_pane.line_projections.get(row))
        .map(|projection| projection.display_text.as_str())
        .unwrap_or("");

    log::debug!(
        "[saya-trace][main][{phase}] viewport_top={viewport_top} abs_row=7 snapshot={snapshot_line:?} visible={projected_line:?} display={projected_display:?}"
    );
}

pub fn collect_workspace_substitute_preview_states(
    snapshot: &vim_core_rs::CoreSnapshot,
    line_ranges: &mut BTreeMap<i32, CoreBufferLineRange>,
    command_line_prompt: Option<char>,
    command_line_buffer: &str,
) -> BTreeMap<i32, SearchVisibleState> {
    let mut search_states = BTreeMap::new();
    if command_line_prompt != Some(':') {
        return search_states;
    }

    for window in &snapshot.windows {
        let Some(line_range) = line_ranges.get(&window.id).cloned() else {
            log::debug!(
                "[main][substitute_preview] skipped window because visible line range is missing: window_id={}, buffer_id={}",
                window.id,
                window.buf_id
            );
            continue;
        };
        let Some(preview) = build_substitute_preview_render(
            window,
            &line_range,
            command_line_prompt,
            command_line_buffer,
        ) else {
            continue;
        };
        log::debug!(
            "[main][substitute_preview] using live substitute preview render state: window_id={}, matches={}, pattern={:?}, preview_lines={}",
            window.id,
            preview.search_state.matches.len(),
            preview.search_state.input_pattern,
            preview.line_range.lines.len()
        );
        line_ranges.insert(window.id, preview.line_range);
        search_states.insert(window.id, preview.search_state);
    }
    search_states
}

pub fn collect_workspace_syntax_lines(
    core_bridge: &crate::core::bridge::CoreBridge,
    snapshot: &vim_core_rs::CoreSnapshot,
    viewport_store: &WindowViewportStore,
    line_ranges: &BTreeMap<i32, CoreBufferLineRange>,
) -> BTreeMap<i32, BTreeMap<usize, Vec<vim_core_rs::CoreSyntaxChunk>>> {
    let mut syntax_lines = BTreeMap::new();
    for window in &snapshot.windows {
        let line_count = line_ranges
            .get(&window.id)
            .map(|range| range.total_line_count)
            .unwrap_or_else(|| buffer_line_count(&snapshot.text));
        let body_height = window.height.saturating_sub(1).max(1);
        let viewport_top = viewport_store
            .get(window.id)
            .map(|viewport| viewport.top_line())
            .unwrap_or_else(|| window.topline.saturating_sub(1));
        let viewport_bottom = viewport_top.saturating_add(body_height).saturating_sub(1);
        let mut window_lines = BTreeMap::new();

        for absolute_row in viewport_top..=viewport_bottom {
            if absolute_row >= line_count {
                break;
            }
            let lnum = i64::try_from(absolute_row.saturating_add(1)).unwrap_or(i64::MAX);
            match core_bridge.get_line_syntax(window.id, lnum) {
                Ok(chunks) if !chunks.is_empty() => {
                    if std::env::var_os("SAYA_TRACE_RENDER").is_some() {
                        log::debug!(
                            "[saya-trace][main][syntax] window_id={} row={} lnum={} chunks={} names={:?}",
                            window.id,
                            absolute_row,
                            lnum,
                            chunks.len(),
                            chunks
                                .iter()
                                .take(8)
                                .filter_map(|chunk| chunk.name.as_deref())
                                .collect::<Vec<_>>()
                        );
                    }
                    log::debug!(
                        "[main] syntax chunks collected: window_id={}, row={}, lnum={}, chunks={}",
                        window.id,
                        absolute_row,
                        lnum,
                        chunks.len()
                    );
                    window_lines.insert(absolute_row, chunks);
                }
                Ok(_) => {}
                Err(error) => {
                    log::debug!(
                        "[main] syntax chunk query skipped for line: window_id={}, row={}, lnum={}, error={:?}",
                        window.id,
                        absolute_row,
                        lnum,
                        error
                    );
                }
            }
        }

        if !window_lines.is_empty() {
            log::debug!(
                "[main] syntax lines collected for window: window_id={}, visible_lines={}",
                window.id,
                window_lines.len()
            );
            syntax_lines.insert(window.id, window_lines);
        }
    }
    syntax_lines
}

#[cfg(feature = "tree-sitter-syntax")]
pub fn collect_workspace_tree_sitter_syntax(
    core_bridge: &mut crate::core::bridge::CoreBridge,
    snapshot: &vim_core_rs::CoreSnapshot,
    viewport_store: &WindowViewportStore,
    line_ranges: &BTreeMap<i32, CoreBufferLineRange>,
) -> BTreeMap<i32, vim_core_rs::CoreTreeSitterRangeSyntax> {
    let mut syntax_by_window = BTreeMap::new();
    for window in &snapshot.windows {
        let line_count = line_ranges
            .get(&window.id)
            .map(|range| range.total_line_count)
            .unwrap_or_else(|| buffer_line_count(&snapshot.text));
        let Some(buffer) = snapshot
            .buffers
            .iter()
            .find(|buffer| buffer.id == window.buf_id)
        else {
            log::debug!(
                "[main] Tree-sitter syntax skipped because window buffer is missing: window_id={}, buffer_id={}",
                window.id,
                window.buf_id
            );
            continue;
        };
        let body_height = window.height.saturating_sub(1).max(1);
        let viewport_top = viewport_store
            .get(window.id)
            .map(|viewport| viewport.top_line())
            .unwrap_or_else(|| window.topline.saturating_sub(1));
        let viewport_bottom = viewport_top
            .saturating_add(body_height)
            .saturating_sub(1)
            .min(line_count.saturating_sub(1));
        if viewport_top > viewport_bottom {
            log::debug!(
                "[main] Tree-sitter syntax skipped because visible range is empty: window_id={}, viewport_top={}, viewport_bottom={}",
                window.id,
                viewport_top,
                viewport_bottom
            );
            continue;
        }
        let buffer_path_hint = buffer_path_hint(buffer);
        if buffer_path_hint != buffer.name {
            log::debug!(
                "[main] using buffer document identity for Tree-sitter language hint: window_id={}, buffer_id={}, buffer_name={:?}, path_hint={:?}",
                window.id,
                buffer.id,
                buffer.name,
                buffer_path_hint
            );
        }
        let range = vim_core_rs::CoreTextRange {
            start: vim_core_rs::CoreTextPosition {
                row: viewport_top,
                col: 0,
            },
            end: vim_core_rs::CoreTextPosition {
                row: viewport_bottom.saturating_add(1),
                col: 0,
            },
        };
        let root_language = vim_core_rs::VimCoreSession::resolve_tree_sitter_root_language(
            vim_core_rs::CoreRootLanguageResolutionRequest {
                range,
                vim_filetype: None,
                buffer_name: (!buffer_path_hint.is_empty()).then(|| buffer_path_hint.to_string()),
                host_language_hint: None,
            },
        );
        if !matches!(
            root_language.status,
            vim_core_rs::CoreLanguageResolutionStatus::Resolved
        ) {
            log::debug!(
                "[main] Tree-sitter syntax skipped because vim-core-rs could not resolve a supported language: window_id={}, buffer_id={}, buffer_name={:?}, path_hint={:?}, resolution={:?}",
                window.id,
                buffer.id,
                buffer.name,
                buffer_path_hint,
                root_language
            );
            continue;
        }
        let request = vim_core_rs::CoreTreeSitterPreparationRequest {
            buffer_id: buffer.id,
            source_revision: Some(buffer.source_revision),
            range,
            vim_filetype: None,
            buffer_name: (!buffer_path_hint.is_empty()).then(|| buffer_path_hint.to_string()),
            host_language_hint: None,
            snapshot_policy: vim_core_rs::CoreTreeSitterSnapshotPolicy::default(),
        };
        let preparation = match core_bridge.request_tree_sitter_syntax_preparation(request) {
            Ok(preparation) => preparation,
            Err(error) => {
                log::debug!(
                    "[main] Tree-sitter preparation request failed: window_id={}, buffer_id={}, source_revision={:?}, error={:?}",
                    window.id,
                    buffer.id,
                    buffer.source_revision,
                    error
                );
                continue;
            }
        };
        while let Some(completed) = core_bridge.poll_tree_sitter_preparation() {
            log::debug!(
                "[main] Tree-sitter preparation poll drained: request_id={}, buffer_id={}, source_revision={:?}, status={:?}, chunks={}",
                completed.request_id.value,
                completed.syntax.buffer_id,
                completed.syntax.source_revision,
                completed.syntax.status,
                completed.syntax.chunks.len()
            );
        }
        let Some(syntax) =
            core_bridge.query_tree_sitter_syntax_range(buffer.id, buffer.source_revision, range)
        else {
            log::debug!(
                "[main] Tree-sitter syntax cache unavailable after preparation: window_id={}, request_id={}, buffer_id={}, source_revision={:?}, preparation_status={:?}",
                window.id,
                preparation.request_id.value,
                buffer.id,
                buffer.source_revision,
                preparation.status
            );
            continue;
        };
        if syntax.source_revision != buffer.source_revision
            || !matches!(syntax.status, vim_core_rs::CoreTreeSitterStatus::Prepared)
            || syntax.has_error
            || !syntax.error_ranges.is_empty()
            || !matches!(
                syntax.budget_status,
                vim_core_rs::CoreTreeSitterBudgetStatus::WithinBudget
            )
            || !tree_sitter_coverage_contains_range(&syntax.covered_ranges, range)
        {
            log::debug!(
                "[main] Tree-sitter syntax not renderable as fresh highlight: window_id={}, buffer_id={}, syntax_revision={:?}, buffer_revision={:?}, status={:?}, has_error={}, error_ranges={}, covered_ranges={}, budget_status={:?}",
                window.id,
                buffer.id,
                syntax.source_revision,
                buffer.source_revision,
                syntax.status,
                syntax.has_error,
                syntax.error_ranges.len(),
                syntax.covered_ranges.len(),
                syntax.budget_status
            );
            continue;
        }
        log::debug!(
            "[main] Tree-sitter syntax render data collected: window_id={}, buffer_id={}, source_revision={:?}, chunks={}, provenance={:?}",
            window.id,
            buffer.id,
            syntax.source_revision,
            syntax.chunks.len(),
            syntax.provenance
        );
        syntax_by_window.insert(window.id, syntax);
    }
    syntax_by_window
}

#[cfg(feature = "tree-sitter-syntax")]
pub(crate) fn tree_sitter_coverage_contains_range(
    covered_ranges: &[vim_core_rs::CoreTextRange],
    range: vim_core_rs::CoreTextRange,
) -> bool {
    covered_ranges
        .iter()
        .any(|covered| covered.start <= range.start && range.end <= covered.end)
}

pub fn collect_workspace_markdown_document_maps(
    markdown_metadata_cache: &mut MarkdownMetadataCache,
    session_state: &crate::app::session::EditorSessionState,
    core_bridge: &crate::core::bridge::CoreBridge,
    snapshot: &vim_core_rs::CoreSnapshot,
) -> BTreeMap<i32, Arc<MarkdownDocumentMap>> {
    if !session_state.markdown_render() {
        if std::env::var_os("SAYA_TRACE_RENDER").is_some() {
            log::debug!(
                "[saya-trace][main][markdown] collected=false reason=markdownrender_off target_path={:?}",
                session_state.target_path()
            );
        }
        log::debug!(
            "[main] skipping markdown metadata collection because markdownrender is off: target_path={:?}",
            session_state.target_path()
        );
        return BTreeMap::new();
    }

    let mut document_maps_by_buffer = BTreeMap::<i32, Arc<MarkdownDocumentMap>>::new();
    let mut cache_status_by_buffer = BTreeMap::new();
    for window in &snapshot.windows {
        if document_maps_by_buffer.contains_key(&window.buf_id) {
            continue;
        }
        let Some(buffer) = snapshot
            .buffers
            .iter()
            .find(|buffer| buffer.id == window.buf_id)
        else {
            log::debug!(
                "[main] skipping markdown metadata collection because window buffer is missing: window_id={}, buffer_id={}",
                window.id,
                window.buf_id
            );
            continue;
        };
        let buffer_path_hint = buffer_path_hint(buffer);
        if buffer_path_hint != buffer.name {
            log::debug!(
                "[main] using buffer document identity for markdown metadata: window_id={}, buffer_id={}, buffer_name={:?}, path_hint={:?}",
                window.id,
                buffer.id,
                buffer.name,
                buffer_path_hint
            );
        }
        if !is_markdown_buffer_name(buffer_path_hint) {
            log::debug!(
                "[main] skipping markdown metadata collection because buffer is not markdown: window_id={}, buffer_id={}, buffer_name={:?}, path_hint={:?}",
                window.id,
                buffer.id,
                buffer.name,
                buffer_path_hint
            );
            continue;
        }
        let key = MarkdownMetadataKey {
            buffer_id: i64::from(buffer.id),
            revision: buffer.source_revision.value,
        };
        if let Some(document_map) = markdown_metadata_cache.cached_document_map(key) {
            cache_status_by_buffer.insert(buffer.id, MarkdownCacheStatus::Hit);
            document_maps_by_buffer.insert(buffer.id, document_map);
            continue;
        }
        if !window.is_active {
            log::debug!(
                "[main] skipping inactive markdown metadata cache miss to avoid fetching unrelated buffer text: window_id={}, buffer_id={}, buffer_name={:?}",
                window.id,
                buffer.id,
                buffer.name
            );
            continue;
        }
        let outcome = markdown_metadata_cache.document_map_with_source(key, || {
            let Some(line_count_range) = core_bridge.buffer_line_range(buffer.id, 0, 0) else {
                log::debug!(
                    "[main] markdown metadata source unavailable because buffer line count is missing: window_id={}, buffer_id={}",
                    window.id,
                    buffer.id
                );
                return String::new();
            };
            let Some(full_range) =
                core_bridge.buffer_line_range(buffer.id, 0, line_count_range.total_line_count)
            else {
                log::debug!(
                    "[main] markdown metadata source unavailable because buffer text is missing: window_id={}, buffer_id={}",
                    window.id,
                    buffer.id
                );
                return String::new();
            };
            let mut source_text = full_range.lines.join("\n");
            if !source_text.is_empty() {
                source_text.push('\n');
            }
            source_text
        });
        cache_status_by_buffer.insert(buffer.id, outcome.status);
        document_maps_by_buffer.insert(buffer.id, Arc::clone(&outcome.document_map));
    }

    let maps = snapshot
        .windows
        .iter()
        .filter_map(|window| {
            document_maps_by_buffer
                .get(&window.buf_id)
                .map(|document_map| (window.id, Arc::clone(document_map)))
        })
        .collect::<BTreeMap<_, _>>();
    if std::env::var_os("SAYA_TRACE_RENDER").is_some() {
        log::debug!(
            "[saya-trace][main][markdown] collected=true target_path={:?} markdownrender={} mapped_buffers={:?} mapped_windows={:?}",
            session_state.target_path(),
            session_state.markdown_render(),
            document_maps_by_buffer.keys().copied().collect::<Vec<_>>(),
            maps.keys().copied().collect::<Vec<_>>()
        );
    }
    log::debug!(
        "[main] collected workspace markdown metadata: mapped_buffers={:?}, cache_status_by_buffer={:?}, mapped_windows={:?}",
        document_maps_by_buffer.keys().copied().collect::<Vec<_>>(),
        cache_status_by_buffer,
        maps.keys().copied().collect::<Vec<_>>()
    );
    maps
}

pub(crate) fn buffer_path_hint(buffer: &vim_core_rs::CoreBufferInfo) -> &str {
    buffer
        .document_id
        .as_deref()
        .and_then(|document_id| document_id.strip_prefix("file://"))
        .filter(|document_id| !document_id.is_empty())
        .unwrap_or(&buffer.name)
}

pub(crate) fn is_markdown_buffer_name(buffer_name: &str) -> bool {
    std::path::Path::new(buffer_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "md" | "markdown" | "mdown"
            )
        })
        .unwrap_or(false)
}
