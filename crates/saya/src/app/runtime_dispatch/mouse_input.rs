//! マウス入力（クリック / ホイール）と貼り付けテキストの
//! ディスパッチを main loop から分離する。

use crate::app::bootstrap::BootstrapOutcome;
use crate::app::event_loop::ShutdownReason;
use crate::app::outcome_consume::{MainOutcomeAccumulator, consume_core_outcomes_from_core};
use crate::app::runtime_dispatch::{
    LsifBridgeHandle, process_pending_host_actions_with_runtime, sync_session_dirty_from_core,
};
use crate::app::session::EditorSessionState;
use crate::core::host_actions::HostActionRuntime;
use crate::presentation::floating_input::{
    active_mermaid_preview_float_id, focus_floating_window_from_mouse_click,
    focus_mermaid_preview_from_mouse_click, handle_mermaid_preview_mouse_wheel,
};
use crate::presentation::floating_window::{
    FloatingMouseOutcome, FloatingWindowId, FloatingWindowManager,
};
use crate::presentation::overlay::effect::RuntimePresentationIntent;
use crate::presentation::screen_model::WorkspaceScreenModel;
use crate::runtime::integration::RuntimeSessionOwner;
use crate::terminal::lifecycle::current_terminal_size;

/// マウスクリックイベントを処理する。フローティングウィンドウへの
/// フォーカスか、エディタ本体への SGR シーケンス転送に振り分ける。
/// shutdown 要求が出たら理由を返す。
#[allow(clippy::too_many_arguments)]
pub async fn dispatch_mouse_click(
    column: u16,
    row: u16,
    outcome: &mut BootstrapOutcome,
    outcome_accumulator: &mut MainOutcomeAccumulator,
    session_state: &mut EditorSessionState,
    last_workspace_model: Option<&WorkspaceScreenModel>,
    floating_window_manager: &mut FloatingWindowManager,
    transient_msg: &mut Option<String>,
    system_warning: &mut Option<String>,
    host_action_runtime: &mut HostActionRuntime,
    runtime_session: &mut Option<RuntimeSessionOwner>,
    need_redraw: &mut bool,
    runtime_presentation_intents: &mut Vec<RuntimePresentationIntent>,
    workspace_projection_dirty: &mut bool,
    lsif_bridge: &LsifBridgeHandle,
) -> Option<ShutdownReason> {
    log::debug!(
        "[main] processing mouse click event at terminal coordinates: column={}, row={}",
        column,
        row
    );
    let (terminal_width, terminal_height) = current_terminal_size();
    let mermaid_preview_focused =
        focus_mermaid_preview_from_mouse_click(session_state, last_workspace_model, column, row);
    let mouse_focus = if mermaid_preview_focused {
        FloatingMouseOutcome::Focused {
            id: active_mermaid_preview_float_id(last_workspace_model)
                .unwrap_or(FloatingWindowId(0)),
        }
    } else {
        focus_floating_window_from_mouse_click(
            floating_window_manager,
            last_workspace_model,
            column,
            row,
            terminal_width,
            terminal_height,
        )
    };
    if matches!(mouse_focus, FloatingMouseOutcome::Focused { .. }) {
        *workspace_projection_dirty = true;
    } else if let Some(sequence) = mouse_click_to_sgr_sequence(last_workspace_model, column, row) {
        log::debug!(
            "[main] dispatching mouse click as SGR sequence: column={}, row={}, sequence={:?}",
            column,
            row,
            sequence
        );
        let _ = outcome.core_bridge.dispatch_key(&sequence);
        consume_core_outcomes_from_core(&mut outcome.core_bridge, outcome_accumulator, need_redraw);

        if let Some(reason) = process_pending_host_actions_with_runtime(
            outcome,
            outcome_accumulator,
            session_state,
            transient_msg,
            system_warning,
            host_action_runtime,
            runtime_session.as_mut(),
            need_redraw,
            runtime_presentation_intents,
            Some(lsif_bridge),
        )
        .await
        {
            return Some(reason);
        }

        sync_session_dirty_from_core(session_state, &outcome.core_bridge);
    } else {
        log::debug!(
            "[main] ignoring mouse click outside editor body: column={}, row={}",
            column,
            row
        );
    }
    *need_redraw = true;
    None
}

/// マウスホイールイベントを処理する。現状は Mermaid プレビューの
/// パン操作のみ受け付ける。
pub fn dispatch_mouse_wheel(
    column: u16,
    row: u16,
    delta_x: i16,
    delta_y: i16,
    session_state: &mut EditorSessionState,
    last_workspace_model: Option<&WorkspaceScreenModel>,
    need_redraw: &mut bool,
    workspace_projection_dirty: &mut bool,
) {
    log::debug!(
        "[main] processing mouse wheel event: column={}, row={}, delta=({}, {})",
        column,
        row,
        delta_x,
        delta_y
    );
    if handle_mermaid_preview_mouse_wheel(
        session_state,
        last_workspace_model,
        column,
        row,
        delta_x,
        delta_y,
    ) {
        *need_redraw = true;
        *workspace_projection_dirty = true;
    }
}

/// 貼り付けテキストを 1 文字ずつコアブリッジへ転送する。
/// shutdown 要求が出たら理由を返す。
#[allow(clippy::too_many_arguments)]
pub async fn dispatch_pasted_text(
    text: &str,
    outcome: &mut BootstrapOutcome,
    outcome_accumulator: &mut MainOutcomeAccumulator,
    session_state: &mut EditorSessionState,
    transient_msg: &mut Option<String>,
    system_warning: &mut Option<String>,
    host_action_runtime: &mut HostActionRuntime,
    runtime_session: &mut Option<RuntimeSessionOwner>,
    need_redraw: &mut bool,
    runtime_presentation_intents: &mut Vec<RuntimePresentationIntent>,
    lsif_bridge: &LsifBridgeHandle,
) -> Option<ShutdownReason> {
    log::debug!(
        "[main] dispatching pasted text to core bridge: chars={}",
        text.chars().count()
    );
    for unit in pasted_text_to_dispatch_units(text) {
        let _ = outcome.core_bridge.dispatch_key(&unit);
    }
    consume_core_outcomes_from_core(&mut outcome.core_bridge, outcome_accumulator, need_redraw);

    if let Some(reason) = process_pending_host_actions_with_runtime(
        outcome,
        outcome_accumulator,
        session_state,
        transient_msg,
        system_warning,
        host_action_runtime,
        runtime_session.as_mut(),
        need_redraw,
        runtime_presentation_intents,
        Some(lsif_bridge),
    )
    .await
    {
        return Some(reason);
    }

    sync_session_dirty_from_core(session_state, &outcome.core_bridge);
    *need_redraw = true;
    None
}

/// エディタ本体の領域内をクリックしたとき、その座標を SGR マウス
/// シーケンスへ変換する。領域外なら `None`。
pub fn mouse_click_to_sgr_sequence(
    workspace_model: Option<&WorkspaceScreenModel>,
    column: u16,
    row: u16,
) -> Option<String> {
    let workspace_model = workspace_model?;
    let inside_editor_body = workspace_model.panes.iter().any(|pane| {
        let body_height = pane.rect.height.saturating_sub(1).max(1);
        let column_offset = column.saturating_sub(pane.rect.x);
        let row_offset = row.saturating_sub(pane.rect.y);
        column >= pane.rect.x
            && row >= pane.rect.y
            && column_offset < pane.rect.width
            && row_offset < body_height
    });

    if inside_editor_body {
        let sgr_column = column.saturating_add(1);
        let sgr_row = row.saturating_add(1);
        Some(format!("\x1b[<0;{sgr_column};{sgr_row}M"))
    } else {
        None
    }
}

/// 貼り付けテキストをコアブリッジへ渡すディスパッチ単位（1 文字ずつ）
/// へ分解する。
pub fn pasted_text_to_dispatch_units(text: &str) -> Vec<String> {
    text.chars().map(|ch| ch.to_string()).collect()
}
