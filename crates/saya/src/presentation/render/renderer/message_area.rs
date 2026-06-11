//! メッセージ欄・ページャ行・プロンプト行のテキスト整形と折り返し。

use super::*;

pub(super) fn message_area_row_count(model: &WorkspaceScreenModel, width: u16) -> u16 {
    let Some(lines) = message_area_lines(model, width) else {
        return 0;
    };
    let desired = u16::try_from(lines.len()).unwrap_or(u16::MAX);
    desired.min(model.message_area_height.max(1))
}

pub(super) fn message_area_text(model: &WorkspaceScreenModel, width: u16) -> Option<Text<'static>> {
    let lines = message_area_lines(model, width)?;
    let height = usize::from(message_area_row_count(model, width));
    if height == 0 {
        return None;
    }
    let max_offset = lines.len().saturating_sub(height);
    let offset = usize::from(model.message_scroll_offset).min(max_offset);
    let visible_lines = lines
        .into_iter()
        .skip(offset)
        .take(height)
        .map(Line::from)
        .collect::<Vec<_>>();
    log::debug!(
        "[tui_renderer] message area text resolved: total_lines={}, height={}, scroll_offset={}, max_offset={}",
        visible_lines.len().saturating_add(offset),
        height,
        offset,
        max_offset
    );
    Some(Text::from(visible_lines))
}

pub(super) fn message_area_lines(model: &WorkspaceScreenModel, width: u16) -> Option<Vec<String>> {
    let message = model
        .visible_message_text()
        .map(str::trim)
        .unwrap_or_default();
    let bell = model.bell.map(|bell| format!("[bell x{}]", bell.count));
    let mut lines = message
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .flat_map(|line| wrap_message_line_to_width(line, width))
        .collect::<Vec<_>>();

    if let Some(bell_marker) = bell {
        if let Some(last) = lines.last_mut() {
            last.push(' ');
            last.push_str(&bell_marker);
        } else {
            lines.push(bell_marker);
        }
    }

    if lines.is_empty() { None } else { Some(lines) }
}

pub(super) fn wrap_message_line_to_width(line: &str, width: u16) -> Vec<String> {
    let max_width = usize::from(width.max(1));
    if display_width(line) <= max_width {
        return vec![line.to_string()];
    }

    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;
    let mut wrap_count = 0usize;
    for ch in line.chars() {
        let ch_width = ch.width().unwrap_or(0);
        if current_width > 0 && current_width.saturating_add(ch_width) > max_width {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
            wrap_count = wrap_count.saturating_add(1);
        }
        current.push(ch);
        current_width = current_width.saturating_add(ch_width);
    }
    if !current.is_empty() {
        lines.push(current);
    }

    log::debug!(
        "[tui_renderer] wrapped message area line: original_width={}, max_width={}, wrapped_lines={}, wrap_count={}",
        display_width(line),
        max_width,
        lines.len(),
        wrap_count
    );
    lines
}

pub(super) fn message_area_style(
    model: &WorkspaceScreenModel,
    theme: &ResolvedTheme,
    text_mode: RenderTextMode,
) -> Style {
    if text_mode == RenderTextMode::Plain {
        return Style::default();
    }
    match model.visible_message_source() {
        Some(crate::core::notification_prompt::MessageLineSource::SystemWarning) => theme
            .ui_style(UiStyleKey::WarningMsg)
            .cloned()
            .map(|style| style_for_text(style, text_mode))
            .unwrap_or_else(|| Style::default().fg(Color::Yellow)),
        _ => ui_style(theme, UiStyleKey::Message, text_mode),
    }
}

pub(super) fn pager_row_text(model: &WorkspaceScreenModel) -> Option<String> {
    model
        .pager_prompt
        .map(|pager| format!("[pager: {:?}]", pager.kind))
}

pub(super) fn prompt_row_text(model: &WorkspaceScreenModel) -> Option<String> {
    model
        .prompt_line
        .as_ref()
        .map(|prompt| match prompt.status {
            crate::core::notification_prompt::InputPromptStatus::Active => {
                format!("{} {}", prompt.prompt, prompt.input)
                    .trim_end()
                    .to_string()
            }
            crate::core::notification_prompt::InputPromptStatus::AwaitingCore { disposition } => {
                format!("{} {} [{:?}]", prompt.prompt, prompt.input, disposition)
                    .trim_end()
                    .to_string()
            }
        })
}
