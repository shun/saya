//! ワークスペース描画出力の構築。スナップショットからの投影、検索状態の
//! 収集、redraw plan の解決を担う。

use crate::app::session::EditorSessionState;
use crate::core::notification_prompt::{PagerPromptView, ProjectionFrame};
use crate::features::dired::RuntimeInputPromptUiState;
use crate::features::search::query::{SearchStateError, SearchVisibleState};
use crate::features::search::refresh::{
    SearchModeHint, SearchRefreshInput, WindowSearchRefreshStore,
};
use crate::features::selector::tui_state::SelectorTuiProjectionSink;
use crate::input::command_line_editor::{
    command_line_cursor_display_col, command_line_display_width,
};
use crate::presentation::floating_models::{
    append_active_mermaid_preview_float, apply_workspace_floating_window_models,
    refresh_buffer_backed_float_lines, refresh_terminal_float_lines, refresh_terminal_panel_lines,
};
use crate::presentation::floating_window::FloatingWindowManager;
use crate::presentation::markdown::structure::MarkdownMetadataCache;
use crate::presentation::panel::PanelManager;
use crate::presentation::render::workspace_projection::{
    collect_workspace_buffer_line_counts, collect_workspace_line_ranges,
    collect_workspace_markdown_document_maps, collect_workspace_substitute_preview_states,
    collect_workspace_syntax_lines, collect_workspace_tree_sitter_syntax,
};
use crate::presentation::screen_model::{
    WorkspaceProjectionError, WorkspaceProjectionInput, WorkspaceScreenModel, project_workspace,
};
use crate::presentation::structural_refresh::{
    RedrawPlan, RedrawPlanSource, StructuralRefreshOutcome,
};
use crate::presentation::viewport::{ViewportSyncMode, WindowViewportStore};
use crate::terminal::float::TerminalFloatManager;
use vim_core_rs::{CoreLightSnapshot, CoreMode, CoreSnapshot};

use crate::presentation::render::redraw_trace::trace_redraw_diagnostic;
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use unicode_width::UnicodeWidthChar;

pub fn terminal_display_invalidated_redraw_plan() -> RedrawPlan {
    RedrawPlan {
        requested: true,
        full: true,
        clear_before_draw: true,
        required_by_structure_change: false,
        source: RedrawPlanSource::TerminalDisplayInvalidation,
        coalesced_count: 0,
    }
}

pub fn effective_workspace_redraw_plan(
    structural_refresh: Option<&StructuralRefreshOutcome>,
    terminal_display_redraw_plan: Option<&RedrawPlan>,
) -> RedrawPlan {
    let mut plan = structural_refresh
        .map(|refresh| refresh.redraw_plan.clone())
        .unwrap_or_default();
    let Some(terminal_plan) = terminal_display_redraw_plan else {
        return plan;
    };

    plan.requested |= terminal_plan.requested;
    plan.full |= terminal_plan.full;
    plan.clear_before_draw |= terminal_plan.clear_before_draw;
    plan.required_by_structure_change |= terminal_plan.required_by_structure_change;
    plan.coalesced_count += terminal_plan.coalesced_count;
    plan.source = if terminal_plan.requested {
        RedrawPlanSource::TerminalDisplayInvalidation
    } else {
        plan.source
    };
    plan
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRenderOutput {
    pub model: WorkspaceScreenModel,
    pub failure_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceRedrawError {
    Projection(WorkspaceProjectionError),
    Search {
        window_id: i32,
        error: SearchStateError,
    },
}

impl fmt::Display for WorkspaceRedrawError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorkspaceRedrawError::Projection(error) => {
                write!(f, "workspace projection failed: {error}")
            }
            WorkspaceRedrawError::Search { window_id, error } => {
                write!(
                    f,
                    "workspace search refresh failed: window_id={window_id}, {error}"
                )
            }
        }
    }
}

impl From<WorkspaceProjectionError> for WorkspaceRedrawError {
    fn from(error: WorkspaceProjectionError) -> Self {
        WorkspaceRedrawError::Projection(error)
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn apply_workspace_redraw_transaction(
    last_successful_workspace_model: &mut Option<WorkspaceScreenModel>,
    render_result: Result<WorkspaceScreenModel, WorkspaceRedrawError>,
) -> Result<WorkspaceRenderOutput, WorkspaceRedrawError> {
    match render_result {
        Ok(model) => {
            *last_successful_workspace_model = Some(model.clone());
            Ok(WorkspaceRenderOutput {
                model,
                failure_message: None,
            })
        }
        Err(error) => {
            log::debug!(
                "[main] workspace redraw failed; attempting rollback to last successful model: error={:?}",
                error
            );
            if let Some(last_successful) = last_successful_workspace_model.as_ref() {
                let failure_message = error.to_string();
                let rollback_model = WorkspaceScreenModel {
                    message_line: crate::presentation::overlay::effect::merge_presentation_message_line(
                        &last_successful.message_line,
                        [crate::core::notification_prompt::MessageLineCandidate::legacy(
                            crate::core::notification_prompt::MessageLineSource::RenderProjectionError,
                            failure_message.as_str(),
                        )],
                    ),
                    ..last_successful.clone()
                };
                Ok(WorkspaceRenderOutput {
                    model: rollback_model,
                    failure_message: Some(failure_message),
                })
            } else {
                Err(error)
            }
        }
    }
}

pub fn build_workspace_render_output(
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    session_state: &mut EditorSessionState,
    viewport_store: &mut WindowViewportStore,
    viewport_sync_mode: ViewportSyncMode,
    search_refresh_store: &mut WindowSearchRefreshStore,
    markdown_metadata_cache: &mut MarkdownMetadataCache,
    command_line_prompt: Option<char>,
    command_line_buffer: &str,
    command_line_cursor_byte_index: usize,
    projection_frame: Option<&ProjectionFrame>,
    runtime_input_prompt: Option<&RuntimeInputPromptUiState>,
    mut structural_refresh: Option<&mut StructuralRefreshOutcome>,
    system_warning: Option<&str>,
    transient_msg: Option<&str>,
    terminal_width: u16,
    terminal_height: u16,
    floating_window_manager: Option<&mut FloatingWindowManager>,
    panel_manager: Option<&mut PanelManager>,
    mut terminal_float_manager: Option<&mut TerminalFloatManager>,
    selector_tui_projection_sink: Option<Arc<SelectorTuiProjectionSink>>,
) -> Result<WorkspaceScreenModel, WorkspaceRedrawError> {
    let total_started_at = std::time::Instant::now();
    let snapshot_started_at = std::time::Instant::now();
    let light_snapshot = outcome.core_bridge.light_snapshot();
    let snapshot = snapshot_from_light_snapshot(&light_snapshot, String::new());
    let snapshot_ms = snapshot_started_at.elapsed().as_millis();
    if let Some(active_window) = snapshot.active_window() {
        let active_buffer = snapshot
            .buffers
            .iter()
            .find(|buffer| buffer.id == active_window.buf_id);
        log::debug!(
            "[main][render] active buffer identity: window_id={}, buffer_id={}, buffer_name={:?}, document_id={:?}, session_target_path={:?}, syntax_enabled={}",
            active_window.id,
            active_window.buf_id,
            active_buffer.map(|buffer| &buffer.name),
            active_buffer.and_then(|buffer| buffer.document_id.as_deref()),
            session_state.target_path(),
            outcome.core_bridge.is_syntax_enabled()
        );
    } else {
        log::debug!(
            "[main][render] no active window while building workspace render output: session_target_path={:?}",
            session_state.target_path()
        );
    }
    trace_redraw_diagnostic(format_args!(
        "workspace render build started: revision={}, mode={:?}, cursor=({},{}), windows={}, command_prompt={:?}, command_buffer_len={}, structural_refresh_present={}",
        snapshot.revision,
        snapshot.mode,
        snapshot.cursor_row,
        snapshot.cursor_col,
        snapshot.windows.len(),
        command_line_prompt,
        command_line_buffer.len(),
        structural_refresh.is_some()
    ));
    if let Some(refresh) = structural_refresh.as_deref() {
        trace_redraw_diagnostic(format_args!(
            "workspace render using structural refresh: redraw_requested={}, full={}, clear_before_draw={}, source={:?}, coalesced_count={}, invalidated_buffers={:?}, invalidated_windows={:?}, layout_dirty={}",
            refresh.redraw_plan.requested,
            refresh.redraw_plan.full,
            refresh.redraw_plan.clear_before_draw,
            refresh.redraw_plan.source,
            refresh.redraw_plan.coalesced_count,
            refresh.invalidation.buffer_ids,
            refresh.invalidation.window_ids,
            refresh.invalidation.layout_dirty
        ));
    }
    let visual_started_at = std::time::Instant::now();
    let visual_selection = if matches!(
        snapshot.mode,
        CoreMode::Visual | CoreMode::VisualLine | CoreMode::VisualBlock
    ) {
        outcome.core_bridge.current_visual_selection()
    } else {
        None
    };
    let visual_ms = visual_started_at.elapsed().as_millis();
    let invalidated_windows = structural_refresh
        .as_deref()
        .map(|refresh| {
            refresh
                .invalidation
                .window_ids
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let buffer_line_counts = collect_workspace_buffer_line_counts(&outcome.core_bridge, &snapshot);
    let active_markdown_preview_source = snapshot.active_window().and_then(|window| {
        let line_count = buffer_line_counts.get(&window.buf_id).copied().unwrap_or(0);
        outcome
            .core_bridge
            .buffer_line_range(window.buf_id, 0, line_count)
            .map(|range| range.lines.join("\n"))
    });
    let viewport_summary = viewport_store.sync_from_windows_for_render(
        &snapshot.windows,
        &invalidated_windows,
        &buffer_line_counts,
        viewport_sync_mode,
    );
    if let Some(refresh) = structural_refresh.as_deref_mut() {
        *refresh = refresh
            .clone()
            .with_viewport_sync_summary(&viewport_summary);
    }
    let line_range_started_at = std::time::Instant::now();
    let mut line_ranges =
        collect_workspace_line_ranges(&outcome.core_bridge, &snapshot, viewport_store);
    let line_range_ms = line_range_started_at.elapsed().as_millis();
    search_refresh_store.retain_windows(
        &snapshot
            .windows
            .iter()
            .map(|window| window.id)
            .collect::<Vec<_>>(),
    );
    let search_started_at = std::time::Instant::now();
    let substitute_preview_search_states = collect_workspace_substitute_preview_states(
        &snapshot,
        &mut line_ranges,
        command_line_prompt,
        command_line_buffer,
    );
    let search_states = if substitute_preview_search_states.is_empty() {
        collect_workspace_search_states(
            &mut outcome.core_bridge,
            &snapshot,
            viewport_store,
            search_refresh_store,
            resolve_prompt_revision(command_line_prompt, command_line_buffer),
            resolve_search_mode_hint(command_line_prompt, command_line_buffer),
        )?
    } else {
        substitute_preview_search_states
    };
    let search_ms = search_started_at.elapsed().as_millis();
    let syntax_enabled = outcome.core_bridge.is_syntax_enabled();
    let syntax_started_at = std::time::Instant::now();
    let syntax_lines = if syntax_enabled {
        collect_workspace_syntax_lines(
            &outcome.core_bridge,
            &snapshot,
            viewport_store,
            &line_ranges,
        )
    } else {
        trace_redraw_diagnostic(format_args!(
            "workspace syntax collection skipped because :syntax is off"
        ));
        BTreeMap::new()
    };
    let syntax_ms = syntax_started_at.elapsed().as_millis();
    #[cfg(feature = "tree-sitter-syntax")]
    let tree_sitter_started_at = std::time::Instant::now();
    #[cfg(feature = "tree-sitter-syntax")]
    let tree_sitter_syntax = if syntax_enabled {
        collect_workspace_tree_sitter_syntax(
            &mut outcome.core_bridge,
            &snapshot,
            viewport_store,
            &line_ranges,
        )
    } else {
        trace_redraw_diagnostic(format_args!(
            "workspace Tree-sitter syntax collection skipped because :syntax is off"
        ));
        BTreeMap::new()
    };
    #[cfg(feature = "tree-sitter-syntax")]
    let tree_sitter_ms = tree_sitter_started_at.elapsed().as_millis();
    let markdown_started_at = std::time::Instant::now();
    let markdown_document_maps = collect_workspace_markdown_document_maps(
        markdown_metadata_cache,
        session_state,
        &outcome.core_bridge,
        &snapshot,
    );
    let markdown_ms = markdown_started_at.elapsed().as_millis();
    let command_preview =
        command_line_prompt.map(|prompt| format!("{}{}", prompt, command_line_buffer));
    let command_preview_cursor_col = command_line_prompt.map(|prompt| {
        command_line_cursor_display_col(
            prompt,
            command_line_buffer,
            command_line_cursor_byte_index,
            session_state.tab_size(),
        )
    });
    let mut notification_prompt = projection_frame.map(ProjectionFrame::workspace_view);
    if let Some(runtime_prompt) = runtime_input_prompt {
        let view = runtime_prompt.view();
        log::debug!(
            "[main][runtime_input] projecting active runtime prompt: title={}, input_len={}",
            runtime_prompt.request.title,
            view.input.len()
        );
        notification_prompt
            .get_or_insert_with(Default::default)
            .input_prompt = Some(view);
    }

    let projection_started_at = std::time::Instant::now();
    let mut projection_result = project_workspace(&WorkspaceProjectionInput {
        snapshot: &snapshot,
        light_snapshot: Some(&light_snapshot),
        line_ranges: &line_ranges,
        session_state,
        visual_selection: visual_selection.as_ref(),
        search_states: &search_states,
        syntax_lines: &syntax_lines,
        #[cfg(feature = "tree-sitter-syntax")]
        tree_sitter_syntax: &tree_sitter_syntax,
        markdown_document_maps: &markdown_document_maps,
        command_preview: command_preview.as_deref(),
        core_message: None,
        notification_prompt: notification_prompt.as_ref(),
        system_warning,
        transient_info: transient_msg,
        viewport_store,
        terminal_width,
        terminal_height,
    });
    let projection_ms = projection_started_at.elapsed().as_millis();
    log::debug!(
        "[PERF][main] build_workspace_render_output text_len={} windows={} snapshot_ms={} line_range_ms={} line_ranges={} visible_text_bytes={} visual_ms={} search_ms={} syntax_ms={} tree_sitter_ms={} markdown_ms={} projection_ms={} total_ms={}",
        snapshot.text.len(),
        snapshot.windows.len(),
        snapshot_ms,
        line_range_ms,
        line_ranges.len(),
        line_ranges
            .values()
            .map(|range| range.lines.iter().map(String::len).sum::<usize>())
            .sum::<usize>(),
        visual_ms,
        search_ms,
        syntax_ms,
        {
            #[cfg(feature = "tree-sitter-syntax")]
            {
                tree_sitter_ms
            }
            #[cfg(not(feature = "tree-sitter-syntax"))]
            {
                0
            }
        },
        markdown_ms,
        projection_ms,
        total_started_at.elapsed().as_millis()
    );
    if let Ok(workspace) = projection_result.as_mut() {
        sync_workspace_message_pager(session_state, workspace, terminal_width);
        if let Some(cursor_col) = command_preview_cursor_col
            && let Some(command_line) = workspace.command_line.as_mut()
        {
            command_line.cursor_col = cursor_col;
        }
        if let Some(manager) = floating_window_manager {
            refresh_buffer_backed_float_lines(manager, &outcome.core_bridge, &light_snapshot);
            if let Some(terminal_manager) = terminal_float_manager.as_deref_mut() {
                refresh_terminal_float_lines(manager, terminal_manager);
            }
            apply_workspace_floating_window_models(
                workspace,
                terminal_width,
                terminal_height,
                manager,
            );
        }
        if let Some(panel_manager) = panel_manager {
            if let Some(terminal_manager) = terminal_float_manager.as_deref_mut() {
                refresh_terminal_panel_lines(
                    panel_manager,
                    terminal_manager,
                    terminal_width,
                    terminal_height,
                );
            }
            let panel_floats =
                panel_manager.resolve_floating_screen_models(terminal_width, terminal_height);
            log::trace!(
                "[main][panel] applied workspace panels: panels={}, terminal=({},{})",
                panel_floats.len(),
                terminal_width,
                terminal_height
            );
            workspace.floats.extend(panel_floats);
        }
        if let Some(sink) = selector_tui_projection_sink
            && let Some(selector_model) = sink.current_model()
        {
            if let Some(selector_float) = sink.workspace_float(terminal_width, terminal_height) {
                log::debug!(
                    "[main][selector] appended selector TUI float to workspace model: session_id={}, rows={}, floats_before={}",
                    selector_model.session_id,
                    selector_model.visible_rows.len(),
                    workspace.floats.len()
                );
                workspace.floats.push(selector_float);
            } else {
                log::debug!(
                    "[main][selector] selector TUI model not visible in workspace render: session_id={}, intent={:?}, hidden={}, cancelled={}",
                    selector_model.session_id,
                    selector_model.intent,
                    selector_model.hidden,
                    selector_model.cancelled
                );
            }
        }
        append_active_mermaid_preview_float(
            workspace,
            &snapshot,
            active_markdown_preview_source
                .as_deref()
                .unwrap_or_default(),
            &markdown_document_maps,
            terminal_width,
            terminal_height,
            session_state,
        );
    }

    match projection_result {
        Ok(workspace) => {
            let projection_summary = workspace.projection_summary();
            trace_redraw_diagnostic(format_args!(
                "workspace render build succeeded: panes={}, active_window_id={}, visible_message={:?}, command_line_active={}, search_overlay_counts={:?}, syntax_chunk_counts={:?}",
                workspace.panes.len(),
                workspace.active_window_id,
                workspace.visible_message_text(),
                workspace.command_line.is_some(),
                workspace
                    .panes
                    .iter()
                    .map(|pane| (pane.window_id, pane.search_overlays.len()))
                    .collect::<Vec<_>>(),
                workspace
                    .panes
                    .iter()
                    .map(|pane| (pane.window_id, pane.syntax_chunks.len()))
                    .collect::<Vec<_>>()
            ));
            if let Some(refresh) = structural_refresh.as_deref_mut() {
                *refresh = refresh.clone().with_projection_summary(projection_summary);
                log::debug!(
                    "[main] structural refresh diagnostics ready before render coordination: redraw_requested={}, projection_status={:?}, viewport_status={:?}",
                    refresh.redraw_plan.requested,
                    refresh.projection.status,
                    refresh.viewport_status
                );
            }
            Ok(workspace)
        }
        Err(error) => {
            trace_redraw_diagnostic(format_args!(
                "workspace render build failed: revision={}, cursor=({},{}), error={}",
                snapshot.revision, snapshot.cursor_row, snapshot.cursor_col, error
            ));
            if let Some(refresh) = structural_refresh.as_deref() {
                let diagnostic =
                    refresh.projection_failure(error.to_string(), refresh.viewport_status);
                log::debug!(
                    "[main] structural projection failure diagnostic prepared before render coordination: {:?}",
                    diagnostic
                );
            }
            Err(WorkspaceRedrawError::from(error))
        }
    }
}

pub fn sync_workspace_message_pager(
    session_state: &mut EditorSessionState,
    workspace: &mut WorkspaceScreenModel,
    terminal_width: u16,
) {
    let visible_message = workspace
        .visible_message_text()
        .map(str::to_owned)
        .unwrap_or_default();
    let pager_message = wrap_message_for_pager(&visible_message, terminal_width);
    let activation_changed =
        session_state.sync_message_pager(&pager_message, workspace.message_area_height);
    if session_state.message_pager_hides_message(&pager_message) {
        workspace.message_line.visible = None;
        workspace.message_scroll_offset = 0;
        workspace.pager_prompt = None;
        log::debug!(
            "[main] hiding dismissed message pager text: message_lines={}",
            visible_message.lines().count()
        );
        return;
    }
    workspace.message_scroll_offset = session_state.message_scroll_offset();
    if workspace.pager_prompt.is_none()
        && let Some(kind) = session_state.message_pager_prompt_kind()
    {
        workspace.pager_prompt = Some(PagerPromptView {
            kind,
            one_shot: false,
        });
    }
    log::debug!(
        "[main] synced message pager: active={}, offset={}, height={}, message_lines={}, activation_changed={}",
        session_state.message_pager_active(),
        session_state.message_scroll_offset(),
        workspace.message_area_height,
        pager_message.lines().count(),
        activation_changed
    );
}

pub fn wrap_message_for_pager(message: &str, width: u16) -> String {
    let max_width = usize::from(width.max(1));
    message
        .lines()
        .flat_map(|line| wrap_message_line_for_pager(line, max_width))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn wrap_message_line_for_pager(line: &str, max_width: usize) -> Vec<String> {
    if command_line_display_width(line, 8) <= max_width {
        return vec![line.to_string()];
    }

    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;
    for ch in line.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if current_width > 0 && current_width.saturating_add(ch_width) > max_width {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
        }
        current.push(ch);
        current_width = current_width.saturating_add(ch_width);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

pub fn snapshot_from_light_snapshot(light: &CoreLightSnapshot, text: String) -> CoreSnapshot {
    CoreSnapshot {
        text,
        revision: light.revision,
        dirty: light.dirty,
        mode: light.mode,
        pending_input: light.pending_input.clone(),
        cursor_row: light.cursor_row,
        cursor_col: light.cursor_col,
        pending_host_actions: light.pending_host_actions,
        buffers: light.buffers.clone(),
        windows: light.windows.clone(),
        pum: light.pum.clone(),
    }
}

pub(crate) fn collect_workspace_search_states(
    core_bridge: &mut crate::core::bridge::CoreBridge,
    snapshot: &vim_core_rs::CoreSnapshot,
    viewport_store: &WindowViewportStore,
    search_refresh_store: &mut WindowSearchRefreshStore,
    prompt_revision: Option<u64>,
    search_mode_hint: SearchModeHint,
) -> Result<BTreeMap<i32, SearchVisibleState>, WorkspaceRedrawError> {
    let mut search_states = BTreeMap::new();
    if matches!(search_mode_hint, SearchModeHint::Hlsearch)
        && !core_bridge.has_search_highlight_activity()
    {
        log::debug!(
            "[main] workspace search refresh skipped because hlsearch has no active pattern"
        );
        return Ok(search_states);
    }
    for window in &snapshot.windows {
        let body_height = usize::try_from(window.height.saturating_sub(1))
            .unwrap_or(1)
            .max(1);
        let viewport_top = viewport_store
            .get(window.id)
            .map(|viewport| viewport.top_line())
            .unwrap_or_else(|| window.topline.saturating_sub(1));
        let outcome = search_refresh_store.update_window(
            core_bridge,
            SearchRefreshInput {
                window_id: window.id,
                revision: snapshot.revision as u64,
                viewport_top,
                viewport_height: body_height,
                cursor_row: window.cursor_row,
                cursor_col: window.cursor_col,
                prompt_revision,
                search_mode_hint,
            },
        );
        trace_redraw_diagnostic(format_args!(
            "search refresh outcome: window_id={}, revision={}, viewport_top={}, viewport_height={}, cursor=({},{}), mode_hint={:?}, query_executed={}, cache_key={:?}, render_state_present={}, match_count={}, current_match={:?}",
            window.id,
            snapshot.revision,
            viewport_top,
            body_height,
            window.cursor_row,
            window.cursor_col,
            search_mode_hint,
            outcome.query_executed,
            outcome.cache_key,
            outcome.render_state.is_some(),
            outcome
                .render_state
                .as_ref()
                .map(|state| state.matches.len())
                .unwrap_or_default(),
            outcome.render_state.as_ref().and_then(|state| {
                state
                    .matches
                    .iter()
                    .find(|search_match| {
                        search_match.kind
                            == crate::features::search::query::SearchMatchKind::Current
                    })
                    .map(|search_match| {
                        (
                            search_match.start_row,
                            search_match.start_col,
                            search_match.end_row,
                            search_match.end_col,
                        )
                    })
            })
        ));
        if let Some(error) = outcome.query_error {
            log::debug!(
                "[main] workspace search refresh failed: window_id={}, error={:?}",
                window.id,
                error
            );
            return Err(WorkspaceRedrawError::Search {
                window_id: window.id,
                error,
            });
        }
        if let Some(render_state) = outcome.render_state {
            search_states.insert(window.id, render_state);
        }
    }
    Ok(search_states)
}

pub(crate) fn resolve_search_mode_hint(
    command_line_prompt: Option<char>,
    command_line_buffer: &str,
) -> SearchModeHint {
    if command_line_prompt == Some('/') && !command_line_buffer.is_empty() {
        SearchModeHint::Incsearch
    } else {
        SearchModeHint::Hlsearch
    }
}

pub fn resolve_prompt_revision(
    command_line_prompt: Option<char>,
    command_line_buffer: &str,
) -> Option<u64> {
    let prompt = command_line_prompt?;
    let mut hasher = DefaultHasher::new();
    prompt.hash(&mut hasher);
    command_line_buffer.hash(&mut hasher);
    Some(hasher.finish())
}

pub fn structural_refresh_is_idle(refresh: Option<&StructuralRefreshOutcome>) -> bool {
    refresh.is_none_or(|refresh| {
        !refresh.redraw_plan.requested
            && !refresh.redraw_plan.full
            && !refresh.redraw_plan.clear_before_draw
            && !refresh.invalidation.has_any()
    })
}
