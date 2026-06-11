//! CoreBridge のセッション初期化・変換ヘルパー。

use super::*;

pub(super) fn configure_utf8_encoding(
    session: &mut VimCoreSession,
) -> Result<(), vim_core_rs::CoreCommandError> {
    // saya は Markdown-first の UTF-8 エディタ。埋め込み Vim の内部エンコーディングを
    // 起動環境の locale（LANG / LC_CTYPE 等）に依存させず、常に UTF-8 へ固定する。
    // これにより locale 未設定の環境でもマルチバイト文字のカーソル移動・表示幅計算が
    // 決定論的になり、latin1 フォールバックによる 1 バイト単位移動を防ぐ。
    log::debug!("[core_bridge] forcing internal encoding: set encoding=utf-8");
    session.execute_ex_command(":set encoding=utf-8")?;
    Ok(())
}

pub(super) fn configure_message_suppression(
    session: &mut VimCoreSession,
) -> Result<(), vim_core_rs::CoreCommandError> {
    log::debug!("[core_bridge] configuring Vim message suppression: report=999999, shortmess+=F");
    session.execute_ex_command(":set report=999999 shortmess+=F")?;
    Ok(())
}

pub(super) fn configure_initial_syntax_state(
    session: &mut VimCoreSession,
) -> Result<(), vim_core_rs::CoreCommandError> {
    log::debug!("[core_bridge] configuring initial Vim syntax state: syntax off");
    session.execute_ex_command("syntax off")?;
    Ok(())
}

pub(super) fn normalize_ex_command(command: &str) -> Option<String> {
    let trimmed = command.trim();
    let trimmed = trimmed.strip_prefix(':').unwrap_or(trimmed).trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.split_whitespace().collect::<Vec<_>>().join(" "))
}

pub(super) fn is_visual_mode(mode: vim_core_rs::CoreMode) -> bool {
    matches!(
        mode,
        vim_core_rs::CoreMode::Visual
            | vim_core_rs::CoreMode::VisualLine
            | vim_core_rs::CoreMode::VisualBlock
    )
}

pub(super) fn normalize_selection_bounds(
    anchor: (usize, usize),
    cursor: (usize, usize),
) -> ((usize, usize), (usize, usize)) {
    if anchor <= cursor {
        (anchor, cursor)
    } else {
        (cursor, anchor)
    }
}

pub(super) fn map_input_response_error(error: CoreInputResponseError) -> PromptResponseError {
    match error {
        CoreInputResponseError::NoPendingInput => {
            PromptResponseError::CoreRejected(PromptResponseRejection::NoPendingInput)
        }
        CoreInputResponseError::CorrelationMismatch { expected, actual } => {
            PromptResponseError::CoreRejected(PromptResponseRejection::CoreCorrelationMismatch {
                expected,
                actual,
            })
        }
        CoreInputResponseError::Command(error) => PromptResponseError::CoreRejected(
            PromptResponseRejection::CommandRejected(format!("{error:?}")),
        ),
        CoreInputResponseError::EvalFailed => {
            PromptResponseError::CoreRejected(PromptResponseRejection::CommandRejected(
                "core eval failed after input response".to_string(),
            ))
        }
    }
}

pub(super) fn legacy_host_action_from_normalized(
    outcome: NormalizedCoreOutcome,
) -> Option<CoreHostAction> {
    match outcome {
        NormalizedCoreOutcome::HostDirective(NormalizedHostDirective::Write {
            path,
            force,
            issued_after_revision,
            trace,
        }) => {
            log::debug!(
                "[core_bridge] projected legacy write host action: sequence={}, revision={}",
                trace.sequence,
                issued_after_revision
            );
            Some(CoreHostAction::Write {
                path,
                force,
                issued_after_revision,
            })
        }
        NormalizedCoreOutcome::HostDirective(NormalizedHostDirective::Quit {
            force,
            issued_after_revision,
            trace,
        }) => {
            log::debug!(
                "[core_bridge] projected legacy quit host action: sequence={}, revision={}",
                trace.sequence,
                issued_after_revision
            );
            Some(CoreHostAction::Quit {
                force,
                issued_after_revision,
            })
        }
        NormalizedCoreOutcome::HostDirective(NormalizedHostDirective::Suspend { trace }) => {
            log::debug!(
                "[core_bridge] projected legacy suspend host action: sequence={}",
                trace.sequence
            );
            Some(CoreHostAction::Suspend)
        }
        NormalizedCoreOutcome::HostDirective(NormalizedHostDirective::VfsRequest {
            request,
            trace,
        }) => {
            log::debug!(
                "[core_bridge] projected legacy vfs host action: sequence={}",
                trace.sequence
            );
            Some(CoreHostAction::VfsRequest(request))
        }
        _ => None,
    }
}

pub(super) fn legacy_message_from_normalized(
    outcome: NormalizedCoreOutcome,
) -> Option<CoreMessageEvent> {
    match outcome {
        NormalizedCoreOutcome::Notification(NormalizedNotification::Message { event, trace }) => {
            log::debug!(
                "[core_bridge] projected legacy message: sequence={}, severity={:?}, category={:?}",
                trace.sequence,
                event.severity,
                event.category
            );
            Some(event)
        }
        _ => None,
    }
}

pub(super) fn legacy_redraw_request_from_normalized(
    outcome: NormalizedCoreOutcome,
) -> Option<PendingRedrawRequest> {
    match outcome {
        NormalizedCoreOutcome::Structural(NormalizedStructuralOutcome::RedrawRequested {
            full,
            clear_before_draw,
            trace,
        }) => {
            log::debug!(
                "[core_bridge] projected legacy redraw request: sequence={}, full={}, clear_before_draw={}",
                trace.sequence,
                full,
                clear_before_draw
            );
            Some(PendingRedrawRequest {
                full,
                clear_before_draw,
            })
        }
        _ => None,
    }
}

pub(super) fn escape_path_for_file_command(target_path: &Path) -> String {
    let mut escaped = String::new();
    for ch in target_path.to_string_lossy().chars() {
        if matches!(ch, ' ' | '\\' | '|' | '"' | '%' | '#' | '<' | '>') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

pub(super) fn completion_replacement_end(
    range: &CompletionRange,
    replacement_text: &str,
) -> (usize, usize) {
    let mut lines = replacement_text.split('\n');
    let first = lines.next().unwrap_or("");
    let mut row = range.start.line;
    let mut col = range.start.character + first.len();
    for line in lines {
        row += 1;
        col = line.len();
    }
    (row, col)
}
