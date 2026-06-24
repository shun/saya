//! フローティングウィンドウ／パネルのキー・マウス入力ハンドリング。
//!
//! 補完メニュー・コアウィンドウフロート・ターミナルフロート・ターミナル
//! パネル・Mermaid プレビューに対するキー入力やマウス操作を、対応する
//! マネージャ（`FloatingWindowManager` / `CompletionFloatManager` /
//! `TerminalFloatManager` / `PanelManager`）へ振り分ける。イベントループ
//! 本体から呼ばれ、処理結果（`FloatingWindowKeyHandling` など）を返す。

use crate::app::session::EditorSessionState;
use crate::features::completion::float::{CompletionFloatInputOutcome, CompletionFloatManager};
use crate::input::router::{EditorIntent, KeyInput, NavigationKey, resolve_intent};
use crate::presentation::floating_window::{
    FloatingInputOutcome, FloatingLifecycleEvent, FloatingMouseOutcome, FloatingWindowId,
    FloatingWindowManager,
};
use crate::presentation::panel::PanelManager;
use crate::presentation::screen_model::WorkspaceScreenModel;
use crate::terminal::float::TerminalFloatManager;
use vim_core_rs::{CoreLightSnapshot, CoreMode};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatingWindowKeyHandling {
    Consumed,
    Closed { id: FloatingWindowId },
}

/// `main` のイベントループで、`command_line_prompt` 等のホスト所有導線を通過した
/// 後に「どの focused サブシステムへキーを振り分けるか」を表す分類結果。
///
/// 実際のキー処理（leaf マネージャ呼び出し）はそれぞれの `dispatch_*` ラッパが
/// 行うが、その**振り分け順序の判断**は副作用のない `focused_input_target_for_key`
/// に集約する。これにより「フロート/補完/パネルは開くがキーが効かない」回帰
/// （継ぎ目断線）を、巨大ループを再構成せずに単体テストで検出できるようにする。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusedInputTarget {
    /// focused なパネル端末が `:` / `/` を受けて command-line 入口へ昇格する。
    PanelCommandLineEntry,
    /// focused な端末パネルへキーを書き込む（`begin_command_line` で昇格しない場合）。
    TerminalPanel,
    /// focused な端末フロートへキーを書き込む。
    TerminalFloat,
    /// focused な core-window フロートへキーを dispatch する。
    CoreWindowFloat,
    /// focused な completion メニューへキーを送る。
    CompletionFloat,
    /// focused な static-lines フロート（ホバー等）へキーを送る。
    FloatingWindow,
    /// どの focused サブシステムにも該当しない（通常編集パイプラインへ）。
    None,
}

/// focused サブシステムへのキー振り分け対象を分類する純粋関数。
///
/// 引数は `main` イベントループが保持する各マネージャの focus 状態と、
/// command-line 昇格に必要な最小限のコンテキストのみ。`main.rs` の
/// `if !handled { ... }` 連鎖と**同じ優先順位**を表現し、本番ループは
/// 観測ログとしてこの関数を実際に通すことで、配線をテストで検証できる
/// 形にしている（C1 の `command_line_entry_for_key` と同じ思想）。
///
/// 優先順位は `dispatch_floating_ui_key`（panel → terminal float → core window
/// float）→ `dispatch_completion_float_key` → `dispatch_floating_window_key`
/// の実コード順に一致させる。
#[allow(clippy::too_many_arguments)]
pub fn focused_input_target_for_key(
    key: &KeyInput,
    mode: CoreMode,
    panel_focused_terminal: bool,
    floating_focused_terminal: bool,
    floating_focused_core_window: bool,
    completion_menu_active: bool,
    floating_focused_static_lines: bool,
) -> FocusedInputTarget {
    if panel_focused_terminal {
        // `begin_command_line_from_focused_panel` は Normal モードの `:` / `/`
        // のみ昇格させ、それ以外は端末パネルへキーを書き込む。
        let is_command_line_entry = matches!(
            (mode, key),
            (CoreMode::Normal, KeyInput::Char(':') | KeyInput::Char('/'))
        );
        return if is_command_line_entry {
            FocusedInputTarget::PanelCommandLineEntry
        } else {
            FocusedInputTarget::TerminalPanel
        };
    }
    if floating_focused_terminal {
        return FocusedInputTarget::TerminalFloat;
    }
    if floating_focused_core_window {
        return FocusedInputTarget::CoreWindowFloat;
    }
    if completion_menu_active {
        return FocusedInputTarget::CompletionFloat;
    }
    if floating_focused_static_lines {
        return FocusedInputTarget::FloatingWindow;
    }
    FocusedInputTarget::None
}

pub fn handle_completion_float_key(
    completion_manager: &mut CompletionFloatManager,
    floating_manager: &mut FloatingWindowManager,
    core_bridge: &mut crate::core::bridge::CoreBridge,
    key: &KeyInput,
    restore_window_id: i32,
) -> Option<FloatingWindowKeyHandling> {
    match completion_manager.handle_key(floating_manager, key, Some(restore_window_id)) {
        CompletionFloatInputOutcome::Selected {
            menu_id,
            selected_index,
        } => {
            log::debug!(
                "[main] completion selection updated from focused input: menu_id={}, selected_index={}",
                menu_id.0,
                selected_index
            );
            Some(FloatingWindowKeyHandling::Consumed)
        }
        CompletionFloatInputOutcome::Accepted { menu_id, candidate } => {
            let insert_text = candidate.insert_text();
            if !insert_text.is_empty() {
                let result = if let Some(replace_range) = candidate.replace_range.as_ref() {
                    core_bridge.apply_completion_replace_range(replace_range, insert_text)
                } else {
                    core_bridge.dispatch_key(insert_text).map(|_| ())
                };
                if let Err(error) = result {
                    log::debug!(
                        "[main] completion candidate insertion failed: menu_id={}, label={:?}, insert_text_len={}, error={:?}",
                        menu_id.0,
                        candidate.label,
                        insert_text.len(),
                        error
                    );
                }
            }
            log::debug!(
                "[main] completion candidate accepted from focused input: menu_id={}, label={:?}",
                menu_id.0,
                candidate.label
            );
            Some(FloatingWindowKeyHandling::Closed { id: menu_id })
        }
        CompletionFloatInputOutcome::Closed {
            menu_id,
            editor_key,
        } => {
            if let Some(editor_key) = editor_key {
                if let EditorIntent::EditKey(core_key) = resolve_intent(&editor_key) {
                    let before = core_bridge.light_snapshot();
                    if let Err(error) = core_bridge.dispatch_key(&core_key) {
                        log::debug!(
                            "[main] completion close editor key dispatch failed: menu_id={}, key={:?}, core_key={:?}, error={:?}",
                            menu_id.0,
                            editor_key,
                            core_key,
                            error
                        );
                    }
                    let after = core_bridge.light_snapshot();
                    let _ =
                        apply_floating_lifecycle_after_core_edit(floating_manager, &before, &after);
                }
            }
            Some(FloatingWindowKeyHandling::Closed { id: menu_id })
        }
        CompletionFloatInputOutcome::Ignored => None,
    }
}

pub fn handle_core_window_float_key(
    manager: &mut FloatingWindowManager,
    core_bridge: &mut crate::core::bridge::CoreBridge,
    key: &KeyInput,
) -> Option<FloatingWindowKeyHandling> {
    let window_id = manager.focused_core_window_id()?;
    let EditorIntent::EditKey(core_key) = resolve_intent(key) else {
        log::debug!(
            "[main][buffer_float] focused core-window float ignored application command key: key={:?}, window_id={}",
            key,
            window_id
        );
        return None;
    };
    let before = core_bridge.light_snapshot();
    if before.active_window_id() != Some(window_id)
        && let Err(error) = core_bridge.switch_to_window(window_id)
    {
        log::debug!(
            "[main][buffer_float] failed to focus core window before dispatch: window_id={}, key={:?}, error={:?}",
            window_id,
            key,
            error
        );
        return None;
    }
    let dispatch_result = core_bridge.dispatch_key(&core_key);
    let after = core_bridge.light_snapshot();
    log::debug!(
        "[main][buffer_float] dispatched key to focused core-window float: window_id={}, key={:?}, core_key={:?}, result={:?}, revision {}->{}, cursor ({},{}) -> ({},{}), mode {:?}->{:?}",
        window_id,
        key,
        core_key,
        dispatch_result,
        before.revision,
        after.revision,
        before.cursor_row,
        before.cursor_col,
        after.cursor_row,
        after.cursor_col,
        before.mode,
        after.mode
    );
    Some(FloatingWindowKeyHandling::Consumed)
}

pub fn handle_terminal_float_key(
    manager: &FloatingWindowManager,
    terminal_manager: &mut TerminalFloatManager,
    key: &KeyInput,
) -> Option<FloatingWindowKeyHandling> {
    let terminal_id = manager.focused_terminal_id()?;
    let result = match key {
        KeyInput::PageUp => terminal_manager.scroll(terminal_id, -8),
        KeyInput::PageDown => terminal_manager.scroll(terminal_id, 8),
        _ => terminal_manager.write_key(terminal_id, key),
    };
    match result {
        Ok(()) => {
            log::debug!(
                "[main][terminal_float] focused terminal float consumed key: terminal_id={}, key={:?}",
                terminal_id,
                key
            );
            Some(FloatingWindowKeyHandling::Consumed)
        }
        Err(error) => {
            log::debug!(
                "[main][terminal_float] focused terminal float failed to consume key: terminal_id={}, key={:?}, error={:?}",
                terminal_id,
                key,
                error
            );
            None
        }
    }
}

pub fn handle_floating_window_key(
    manager: &mut FloatingWindowManager,
    key: &KeyInput,
    restore_window_id: i32,
) -> Option<FloatingWindowKeyHandling> {
    match manager.handle_focused_static_lines_key_with_restore(key, Some(restore_window_id)) {
        FloatingInputOutcome::Consumed => Some(FloatingWindowKeyHandling::Consumed),
        FloatingInputOutcome::Closed { id } => Some(FloatingWindowKeyHandling::Closed { id }),
        FloatingInputOutcome::Ignored => None,
    }
}

pub fn handle_mermaid_preview_key(
    session_state: &mut EditorSessionState,
    key: &KeyInput,
) -> Option<FloatingWindowKeyHandling> {
    if !session_state.mermaid_preview_focused() {
        return None;
    }
    let float_id = FloatingWindowId(9_000_000_000);
    let outcome = match key {
        KeyInput::Escape | KeyInput::Ctrl('[') | KeyInput::Char('q') => {
            session_state.close_mermaid_preview("focused_key_close");
            FloatingWindowKeyHandling::Closed { id: float_id }
        }
        KeyInput::Char('+') | KeyInput::Char('=') => {
            session_state.zoom_mermaid_preview_in();
            FloatingWindowKeyHandling::Consumed
        }
        KeyInput::Char('-') => {
            session_state.zoom_mermaid_preview_out();
            FloatingWindowKeyHandling::Consumed
        }
        KeyInput::Char('0') => {
            session_state.zoom_mermaid_preview_fit();
            FloatingWindowKeyHandling::Consumed
        }
        KeyInput::Char('1') => {
            session_state.zoom_mermaid_preview_actual_size();
            FloatingWindowKeyHandling::Consumed
        }
        KeyInput::Char('h') | KeyInput::Left => {
            session_state.pan_mermaid_preview(-64, 0);
            FloatingWindowKeyHandling::Consumed
        }
        KeyInput::Char('l') | KeyInput::Right => {
            session_state.pan_mermaid_preview(64, 0);
            FloatingWindowKeyHandling::Consumed
        }
        KeyInput::Char('k') | KeyInput::Up => {
            session_state.pan_mermaid_preview(0, -64);
            FloatingWindowKeyHandling::Consumed
        }
        KeyInput::Char('j') | KeyInput::Down => {
            session_state.pan_mermaid_preview(0, 64);
            FloatingWindowKeyHandling::Consumed
        }
        KeyInput::Ctrl('b') | KeyInput::Ctrl('B') | KeyInput::PageUp => {
            session_state.pan_mermaid_preview(0, -256);
            FloatingWindowKeyHandling::Consumed
        }
        KeyInput::Ctrl('f') | KeyInput::Ctrl('F') | KeyInput::PageDown => {
            session_state.pan_mermaid_preview(0, 256);
            FloatingWindowKeyHandling::Consumed
        }
        KeyInput::Char('H') | KeyInput::ShiftedNav(NavigationKey::Left) => {
            session_state.pan_mermaid_preview(-256, 0);
            FloatingWindowKeyHandling::Consumed
        }
        KeyInput::Char('L') | KeyInput::ShiftedNav(NavigationKey::Right) => {
            session_state.pan_mermaid_preview(256, 0);
            FloatingWindowKeyHandling::Consumed
        }
        _ => {
            log::debug!(
                "[main][markdown_preview] focused Mermaid preview ignored key: key={:?}",
                key
            );
            return None;
        }
    };
    log::debug!(
        "[main][markdown_preview] focused Mermaid preview handled key: key={:?}, outcome={:?}",
        key,
        outcome
    );
    Some(outcome)
}

pub fn active_mermaid_preview_float_id(
    workspace: Option<&WorkspaceScreenModel>,
) -> Option<FloatingWindowId> {
    workspace?
        .floats
        .iter()
        .find(|float| !float.images.is_empty())
        .map(|float| float.id)
}

pub fn focus_mermaid_preview_from_mouse_click(
    session_state: &mut EditorSessionState,
    workspace: Option<&WorkspaceScreenModel>,
    column: u16,
    row: u16,
) -> bool {
    if !mouse_cell_hits_mermaid_preview(workspace, column, row) {
        return false;
    }
    session_state.focus_mermaid_preview();
    log::debug!(
        "[main][markdown_preview] Mermaid preview focused by mouse click: column={}, row={}",
        column,
        row
    );
    true
}

pub fn handle_mermaid_preview_mouse_wheel(
    session_state: &mut EditorSessionState,
    workspace: Option<&WorkspaceScreenModel>,
    column: u16,
    row: u16,
    delta_x: i16,
    delta_y: i16,
) -> bool {
    if !session_state.mermaid_preview_focused()
        || !mouse_cell_hits_mermaid_preview(workspace, column, row)
    {
        return false;
    }
    session_state.pan_mermaid_preview(i32::from(delta_x) * 96, i32::from(delta_y) * 96);
    log::debug!(
        "[main][markdown_preview] Mermaid preview handled mouse wheel: column={}, row={}, delta=({}, {})",
        column,
        row,
        delta_x,
        delta_y
    );
    true
}

pub(crate) fn mouse_cell_hits_mermaid_preview(
    workspace: Option<&WorkspaceScreenModel>,
    column: u16,
    row: u16,
) -> bool {
    workspace
        .into_iter()
        .flat_map(|workspace| workspace.floats.iter())
        .filter(|float| !float.images.is_empty())
        .any(|float| {
            column >= float.rect.x
                && column < float.rect.x.saturating_add(float.rect.width)
                && row >= float.rect.y
                && row < float.rect.y.saturating_add(float.rect.height)
        })
}

pub fn handle_terminal_panel_key(
    manager: &mut PanelManager,
    terminal_manager: &mut TerminalFloatManager,
    key: &KeyInput,
) -> Option<FloatingWindowKeyHandling> {
    let terminal_id = manager.focused_terminal_id()?;
    if matches!(key, KeyInput::Ctrl('w') | KeyInput::Ctrl('W')) {
        let had_focus = manager.unfocus();
        log::debug!(
            "[main][panel] terminal panel unfocused from key: key={:?}, had_focus={}",
            key,
            had_focus
        );
        return had_focus.then_some(FloatingWindowKeyHandling::Consumed);
    }
    let result = match key {
        KeyInput::PageUp => terminal_manager.scroll(terminal_id, -8),
        KeyInput::PageDown => terminal_manager.scroll(terminal_id, 8),
        _ => terminal_manager.write_key(terminal_id, key),
    };
    match result {
        Ok(()) => {
            log::debug!(
                "[main][panel] focused terminal panel consumed key: terminal_id={}, key={:?}",
                terminal_id,
                key
            );
            Some(FloatingWindowKeyHandling::Consumed)
        }
        Err(error) => {
            log::debug!(
                "[main][panel] focused terminal panel failed to consume key: terminal_id={}, key={:?}, error={:?}",
                terminal_id,
                key,
                error
            );
            None
        }
    }
}

pub fn begin_command_line_from_focused_panel(
    manager: &mut PanelManager,
    key: &KeyInput,
    mode: CoreMode,
) -> Option<char> {
    let prompt = match (mode, key) {
        (CoreMode::Normal, KeyInput::Char(':')) => ':',
        (CoreMode::Normal, KeyInput::Char('/')) => '/',
        _ => return None,
    };
    let terminal_id = manager.focused_terminal_id()?;
    let had_focus = manager.unfocus();
    log::debug!(
        "[main][panel] focused terminal panel yielded command-line prompt: terminal_id={}, prompt={}, had_focus={}",
        terminal_id,
        prompt,
        had_focus
    );
    had_focus.then_some(prompt)
}

pub fn focus_floating_window_from_mouse_click(
    manager: &mut FloatingWindowManager,
    workspace: Option<&WorkspaceScreenModel>,
    column: u16,
    row: u16,
    terminal_width: u16,
    terminal_height: u16,
) -> FloatingMouseOutcome {
    let Some(workspace) = workspace else {
        log::debug!(
            "[main] floating mouse focus skipped because no workspace model is available: column={}, row={}",
            column,
            row
        );
        return FloatingMouseOutcome::PassThrough;
    };
    let pane_rects = workspace
        .panes
        .iter()
        .map(|pane| (pane.window_id, pane.rect))
        .collect::<Vec<_>>();
    let outcome = manager.focus_topmost_at(
        column,
        row,
        terminal_width,
        terminal_height,
        &pane_rects,
        Some(workspace.active_window_id),
    );
    log::debug!(
        "[main] floating mouse focus resolved: column={}, row={}, outcome={:?}",
        column,
        row,
        outcome
    );
    outcome
}

pub fn apply_floating_lifecycle_after_core_edit(
    floating_window_manager: &mut FloatingWindowManager,
    before: &CoreLightSnapshot,
    after: &CoreLightSnapshot,
) -> bool {
    let restore_window_id = after
        .active_window_id()
        .or_else(|| before.active_window_id());
    let mut closed = Vec::new();
    if before.cursor_row != after.cursor_row
        || before.cursor_col != after.cursor_col
        || before.active_window_id() != after.active_window_id()
    {
        if let Some(window_id) = restore_window_id {
            closed.extend(
                floating_window_manager
                    .apply_lifecycle_event(
                        FloatingLifecycleEvent::CursorMoved {
                            window_id,
                            row: after.cursor_row,
                            col: after.cursor_col,
                        },
                        Some(window_id),
                    )
                    .closed,
            );
        }
    }
    if before.mode != CoreMode::Insert
        && after.mode == CoreMode::Insert
        && let Some(window_id) = restore_window_id
    {
        closed.extend(
            floating_window_manager
                .apply_lifecycle_event(
                    FloatingLifecycleEvent::InsertStarted { window_id },
                    Some(window_id),
                )
                .closed,
        );
    }
    if before.mode != after.mode
        && let Some(window_id) = restore_window_id
    {
        closed.extend(
            floating_window_manager
                .apply_lifecycle_event(
                    FloatingLifecycleEvent::ModeChanged {
                        window_id,
                        from: floating_editor_mode_from_core(before.mode),
                        to: floating_editor_mode_from_core(after.mode),
                    },
                    Some(window_id),
                )
                .closed,
        );
    }
    if let (Some(before_window_id), Some(after_window_id)) =
        (before.active_window_id(), after.active_window_id())
        && before_window_id != after_window_id
    {
        closed.extend(
            floating_window_manager
                .apply_lifecycle_event(
                    FloatingLifecycleEvent::WindowLeft {
                        from_window_id: before_window_id,
                        to_window_id: after_window_id,
                    },
                    Some(after_window_id),
                )
                .closed,
        );
    }
    if before.revision != after.revision {
        closed.extend(
            floating_window_manager
                .apply_lifecycle_event(
                    FloatingLifecycleEvent::BufferChanged {
                        buffer_id: after
                            .buffers
                            .iter()
                            .find(|buffer| buffer.is_active)
                            .map(|buffer| buffer.id)
                            .unwrap_or(0),
                        revision: after.revision,
                    },
                    restore_window_id,
                )
                .closed,
        );
    }
    let did_close = !closed.is_empty();
    if did_close {
        log::debug!(
            "[main][floating_window] lifecycle closed float(s) after core edit: closed={:?}, cursor=({},{}), revision={}",
            closed.iter().map(|id| id.0).collect::<Vec<_>>(),
            after.cursor_row,
            after.cursor_col,
            after.revision
        );
    }
    did_close
}

/// vim_core_rs の CoreMode を float lifecycle 用の中立 EditorMode へ変換する。
pub(crate) fn floating_editor_mode_from_core(
    mode: CoreMode,
) -> crate::presentation::floating_window::EditorMode {
    use crate::presentation::floating_window::EditorMode;
    match mode {
        CoreMode::Normal | CoreMode::OperatorPending => EditorMode::Normal,
        CoreMode::Insert => EditorMode::Insert,
        CoreMode::Visual
        | CoreMode::VisualLine
        | CoreMode::VisualBlock
        | CoreMode::Select
        | CoreMode::SelectLine
        | CoreMode::SelectBlock => EditorMode::Visual,
        CoreMode::Replace => EditorMode::Replace,
        CoreMode::CommandLine => EditorMode::Command,
    }
}

#[cfg(test)]
#[path = "floating_input_test.rs"]
mod focused_input_target_tests;
