//! コマンドライン専用の軽量リドロー経路と、コア画面サイズ同期の
//! ヘルパー群。main loop のリドロー段から分離する。

use crate::app::bootstrap::BootstrapOutcome;
use crate::input::command_line_editor::command_line_cursor_display_col;
use crate::presentation::overlay::optional_graphics::OverlayTerminalWriter;
use crate::presentation::render::coordinator::{RenderFrameError, TuiRenderCoordinator};
use crate::presentation::render::redraw_trace::trace_redraw_diagnostic;
use crate::presentation::render::workspace_output::structural_refresh_is_idle;
use crate::presentation::screen_model::{CommandLineModel, WorkspaceScreenModel};
use crate::presentation::structural_refresh::StructuralRefreshOutcome;
use crate::terminal::lifecycle::TerminalSize;

/// 端末サイズが前回同期時と変わっていればコアブリッジへ反映する。
/// 反映したら `true` を返す。
pub fn sync_core_screen_size_if_changed(
    outcome: &mut BootstrapOutcome,
    last_synced_terminal_size: &mut Option<TerminalSize>,
    terminal_size: TerminalSize,
) -> bool {
    if *last_synced_terminal_size == Some(terminal_size) {
        log::debug!(
            "[main] skipping unchanged core screen size sync: rows={}, cols={}",
            terminal_size.rows,
            terminal_size.columns
        );
        return false;
    }

    trace_redraw_diagnostic(format_args!(
        "core screen size sync requested: previous={:?}, next={:?}",
        last_synced_terminal_size, terminal_size
    ));
    outcome.core_bridge.set_screen_size(
        i32::from(terminal_size.rows),
        i32::from(terminal_size.columns),
    );
    *last_synced_terminal_size = Some(terminal_size);
    true
}

/// コマンドライン専用リドローのために、直近のワークスペースを再利用して
/// コマンドラインだけ差し替えたモデルを構築する。再利用できない場合は `None`。
pub fn build_command_line_only_workspace(
    last_workspace: Option<&WorkspaceScreenModel>,
    command_line_prompt: Option<char>,
    command_line_buffer: &str,
    command_line_cursor_byte_index: usize,
    tab_size: u16,
) -> Option<WorkspaceScreenModel> {
    if command_line_prompt != Some(':') {
        return None;
    }
    if substitute_preview_command_may_need_workspace_projection(command_line_buffer) {
        trace_redraw_diagnostic(format_args!(
            "command-line-only redraw bypassed because substitute live preview needs search overlay projection"
        ));
        return None;
    }
    let last_workspace = last_workspace?;
    let preview = format!(":{}", command_line_buffer);
    let cursor_col = command_line_cursor_display_col(
        ':',
        command_line_buffer,
        command_line_cursor_byte_index,
        tab_size,
    );
    log::debug!(
        "[main] reusing last workspace for command-line-only redraw: prompt=:, buffer_len={}, cursor_col={}",
        command_line_buffer.len(),
        cursor_col
    );

    let mut workspace = last_workspace.clone();
    workspace.command_line = Some(CommandLineModel {
        text: preview,
        cursor_col,
    });
    Some(workspace)
}

fn substitute_preview_command_may_need_workspace_projection(command_line_buffer: &str) -> bool {
    let input = command_line_buffer
        .trim_start()
        .strip_prefix(':')
        .unwrap_or(command_line_buffer.trim_start());
    let input = input
        .strip_prefix('%')
        .unwrap_or_else(|| strip_ex_range_prefix(input));
    let input = input.trim_start();
    input
        .strip_prefix("s")
        .is_some_and(|rest| substitute_command_boundary(rest))
        || input
            .strip_prefix("substitute")
            .is_some_and(|rest| substitute_command_boundary(rest))
}

fn strip_ex_range_prefix(input: &str) -> &str {
    let range_len = input
        .char_indices()
        .take_while(|(_, ch)| matches!(ch, '0'..='9' | '.' | '$' | ',' | ';' | '+' | '-'))
        .last()
        .map(|(index, ch)| index + ch.len_utf8())
        .unwrap_or(0);
    &input[range_len..]
}

fn substitute_command_boundary(rest: &str) -> bool {
    rest.bytes()
        .next()
        .is_none_or(|byte| byte.is_ascii_punctuation() || byte.is_ascii_whitespace())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandLineOnlyRedraw {
    Rendered,
    NotApplicable,
    Fallback,
}

/// 構造リフレッシュが落ち着いていてプロジェクションも汚れていない場合に、
/// ワークスペース全体を描き直さずコマンドラインのみ更新する軽量経路。
pub fn render_command_line_only_redraw_if_possible(
    render_coordinator: &mut TuiRenderCoordinator,
    overlay_writer: Option<&mut dyn OverlayTerminalWriter>,
    last_workspace_model: &mut Option<WorkspaceScreenModel>,
    structural_refresh: Option<&StructuralRefreshOutcome>,
    workspace_projection_dirty: bool,
    command_line_prompt: Option<char>,
    command_line_buffer: &str,
    command_line_cursor_byte_index: usize,
    tab_size: u16,
) -> CommandLineOnlyRedraw {
    if !structural_refresh_is_idle(structural_refresh) {
        return CommandLineOnlyRedraw::NotApplicable;
    }
    if workspace_projection_dirty {
        trace_redraw_diagnostic(format_args!(
            "command-line-only redraw bypassed because workspace projection is dirty"
        ));
        return CommandLineOnlyRedraw::NotApplicable;
    }
    let Some(workspace) = build_command_line_only_workspace(
        last_workspace_model.as_ref(),
        command_line_prompt,
        command_line_buffer,
        command_line_cursor_byte_index,
        tab_size,
    ) else {
        return CommandLineOnlyRedraw::NotApplicable;
    };
    let Some(command_line) = workspace.command_line.as_ref() else {
        return CommandLineOnlyRedraw::NotApplicable;
    };

    trace_redraw_diagnostic(format_args!(
        "workspace redraw skipped for command-line-only overlay: command_prompt={:?}, command_buffer_len={}",
        command_line_prompt,
        command_line_buffer.len()
    ));
    match render_coordinator.render_command_line_overlay(command_line, overlay_writer) {
        Ok(()) => {
            *last_workspace_model = Some(workspace);
            CommandLineOnlyRedraw::Rendered
        }
        Err(error) => {
            trace_command_line_overlay_fallback(&error);
            log::debug!(
                "[main] command-line-only overlay failed; falling back to full workspace render: error={:?}",
                error
            );
            CommandLineOnlyRedraw::Fallback
        }
    }
}

fn trace_command_line_overlay_fallback(error: &RenderFrameError) {
    trace_redraw_diagnostic(format_args!(
        "command-line-only overlay fallback to workspace redraw: error={:?}",
        error
    ));
}
