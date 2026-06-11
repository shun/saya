//! ペイン単位の描画（ステータスライン・バッファテキスト・診断トレース）。

use super::*;

pub(super) fn render_pane(
    f: &mut Frame<'_>,
    model: &ScreenModel,
    is_active: bool,
    rect: Rect,
    text_mode: RenderTextMode,
) {
    let body_height = rect.height.saturating_sub(1).max(1);
    let body_rect = Rect {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: body_height,
    };
    let status_rect = Rect {
        x: rect.x,
        y: rect.y.saturating_add(body_height),
        width: rect.width,
        height: 1,
    };
    trace_redraw_diagnostic(format_args!(
        "renderer applying pane Clear widget: window_id={}, active={}, rect=({}, {}, {}, {}), cursor=({},{}), search_overlays={}",
        model.window_id,
        is_active,
        rect.x,
        rect.y,
        rect.width,
        rect.height,
        model.cursor_row,
        model.cursor_col,
        model.search_overlays.len()
    ));
    f.render_widget(Clear, rect);
    let buffer_content = Paragraph::new(render_buffer_text(model, body_rect.width, text_mode))
        .block(Block::default());
    trace_renderer_line(model, body_rect.width);
    f.render_widget(buffer_content, body_rect);

    let status_style = status_style(is_active, text_mode, &model.resolved_theme);
    let status_bar = Paragraph::new(render_status_line(model)).style(status_style);
    f.render_widget(status_bar, status_rect);
}

pub(super) fn status_style(
    is_active: bool,
    text_mode: RenderTextMode,
    theme: &ResolvedTheme,
) -> Style {
    if text_mode == RenderTextMode::Plain {
        return Style::default();
    }
    let key = if is_active {
        UiStyleKey::StatusActive
    } else {
        UiStyleKey::StatusInactive
    };
    if let Some(style) = theme.ui_style(key).cloned() {
        return style_for_text(style, text_mode);
    }
    if is_active {
        Style::default().bg(Color::White).fg(Color::Black)
    } else {
        Style::default().bg(Color::DarkGray).fg(Color::White)
    }
}

pub(super) fn ui_style(theme: &ResolvedTheme, key: UiStyleKey, text_mode: RenderTextMode) -> Style {
    if text_mode == RenderTextMode::Plain {
        return Style::default();
    }
    theme
        .ui_style(key)
        .cloned()
        .map(|style| style_for_text(style, text_mode))
        .unwrap_or_default()
}

pub(super) fn render_status_line(model: &ScreenModel) -> String {
    model.status_line.clone()
}

#[cfg(test)]
pub(super) fn render_message_line(model: &ScreenModel) -> &str {
    let message = model.message_line.as_deref().unwrap_or("");
    if message.trim().is_empty() {
        ""
    } else {
        message
    }
}

#[cfg(test)]
pub(super) fn draw_editor_frame<B: Backend>(
    terminal: &mut Terminal<B>,
    model: &ScreenModel,
    force_full_clear: bool,
) -> Result<(), B::Error> {
    draw_workspace_frame(
        terminal,
        &WorkspaceScreenModel {
            panes: vec![model.clone()],
            floats: vec![],
            active_window_id: model.window_id,
            message_line: model.message_line.as_deref().map_or_else(
                || {
                    crate::core::notification_prompt::resolve_workspace_message_line(Vec::<
                        crate::core::notification_prompt::MessageLineCandidate,
                    >::new(
                    ))
                },
                |message| {
                    crate::core::notification_prompt::resolve_workspace_message_line(vec![
                        crate::core::notification_prompt::MessageLineCandidate::legacy(
                            crate::core::notification_prompt::MessageLineSource::TransientInfo,
                            message,
                        ),
                    ])
                },
            ),
            message_area_height: 5,
            message_scroll_offset: 0,
            prompt_line: None,
            pager_prompt: None,
            suppressed_prompt_hints: vec![],
            bell: None,
            command_line: model.command_cursor_col.map(|cursor_col| CommandLineModel {
                text: model.message_line.clone().unwrap_or_default(),
                cursor_col,
            }),
        },
        force_full_clear,
        RenderTextMode::StyledTrueColor,
    )
}

pub(super) fn trace_redraw_diagnostic(args: std::fmt::Arguments<'_>) {
    crate::presentation::render::redraw_trace::trace_redraw_diagnostic(args);
}

pub(super) fn render_buffer_text(
    model: &ScreenModel,
    width: u16,
    text_mode: RenderTextMode,
) -> Text<'static> {
    let lines = model
        .lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let projected_line = projected_display_line(model, index, line);
            let display_line = projected_line.as_deref().unwrap_or(line);
            render_line(model, index, display_line, width, text_mode)
        })
        .collect::<Vec<_>>();
    Text::from(lines)
}

pub(super) fn projected_display_line(
    model: &ScreenModel,
    index: usize,
    raw_line: &str,
) -> Option<String> {
    let projection = model.line_projections.get(index)?;
    let gutter_prefix =
        slice_line_by_display_columns(raw_line, 0, usize::from(projection.line_start_col));
    let display_line = format!("{gutter_prefix}{}", projection.display_text);
    log::debug!(
        "[tui_renderer] using projected markdown line: window_id={}, row={}, raw={:?}, display={:?}, line_start_col={}",
        model.window_id,
        index,
        projection.raw_text,
        display_line,
        projection.line_start_col
    );
    Some(display_line)
}

pub(super) fn trace_renderer_line(model: &ScreenModel, width: u16) {
    if std::env::var_os("SAYA_TRACE_RENDER").is_none() {
        return;
    }

    let line = model.lines.get(6).map(String::as_str).unwrap_or("");
    log::debug!(
        "[saya-trace][renderer] body_width={} rel_row=7 line={line:?}",
        width
    );
}
