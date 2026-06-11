//! ワークスペース描画向けのフローティングウィンドウモデル構築・更新。
//!
//! Mermaid プレビューフロートの組み立て、コアウィンドウ／ターミナルフロート
//! ／ターミナルパネルの表示行リフレッシュ、フローティングウィンドウモデルの
//! ワークスペースへの適用を担う。描画本体（`build_workspace_render_output`）
//! から呼ばれる、副作用の小さいモデル投影ロジック。

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::app::session::{EditorSessionState, MermaidPreviewZoom};
use crate::presentation::floating_window::{
    FloatingBorder, FloatingChrome, FloatingContentRef, FloatingCursor, FloatingImage,
    FloatingImageSource, FloatingImageView, FloatingWindowId, FloatingWindowManager,
    FloatingZIndex,
};
use crate::presentation::markdown::structure::{MarkdownBlockKind, MarkdownDocumentMap};
use crate::presentation::panel::PanelManager;
use crate::presentation::screen_model::WorkspaceScreenModel;
use crate::terminal::float::TerminalFloatManager;
use vim_core_rs::{CoreLightSnapshot, CoreMode, CoreSnapshot};
pub fn append_active_mermaid_preview_float(
    workspace: &mut WorkspaceScreenModel,
    snapshot: &CoreSnapshot,
    source_text: &str,
    markdown_document_maps: &BTreeMap<i32, Arc<MarkdownDocumentMap>>,
    terminal_width: u16,
    terminal_height: u16,
    session_state: &mut EditorSessionState,
) {
    let auto_enabled = session_state.mermaid_preview_auto();
    let manual_active = session_state.mermaid_preview_manual_active();
    let preview_closed = session_state.mermaid_preview_closed();
    if !auto_enabled && !manual_active {
        log::debug!(
            "[main][markdown_preview] skip Mermaid preview because auto preview is disabled and no manual request is pending"
        );
        return;
    }
    if snapshot.mode != CoreMode::Normal {
        log::debug!(
            "[main][markdown_preview] skip Mermaid preview because mode is not Normal: mode={:?}, auto_enabled={}, manual_active={}",
            snapshot.mode,
            auto_enabled,
            manual_active
        );
        if manual_active {
            session_state.clear_mermaid_preview_manual("mode_not_normal");
        }
        return;
    }
    let Some(active_window) = snapshot.active_window() else {
        log::debug!(
            "[main][markdown_preview] skip Mermaid preview because active window is absent"
        );
        if manual_active {
            session_state.clear_mermaid_preview_manual("active_window_absent");
        }
        return;
    };
    let Some(map) = markdown_document_maps.get(&active_window.id) else {
        log::debug!(
            "[main][markdown_preview] skip Mermaid preview because markdown map is absent: window_id={}",
            active_window.id
        );
        if manual_active {
            session_state.clear_mermaid_preview_manual("markdown_map_absent");
        }
        return;
    };
    let Some(block) = map.blocks.iter().find(|block| {
        matches!(
            block.kind,
            MarkdownBlockKind::FencedCodeBlock { ref info, .. }
                if info
                    .as_deref()
                    .and_then(|info| info.split_whitespace().next())
                    .is_some_and(|language| language.eq_ignore_ascii_case("mermaid"))
        ) && (block.range.start.line..=block.range.end.line).contains(&active_window.cursor_row)
    }) else {
        log::debug!(
            "[main][markdown_preview] skip Mermaid preview because cursor is outside Mermaid blocks: window_id={}, cursor_row={}",
            active_window.id,
            active_window.cursor_row
        );
        session_state.reopen_mermaid_preview_if_closed("cursor_outside_mermaid_block");
        if manual_active {
            session_state.clear_mermaid_preview_manual("cursor_outside_mermaid_block");
        }
        return;
    };
    if preview_closed && !manual_active {
        log::debug!(
            "[main][markdown_preview] skip Mermaid preview because preview was closed for current block: window_id={}, cursor_row={}",
            active_window.id,
            active_window.cursor_row
        );
        return;
    }
    let source_lines = source_text.lines().collect::<Vec<_>>();
    if source_lines.get(block.range.end.line).is_none() {
        log::debug!(
            "[main][markdown_preview] skip Mermaid preview because block source is partial: window_id={}, start_row={}, end_row={}, available_lines={}",
            active_window.id,
            block.range.start.line,
            block.range.end.line,
            source_lines.len()
        );
        if manual_active {
            session_state.clear_mermaid_preview_manual("partial_block_source");
        }
        return;
    }
    let body = source_lines
        .iter()
        .take(block.range.end.line)
        .skip(block.range.start.line + 1)
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    if body.trim().is_empty() {
        log::debug!(
            "[main][markdown_preview] skip Mermaid preview because body is empty: window_id={}, start_row={}",
            active_window.id,
            block.range.start.line
        );
        if manual_active {
            session_state.clear_mermaid_preview_manual("empty_mermaid_body");
        }
        return;
    }
    let Some(active_pane) = workspace
        .panes
        .iter()
        .find(|pane| pane.window_id == active_window.id)
    else {
        log::debug!(
            "[main][markdown_preview] skip Mermaid preview because active pane is absent: window_id={}",
            active_window.id
        );
        if manual_active {
            session_state.clear_mermaid_preview_manual("active_pane_absent");
        }
        return;
    };
    let width = mermaid_preview_float_dimension(
        terminal_width,
        session_state.mermaid_preview_width_percent(),
        100,
        40,
        u16::MAX,
        2,
    );
    let height = mermaid_preview_float_dimension(
        terminal_height,
        session_state.mermaid_preview_height_percent(),
        100,
        12,
        u16::MAX,
        2,
    );
    let x = terminal_width.saturating_sub(width).saturating_sub(1);
    let y = active_pane
        .rect
        .y
        .saturating_add(1)
        .min(terminal_height.saturating_sub(height).saturating_sub(1));
    let content_width = width.saturating_sub(2).max(1);
    let content_height = height.saturating_sub(2).max(1);
    let float_id = FloatingWindowId(9_000_000_000u64.saturating_add(active_window.id as u64));
    let preview_view = session_state.mermaid_preview_view();
    let image_view = match preview_view.zoom {
        MermaidPreviewZoom::Fit => FloatingImageView::fit(),
        MermaidPreviewZoom::Percent(percent) => FloatingImageView {
            zoom_percent: Some(percent),
            pan_x_px: preview_view.pan_x_px,
            pan_y_px: preview_view.pan_y_px,
        },
    };
    let zoom_label = match preview_view.zoom {
        MermaidPreviewZoom::Fit => "fit".to_string(),
        MermaidPreviewZoom::Percent(percent) => format!("{percent}%"),
    };
    workspace
        .floats
        .push(crate::presentation::floating_window::FloatingScreenModel {
            id: float_id,
            content: FloatingContentRef::StaticLines {
                content_id: float_id.0,
            },
            rect: crate::presentation::screen_model::PaneRect {
                x,
                y,
                width,
                height,
            },
            lines: vec![
                format!("Mermaid preview [{zoom_label}]"),
                " ".repeat(usize::from(content_width)),
            ],
            inline_styles: Vec::new(),
            images: vec![FloatingImage {
                line: 1,
                column: 0,
                max_width: content_width,
                max_height: content_height.saturating_sub(1).max(1),
                view: image_view,
                source: FloatingImageSource::Mermaid {
                    buffer_id: active_window.buf_id,
                    row: block.range.start.line,
                    alt_text: "mermaid diagram".to_string(),
                    background: session_state.mermaid_preview_background().to_string(),
                    source: body,
                },
            }],
            cursor: None,
            focusable: preview_view.focused,
            mouse: true,
            chrome: FloatingChrome {
                border: FloatingBorder::Single,
            },
            zindex: FloatingZIndex::Hover.value(),
            creation_order: u64::MAX,
        });
    log::debug!(
        "[main][markdown_preview] appended Mermaid preview float: trigger={}, window_id={}, buffer_id={}, start_row={}, end_row={}, float_id={}, rect=({},{},{},{}), body_bytes={}",
        if manual_active { "manual" } else { "auto" },
        active_window.id,
        active_window.buf_id,
        block.range.start.line,
        block.range.end.line,
        float_id.0,
        x,
        y,
        width,
        height,
        workspace
            .floats
            .last()
            .and_then(|float| float.images.first())
            .map(|image| match &image.source {
                FloatingImageSource::Mermaid { source, .. } => source.len(),
            })
            .unwrap_or(0)
    );
}

pub(crate) fn mermaid_preview_float_dimension(
    terminal_extent: u16,
    numerator: u16,
    denominator: u16,
    min: u16,
    max: u16,
    reserved: u16,
) -> u16 {
    let available = terminal_extent.saturating_sub(reserved).max(1);
    let denominator = u32::from(denominator.max(1));
    let preferred = u32::from(terminal_extent)
        .saturating_mul(u32::from(numerator))
        .div_ceil(denominator)
        .min(u32::from(u16::MAX)) as u16;
    preferred.max(min).min(max).min(available).max(1)
}

pub fn apply_workspace_floating_window_models(
    workspace: &mut WorkspaceScreenModel,
    terminal_width: u16,
    terminal_height: u16,
    manager: &FloatingWindowManager,
) {
    let pane_rects = workspace
        .panes
        .iter()
        .map(|pane| (pane.window_id, pane.rect))
        .collect::<Vec<_>>();
    let cursors = workspace
        .panes
        .iter()
        .map(|pane| (pane.window_id, pane.cursor_row, pane.cursor_col))
        .collect::<Vec<_>>();
    workspace.floats = manager.resolve_screen_models_with_cursors(
        terminal_width,
        terminal_height,
        &pane_rects,
        &cursors,
        Some(workspace.active_window_id),
    );
    log::trace!(
        "[main][floating_window] applied workspace floats: floats={}, terminal=({},{}), active_window_id={}",
        workspace.floats.len(),
        terminal_width,
        terminal_height,
        workspace.active_window_id
    );
}

pub fn refresh_buffer_backed_float_lines(
    manager: &mut FloatingWindowManager,
    core_bridge: &crate::core::bridge::CoreBridge,
    snapshot: &CoreLightSnapshot,
) {
    for request in manager.core_window_float_view_requests() {
        let Some(window) = snapshot.window(request.window_id) else {
            log::debug!(
                "[main][buffer_float] skipping core-window float refresh because window is missing: float_id={}, window_id={}",
                request.float_id.0,
                request.window_id
            );
            continue;
        };
        let start_row = window.topline.saturating_sub(1);
        let Some(range) = core_bridge.buffer_line_range(
            window.buf_id,
            start_row,
            usize::from(request.content_height),
        ) else {
            log::debug!(
                "[main][buffer_float] skipping core-window float refresh because buffer range is missing: float_id={}, window_id={}, buffer_id={}",
                request.float_id.0,
                request.window_id,
                window.buf_id
            );
            continue;
        };
        let returned = range.lines.len();
        let _ = manager.replace_core_window_lines(request.float_id, range.lines);
        log::trace!(
            "[main][buffer_float] refreshed core-window float lines: float_id={}, window_id={}, buffer_id={}, start_row={}, requested_lines={}, returned_lines={}",
            request.float_id.0,
            request.window_id,
            window.buf_id,
            start_row,
            request.content_height,
            returned
        );
    }
}

pub fn refresh_terminal_float_lines(
    manager: &mut FloatingWindowManager,
    terminal_manager: &mut TerminalFloatManager,
) {
    terminal_manager.drain();
    for request in manager.terminal_float_view_requests() {
        let _ = terminal_manager.resize(
            request.terminal_id,
            request.content_width,
            request.content_height,
        );
        let snapshot = terminal_manager.screen_snapshot(request.terminal_id);
        let mut lines = snapshot
            .as_ref()
            .map(|snapshot| snapshot.rendered_lines())
            .unwrap_or_else(|| terminal_manager.rendered_lines(request.terminal_id));
        lines.truncate(usize::from(request.content_height));
        let inline_styles = snapshot
            .map(|snapshot| {
                snapshot
                    .inline_styles()
                    .into_iter()
                    .filter(|style| style.line < usize::from(request.content_height))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let returned = lines.len();
        let _ = manager.replace_terminal_lines(request.float_id, lines);
        let _ = manager.set_inline_styles(request.float_id, inline_styles);
        log::trace!(
            "[main][terminal_float] refreshed terminal float lines: float_id={}, terminal_id={}, requested_size=({},{}), returned_lines={}",
            request.float_id.0,
            request.terminal_id,
            request.content_width,
            request.content_height,
            returned
        );
    }
}

pub fn refresh_terminal_panel_lines(
    panel_manager: &mut PanelManager,
    terminal_manager: &mut TerminalFloatManager,
    terminal_width: u16,
    terminal_height: u16,
) {
    terminal_manager.drain();
    for request in panel_manager.terminal_view_requests(terminal_width, terminal_height) {
        let _ = terminal_manager.resize(
            request.terminal_id,
            request.content_width,
            request.content_height,
        );
        let snapshot = terminal_manager.screen_snapshot(request.terminal_id);
        let mut lines = snapshot
            .as_ref()
            .map(|snapshot| snapshot.rendered_lines())
            .unwrap_or_else(|| terminal_manager.rendered_lines(request.terminal_id));
        lines.truncate(usize::from(request.content_height));
        let cursor = terminal_manager
            .cursor_position(request.terminal_id)
            .map(|(line, column)| FloatingCursor {
                line: usize::from(line).min(usize::from(request.content_height.saturating_sub(1))),
                column: usize::from(column),
            });
        let inline_styles = snapshot
            .map(|snapshot| {
                snapshot
                    .inline_styles()
                    .into_iter()
                    .filter(|style| style.line < usize::from(request.content_height))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let returned = lines.len();
        let _ = panel_manager.replace_terminal_lines(&request.id, lines);
        let _ = panel_manager.replace_terminal_inline_styles(&request.id, inline_styles);
        let _ = panel_manager.replace_terminal_cursor(&request.id, cursor);
        log::trace!(
            "[main][panel] refreshed terminal panel lines: id={}, terminal_id={}, requested_size=({},{}), returned_lines={}, cursor={:?}",
            request.id,
            request.terminal_id,
            request.content_width,
            request.content_height,
            returned,
            cursor
        );
    }
}
