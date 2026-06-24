pub(super) use std::collections::{BTreeMap, BTreeSet};
pub(super) use std::path::PathBuf;
pub(super) use std::sync::Arc;
pub(super) use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(super) use super::*;
pub(super) use crate::app::bootstrap::StartupKeymapMode;
pub(super) use crate::app::host_command::{
    MainHostCommand, parse_main_host_command, startup_registered_command_name_for_ex_command,
};
pub(super) use crate::app::outcome_consume::dispatch_prompt_response_command;
pub(super) use crate::app::runtime_dispatch::{
    MainRuntimeHostSession, SaveSnapshotOutcome, apply_runtime_dispatch_outcome,
    execute_runtime_host_command, execute_runtime_host_command_with_floats,
    handle_selector_accept_action, merge_shutdown_reason, mouse_click_to_sgr_sequence,
    normal_quit_warning_message, pasted_text_to_dispatch_units,
    prioritize_save_family_host_directives, save_error_message,
    save_snapshot_result_with_confirmation, save_snapshot_result_with_path_override,
    shutdown_reason_from_quit_decision,
};
pub(super) use crate::app::session::{
    DirectoryBufferPlannedOperation, EditorSessionState, MermaidPreviewZoom, QuitDecision,
    SaveRequestError,
};
pub(super) use crate::core::outcome::NormalizedHostDirective;
pub(super) use crate::features::completion::session::CompletionShowRequest;
pub(super) use crate::features::dired::{
    apply_directory_buffer_operation_plan, execute_runtime_filer_operation,
};
pub(super) use crate::features::lsp::float::{
    PopupSizeBasis, PopupSizeSpec, PopupSizeValue, ResolvedPopupSizeLimit,
};
pub(super) use crate::features::lsp::host_commands::{
    LspHoverPopupKind, LspPopupKind, PopupSizingContext, default_lsp_popup_basis,
    parse_lsp_hover_popup_kind, parse_lsp_popup_size_spec, resolve_lsp_popup_size_limit,
};
pub(super) use crate::features::selector::runtime::RuntimeSelectorControllerCommand;
pub(super) use crate::presentation::floating_input::{
    begin_command_line_from_focused_panel, focus_floating_window_from_mouse_click,
    focus_mermaid_preview_from_mouse_click, handle_core_window_float_key,
    handle_floating_window_key, handle_mermaid_preview_key, handle_mermaid_preview_mouse_wheel,
    handle_terminal_float_key, handle_terminal_panel_key,
};
pub(super) use crate::presentation::floating_models::{
    append_active_mermaid_preview_float, refresh_buffer_backed_float_lines,
    refresh_terminal_float_lines,
};
pub(super) use crate::presentation::floating_window::{
    FloatingBorder, FloatingChrome, FloatingImageSource, FloatingMouseOutcome, FloatingPlacement,
    FloatingRelativeTo, FloatingSize, FloatingWindowId, FloatingZIndex,
};
pub(super) use crate::presentation::overlay::optional_graphics::RecordingOverlayWriter;
pub(super) use crate::presentation::panel::{
    PanelCloseBehavior, PanelContent, PanelNode, PanelOpenRequest, PanelPosition, PanelSize,
};
pub(super) use crate::presentation::render::command_line_redraw::build_command_line_only_workspace;
pub(super) use crate::presentation::render::redraw_trace::{
    RedrawTraceCounts, redraw_trace_diagnostic_counts as test_redraw_trace_diagnostic_counts,
    reset_redraw_trace_diagnostic_counts as reset_test_redraw_trace_diagnostic_counts,
};
pub(super) use crate::presentation::render::workspace_output::{
    apply_workspace_redraw_transaction, resolve_prompt_revision, structural_refresh_is_idle,
    wrap_message_for_pager,
};
pub(super) use crate::presentation::render::workspace_projection::{
    collect_workspace_line_ranges, collect_workspace_markdown_document_maps,
    collect_workspace_tree_sitter_syntax,
};
pub(super) use crate::presentation::runtime_commands::{
    execute_runtime_panel_open, execute_runtime_window_close_float,
    execute_runtime_window_open_float,
};
pub(super) use crate::presentation::screen_model::ScreenCursorStyle;
pub(super) use crate::presentation::structural_refresh::StructuralRefresh;
pub(super) use crate::runtime::integration::{
    RuntimeDispatchOutcome, RuntimeHostSession, RuntimeShutdownIntent,
};
pub(super) use crate::runtime::live::{
    RuntimeCommandError, RuntimeFilerError, RuntimeFilerErrorKind, RuntimeFilerOperation,
    RuntimeFilerOperationKind, RuntimeFloatContentRequest, RuntimeFloatOpenRequest,
    RuntimeFloatRelativeToRequest, RuntimeFloatZIndexRequest, RuntimePanelContentRequest,
    RuntimePanelNodeRequest, RuntimePanelOpenRequest,
};
pub(super) use crate::terminal::float::{TerminalFloatCloseBehavior, TerminalFloatSpawnRequest};
pub(super) use vim_core_rs::CoreMessageEvent;

pub(super) fn redraw_trace_observation_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

pub(super) fn unique_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-main-test-{name}-{nanos}"))
}

pub(super) fn unique_repo_relative_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    PathBuf::from("tmp").join(format!("saya-main-test-{name}-{nanos}"))
}

pub(super) fn wait_for_test_condition(mut predicate: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if predicate() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(predicate(), "condition did not become true before timeout");
}

pub(super) fn viewport_sync_mode_for_input(key: &KeyInput) -> ViewportSyncMode {
    match key {
        KeyInput::Char('j')
        | KeyInput::Char('k')
        | KeyInput::Down
        | KeyInput::Up
        | KeyInput::ShiftedNav(crate::input::router::NavigationKey::Down)
        | KeyInput::ShiftedNav(crate::input::router::NavigationKey::Up)
        | KeyInput::CtrlNav(crate::input::router::NavigationKey::Down)
        | KeyInput::CtrlNav(crate::input::router::NavigationKey::Up) => {
            ViewportSyncMode::SmoothLineMotion
        }
        _ => ViewportSyncMode::Core,
    }
}

pub(super) fn handle_directory_operation_confirmation_key_without_runtime(
    key: &KeyInput,
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    session_state: &mut crate::app::session::EditorSessionState,
    transient_msg: &mut Option<String>,
    need_redraw: &mut bool,
) -> Option<Option<ShutdownReason>> {
    let Some(action) = crate::app::runtime_dispatch::directory_operation_confirmation_key_action(
        key,
        session_state,
    ) else {
        return None;
    };
    *need_redraw = true;
    let mut shutdown_reason = None;
    match action {
        crate::app::runtime_dispatch::DirectoryOperationConfirmationKeyAction::Confirm => {
            let snapshot = outcome.core_bridge.snapshot();
            let save_outcome = crate::app::runtime_dispatch::save_snapshot_result_with_confirmation(
                &snapshot.text,
                session_state,
                None,
                true,
                Some(outcome.core_bridge.revision()),
            );
            *transient_msg = save_outcome.transient_message;
            if save_outcome.wrote {
                crate::app::runtime_dispatch::refresh_directory_buffer_after_confirmed_save(
                    outcome,
                    session_state,
                    transient_msg,
                );
                shutdown_reason =
                    crate::app::runtime_dispatch::take_pending_directory_save_then_quit_shutdown(
                        session_state,
                    );
            }
        }
        crate::app::runtime_dispatch::DirectoryOperationConfirmationKeyAction::Cancel => {
            *transient_msg = Some(
                crate::app::runtime_dispatch::directory_operation_cancel_message(session_state),
            );
        }
        crate::app::runtime_dispatch::DirectoryOperationConfirmationKeyAction::KeepWaiting => {
            *transient_msg = Some(
                "Apply directory operations? Press y or Enter for OK, n or Esc to cancel"
                    .to_string(),
            );
        }
    }
    Some(shutdown_reason)
}

pub(super) fn latest_user_visible_message(messages: Vec<CoreMessageEvent>) -> Option<String> {
    messages
        .into_iter()
        .filter_map(|event| {
            let trimmed = event.content.trim();
            if trimmed.is_empty() || !event.category.is_user_visible() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .last()
}

pub(super) fn is_markdown_target_path(path: Option<&PathBuf>) -> bool {
    path.and_then(|path| path.extension())
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "md" | "markdown" | "mdown"
            )
        })
        .unwrap_or(false)
}

pub(super) fn resolve_runtime_current_window_id(
    snapshot: &vim_core_rs::CoreSnapshot,
) -> Option<u64> {
    let active_window_id = snapshot
        .active_window_id()
        .map(|window_id| window_id as u64);
    log::debug!(
        "[main] resolve runtime current window id: snapshot_active_window_id={:?}, chosen_window_id={:?}",
        snapshot.active_window_id(),
        active_window_id,
    );
    active_window_id
}

pub(super) fn main_test_workspace() -> WorkspaceScreenModel {
    WorkspaceScreenModel {
        panes: vec![crate::presentation::screen_model::ScreenModel {
            window_id: 1,
            buffer_id: 1,
            rect: crate::presentation::screen_model::PaneRect {
                x: 0,
                y: 0,
                width: 20,
                height: 3,
            },
            file_name: "alpha.txt".to_string(),
            mode_label: "NORMAL".to_string(),
            status_line: "test.txt | NORMAL".to_string(),
            cursor_style: ScreenCursorStyle::Block,
            dirty: false,
            lines: vec!["alpha".to_string()],
            line_projections: vec![],
            cursor_row: 0,
            cursor_col: 0,
            visual_selection: None,
            search_overlays: vec![],
            syntax_chunks: vec![],
            markdown_style_ranges: vec![],
            filer_style_ranges: vec![],
            resolved_theme: crate::presentation::theme::ResolvedTheme::default(),
            message_line: None,
            command_cursor_col: None,
            is_active: true,
        }],
        floats: vec![],
        active_window_id: 1,
        message_line: crate::core::notification_prompt::resolve_workspace_message_line(Vec::<
            crate::core::notification_prompt::MessageLineCandidate,
        >::new(
        )),
        message_area_height: 5,
        message_scroll_offset: 0,
        prompt_line: None,
        pager_prompt: None,
        suppressed_prompt_hints: vec![],
        bell: None,
        command_line: None,
    }
}
