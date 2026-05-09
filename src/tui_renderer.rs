use crate::floating_window::{FloatingBorder, FloatingScreenModel};
#[cfg(test)]
use crate::screen_model::PaneRect;
use crate::screen_model::{
    CommandLineModel, ScreenCursorStyle, ScreenModel, ScreenSyntaxCategory, ScreenSyntaxModifier,
    ScreenTreeSitterSyntax, WorkspaceScreenModel,
};
use crate::terminal_lifecycle::TerminalBackend;
use crate::theme::{
    ResolvedTextStyle, ResolvedTheme, ResolvedThemeColor, SyntaxSemanticStyleKey, UiStyleKey,
};
use crossterm::{cursor, event, execute, queue, style, terminal};
use ratatui::Terminal;
use ratatui::prelude::*;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Clear, Paragraph};
use std::io::{self, Stdout, Write};
use unicode_width::UnicodeWidthChar;

pub struct CrosstermBackendImpl;

impl TerminalBackend for CrosstermBackendImpl {
    fn enable_raw_mode(&mut self) -> io::Result<()> {
        terminal::enable_raw_mode()
    }

    fn enter_alternate_screen(&mut self) -> io::Result<()> {
        execute!(io::stdout(), terminal::EnterAlternateScreen)
    }

    fn enable_mouse_capture(&mut self) -> io::Result<()> {
        execute!(io::stdout(), event::EnableMouseCapture)
    }

    fn enable_bracketed_paste(&mut self) -> io::Result<()> {
        execute!(io::stdout(), event::EnableBracketedPaste)
    }

    fn set_cursor_style(&mut self, style: ScreenCursorStyle) -> io::Result<()> {
        let crossterm_style = match style {
            ScreenCursorStyle::Block => cursor::SetCursorStyle::SteadyBlock,
            ScreenCursorStyle::SteadyBar => cursor::SetCursorStyle::SteadyBar,
            ScreenCursorStyle::UnderScore => cursor::SetCursorStyle::SteadyUnderScore,
        };
        execute!(io::stdout(), crossterm_style)
    }

    fn reset_cursor_style(&mut self) -> io::Result<()> {
        execute!(io::stdout(), cursor::SetCursorStyle::DefaultUserShape)
    }

    fn disable_bracketed_paste(&mut self) -> io::Result<()> {
        execute!(io::stdout(), event::DisableBracketedPaste)
    }

    fn disable_mouse_capture(&mut self) -> io::Result<()> {
        execute!(io::stdout(), event::DisableMouseCapture)
    }

    fn leave_alternate_screen(&mut self) -> io::Result<()> {
        execute!(io::stdout(), terminal::LeaveAlternateScreen)
    }

    fn disable_raw_mode(&mut self) -> io::Result<()> {
        terminal::disable_raw_mode()
    }
}

pub struct TuiRenderer {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    needs_full_clear: bool,
    last_command_line_overlay: Option<CommandLineModel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RenderFrameOptions {
    pub full_redraw: bool,
    pub clear_before_draw: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderTextMode {
    Plain,
    StyledMonochrome,
    StyledAnsi,
    StyledTrueColor,
}

impl TuiRenderer {
    pub fn new() -> io::Result<Self> {
        let backend = CrosstermBackend::new(io::stdout());
        let terminal = Terminal::new(backend)?;
        Ok(Self {
            terminal,
            needs_full_clear: true,
            last_command_line_overlay: None,
        })
    }

    pub fn draw(&mut self, model: &WorkspaceScreenModel) -> io::Result<()> {
        self.draw_with_mode(model, RenderTextMode::StyledTrueColor)
    }

    pub fn draw_with_mode(
        &mut self,
        model: &WorkspaceScreenModel,
        text_mode: RenderTextMode,
    ) -> io::Result<()> {
        self.draw_with_mode_and_options(model, text_mode, RenderFrameOptions::default())
    }

    pub fn draw_with_mode_and_options(
        &mut self,
        model: &WorkspaceScreenModel,
        text_mode: RenderTextMode,
        options: RenderFrameOptions,
    ) -> io::Result<()> {
        let force_full_clear =
            self.needs_full_clear || options.full_redraw || options.clear_before_draw;
        log::debug!(
            "[tui_renderer] drawing workspace frame: initial_clear={}, full_redraw={}, clear_before_draw={}, force_full_clear={}",
            self.needs_full_clear,
            options.full_redraw,
            options.clear_before_draw,
            force_full_clear
        );
        trace_redraw_diagnostic(format_args!(
            "renderer frame requested: panes={}, active_window_id={}, initial_clear={}, full_redraw={}, clear_before_draw={}, force_full_clear={}",
            model.panes.len(),
            model.active_window_id,
            self.needs_full_clear,
            options.full_redraw,
            options.clear_before_draw,
            force_full_clear
        ));
        draw_workspace_frame(&mut self.terminal, model, force_full_clear, text_mode)?;
        self.needs_full_clear = false;
        self.last_command_line_overlay = model.command_line.clone();
        Ok(())
    }

    pub fn draw_command_line_overlay(&mut self, command_line: &CommandLineModel) -> io::Result<()> {
        let area = self.terminal.size()?;
        let row = area.height.saturating_sub(1);
        let update =
            command_line_overlay_update(self.last_command_line_overlay.as_ref(), command_line);
        let start_col = update.start_col.min(area.width);
        let cursor_col = update.cursor_col.min(area.width);
        let backend = self.terminal.backend_mut();
        if update.clear_current_line_first {
            queue!(
                backend,
                cursor::MoveTo(0, row),
                terminal::Clear(terminal::ClearType::CurrentLine)
            )?;
        }
        queue!(
            backend,
            cursor::MoveTo(start_col, row),
            style::Print(update.text.as_str())
        )?;
        if update.clear_after_text {
            queue!(backend, terminal::Clear(terminal::ClearType::UntilNewLine))?;
        }
        queue!(backend, cursor::MoveTo(cursor_col, row))?;
        Write::flush(backend)?;
        self.last_command_line_overlay = Some(command_line.clone());
        self.needs_full_clear = true;
        log::debug!(
            "[tui_renderer] drew command-line-only overlay and marked next frame for full clear: row={}, start_col={}, cursor_col={}, text_len={}, clear_current_line_first={}, clear_after_text={}",
            row,
            start_col,
            cursor_col,
            update.text.len(),
            update.clear_current_line_first,
            update.clear_after_text
        );
        trace_redraw_diagnostic(format_args!(
            "renderer command-line-only overlay: row={}, start_col={}, cursor_col={}, text_len={}, clear_current_line_first={}, clear_after_text={}, next_full_clear=true",
            row,
            start_col,
            cursor_col,
            update.text.len(),
            update.clear_current_line_first,
            update.clear_after_text
        ));
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandLineOverlayUpdate {
    start_col: u16,
    cursor_col: u16,
    text: String,
    clear_current_line_first: bool,
    clear_after_text: bool,
}

fn command_line_overlay_update(
    previous: Option<&CommandLineModel>,
    next: &CommandLineModel,
) -> CommandLineOverlayUpdate {
    let Some(previous) = previous else {
        return CommandLineOverlayUpdate {
            start_col: 0,
            cursor_col: next.cursor_col,
            text: next.text.clone(),
            clear_current_line_first: true,
            clear_after_text: false,
        };
    };

    let prefix_end = common_prefix_byte_len(&previous.text, &next.text);
    let prefix = &next.text[..prefix_end];
    let suffix = &next.text[prefix_end..];
    let previous_suffix = &previous.text[prefix_end..];
    CommandLineOverlayUpdate {
        start_col: u16::try_from(display_width(prefix)).unwrap_or(u16::MAX),
        cursor_col: next.cursor_col,
        text: suffix.to_string(),
        clear_current_line_first: false,
        clear_after_text: !previous_suffix.is_empty(),
    }
}

fn common_prefix_byte_len(left: &str, right: &str) -> usize {
    let mut prefix = 0usize;
    for ((left_index, left_ch), (right_index, right_ch)) in
        left.char_indices().zip(right.char_indices())
    {
        if left_ch != right_ch {
            return left_index.min(right_index);
        }
        prefix = left_index + left_ch.len_utf8();
    }
    prefix.min(left.len()).min(right.len())
}

fn draw_workspace_frame<B: Backend>(
    terminal: &mut Terminal<B>,
    model: &WorkspaceScreenModel,
    force_full_clear: bool,
    text_mode: RenderTextMode,
) -> io::Result<()> {
    let _color_output_guard = CrosstermColorOutputGuard::for_text_mode(text_mode);
    if force_full_clear {
        trace_redraw_diagnostic(format_args!(
            "renderer issuing terminal.clear before draw: force_full_clear=true"
        ));
        terminal.clear()?;
    } else {
        trace_redraw_diagnostic(format_args!(
            "renderer skipping terminal.clear before draw: force_full_clear=false"
        ));
    }
    terminal.draw(|f| render_workspace(f, model, text_mode))?;
    Ok(())
}

struct CrosstermColorOutputGuard {
    restore_no_color: bool,
}

impl CrosstermColorOutputGuard {
    fn for_text_mode(text_mode: RenderTextMode) -> Self {
        let restore_no_color = colors_enabled(text_mode)
            && std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
        if restore_no_color {
            log::debug!(
                "[tui_renderer] forcing crossterm color output for highlighted frame: text_mode={text_mode:?}, no_color_present=true"
            );
            log::debug!(
                "[saya-trace][renderer][color] force_color_output=true text_mode={text_mode:?} no_color_present=true"
            );
            style::force_color_output(true);
        } else {
            log::debug!(
                "[tui_renderer] using crossterm color output default for frame: text_mode={text_mode:?}, no_color_present={}",
                std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty())
            );
        }
        Self { restore_no_color }
    }
}

impl Drop for CrosstermColorOutputGuard {
    fn drop(&mut self) {
        if self.restore_no_color {
            log::debug!(
                "[tui_renderer] restoring crossterm NO_COLOR color suppression after frame"
            );
            log::debug!(
                "[saya-trace][renderer][color] force_color_output=false restore_no_color=true"
            );
            style::force_color_output(false);
        }
    }
}

fn render_workspace(f: &mut Frame<'_>, model: &WorkspaceScreenModel, text_mode: RenderTextMode) {
    let size = f.area();
    trace_redraw_diagnostic(format_args!(
        "renderer applying workspace Clear widget: area=({}, {}, {}, {}), panes={}, active_window_id={}",
        size.x,
        size.y,
        size.width,
        size.height,
        model.panes.len(),
        model.active_window_id
    ));
    f.render_widget(Clear, size);
    let layout = compute_workspace_layout(size, model);

    for pane in &layout.panes {
        render_pane(f, pane.model, pane.is_active, pane.rect, text_mode);
    }
    let theme = active_workspace_theme(model);

    if let Some((message_area, message_rect)) = message_area_text(model).zip(layout.message_rect) {
        f.render_widget(
            Paragraph::new(message_area).style(ui_style(theme, UiStyleKey::Message, text_mode)),
            message_rect,
        );
    }

    if let Some((pager_line, pager_rect)) = pager_row_text(model).zip(layout.pager_rect) {
        f.render_widget(
            Paragraph::new(pager_line).style(ui_style(theme, UiStyleKey::Message, text_mode)),
            pager_rect,
        );
    }

    if let Some((prompt_line, prompt_rect)) = prompt_row_text(model).zip(layout.prompt_rect) {
        f.render_widget(
            Paragraph::new(prompt_line).style(ui_style(theme, UiStyleKey::Prompt, text_mode)),
            prompt_rect,
        );
    }

    let command_cursor = if let Some((command_line, command_rect)) =
        model.command_line.as_ref().zip(layout.command_rect)
    {
        f.render_widget(
            Paragraph::new(command_line.text.as_str()).style(ui_style(
                theme,
                UiStyleKey::Prompt,
                text_mode,
            )),
            command_rect,
        );
        Some((command_line.cursor_col.min(size.width), command_rect.y))
    } else {
        None
    };

    render_floats(f, &model.floats, text_mode, theme);

    if let Some((cursor_x, cursor_y)) = command_cursor {
        f.set_cursor_position((cursor_x, cursor_y));
        return;
    }

    if let Some((cursor_x, cursor_y)) = layout.cursor {
        f.set_cursor_position((cursor_x, cursor_y));
    }
}

fn render_floats(
    f: &mut Frame<'_>,
    floats: &[FloatingScreenModel],
    text_mode: RenderTextMode,
    theme: &ResolvedTheme,
) {
    let mut sorted = floats.iter().collect::<Vec<_>>();
    sorted.sort_by_key(|float| (float.zindex, float.creation_order));

    for float in sorted {
        let rect = Rect {
            x: float.rect.x,
            y: float.rect.y,
            width: float.rect.width,
            height: float.rect.height,
        };
        log::debug!(
            "[tui_renderer] rendering float: id={}, rect=({},{},{},{}), lines={}, border={:?}, zindex={}, creation_order={}",
            float.id.0,
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            float.lines.len(),
            float.chrome.border,
            float.zindex,
            float.creation_order
        );
        f.render_widget(Clear, rect);
        let text = Text::from(
            float
                .lines
                .iter()
                .map(|line| Line::from(line.clone()))
                .collect::<Vec<_>>(),
        );
        let paragraph = match float.chrome.border {
            FloatingBorder::None => {
                Paragraph::new(text).style(ui_style(theme, UiStyleKey::Message, text_mode))
            }
            FloatingBorder::Single => Paragraph::new(text)
                .style(ui_style(theme, UiStyleKey::Message, text_mode))
                .block(Block::bordered()),
        };
        f.render_widget(paragraph, rect);
    }
}

fn active_workspace_theme(model: &WorkspaceScreenModel) -> &ResolvedTheme {
    model
        .panes
        .iter()
        .find(|pane| pane.window_id == model.active_window_id)
        .or_else(|| model.panes.first())
        .map(|pane| &pane.resolved_theme)
        .unwrap_or_else(|| {
            static DEFAULT_THEME: std::sync::OnceLock<ResolvedTheme> = std::sync::OnceLock::new();
            DEFAULT_THEME.get_or_init(ResolvedTheme::default)
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PaneLayout<'a> {
    model: &'a ScreenModel,
    rect: Rect,
    is_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkspaceLayout<'a> {
    panes: Vec<PaneLayout<'a>>,
    message_rect: Option<Rect>,
    pager_rect: Option<Rect>,
    prompt_rect: Option<Rect>,
    command_rect: Option<Rect>,
    cursor: Option<(u16, u16)>,
}

fn compute_workspace_layout<'a>(
    size: Rect,
    model: &'a WorkspaceScreenModel,
) -> WorkspaceLayout<'a> {
    let global_rows = workspace_global_rows(model);
    let workspace_height = size.height.saturating_sub(global_rows).max(1);

    let panes = model
        .panes
        .iter()
        .map(|pane| PaneLayout {
            model: pane,
            is_active: pane.window_id == model.active_window_id,
            rect: Rect {
                x: pane.rect.x.min(size.width),
                y: pane.rect.y.min(workspace_height),
                width: pane
                    .rect
                    .width
                    .min(size.width.saturating_sub(pane.rect.x))
                    .max(1),
                height: pane
                    .rect
                    .height
                    .min(workspace_height.saturating_sub(pane.rect.y))
                    .max(1),
            },
        })
        .collect::<Vec<_>>();

    let mut next_row = size.height;
    let command_rect = bottom_row_rect(
        size.width,
        &mut next_row,
        model.command_line.as_ref().map(|_| ()),
    );
    let prompt_rect = bottom_row_rect(size.width, &mut next_row, prompt_row_text(model));
    let pager_rect = bottom_row_rect(size.width, &mut next_row, pager_row_text(model));
    let message_rect = bottom_rect(
        size.width,
        &mut next_row,
        Some(message_area_row_count(model)).filter(|height| *height > 0),
    );

    let cursor = if command_rect.is_some() {
        None
    } else {
        panes
            .iter()
            .find(|pane| pane.is_active)
            .and_then(|active_pane| {
                let body_height = active_pane.rect.height.saturating_sub(1);
                (active_pane.model.cursor_row < body_height).then_some((
                    active_pane
                        .rect
                        .x
                        .saturating_add(active_pane.model.cursor_col),
                    active_pane
                        .rect
                        .y
                        .saturating_add(active_pane.model.cursor_row),
                ))
            })
    };

    WorkspaceLayout {
        panes,
        message_rect,
        pager_rect,
        prompt_rect,
        command_rect,
        cursor,
    }
}

fn workspace_global_rows(model: &WorkspaceScreenModel) -> u16 {
    message_area_row_count(model)
        + u16::from(pager_row_text(model).is_some())
        + u16::from(prompt_row_text(model).is_some())
        + u16::from(model.command_line.is_some())
}

fn bottom_row_rect<T>(width: u16, next_row: &mut u16, row: Option<T>) -> Option<Rect> {
    bottom_rect(width, next_row, row.map(|_| 1))
}

fn bottom_rect(width: u16, next_row: &mut u16, height: Option<u16>) -> Option<Rect> {
    let height = height?.max(1);
    if *next_row <= 1 {
        return None;
    }
    let height = height.min(next_row.saturating_sub(1));
    *next_row = next_row.saturating_sub(height);
    Some(Rect {
        x: 0,
        y: *next_row,
        width,
        height,
    })
}

fn message_area_row_count(model: &WorkspaceScreenModel) -> u16 {
    let Some(lines) = message_area_lines(model) else {
        return 0;
    };
    let desired = u16::try_from(lines.len()).unwrap_or(u16::MAX);
    desired.min(model.message_area_height.max(1))
}

fn message_area_text(model: &WorkspaceScreenModel) -> Option<Text<'static>> {
    let lines = message_area_lines(model)?;
    let height = usize::from(message_area_row_count(model));
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

fn message_area_lines(model: &WorkspaceScreenModel) -> Option<Vec<String>> {
    let message = model
        .visible_message_text()
        .map(str::trim)
        .unwrap_or_default();
    let bell = model.bell.map(|bell| format!("[bell x{}]", bell.count));
    let mut lines = message
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .map(ToString::to_string)
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

fn pager_row_text(model: &WorkspaceScreenModel) -> Option<String> {
    model
        .pager_prompt
        .map(|pager| format!("[pager: {:?}]", pager.kind))
}

fn prompt_row_text(model: &WorkspaceScreenModel) -> Option<String> {
    model
        .prompt_line
        .as_ref()
        .map(|prompt| match prompt.status {
            crate::core_notification_prompt::InputPromptStatus::Active => {
                format!("{} {}", prompt.prompt, prompt.input)
                    .trim_end()
                    .to_string()
            }
            crate::core_notification_prompt::InputPromptStatus::AwaitingCore { disposition } => {
                format!("{} {} [{:?}]", prompt.prompt, prompt.input, disposition)
                    .trim_end()
                    .to_string()
            }
        })
}

fn render_pane(
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

fn status_style(is_active: bool, text_mode: RenderTextMode, theme: &ResolvedTheme) -> Style {
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

fn ui_style(theme: &ResolvedTheme, key: UiStyleKey, text_mode: RenderTextMode) -> Style {
    if text_mode == RenderTextMode::Plain {
        return Style::default();
    }
    theme
        .ui_style(key)
        .cloned()
        .map(|style| style_for_text(style, text_mode))
        .unwrap_or_default()
}

fn render_status_line(model: &ScreenModel) -> String {
    let status = format!("{} | {}", model.file_name, model.mode_label);
    if model.dirty {
        format!("{status} [+]!")
    } else {
        status
    }
}

#[cfg(test)]
fn render_message_line(model: &ScreenModel) -> &str {
    let message = model.message_line.as_deref().unwrap_or("");
    if message.trim().is_empty() {
        ""
    } else {
        message
    }
}

#[cfg(test)]
fn draw_editor_frame<B: Backend>(
    terminal: &mut Terminal<B>,
    model: &ScreenModel,
    force_full_clear: bool,
) -> io::Result<()> {
    draw_workspace_frame(
        terminal,
        &WorkspaceScreenModel {
            panes: vec![model.clone()],
            floats: vec![],
            active_window_id: model.window_id,
            message_line: model.message_line.as_deref().map_or_else(
                || {
                    crate::core_notification_prompt::resolve_workspace_message_line(Vec::<
                        crate::core_notification_prompt::MessageLineCandidate,
                    >::new(
                    ))
                },
                |message| {
                    crate::core_notification_prompt::resolve_workspace_message_line(vec![
                        crate::core_notification_prompt::MessageLineCandidate::legacy(
                            crate::core_notification_prompt::MessageLineSource::TransientInfo,
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

fn trace_redraw_diagnostic(args: std::fmt::Arguments<'_>) {
    let message = args.to_string();
    log::debug!("[redraw_diagnostic] {message}");
}

fn render_buffer_text(model: &ScreenModel, width: u16, text_mode: RenderTextMode) -> Text<'static> {
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

fn projected_display_line(model: &ScreenModel, index: usize, raw_line: &str) -> Option<String> {
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

fn trace_renderer_line(model: &ScreenModel, width: u16) {
    if std::env::var_os("SAYA_TRACE_RENDER").is_none() {
        return;
    }

    let line = model.lines.get(6).map(String::as_str).unwrap_or("");
    log::debug!(
        "[saya-trace][renderer] body_width={} rel_row=7 line={line:?}",
        width
    );
}

fn render_line(
    model: &ScreenModel,
    index: usize,
    line: &str,
    width: u16,
    text_mode: RenderTextMode,
) -> Line<'static> {
    let row = u16::try_from(index).unwrap_or(u16::MAX);
    let overlays = collect_render_overlays(model, row, line);
    let base_style = model
        .resolved_theme
        .ui_style(UiStyleKey::Text)
        .cloned()
        .unwrap_or_default();
    if overlays.is_empty() {
        let style = style_for_text(base_style, text_mode);
        let line = if style == Style::default() {
            Line::from(line.to_string())
        } else {
            Line::from(Span::styled(line.to_string(), style))
        };
        return pad_line_to_width(line, width);
    }

    render_layered_line(
        line,
        &overlays,
        width,
        text_mode,
        &model.resolved_theme,
        base_style,
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RenderOverlayKind {
    Ui(UiStyleKey),
    Markdown(ResolvedTextStyle),
    Syntax(RenderSyntaxStyle),
    VisualSelection,
    Search(crate::search_query::SearchMatchKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RenderSyntaxStyle {
    vim_family: Option<&'static str>,
    tree_sitter: Option<RenderTreeSitterSyntaxStyle>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RenderTreeSitterSyntaxStyle {
    category: ScreenSyntaxCategory,
    definition: bool,
    documentation: bool,
    deprecated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderOverlayRange {
    start_col: usize,
    end_col_exclusive: usize,
    kind: RenderOverlayKind,
}

fn collect_render_overlays(model: &ScreenModel, row: u16, line: &str) -> Vec<RenderOverlayRange> {
    let mut overlays = Vec::new();

    if let Some(end_col_exclusive) = line_number_gutter_end_col(line) {
        overlays.push(RenderOverlayRange {
            start_col: 0,
            end_col_exclusive,
            kind: RenderOverlayKind::Ui(UiStyleKey::Gutter),
        });
    }

    overlays.extend(
        model
            .markdown_style_ranges
            .iter()
            .filter(|range| range.row == row)
            .filter(|range| range.end_col_exclusive > range.start_col)
            .map(|range| RenderOverlayRange {
                start_col: usize::from(range.start_col),
                end_col_exclusive: usize::from(range.end_col_exclusive),
                kind: RenderOverlayKind::Markdown(range.style.clone()),
            }),
    );

    overlays.extend(
        model
            .syntax_chunks
            .iter()
            .filter(|chunk| chunk.row == row)
            .filter(|chunk| chunk.end_col_exclusive > chunk.start_col)
            .map(|chunk| RenderOverlayRange {
                start_col: usize::from(chunk.start_col),
                end_col_exclusive: usize::from(chunk.end_col_exclusive),
                kind: RenderOverlayKind::Syntax(syntax_style(chunk)),
            }),
    );

    if let Some(selection) = model.visual_selection {
        if row >= selection.start_row && row <= selection.end_row {
            let start_col = if row == selection.start_row {
                usize::from(selection.start_col)
            } else {
                usize::from(selection.line_start_col)
            };
            let end_col_exclusive = if row == selection.end_row {
                usize::from(selection.end_col_exclusive)
            } else {
                display_width(line)
            };
            if end_col_exclusive > start_col {
                overlays.push(RenderOverlayRange {
                    start_col,
                    end_col_exclusive,
                    kind: RenderOverlayKind::VisualSelection,
                });
            }
        }
    }

    overlays.extend(
        model
            .search_overlays
            .iter()
            .filter(|overlay| overlay.row == row)
            .filter(|overlay| overlay.end_col_exclusive > overlay.start_col)
            .map(|overlay| RenderOverlayRange {
                start_col: usize::from(overlay.start_col),
                end_col_exclusive: usize::from(overlay.end_col_exclusive),
                kind: RenderOverlayKind::Search(overlay.kind),
            }),
    );

    overlays.sort_by_key(|overlay| {
        (
            overlay.start_col,
            overlay.end_col_exclusive,
            overlay_kind_rank(&overlay.kind),
        )
    });
    overlays
}

fn render_layered_line(
    line: &str,
    overlays: &[RenderOverlayRange],
    width: u16,
    text_mode: RenderTextMode,
    theme: &ResolvedTheme,
    base_style: ResolvedTextStyle,
) -> Line<'static> {
    let line_width = display_width(line);
    let mut boundaries = vec![0usize, line_width];
    for overlay in overlays {
        boundaries.push(overlay.start_col.min(line_width));
        boundaries.push(overlay.end_col_exclusive.min(line_width));
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    let mut spans = Vec::new();
    for window in boundaries.windows(2) {
        let start_col = window[0];
        let end_col_exclusive = window[1];
        if end_col_exclusive <= start_col {
            continue;
        }
        let text = slice_line_by_display_columns(line, start_col, end_col_exclusive);
        let style = overlays
            .iter()
            .filter(|overlay| {
                overlay.start_col < end_col_exclusive && overlay.end_col_exclusive > start_col
            })
            .max_by_key(|overlay| overlay_kind_rank(&overlay.kind))
            .map(|overlay| style_for_overlay_kind(overlay.kind.clone(), text_mode, theme))
            .unwrap_or_else(|| style_for_text(base_style.clone(), text_mode));
        if style == Style::default() {
            spans.push(Span::raw(text));
        } else {
            spans.push(Span::styled(text, style));
        }
    }

    pad_line_to_width(Line::from(spans), width)
}

fn overlay_kind_rank(kind: &RenderOverlayKind) -> usize {
    match kind {
        RenderOverlayKind::Ui(_) => 0,
        RenderOverlayKind::Syntax(_) => 1,
        RenderOverlayKind::Markdown(_) => 2,
        RenderOverlayKind::Search(crate::search_query::SearchMatchKind::Regular) => 3,
        RenderOverlayKind::Search(crate::search_query::SearchMatchKind::Incremental) => 4,
        RenderOverlayKind::Search(crate::search_query::SearchMatchKind::Current) => 5,
        RenderOverlayKind::VisualSelection => 6,
    }
}

fn style_for_overlay_kind(
    kind: RenderOverlayKind,
    text_mode: RenderTextMode,
    theme: &ResolvedTheme,
) -> Style {
    if text_mode == RenderTextMode::Plain {
        return Style::default();
    }
    match kind {
        RenderOverlayKind::Ui(key) => theme
            .ui_style(key)
            .cloned()
            .map(|style| style_for_text(style, text_mode))
            .unwrap_or_default(),
        RenderOverlayKind::Markdown(style) => style_for_markdown(style, text_mode),
        RenderOverlayKind::Syntax(style) => style_for_syntax(style, text_mode, theme),
        RenderOverlayKind::VisualSelection => Style::default().add_modifier(Modifier::REVERSED),
        RenderOverlayKind::Search(crate::search_query::SearchMatchKind::Current) => {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        }
        RenderOverlayKind::Search(crate::search_query::SearchMatchKind::Incremental) => {
            Style::default().fg(Color::White).bg(Color::Blue)
        }
        RenderOverlayKind::Search(crate::search_query::SearchMatchKind::Regular) => {
            Style::default().fg(Color::Black).bg(Color::Yellow)
        }
    }
}

fn line_number_gutter_end_col(line: &str) -> Option<usize> {
    let mut saw_digit = false;
    let mut width = 0usize;
    for ch in line.chars() {
        if ch == ' ' && !saw_digit {
            width += 1;
            continue;
        }
        if ch.is_ascii_digit() {
            saw_digit = true;
            width += 1;
            continue;
        }
        if ch == ' ' && saw_digit {
            return Some(width + 1);
        }
        return None;
    }
    None
}

fn style_for_markdown(style: ResolvedTextStyle, text_mode: RenderTextMode) -> Style {
    style_for_text(style, text_mode)
}

fn style_for_text(style: ResolvedTextStyle, text_mode: RenderTextMode) -> Style {
    let mut rendered = Style::default();
    if colors_enabled(text_mode) {
        if let Some(fg) = style.fg {
            rendered = rendered.fg(color_for_resolved_theme_color(&fg));
        }
        if let Some(bg) = style.bg {
            rendered = rendered.bg(color_for_resolved_theme_color(&bg));
        }
    }
    if style.bold {
        rendered = rendered.add_modifier(Modifier::BOLD);
    }
    if style.italic {
        rendered = rendered.add_modifier(Modifier::ITALIC);
    }
    if style.underline {
        rendered = rendered.add_modifier(Modifier::UNDERLINED);
    }
    if style.strikethrough {
        rendered = rendered.add_modifier(Modifier::CROSSED_OUT);
    }
    rendered
}

fn colors_enabled(text_mode: RenderTextMode) -> bool {
    matches!(
        text_mode,
        RenderTextMode::StyledAnsi | RenderTextMode::StyledTrueColor
    )
}

fn color_for_resolved_theme_color(color: &ResolvedThemeColor) -> Color {
    let hex = color.0.trim_start_matches('#');
    if hex.len() == 6 {
        if let (Ok(red), Ok(green), Ok(blue)) = (
            u8::from_str_radix(&hex[0..2], 16),
            u8::from_str_radix(&hex[2..4], 16),
            u8::from_str_radix(&hex[4..6], 16),
        ) {
            return Color::Rgb(red, green, blue);
        }
    }
    log::debug!(
        "[tui_renderer] unresolved renderer color fallback used for theme color: {:?}",
        color
    );
    Color::Reset
}

fn syntax_style(chunk: &crate::screen_model::ScreenSyntaxChunk) -> RenderSyntaxStyle {
    RenderSyntaxStyle {
        vim_family: syntax_family(chunk.name.as_deref()),
        tree_sitter: chunk.tree_sitter.as_ref().map(tree_sitter_syntax_style),
    }
}

fn tree_sitter_syntax_style(syntax: &ScreenTreeSitterSyntax) -> RenderTreeSitterSyntaxStyle {
    RenderTreeSitterSyntaxStyle {
        category: syntax.category,
        definition: syntax.modifiers.contains(&ScreenSyntaxModifier::Definition),
        documentation: syntax
            .modifiers
            .contains(&ScreenSyntaxModifier::Documentation),
        deprecated: syntax.modifiers.contains(&ScreenSyntaxModifier::Deprecated),
    }
}

fn style_for_syntax(
    style: RenderSyntaxStyle,
    text_mode: RenderTextMode,
    theme: &ResolvedTheme,
) -> Style {
    if let Some(tree_sitter) = style.tree_sitter {
        return style_for_tree_sitter_syntax(tree_sitter, text_mode, theme);
    }
    style_for_syntax_family(style.vim_family, text_mode, theme)
}

fn style_for_tree_sitter_syntax(
    syntax: RenderTreeSitterSyntaxStyle,
    text_mode: RenderTextMode,
    theme: &ResolvedTheme,
) -> Style {
    if text_mode == RenderTextMode::Plain {
        return Style::default();
    }
    let key = syntax_key_for_tree_sitter(syntax.category);
    let mut style = theme
        .syntax_style(key)
        .cloned()
        .map(|style| style_for_text(style, text_mode))
        .unwrap_or_else(|| fallback_style_for_tree_sitter_syntax(syntax.category, text_mode));
    if syntax.definition || syntax.documentation {
        style = style.add_modifier(Modifier::BOLD);
    }
    if syntax.deprecated {
        style = style.add_modifier(Modifier::CROSSED_OUT);
    }
    style
}

fn syntax_key_for_tree_sitter(category: ScreenSyntaxCategory) -> SyntaxSemanticStyleKey {
    match category {
        ScreenSyntaxCategory::Comment => SyntaxSemanticStyleKey::Comment,
        ScreenSyntaxCategory::String => SyntaxSemanticStyleKey::String,
        ScreenSyntaxCategory::Constant | ScreenSyntaxCategory::Number => {
            SyntaxSemanticStyleKey::Constant
        }
        ScreenSyntaxCategory::Keyword | ScreenSyntaxCategory::Operator => {
            SyntaxSemanticStyleKey::Statement
        }
        ScreenSyntaxCategory::Function | ScreenSyntaxCategory::Constructor => {
            SyntaxSemanticStyleKey::Function
        }
        ScreenSyntaxCategory::Type => SyntaxSemanticStyleKey::Type,
        ScreenSyntaxCategory::Punctuation => SyntaxSemanticStyleKey::Punctuation,
        ScreenSyntaxCategory::Markup | ScreenSyntaxCategory::Tag | ScreenSyntaxCategory::Label => {
            SyntaxSemanticStyleKey::Markup
        }
        ScreenSyntaxCategory::Variable
        | ScreenSyntaxCategory::Property
        | ScreenSyntaxCategory::Attribute => SyntaxSemanticStyleKey::Identifier,
        ScreenSyntaxCategory::Module
        | ScreenSyntaxCategory::Text
        | ScreenSyntaxCategory::Unknown => SyntaxSemanticStyleKey::Default,
    }
}

fn fallback_style_for_tree_sitter_syntax(
    category: ScreenSyntaxCategory,
    text_mode: RenderTextMode,
) -> Style {
    if !colors_enabled(text_mode) {
        return Style::default();
    }
    match category {
        ScreenSyntaxCategory::Comment => Style::default().fg(Color::DarkGray),
        ScreenSyntaxCategory::String => Style::default().fg(Color::Green),
        ScreenSyntaxCategory::Constant | ScreenSyntaxCategory::Number => {
            Style::default().fg(Color::Magenta)
        }
        ScreenSyntaxCategory::Keyword | ScreenSyntaxCategory::Operator => {
            Style::default().fg(Color::Cyan)
        }
        ScreenSyntaxCategory::Function
        | ScreenSyntaxCategory::Constructor
        | ScreenSyntaxCategory::Type
        | ScreenSyntaxCategory::Variable
        | ScreenSyntaxCategory::Property
        | ScreenSyntaxCategory::Attribute => Style::default().fg(Color::Yellow),
        ScreenSyntaxCategory::Markup | ScreenSyntaxCategory::Tag | ScreenSyntaxCategory::Label => {
            Style::default().fg(Color::Blue)
        }
        ScreenSyntaxCategory::Module
        | ScreenSyntaxCategory::Punctuation
        | ScreenSyntaxCategory::Text
        | ScreenSyntaxCategory::Unknown => Style::default().fg(Color::White),
    }
}

fn syntax_family(name: Option<&str>) -> Option<&'static str> {
    let name = name?;
    let normalized = name.to_ascii_lowercase();
    if normalized.contains("comment") || normalized.contains("todo") {
        Some("comment")
    } else if normalized.contains("string") || normalized.contains("character") {
        Some("string")
    } else if normalized.contains("number")
        || normalized.contains("float")
        || normalized.contains("boolean")
        || normalized.contains("constant")
        || normalized.contains("char")
    {
        Some("constant")
    } else if normalized.contains("operator")
        || normalized.contains("punctuation")
        || normalized.contains("delimiter")
        || normalized.contains("separator")
        || normalized.contains("sigil")
        || normalized.contains("arrow")
        || normalized.contains("modpathsep")
    {
        Some("punctuation")
    } else if normalized.contains("statement")
        || normalized.contains("keyword")
        || normalized.contains("conditional")
        || normalized.contains("repeat")
        || normalized.contains("storage")
        || normalized.contains("storageclass")
        || normalized.contains("visibility")
        || normalized.contains("preproc")
        || normalized.contains("include")
        || normalized.contains("define")
        || normalized.contains("exception")
    {
        Some("statement")
    } else if normalized.contains("function")
        || normalized.contains("func")
        || normalized.contains("method")
        || normalized.contains("macro")
    {
        Some("function")
    } else if normalized.contains("type")
        || normalized.contains("struct")
        || normalized.contains("enum")
        || normalized.contains("trait")
        || normalized.contains("typedef")
        || normalized.contains("modpath")
    {
        Some("type")
    } else if normalized.contains("identifier") {
        Some("identifier")
    } else {
        Some("default")
    }
}

fn style_for_syntax_family(
    family: Option<&'static str>,
    text_mode: RenderTextMode,
    theme: &ResolvedTheme,
) -> Style {
    if text_mode == RenderTextMode::Plain || !colors_enabled(text_mode) {
        return Style::default();
    }
    let key = match family {
        Some("comment") => SyntaxSemanticStyleKey::Comment,
        Some("string") => SyntaxSemanticStyleKey::String,
        Some("constant") => SyntaxSemanticStyleKey::Constant,
        Some("statement") => SyntaxSemanticStyleKey::Statement,
        Some("identifier") => SyntaxSemanticStyleKey::Identifier,
        Some("type") => SyntaxSemanticStyleKey::Type,
        Some("function") => SyntaxSemanticStyleKey::Function,
        Some("punctuation") => SyntaxSemanticStyleKey::Punctuation,
        Some("default") | None => SyntaxSemanticStyleKey::Default,
        Some(_) => SyntaxSemanticStyleKey::Default,
    };
    if let Some(style) = theme.syntax_style(key).cloned() {
        return style_for_text(style, text_mode);
    }
    match family {
        Some("comment") => Style::default().fg(Color::DarkGray),
        Some("string") => Style::default().fg(Color::Green),
        Some("constant") => Style::default().fg(Color::Magenta),
        Some("statement") => Style::default().fg(Color::Cyan),
        Some("identifier") => Style::default().fg(Color::Yellow),
        Some("type") => Style::default().fg(Color::Blue),
        Some("function") => Style::default().fg(Color::Yellow),
        Some("punctuation") => Style::default().fg(Color::White),
        Some("default") | None => Style::default().fg(Color::White),
        Some(_) => Style::default().fg(Color::White),
    }
}

fn slice_line_by_display_columns(line: &str, start_col: usize, end_col_exclusive: usize) -> String {
    let mut result = String::new();
    let mut display_col = 0usize;

    for ch in line.chars() {
        let width = ch.width().unwrap_or(0);
        let next_col = display_col.saturating_add(width);
        if next_col <= start_col {
            display_col = next_col;
            continue;
        }
        if display_col >= end_col_exclusive {
            break;
        }
        result.push(ch);
        display_col = next_col;
    }

    result
}

fn pad_line_to_width(mut line: Line<'static>, width: u16) -> Line<'static> {
    let rendered_width = line.width();
    let target_width = usize::from(width);
    if rendered_width < target_width {
        line.spans
            .push(Span::raw(" ".repeat(target_width - rendered_width)));
    }
    line
}

fn display_width(text: &str) -> usize {
    text.chars().map(|ch| ch.width().unwrap_or(0)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::bootstrap::prepare_launch;
    use crate::cli::{ConfigSource, InputSource, LaunchRequest};
    use crate::config_runtime::{StartupRegistry, StartupRegistryEntry};
    use crate::core_notification_prompt::{
        BellIndication, InputPromptStatus, InputPromptView, MessageLineCandidate,
        MessageLineSource, PagerPromptView, PromptHintSuppressionReason, SuppressedPromptHint,
        resolve_workspace_message_line,
    };
    use crate::editor_session::EditorSessionState;
    use crate::floating_window::{
        FloatingBorder, FloatingChrome, FloatingContentRef, FloatingScreenModel, FloatingWindowId,
    };
    use crate::markdown_structure::MarkdownDocumentMap;
    use crate::screen_model::{
        ProjectionInput, ScreenLineProjection, ScreenSearchOverlay, project,
    };
    use crate::screen_model::{ScreenMarkdownStyleRange, ScreenSelection, ScreenSyntaxChunk};
    use crate::search_query::SearchMatchKind;
    use crate::session_guard::test_lock as session_test_lock;
    use crate::theme::{ThemeRegistry, ThemeTextStyleDeclaration};
    use ratatui::backend::{CrosstermBackend, TestBackend};
    use ratatui::layout::{Position, Rect};
    use ratatui::{TerminalOptions, Viewport};
    use vim_core_rs::{CoreInputRequestKind, CorePagerPromptKind};

    #[derive(Clone, Default)]
    struct CaptureWriter(Rc<RefCell<Vec<u8>>>);

    impl std::io::Write for CaptureWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.borrow_mut().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl CaptureWriter {
        fn bytes(&self) -> Vec<u8> {
            self.0.borrow().clone()
        }
    }

    fn unique_renderer_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        std::env::temp_dir().join(format!("saya-renderer-{name}-{nanos}"))
    }

    fn screen_model_with_message(message_line: Option<&str>) -> ScreenModel {
        ScreenModel {
            window_id: 1,
            buffer_id: 1,
            rect: PaneRect {
                x: 0,
                y: 0,
                width: 40,
                height: 3,
            },
            file_name: "test.txt".to_string(),
            mode_label: "NORMAL".to_string(),
            cursor_style: ScreenCursorStyle::Block,
            dirty: true,
            lines: vec!["hello".to_string()],
            line_projections: vec![],
            cursor_row: 0,
            cursor_col: 0,
            visual_selection: Some(ScreenSelection {
                start_row: 0,
                start_col: 0,
                line_start_col: 0,
                end_row: 0,
                end_col_exclusive: 1,
            }),
            search_overlays: vec![],
            syntax_chunks: vec![],
            markdown_style_ranges: vec![],
            resolved_theme: crate::theme::ResolvedTheme::default(),
            message_line: message_line.map(ToString::to_string),
            command_cursor_col: None,
            is_active: true,
        }
    }

    fn workspace_with_typed_message(message_line: Option<&str>) -> WorkspaceScreenModel {
        WorkspaceScreenModel {
            panes: vec![screen_model_with_message(None)],
            floats: vec![],
            active_window_id: 1,
            message_line: message_line.map_or_else(
                || resolve_workspace_message_line(Vec::<MessageLineCandidate>::new()),
                |message| {
                    resolve_workspace_message_line(vec![MessageLineCandidate::legacy(
                        MessageLineSource::CoreNotification,
                        message,
                    )])
                },
            ),
            prompt_line: None,
            pager_prompt: None,
            suppressed_prompt_hints: vec![],
            bell: None,
            command_line: None,
            message_area_height: 5,
            message_scroll_offset: 0,
        }
    }

    fn projection(raw_text: &str, display_text: &str, line_start_col: u16) -> ScreenLineProjection {
        ScreenLineProjection {
            absolute_row: 0,
            raw_text: raw_text.to_string(),
            display_text: display_text.to_string(),
            spans: vec![],
            cells: vec![],
            line_start_col,
        }
    }

    fn rendered_text_line(text: &Text<'_>, index: usize) -> String {
        text.lines[index]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    }

    fn theme_from_entries(entries: Vec<StartupRegistryEntry>) -> crate::theme::ResolvedTheme {
        ThemeRegistry::from_startup_registry(&StartupRegistry::from_entries(entries)).resolve()
    }

    fn color_env_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct NoColorGuard {
        previous: Option<std::ffi::OsString>,
    }

    impl NoColorGuard {
        fn set() -> Self {
            let previous = std::env::var_os("NO_COLOR");
            unsafe {
                std::env::set_var("NO_COLOR", "1");
            }
            style::force_color_output(false);
            Self { previous }
        }
    }

    impl Drop for NoColorGuard {
        fn drop(&mut self) {
            match self.previous.as_ref() {
                Some(value) => unsafe {
                    std::env::set_var("NO_COLOR", value);
                },
                None => unsafe {
                    std::env::remove_var("NO_COLOR");
                },
            }
            style::force_color_output(
                std::env::var_os("NO_COLOR").is_none_or(|value| value.is_empty()),
            );
        }
    }

    #[test]
    fn status_line_does_not_embed_message_line() {
        let model = screen_model_with_message(Some("保存しました"));

        assert_eq!(render_status_line(&model), "test.txt | NORMAL [+]!");
    }

    #[test]
    fn message_line_uses_transient_message_area() {
        let model = screen_model_with_message(Some("保存しました"));

        assert_eq!(render_message_line(&model), "保存しました");
    }

    #[test]
    fn message_line_is_empty_when_no_message_exists() {
        let model = screen_model_with_message(None);

        assert_eq!(render_message_line(&model), "");
    }

    #[test]
    fn message_line_with_whitespace_is_not_rendered() {
        let model = screen_model_with_message(Some("   "));

        assert_eq!(render_message_line(&model), "");
    }

    #[test]
    fn message_area_uses_default_five_rows_for_multiline_messages() {
        let model = workspace_with_typed_message(Some("one\ntwo\nthree\nfour\nfive\nsix"));

        let layout = compute_workspace_layout(
            Rect {
                x: 0,
                y: 0,
                width: 20,
                height: 8,
            },
            &model,
        );

        assert_eq!(
            layout.message_rect,
            Some(Rect {
                x: 0,
                y: 3,
                width: 20,
                height: 5,
            })
        );
        assert_eq!(layout.panes[0].rect.height, 3);
    }

    #[test]
    fn message_area_height_can_be_configured_for_workspace_layout() {
        let mut model = workspace_with_typed_message(Some("one\ntwo\nthree"));
        model.message_area_height = 2;
        model.panes[0].rect.height = 6;

        let layout = compute_workspace_layout(
            Rect {
                x: 0,
                y: 0,
                width: 20,
                height: 6,
            },
            &model,
        );

        assert_eq!(
            layout.message_rect,
            Some(Rect {
                x: 0,
                y: 4,
                width: 20,
                height: 2,
            })
        );
        assert_eq!(layout.panes[0].rect.height, 4);
    }

    #[test]
    fn message_area_scroll_offset_selects_visible_tail() {
        let mut model = workspace_with_typed_message(Some("one\ntwo\nthree\nfour"));
        model.message_area_height = 2;
        model.message_scroll_offset = 1;

        let message = message_area_text(&model).expect("message area should render");
        let rendered = message
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();

        assert_eq!(rendered, vec!["two", "three"]);
    }

    #[test]
    fn search_overlay_precedence_prefers_current_over_incremental_and_regular() {
        let model = ScreenModel {
            window_id: 1,
            buffer_id: 1,
            rect: PaneRect {
                x: 0,
                y: 0,
                width: 6,
                height: 3,
            },
            file_name: "test.txt".to_string(),
            mode_label: "NORMAL".to_string(),
            cursor_style: ScreenCursorStyle::Block,
            dirty: false,
            lines: vec!["abcdef".to_string()],
            line_projections: vec![],
            cursor_row: 0,
            cursor_col: 0,
            visual_selection: None,
            search_overlays: vec![
                ScreenSearchOverlay {
                    row: 0,
                    start_col: 0,
                    end_col_exclusive: 6,
                    kind: SearchMatchKind::Regular,
                },
                ScreenSearchOverlay {
                    row: 0,
                    start_col: 1,
                    end_col_exclusive: 5,
                    kind: SearchMatchKind::Incremental,
                },
                ScreenSearchOverlay {
                    row: 0,
                    start_col: 2,
                    end_col_exclusive: 4,
                    kind: SearchMatchKind::Current,
                },
            ],
            syntax_chunks: vec![],
            markdown_style_ranges: vec![],
            resolved_theme: crate::theme::ResolvedTheme::default(),
            message_line: None,
            command_cursor_col: None,
            is_active: true,
        };

        let text = render_buffer_text(&model, 6, RenderTextMode::StyledTrueColor);
        let line = &text.lines[0];

        assert_eq!(line.spans.len(), 5);
        assert_eq!(line.spans[0].content.as_ref(), "a");
        assert_eq!(line.spans[1].content.as_ref(), "b");
        assert_eq!(line.spans[2].content.as_ref(), "cd");
        assert_eq!(line.spans[3].content.as_ref(), "e");
        assert_eq!(line.spans[4].content.as_ref(), "f");
        assert_eq!(
            line.spans[0].style,
            Style::default().fg(Color::Black).bg(Color::Yellow)
        );
        assert_eq!(
            line.spans[1].style,
            Style::default().fg(Color::White).bg(Color::Blue)
        );
        assert_eq!(
            line.spans[2].style,
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        );
        assert_eq!(
            line.spans[3].style,
            Style::default().fg(Color::White).bg(Color::Blue)
        );
        assert_eq!(
            line.spans[4].style,
            Style::default().fg(Color::Black).bg(Color::Yellow)
        );
    }

    #[test]
    fn render_buffer_text_uses_line_projection_display_text_when_present() {
        let mut model = screen_model_with_message(None);
        model.lines = vec!["# Heading".to_string()];
        model.is_active = false;
        model.line_projections = vec![ScreenLineProjection {
            absolute_row: 0,
            raw_text: "# Heading".to_string(),
            display_text: "Heading".to_string(),
            spans: vec![],
            cells: vec![],
            line_start_col: 0,
        }];

        let text = render_buffer_text(&model, 10, RenderTextMode::Plain);
        let rendered = text.lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert_eq!(rendered, "Heading   ");
    }

    #[test]
    fn render_buffer_text_applies_overlays_to_projected_display_text() {
        let mut model = screen_model_with_message(None);
        model.lines = vec!["# Heading".to_string()];
        model.is_active = false;
        model.line_projections = vec![ScreenLineProjection {
            absolute_row: 0,
            raw_text: "# Heading".to_string(),
            display_text: "Heading".to_string(),
            spans: vec![],
            cells: vec![],
            line_start_col: 0,
        }];
        model.visual_selection = None;
        model.search_overlays = vec![ScreenSearchOverlay {
            row: 0,
            start_col: 0,
            end_col_exclusive: 7,
            kind: SearchMatchKind::Regular,
        }];

        let text = render_buffer_text(&model, 10, RenderTextMode::StyledTrueColor);
        let line = &text.lines[0];

        assert_eq!(line.spans[0].content.as_ref(), "Heading");
        assert_eq!(
            line.spans[0].style,
            Style::default().fg(Color::Black).bg(Color::Yellow)
        );
        assert_eq!(line.spans[1].content.as_ref(), "   ");
    }

    #[test]
    fn render_buffer_text_applies_resolved_markdown_style_ranges() {
        let mut model = screen_model_with_message(None);
        model.lines = vec!["## Heading".to_string()];
        model.is_active = false;
        model.visual_selection = None;
        model.line_projections = vec![ScreenLineProjection {
            absolute_row: 0,
            raw_text: "## Heading".to_string(),
            display_text: "Heading".to_string(),
            spans: vec![],
            cells: vec![],
            line_start_col: 0,
        }];
        model.markdown_style_ranges = vec![ScreenMarkdownStyleRange {
            row: 0,
            start_col: 0,
            end_col_exclusive: 7,
            style: ResolvedTextStyle {
                fg: Some(ResolvedThemeColor("#9ece6a".to_string())),
                underline: true,
                bold: true,
                ..ResolvedTextStyle::default()
            },
        }];

        let text = render_buffer_text(&model, 10, RenderTextMode::StyledTrueColor);
        let line = &text.lines[0];

        assert_eq!(line.spans[0].content.as_ref(), "Heading");
        assert_eq!(
            line.spans[0].style,
            Style::default()
                .fg(Color::Rgb(0x9e, 0xce, 0x6a))
                .add_modifier(Modifier::UNDERLINED)
                .add_modifier(Modifier::BOLD)
        );
    }

    #[test]
    fn render_buffer_text_applies_ui_text_style_without_markdown() {
        let mut model = screen_model_with_message(None);
        model.lines = vec!["let value = 1;".to_string()];
        model.visual_selection = None;
        model.resolved_theme = theme_from_entries(vec![
            StartupRegistryEntry::ThemePalette {
                name: "fg".to_string(),
                value: "#c0caf5".to_string(),
            },
            StartupRegistryEntry::ThemePalette {
                name: "bg".to_string(),
                value: "#24283b".to_string(),
            },
            StartupRegistryEntry::ThemeUiStyle {
                key: UiStyleKey::Text,
                style: ThemeTextStyleDeclaration {
                    fg: Some("fg".to_string()),
                    bg: Some("bg".to_string()),
                    ..ThemeTextStyleDeclaration::default()
                },
            },
        ]);

        let text = render_buffer_text(&model, 16, RenderTextMode::StyledTrueColor);
        let line = &text.lines[0];

        assert_eq!(line.spans[0].content.as_ref(), "let value = 1;");
        assert_eq!(
            line.spans[0].style,
            Style::default()
                .fg(Color::Rgb(0xc0, 0xca, 0xf5))
                .bg(Color::Rgb(0x24, 0x28, 0x3b))
        );
    }

    #[test]
    fn render_buffer_text_applies_ui_gutter_style_to_line_number_prefix() {
        let mut model = screen_model_with_message(None);
        model.lines = vec!["   1 let value = 1;".to_string()];
        model.visual_selection = None;
        model.resolved_theme = theme_from_entries(vec![
            StartupRegistryEntry::ThemePalette {
                name: "gutter".to_string(),
                value: "#3b4261".to_string(),
            },
            StartupRegistryEntry::ThemePalette {
                name: "fg".to_string(),
                value: "#c0caf5".to_string(),
            },
            StartupRegistryEntry::ThemeUiStyle {
                key: UiStyleKey::Gutter,
                style: ThemeTextStyleDeclaration {
                    fg: Some("gutter".to_string()),
                    ..ThemeTextStyleDeclaration::default()
                },
            },
            StartupRegistryEntry::ThemeUiStyle {
                key: UiStyleKey::Text,
                style: ThemeTextStyleDeclaration {
                    fg: Some("fg".to_string()),
                    ..ThemeTextStyleDeclaration::default()
                },
            },
        ]);

        let text = render_buffer_text(&model, 20, RenderTextMode::StyledTrueColor);
        let line = &text.lines[0];

        assert_eq!(line.spans[0].content.as_ref(), "   1 ");
        assert_eq!(
            line.spans[0].style,
            Style::default().fg(Color::Rgb(0x3b, 0x42, 0x61))
        );
        assert_eq!(line.spans[1].content.as_ref(), "let value = 1;");
        assert_eq!(
            line.spans[1].style,
            Style::default().fg(Color::Rgb(0xc0, 0xca, 0xf5))
        );
    }

    #[test]
    fn render_buffer_text_keeps_markdown_projection_with_line_numbers_and_ui_theme() {
        let mut model = screen_model_with_message(None);
        model.lines = vec!["   1 ## Heading".to_string()];
        model.visual_selection = None;
        model.line_projections = vec![ScreenLineProjection {
            absolute_row: 0,
            raw_text: "## Heading".to_string(),
            display_text: "Heading".to_string(),
            spans: vec![],
            cells: vec![],
            line_start_col: 5,
        }];
        model.markdown_style_ranges = vec![ScreenMarkdownStyleRange {
            row: 0,
            start_col: 5,
            end_col_exclusive: 12,
            style: ResolvedTextStyle {
                fg: Some(ResolvedThemeColor("#9ece6a".to_string())),
                bold: true,
                ..ResolvedTextStyle::default()
            },
        }];
        model.resolved_theme = theme_from_entries(vec![
            StartupRegistryEntry::ThemePalette {
                name: "fg".to_string(),
                value: "#c0caf5".to_string(),
            },
            StartupRegistryEntry::ThemePalette {
                name: "bg".to_string(),
                value: "#24283b".to_string(),
            },
            StartupRegistryEntry::ThemePalette {
                name: "gutter".to_string(),
                value: "#3b4261".to_string(),
            },
            StartupRegistryEntry::ThemeUiStyle {
                key: UiStyleKey::Text,
                style: ThemeTextStyleDeclaration {
                    fg: Some("fg".to_string()),
                    bg: Some("bg".to_string()),
                    ..ThemeTextStyleDeclaration::default()
                },
            },
            StartupRegistryEntry::ThemeUiStyle {
                key: UiStyleKey::Gutter,
                style: ThemeTextStyleDeclaration {
                    fg: Some("gutter".to_string()),
                    bg: Some("bg".to_string()),
                    ..ThemeTextStyleDeclaration::default()
                },
            },
        ]);

        let text = render_buffer_text(&model, 16, RenderTextMode::StyledTrueColor);
        let line = &text.lines[0];

        assert_eq!(rendered_text_line(&text, 0), "   1 Heading    ");
        assert_eq!(line.spans[0].content.as_ref(), "   1 ");
        assert_eq!(line.spans[1].content.as_ref(), "Heading");
        assert_eq!(
            line.spans[1].style,
            Style::default()
                .fg(Color::Rgb(0x9e, 0xce, 0x6a))
                .add_modifier(Modifier::BOLD)
        );
    }

    #[test]
    fn render_buffer_text_applies_theme_syntax_style() {
        let mut model = screen_model_with_message(None);
        model.lines = vec!["// comment".to_string()];
        model.visual_selection = None;
        model.syntax_chunks = vec![ScreenSyntaxChunk {
            row: 0,
            start_col: 0,
            end_col_exclusive: 10,
            syn_id: 1,
            name: Some("Comment".to_string()),
            tree_sitter: None,
        }];
        model.resolved_theme = theme_from_entries(vec![
            StartupRegistryEntry::ThemePalette {
                name: "comment".to_string(),
                value: "#565f89".to_string(),
            },
            StartupRegistryEntry::ThemeSyntaxStyle {
                key: SyntaxSemanticStyleKey::Comment,
                style: ThemeTextStyleDeclaration {
                    fg: Some("comment".to_string()),
                    italic: Some(true),
                    ..ThemeTextStyleDeclaration::default()
                },
            },
        ]);

        let text = render_buffer_text(&model, 12, RenderTextMode::StyledTrueColor);
        let line = &text.lines[0];

        assert_eq!(line.spans[0].content.as_ref(), "// comment");
        assert_eq!(
            line.spans[0].style,
            Style::default()
                .fg(Color::Rgb(0x56, 0x5f, 0x89))
                .add_modifier(Modifier::ITALIC)
        );
    }

    #[test]
    fn render_buffer_text_maps_vim_syntax_groups_to_type_and_function_theme_styles() {
        let mut model = screen_model_with_message(None);
        model.lines = vec!["const std::PathBuf macro".to_string()];
        model.visual_selection = None;
        model.syntax_chunks = vec![
            ScreenSyntaxChunk {
                row: 0,
                start_col: 0,
                end_col_exclusive: 5,
                syn_id: 1,
                name: Some("rustStorage".to_string()),
                tree_sitter: None,
            },
            ScreenSyntaxChunk {
                row: 0,
                start_col: 6,
                end_col_exclusive: 9,
                syn_id: 2,
                name: Some("rustModPath".to_string()),
                tree_sitter: None,
            },
            ScreenSyntaxChunk {
                row: 0,
                start_col: 9,
                end_col_exclusive: 11,
                syn_id: 3,
                name: Some("rustModPathSep".to_string()),
                tree_sitter: None,
            },
            ScreenSyntaxChunk {
                row: 0,
                start_col: 11,
                end_col_exclusive: 18,
                syn_id: 4,
                name: Some("rustType".to_string()),
                tree_sitter: None,
            },
            ScreenSyntaxChunk {
                row: 0,
                start_col: 19,
                end_col_exclusive: 24,
                syn_id: 5,
                name: Some("rustMacro".to_string()),
                tree_sitter: None,
            },
        ];
        model.resolved_theme = theme_from_entries(vec![
            StartupRegistryEntry::ThemePalette {
                name: "statement".to_string(),
                value: "#bb9af7".to_string(),
            },
            StartupRegistryEntry::ThemePalette {
                name: "function".to_string(),
                value: "#7aa2f7".to_string(),
            },
            StartupRegistryEntry::ThemePalette {
                name: "type".to_string(),
                value: "#2ac3de".to_string(),
            },
            StartupRegistryEntry::ThemePalette {
                name: "punctuation".to_string(),
                value: "#737aa2".to_string(),
            },
            StartupRegistryEntry::ThemeSyntaxStyle {
                key: SyntaxSemanticStyleKey::Statement,
                style: ThemeTextStyleDeclaration {
                    fg: Some("statement".to_string()),
                    ..ThemeTextStyleDeclaration::default()
                },
            },
            StartupRegistryEntry::ThemeSyntaxStyle {
                key: SyntaxSemanticStyleKey::Function,
                style: ThemeTextStyleDeclaration {
                    fg: Some("function".to_string()),
                    ..ThemeTextStyleDeclaration::default()
                },
            },
            StartupRegistryEntry::ThemeSyntaxStyle {
                key: SyntaxSemanticStyleKey::Type,
                style: ThemeTextStyleDeclaration {
                    fg: Some("type".to_string()),
                    ..ThemeTextStyleDeclaration::default()
                },
            },
            StartupRegistryEntry::ThemeSyntaxStyle {
                key: SyntaxSemanticStyleKey::Punctuation,
                style: ThemeTextStyleDeclaration {
                    fg: Some("punctuation".to_string()),
                    ..ThemeTextStyleDeclaration::default()
                },
            },
        ]);

        let text = render_buffer_text(&model, 24, RenderTextMode::StyledTrueColor);
        let line = &text.lines[0];

        assert_eq!(rendered_text_line(&text, 0), "const std::PathBuf macro");
        assert_eq!(line.spans[0].content.as_ref(), "const");
        assert_eq!(
            line.spans[0].style,
            Style::default().fg(Color::Rgb(0xbb, 0x9a, 0xf7))
        );
        assert_eq!(line.spans[2].content.as_ref(), "std");
        assert_eq!(
            line.spans[2].style,
            Style::default().fg(Color::Rgb(0x2a, 0xc3, 0xde))
        );
        assert_eq!(line.spans[3].content.as_ref(), "::");
        assert_eq!(
            line.spans[3].style,
            Style::default().fg(Color::Rgb(0x73, 0x7a, 0xa2))
        );
        assert_eq!(line.spans[4].content.as_ref(), "PathBuf");
        assert_eq!(
            line.spans[4].style,
            Style::default().fg(Color::Rgb(0x2a, 0xc3, 0xde))
        );
        assert_eq!(line.spans[6].content.as_ref(), "macro");
        assert_eq!(
            line.spans[6].style,
            Style::default().fg(Color::Rgb(0x7a, 0xa2, 0xf7))
        );
    }

    #[test]
    fn markdown_style_ranges_override_syntax_highlight_on_same_cells() {
        let mut model = screen_model_with_message(None);
        model.lines = vec!["## Heading".to_string()];
        model.is_active = false;
        model.visual_selection = None;
        model.line_projections = vec![ScreenLineProjection {
            absolute_row: 0,
            raw_text: "## Heading".to_string(),
            display_text: "Heading".to_string(),
            spans: vec![],
            cells: vec![],
            line_start_col: 0,
        }];
        model.syntax_chunks = vec![ScreenSyntaxChunk {
            row: 0,
            start_col: 0,
            end_col_exclusive: 7,
            syn_id: 9,
            name: Some("Title".to_string()),
            tree_sitter: None,
        }];
        model.markdown_style_ranges = vec![ScreenMarkdownStyleRange {
            row: 0,
            start_col: 0,
            end_col_exclusive: 7,
            style: ResolvedTextStyle {
                fg: Some(ResolvedThemeColor("#9ece6a".to_string())),
                underline: true,
                ..ResolvedTextStyle::default()
            },
        }];

        let text = render_buffer_text(&model, 10, RenderTextMode::StyledTrueColor);
        let line = &text.lines[0];

        assert_eq!(line.spans[0].content.as_ref(), "Heading");
        assert_eq!(
            line.spans[0].style,
            Style::default()
                .fg(Color::Rgb(0x9e, 0xce, 0x6a))
                .add_modifier(Modifier::UNDERLINED),
            "Markdown semantic theme should win over core syntax style on projected Markdown cells"
        );
    }

    #[test]
    fn plain_text_mode_preserves_markdown_text_while_removing_style() {
        let mut model = screen_model_with_message(None);
        model.lines = vec!["`code`".to_string()];
        model.is_active = false;
        model.visual_selection = None;
        model.line_projections = vec![ScreenLineProjection {
            absolute_row: 0,
            raw_text: "`code`".to_string(),
            display_text: "code".to_string(),
            spans: vec![],
            cells: vec![],
            line_start_col: 0,
        }];
        model.markdown_style_ranges = vec![ScreenMarkdownStyleRange {
            row: 0,
            start_col: 0,
            end_col_exclusive: 4,
            style: ResolvedTextStyle {
                fg: Some(ResolvedThemeColor("#ff9e64".to_string())),
                ..ResolvedTextStyle::default()
            },
        }];

        let text = render_buffer_text(&model, 6, RenderTextMode::Plain);

        assert_eq!(rendered_text_line(&text, 0), "code  ");
        assert!(
            text.lines[0]
                .spans
                .iter()
                .all(|span| span.style == Style::default())
        );
    }

    #[test]
    fn monochrome_text_mode_preserves_markdown_modifiers_while_removing_colors() {
        let mut model = screen_model_with_message(None);
        model.lines = vec!["## Heading".to_string()];
        model.is_active = false;
        model.visual_selection = None;
        model.line_projections = vec![ScreenLineProjection {
            absolute_row: 0,
            raw_text: "## Heading".to_string(),
            display_text: "Heading".to_string(),
            spans: vec![],
            cells: vec![],
            line_start_col: 0,
        }];
        model.markdown_style_ranges = vec![ScreenMarkdownStyleRange {
            row: 0,
            start_col: 0,
            end_col_exclusive: 7,
            style: ResolvedTextStyle {
                fg: Some(ResolvedThemeColor("#9ece6a".to_string())),
                bold: true,
                underline: true,
                ..ResolvedTextStyle::default()
            },
        }];

        let text = render_buffer_text(&model, 10, RenderTextMode::StyledMonochrome);
        let line = &text.lines[0];

        assert_eq!(line.spans[0].content.as_ref(), "Heading");
        assert_eq!(
            line.spans[0].style,
            Style::default()
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::UNDERLINED),
            "NO_COLOR mode should remove theme colors without dropping text modifiers"
        );
    }

    #[test]
    fn crossterm_backend_emits_bold_sgr_for_markdown_heading_theme() {
        let writer = CaptureWriter::default();
        let backend = CrosstermBackend::new(writer.clone());
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(Rect::new(0, 0, 24, 4)),
            },
        )
        .expect("crossterm test terminal should initialize");
        let mut model = screen_model_with_message(None);
        model.lines = vec!["## プロジェクト概要".to_string()];
        model.is_active = false;
        model.visual_selection = None;
        model.line_projections = vec![ScreenLineProjection {
            absolute_row: 0,
            raw_text: "## プロジェクト概要".to_string(),
            display_text: "プロジェクト概要".to_string(),
            spans: vec![],
            cells: vec![],
            line_start_col: 0,
        }];
        model.markdown_style_ranges = vec![ScreenMarkdownStyleRange {
            row: 0,
            start_col: 0,
            end_col_exclusive: 16,
            style: ResolvedTextStyle {
                fg: Some(ResolvedThemeColor("#9ece6a".to_string())),
                bold: true,
                underline: true,
                ..ResolvedTextStyle::default()
            },
        }];

        draw_editor_frame(&mut terminal, &model, true).expect("markdown heading should render");
        let bytes = writer.bytes();
        let output = String::from_utf8_lossy(&bytes);

        assert!(
            output.contains("\u{1b}[1m"),
            "Crossterm output should include SGR 1 for bold: {output:?}"
        );
        assert!(
            output.contains("\u{1b}[4m"),
            "Crossterm output should include SGR 4 for underline: {output:?}"
        );
    }

    #[test]
    fn crossterm_backend_forces_color_sgr_for_syntax_highlight_even_when_no_color_is_set() {
        let _lock = color_env_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _no_color_guard = NoColorGuard::set();
        let writer = CaptureWriter::default();
        let backend = CrosstermBackend::new(writer.clone());
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(Rect::new(0, 0, 24, 4)),
            },
        )
        .expect("crossterm test terminal should initialize");
        let mut model = screen_model_with_message(None);
        model.lines = vec!["let value = 1;".to_string()];
        model.is_active = true;
        model.visual_selection = None;
        model.syntax_chunks = vec![ScreenSyntaxChunk {
            row: 0,
            start_col: 0,
            end_col_exclusive: 3,
            syn_id: 1,
            name: Some("rustKeyword".to_string()),
            tree_sitter: None,
        }];
        model.resolved_theme = theme_from_entries(vec![
            StartupRegistryEntry::ThemePalette {
                name: "keyword".to_string(),
                value: "#bb9af7".to_string(),
            },
            StartupRegistryEntry::ThemeSyntaxStyle {
                key: SyntaxSemanticStyleKey::Statement,
                style: ThemeTextStyleDeclaration {
                    fg: Some("keyword".to_string()),
                    ..ThemeTextStyleDeclaration::default()
                },
            },
        ]);

        draw_workspace_frame(
            &mut terminal,
            &WorkspaceScreenModel {
                panes: vec![model.clone()],
                floats: vec![],
                active_window_id: model.window_id,
                message_line: resolve_workspace_message_line(Vec::<MessageLineCandidate>::new()),
                message_area_height: 5,
                message_scroll_offset: 0,
                prompt_line: None,
                pager_prompt: None,
                suppressed_prompt_hints: vec![],
                bell: None,
                command_line: None,
            },
            true,
            RenderTextMode::StyledTrueColor,
        )
        .expect("syntax highlight should render with forced color");
        let bytes = writer.bytes();
        let output = String::from_utf8_lossy(&bytes);

        assert!(
            output.contains("\u{1b}[38;2;187;154;247"),
            "syntax on should emit color SGR even when NO_COLOR is set: {output:?}"
        );
    }

    #[test]
    fn crossterm_backend_omits_bold_sgr_when_heading_level_disables_bold() {
        let writer = CaptureWriter::default();
        let backend = CrosstermBackend::new(writer.clone());
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(Rect::new(0, 0, 24, 4)),
            },
        )
        .expect("crossterm test terminal should initialize");
        let mut model = screen_model_with_message(None);
        model.lines = vec!["## プロジェクト概要".to_string()];
        model.is_active = false;
        model.visual_selection = None;
        model.line_projections = vec![ScreenLineProjection {
            absolute_row: 0,
            raw_text: "## プロジェクト概要".to_string(),
            display_text: "プロジェクト概要".to_string(),
            spans: vec![],
            cells: vec![],
            line_start_col: 0,
        }];
        model.markdown_style_ranges = vec![ScreenMarkdownStyleRange {
            row: 0,
            start_col: 0,
            end_col_exclusive: 16,
            style: ResolvedTextStyle {
                fg: Some(ResolvedThemeColor("#9ece6a".to_string())),
                bold: false,
                underline: true,
                ..ResolvedTextStyle::default()
            },
        }];

        draw_editor_frame(&mut terminal, &model, true).expect("markdown heading should render");
        let bytes = writer.bytes();
        let output = String::from_utf8_lossy(&bytes);

        assert!(
            !output.contains("\u{1b}[1m"),
            "Crossterm output should not include SGR 1 when bold=false: {output:?}"
        );
        assert!(
            output.contains("\u{1b}[4m"),
            "Crossterm output should still include SGR 4 for underline: {output:?}"
        );
    }

    #[test]
    fn crossterm_backend_emits_bold_sgr_for_startup_config_heading1_with_line_numbers() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let target_path = unique_renderer_path("heading1-target.md");
        let config_path = unique_renderer_path("heading1-init.ts");
        let markdown_source = "# AGENTS.md\n\nbody\n";
        std::fs::write(&target_path, markdown_source).expect("target file");
        std::fs::write(
            &config_path,
            r##"
                saya.options.lineNumbers = true;
                saya.options.syntax = true;
                saya.theme.palette = {
                    accent: "#7aa2f7",
                    heading2: "#9ece6a",
                };
                saya.theme.markdown = {
                    heading: { fg: "accent", bold: true },
                    heading2: { fg: "heading2", underline: true, bold: false },
                };
            "##,
        )
        .expect("config file");

        let outcome = prepare_launch(LaunchRequest {
            input_source: InputSource::File(target_path.clone()),
            config_source: ConfigSource::File(config_path.clone()),
            ..LaunchRequest::default()
        })
        .expect("startup with typescript theme config");
        let markdown_map = MarkdownDocumentMap::parse(markdown_source);
        let session_state = outcome.editor_session_state();
        let model = project(
            &ProjectionInput::new(&outcome.initial_snapshot, &session_state, None)
                .with_markdown_document_map(Some(&markdown_map)),
        );
        let heading = model
            .markdown_style_ranges
            .iter()
            .find(|range| range.row == 0)
            .expect("heading1 range should be projected");

        assert_eq!(model.line_projections[0].display_text, "# AGENTS.md");
        assert_eq!(
            heading.start_col, model.line_projections[0].line_start_col,
            "active raw heading1 should style the heading after the line-number gutter"
        );
        assert!(
            heading.style.bold,
            "startup heading theme should keep heading1 bold before renderer output"
        );

        let writer = CaptureWriter::default();
        let backend = CrosstermBackend::new(writer.clone());
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(Rect::new(0, 0, 32, 4)),
            },
        )
        .expect("crossterm test terminal should initialize");

        draw_editor_frame(&mut terminal, &model, true).expect("heading1 should render");
        let bytes = writer.bytes();
        let output = String::from_utf8_lossy(&bytes);

        assert!(
            output.contains("\u{1b}[1m"),
            "startup-configured heading1 should emit SGR 1 for bold: {output:?}"
        );

        std::fs::remove_file(&target_path).expect("remove target");
        std::fs::remove_file(&config_path).expect("remove config");
    }

    #[test]
    fn crossterm_backend_emits_bold_sgr_for_markdown_heading_in_monochrome_mode() {
        let writer = CaptureWriter::default();
        let backend = CrosstermBackend::new(writer.clone());
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(Rect::new(0, 0, 24, 4)),
            },
        )
        .expect("crossterm test terminal should initialize");
        let mut model = screen_model_with_message(None);
        model.lines = vec!["   1 # AGENTS.md".to_string()];
        model.is_active = true;
        model.visual_selection = None;
        model.line_projections = vec![ScreenLineProjection {
            absolute_row: 0,
            raw_text: "# AGENTS.md".to_string(),
            display_text: "# AGENTS.md".to_string(),
            spans: vec![],
            cells: vec![],
            line_start_col: 5,
        }];
        model.markdown_style_ranges = vec![ScreenMarkdownStyleRange {
            row: 0,
            start_col: 5,
            end_col_exclusive: 16,
            style: ResolvedTextStyle {
                fg: Some(ResolvedThemeColor("#7aa2f7".to_string())),
                bold: true,
                ..ResolvedTextStyle::default()
            },
        }];

        draw_workspace_frame(
            &mut terminal,
            &WorkspaceScreenModel {
                panes: vec![model.clone()],
                floats: vec![],
                active_window_id: model.window_id,
                message_line: resolve_workspace_message_line(Vec::<MessageLineCandidate>::new()),
                message_area_height: 5,
                message_scroll_offset: 0,
                prompt_line: None,
                pager_prompt: None,
                suppressed_prompt_hints: vec![],
                bell: None,
                command_line: None,
            },
            true,
            RenderTextMode::StyledMonochrome,
        )
        .expect("monochrome heading should render");
        let bytes = writer.bytes();
        let output = String::from_utf8_lossy(&bytes);

        assert!(
            output.contains("\u{1b}[1m"),
            "monochrome mode should still emit SGR 1 for bold: {output:?}"
        );
        assert!(
            !output.contains("38;2"),
            "monochrome mode should drop theme color SGR while keeping bold: {output:?}"
        );
    }

    #[test]
    fn render_buffer_text_keeps_line_number_gutter_with_projected_display_text() {
        let mut model = screen_model_with_message(None);
        model.lines = vec!["   1 # Heading".to_string()];
        model.is_active = false;
        model.visual_selection = None;
        model.line_projections = vec![ScreenLineProjection {
            absolute_row: 0,
            raw_text: "# Heading".to_string(),
            display_text: "Heading".to_string(),
            spans: vec![],
            cells: vec![],
            line_start_col: 5,
        }];

        let text = render_buffer_text(&model, 14, RenderTextMode::Plain);
        let rendered = text.lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert_eq!(rendered, "   1 Heading  ");
    }

    #[test]
    fn render_buffer_text_uses_raw_projection_display_text_on_active_cursor_row() {
        let mut model = screen_model_with_message(None);
        model.lines = vec!["# Heading".to_string()];
        model.is_active = true;
        model.cursor_row = 0;
        model.visual_selection = None;
        model.line_projections = vec![ScreenLineProjection {
            absolute_row: 0,
            raw_text: "# Heading".to_string(),
            display_text: "# Heading".to_string(),
            spans: vec![],
            cells: vec![],
            line_start_col: 0,
        }];

        let text = render_buffer_text(&model, 10, RenderTextMode::Plain);
        let rendered = text.lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert_eq!(rendered, "# Heading ");
    }

    #[test]
    fn render_buffer_text_uses_projection_display_text_even_for_active_cursor_row() {
        let mut model = screen_model_with_message(None);
        model.lines = vec!["# Heading".to_string()];
        model.is_active = true;
        model.cursor_row = 0;
        model.visual_selection = None;
        model.line_projections = vec![ScreenLineProjection {
            absolute_row: 0,
            raw_text: "# Heading".to_string(),
            display_text: "Heading".to_string(),
            spans: vec![],
            cells: vec![],
            line_start_col: 0,
        }];

        let text = render_buffer_text(&model, 10, RenderTextMode::Plain);
        let rendered = text.lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert_eq!(rendered, "Heading   ");
    }

    #[test]
    fn render_buffer_text_does_not_reinterpret_projection_for_active_or_inactive_rows() {
        let mut active_model = screen_model_with_message(None);
        active_model.lines = vec!["# Active".to_string()];
        active_model.is_active = true;
        active_model.cursor_row = 0;
        active_model.visual_selection = None;
        active_model.line_projections = vec![projection("# Active", "Active", 0)];

        let active_text = render_buffer_text(&active_model, 10, RenderTextMode::Plain);

        assert_eq!(rendered_text_line(&active_text, 0), "Active    ");

        let mut inactive_model = screen_model_with_message(None);
        inactive_model.lines = vec!["# Inactive".to_string()];
        inactive_model.is_active = false;
        inactive_model.cursor_row = 0;
        inactive_model.visual_selection = None;
        inactive_model.line_projections = vec![projection("# Inactive", "# Inactive", 0)];

        let inactive_text = render_buffer_text(&inactive_model, 12, RenderTextMode::Plain);

        assert_eq!(rendered_text_line(&inactive_text, 0), "# Inactive  ");
    }

    #[test]
    fn render_buffer_text_renders_raw_block_rows_from_projection_display_text() {
        let mut model = screen_model_with_message(None);
        model.lines = vec![
            "# Title".to_string(),
            "- [x] done".to_string(),
            "tail".to_string(),
        ];
        model.is_active = true;
        model.cursor_row = 1;
        model.visual_selection = None;
        model.line_projections = vec![
            ScreenLineProjection {
                absolute_row: 0,
                raw_text: "# Title".to_string(),
                display_text: "# Title".to_string(),
                spans: vec![],
                cells: vec![],
                line_start_col: 0,
            },
            ScreenLineProjection {
                absolute_row: 1,
                raw_text: "- [x] done".to_string(),
                display_text: "- [x] done".to_string(),
                spans: vec![],
                cells: vec![],
                line_start_col: 0,
            },
            ScreenLineProjection {
                absolute_row: 2,
                raw_text: "tail".to_string(),
                display_text: "tail".to_string(),
                spans: vec![],
                cells: vec![],
                line_start_col: 0,
            },
        ];

        let text = render_buffer_text(&model, 12, RenderTextMode::Plain);
        let rendered = text
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();

        assert_eq!(
            rendered,
            vec![
                "# Title     ".to_string(),
                "- [x] done  ".to_string(),
                "tail        ".to_string(),
            ]
        );
    }

    #[test]
    fn render_buffer_text_preserves_gutter_and_overlay_for_raw_projection_row() {
        let mut model = screen_model_with_message(None);
        model.lines = vec!["   1 # Heading".to_string()];
        model.is_active = true;
        model.cursor_row = 0;
        model.visual_selection = None;
        model.line_projections = vec![ScreenLineProjection {
            absolute_row: 0,
            raw_text: "# Heading".to_string(),
            display_text: "# Heading".to_string(),
            spans: vec![],
            cells: vec![],
            line_start_col: 5,
        }];
        model.search_overlays = vec![ScreenSearchOverlay {
            row: 0,
            start_col: 5,
            end_col_exclusive: 14,
            kind: SearchMatchKind::Regular,
        }];

        let text = render_buffer_text(&model, 16, RenderTextMode::StyledTrueColor);
        let line = &text.lines[0];

        assert_eq!(line.spans[0].content.as_ref(), "   1 ");
        assert_eq!(line.spans[0].style, Style::default());
        assert_eq!(line.spans[1].content.as_ref(), "# Heading");
        assert_eq!(
            line.spans[1].style,
            Style::default().fg(Color::Black).bg(Color::Yellow)
        );
        assert_eq!(line.spans[2].content.as_ref(), "  ");
    }

    #[test]
    fn render_buffer_text_applies_gutter_and_overlays_in_projected_display_space() {
        let mut model = screen_model_with_message(None);
        model.lines = vec!["   1 # Heading".to_string()];
        model.is_active = false;
        model.cursor_row = 0;
        model.visual_selection = Some(ScreenSelection {
            start_row: 0,
            start_col: 9,
            line_start_col: 5,
            end_row: 0,
            end_col_exclusive: 11,
        });
        model.search_overlays = vec![ScreenSearchOverlay {
            row: 0,
            start_col: 6,
            end_col_exclusive: 8,
            kind: SearchMatchKind::Regular,
        }];
        model.syntax_chunks = vec![ScreenSyntaxChunk {
            row: 0,
            start_col: 5,
            end_col_exclusive: 12,
            syn_id: 7,
            name: Some("Keyword".to_string()),
            tree_sitter: None,
        }];
        model.line_projections = vec![projection("# Heading", "Heading", 5)];

        let text = render_buffer_text(&model, 14, RenderTextMode::StyledTrueColor);
        let line = &text.lines[0];

        assert_eq!(line.spans.len(), 7);
        assert_eq!(line.spans[0].content.as_ref(), "   1 ");
        assert_eq!(line.spans[0].style, Style::default());
        assert_eq!(line.spans[1].content.as_ref(), "H");
        assert_eq!(line.spans[1].style, Style::default().fg(Color::Cyan));
        assert_eq!(line.spans[2].content.as_ref(), "ea");
        assert_eq!(
            line.spans[2].style,
            Style::default().fg(Color::Black).bg(Color::Yellow)
        );
        assert_eq!(line.spans[3].content.as_ref(), "d");
        assert_eq!(line.spans[3].style, Style::default().fg(Color::Cyan));
        assert_eq!(line.spans[4].content.as_ref(), "in");
        assert_eq!(
            line.spans[4].style,
            Style::default().add_modifier(Modifier::REVERSED)
        );
        assert_eq!(line.spans[5].content.as_ref(), "g");
        assert_eq!(line.spans[5].style, Style::default().fg(Color::Cyan));
        assert_eq!(line.spans[6].content.as_ref(), "  ");
    }

    #[test]
    fn render_buffer_text_falls_back_to_lines_when_line_projections_are_empty() {
        let mut model = screen_model_with_message(None);
        model.lines = vec!["# Heading".to_string()];
        model.line_projections = vec![];
        model.visual_selection = None;

        let text = render_buffer_text(&model, 10, RenderTextMode::Plain);
        let rendered = text.lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert_eq!(rendered, "# Heading ");
    }

    #[test]
    fn syntax_chunks_style_spans_without_changing_line_text() {
        let model = ScreenModel {
            window_id: 1,
            buffer_id: 1,
            rect: PaneRect {
                x: 0,
                y: 0,
                width: 12,
                height: 3,
            },
            file_name: "test.rs".to_string(),
            mode_label: "NORMAL".to_string(),
            cursor_style: ScreenCursorStyle::Block,
            dirty: false,
            lines: vec!["let value".to_string()],
            line_projections: vec![],
            cursor_row: 0,
            cursor_col: 0,
            visual_selection: None,
            search_overlays: vec![],
            syntax_chunks: vec![ScreenSyntaxChunk {
                row: 0,
                start_col: 0,
                end_col_exclusive: 3,
                syn_id: 7,
                name: Some("Keyword".to_string()),
                tree_sitter: None,
            }],
            markdown_style_ranges: vec![],
            resolved_theme: crate::theme::ResolvedTheme::default(),
            message_line: None,
            command_cursor_col: None,
            is_active: true,
        };

        let text = render_buffer_text(&model, 12, RenderTextMode::StyledTrueColor);
        let line = &text.lines[0];

        assert_eq!(line.spans[0].content.as_ref(), "let");
        assert_eq!(line.spans[0].style, Style::default().fg(Color::Cyan));
        assert_eq!(line.spans[1].content.as_ref(), " value");
        assert_eq!(
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>(),
            "let value   ",
            "syntax styling must not alter rendered line text"
        );
    }

    #[test]
    fn tree_sitter_syntax_styles_use_category_and_modifier_not_capture_name() {
        use crate::screen_model::{
            ScreenSyntaxCategory, ScreenSyntaxModifier, ScreenTreeSitterSyntax,
        };

        let model = ScreenModel {
            window_id: 1,
            buffer_id: 1,
            rect: PaneRect {
                x: 0,
                y: 0,
                width: 12,
                height: 3,
            },
            file_name: "test.rs".to_string(),
            mode_label: "NORMAL".to_string(),
            cursor_style: ScreenCursorStyle::Block,
            dirty: false,
            lines: vec!["fn value".to_string()],
            line_projections: vec![],
            cursor_row: 0,
            cursor_col: 0,
            visual_selection: None,
            search_overlays: vec![],
            syntax_chunks: vec![ScreenSyntaxChunk {
                row: 0,
                start_col: 0,
                end_col_exclusive: 2,
                syn_id: 0,
                name: Some("ignored.capture".to_string()),
                tree_sitter: Some(ScreenTreeSitterSyntax {
                    category: ScreenSyntaxCategory::Keyword,
                    modifiers: vec![ScreenSyntaxModifier::Definition],
                    capture_name: "ignored.capture".to_string(),
                }),
            }],
            markdown_style_ranges: vec![],
            resolved_theme: crate::theme::ResolvedTheme::default(),
            message_line: None,
            command_cursor_col: None,
            is_active: true,
        };

        let text = render_buffer_text(&model, 12, RenderTextMode::StyledTrueColor);
        let line = &text.lines[0];

        assert_eq!(line.spans[0].content.as_ref(), "fn");
        assert_eq!(
            line.spans[0].style,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            "Tree-sitter styling must use normalized category/modifier data"
        );
        assert_eq!(line.spans[1].content.as_ref(), " value");
    }

    #[test]
    fn visual_selection_overrides_search_overlay_when_ranges_overlap() {
        let model = ScreenModel {
            window_id: 1,
            buffer_id: 1,
            rect: PaneRect {
                x: 0,
                y: 0,
                width: 6,
                height: 3,
            },
            file_name: "test.txt".to_string(),
            mode_label: "VISUAL".to_string(),
            cursor_style: ScreenCursorStyle::Block,
            dirty: false,
            lines: vec!["abcdef".to_string()],
            line_projections: vec![],
            cursor_row: 0,
            cursor_col: 0,
            visual_selection: Some(ScreenSelection {
                start_row: 0,
                start_col: 2,
                line_start_col: 2,
                end_row: 0,
                end_col_exclusive: 4,
            }),
            search_overlays: vec![ScreenSearchOverlay {
                row: 0,
                start_col: 0,
                end_col_exclusive: 6,
                kind: SearchMatchKind::Regular,
            }],
            syntax_chunks: vec![],
            markdown_style_ranges: vec![],
            resolved_theme: crate::theme::ResolvedTheme::default(),
            message_line: None,
            command_cursor_col: None,
            is_active: true,
        };

        let text = render_buffer_text(&model, 6, RenderTextMode::StyledTrueColor);
        let line = &text.lines[0];

        assert_eq!(line.spans.len(), 3);
        assert_eq!(line.spans[0].content.as_ref(), "ab");
        assert_eq!(line.spans[1].content.as_ref(), "cd");
        assert_eq!(line.spans[2].content.as_ref(), "ef");
        assert_eq!(
            line.spans[0].style,
            Style::default().fg(Color::Black).bg(Color::Yellow)
        );
        assert_eq!(
            line.spans[1].style,
            Style::default().add_modifier(Modifier::REVERSED)
        );
        assert_eq!(
            line.spans[2].style,
            Style::default().fg(Color::Black).bg(Color::Yellow)
        );
    }

    #[test]
    fn search_overlay_renders_full_width_glyph_with_background_highlight() {
        let model = ScreenModel {
            window_id: 1,
            buffer_id: 1,
            rect: PaneRect {
                x: 0,
                y: 0,
                width: 6,
                height: 3,
            },
            file_name: "test.txt".to_string(),
            mode_label: "NORMAL".to_string(),
            cursor_style: ScreenCursorStyle::Block,
            dirty: false,
            lines: vec!["xあx".to_string()],
            line_projections: vec![],
            cursor_row: 0,
            cursor_col: 0,
            visual_selection: None,
            search_overlays: vec![ScreenSearchOverlay {
                row: 0,
                start_col: 1,
                end_col_exclusive: 3,
                kind: SearchMatchKind::Regular,
            }],
            syntax_chunks: vec![],
            markdown_style_ranges: vec![],
            resolved_theme: crate::theme::ResolvedTheme::default(),
            message_line: None,
            command_cursor_col: None,
            is_active: true,
        };

        let text = render_buffer_text(&model, 6, RenderTextMode::StyledTrueColor);
        let line = &text.lines[0];

        assert_eq!(line.spans.len(), 4);
        assert_eq!(line.spans[0].content.as_ref(), "x");
        assert_eq!(line.spans[1].content.as_ref(), "あ");
        assert_eq!(line.spans[2].content.as_ref(), "x");
        assert!(
            line.spans[3].content.as_ref().chars().all(|ch| ch == ' '),
            "rendered line should keep trailing padding spaces"
        );
        assert_eq!(
            line.spans[1].style,
            Style::default().fg(Color::Black).bg(Color::Yellow)
        );
    }

    #[test]
    fn multiline_selection_does_not_highlight_line_number_gutter() {
        let model = ScreenModel {
            window_id: 1,
            buffer_id: 1,
            rect: PaneRect {
                x: 0,
                y: 0,
                width: 20,
                height: 4,
            },
            file_name: "test.txt".to_string(),
            mode_label: "V-LINE".to_string(),
            cursor_style: ScreenCursorStyle::Block,
            dirty: false,
            lines: vec![" 1 alpha".to_string(), " 2 beta".to_string()],
            line_projections: vec![],
            cursor_row: 1,
            cursor_col: 3,
            visual_selection: Some(ScreenSelection {
                start_row: 0,
                start_col: 3,
                line_start_col: 3,
                end_row: 1,
                end_col_exclusive: 7,
            }),
            search_overlays: vec![],
            syntax_chunks: vec![],
            markdown_style_ranges: vec![],
            resolved_theme: crate::theme::ResolvedTheme::default(),
            message_line: None,
            command_cursor_col: None,
            is_active: true,
        };

        let text = render_buffer_text(&model, 20, RenderTextMode::StyledTrueColor);
        let second_line = &text.lines[1];

        assert_eq!(second_line.spans.len(), 3);
        assert_eq!(second_line.spans[0].content.as_ref(), " 2 ");
        assert_eq!(second_line.spans[1].content.as_ref(), "beta");
        assert!(
            second_line.spans[2]
                .content
                .as_ref()
                .chars()
                .all(|ch| ch == ' '),
            "末尾はパディング空白で埋めること"
        );
    }

    #[test]
    fn redraw_clears_stale_tail_when_line_becomes_shorter() {
        let mut terminal =
            Terminal::new(TestBackend::new(40, 4)).expect("test terminal should initialize");
        let mut long_model = screen_model_with_message(None);
        long_model.lines = vec!["## プロジェクト概要    13 seconds ago".to_string()];
        long_model.dirty = false;
        let mut short_model = screen_model_with_message(None);
        short_model.lines = vec!["## プロジェクト概要".to_string()];
        short_model.dirty = false;

        draw_editor_frame(&mut terminal, &long_model, true).expect("first draw should succeed");
        draw_editor_frame(&mut terminal, &short_model, false)
            .expect("short line redraw should succeed");

        let rendered = terminal.backend().buffer().content();
        let first_row: String = rendered.iter().take(40).map(|cell| cell.symbol()).collect();

        assert!(
            !first_row.contains("seconds ago"),
            "短い行への再描画で古い suffix が残らないこと: {:?}",
            first_row
        );
    }

    #[test]
    fn redraw_clears_stale_tail_when_projected_display_becomes_shorter() {
        let mut terminal =
            Terminal::new(TestBackend::new(40, 4)).expect("test terminal should initialize");
        let mut long_model = screen_model_with_message(None);
        long_model.lines = vec!["# Long projected tail".to_string()];
        long_model.line_projections = vec![projection(
            "# Long projected tail",
            "Long projected tail",
            0,
        )];
        long_model.visual_selection = None;
        long_model.dirty = false;

        let mut short_model = screen_model_with_message(None);
        short_model.lines = vec!["# Short".to_string()];
        short_model.line_projections = vec![projection("# Short", "Short", 0)];
        short_model.visual_selection = None;
        short_model.dirty = false;

        draw_editor_frame(&mut terminal, &long_model, true).expect("first draw should succeed");
        draw_editor_frame(&mut terminal, &short_model, false)
            .expect("short projected redraw should succeed");

        let rendered = terminal.backend().buffer().content();
        let first_row: String = rendered.iter().take(40).map(|cell| cell.symbol()).collect();

        assert!(
            !first_row.contains("projected tail"),
            "shorter projected redraw must clear stale suffix: {:?}",
            first_row
        );
    }

    #[test]
    fn integrated_update_cycle_keeps_message_status_and_cursor_in_sync() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut outcome = prepare_launch(LaunchRequest::default()).expect("launch should succeed");
        let mut session_state = EditorSessionState::new(outcome.target_path.clone());

        outcome.core_bridge.dispatch_key("i").expect("insert mode");
        outcome.core_bridge.dispatch_key("H").expect("insert text");
        outcome
            .core_bridge
            .dispatch_key("\x1b")
            .expect("leave insert mode");
        session_state.update_dirty(outcome.core_bridge.snapshot().dirty);

        let snapshot = outcome.core_bridge.snapshot();
        let model = project(&ProjectionInput::new(
            &snapshot,
            &session_state,
            Some("Action failed"),
        ));

        let mut terminal =
            Terminal::new(TestBackend::new(40, 4)).expect("test terminal should initialize");
        draw_editor_frame(&mut terminal, &model, true).expect("render should succeed");

        let rendered = format!("{}", terminal.backend());
        let rows: Vec<&str> = rendered.lines().collect();

        assert!(
            rows.get(2).is_some_and(|row| row.contains(&model.file_name)
                && row.contains(&model.mode_label)
                && row.contains("[+]!")),
            "status line should reflect file name, mode, and dirty state: {:?}",
            rows.get(2)
        );
        assert!(
            rows.get(3).is_some_and(|row| row.contains("Action failed")),
            "message line should render the projected transient message: {:?}",
            rows.get(3)
        );
        assert!(
            rows.get(0).is_some_and(|row| row.contains('H')),
            "buffer area should include the edited content after the update cycle: {:?}",
            rows.get(0)
        );
        terminal
            .backend_mut()
            .assert_cursor_position(Position::new(model.cursor_col, model.cursor_row));
    }

    #[test]
    fn workspace_render_uses_active_window_id_even_when_pane_flags_are_stale() {
        let mut terminal =
            Terminal::new(TestBackend::new(40, 8)).expect("test terminal should initialize");
        let model = WorkspaceScreenModel {
            panes: vec![
                ScreenModel {
                    window_id: 10,
                    buffer_id: 10,
                    rect: PaneRect {
                        x: 0,
                        y: 0,
                        width: 20,
                        height: 4,
                    },
                    file_name: "left.txt".to_string(),
                    mode_label: "NORMAL".to_string(),
                    cursor_style: ScreenCursorStyle::Block,
                    dirty: false,
                    lines: vec!["left".to_string()],
                    line_projections: vec![],
                    cursor_row: 0,
                    cursor_col: 0,
                    visual_selection: None,
                    search_overlays: vec![],
                    syntax_chunks: vec![],
                    markdown_style_ranges: vec![],
                    resolved_theme: crate::theme::ResolvedTheme::default(),
                    message_line: None,
                    command_cursor_col: None,
                    is_active: false,
                },
                ScreenModel {
                    window_id: 20,
                    buffer_id: 20,
                    rect: PaneRect {
                        x: 20,
                        y: 0,
                        width: 20,
                        height: 4,
                    },
                    file_name: "right.txt".to_string(),
                    mode_label: "NORMAL".to_string(),
                    cursor_style: ScreenCursorStyle::Block,
                    dirty: false,
                    lines: vec!["right".to_string()],
                    line_projections: vec![],
                    cursor_row: 1,
                    cursor_col: 2,
                    visual_selection: None,
                    search_overlays: vec![],
                    syntax_chunks: vec![],
                    markdown_style_ranges: vec![],
                    resolved_theme: crate::theme::ResolvedTheme::default(),
                    message_line: None,
                    command_cursor_col: None,
                    is_active: false,
                },
            ],
            floats: vec![],
            active_window_id: 20,
            message_line: resolve_workspace_message_line(Vec::<MessageLineCandidate>::new()),
            message_area_height: 5,
            message_scroll_offset: 0,
            prompt_line: None,
            pager_prompt: None,
            suppressed_prompt_hints: vec![],
            bell: None,
            command_line: None,
        };

        draw_workspace_frame(&mut terminal, &model, true, RenderTextMode::StyledTrueColor)
            .expect("workspace render should succeed");

        terminal
            .backend_mut()
            .assert_cursor_position(Position::new(22, 1));
    }

    #[test]
    fn workspace_render_composes_floats_above_panes_by_zindex() {
        let mut terminal =
            Terminal::new(TestBackend::new(20, 6)).expect("test terminal should initialize");
        let mut model = workspace_with_typed_message(None);
        model.panes[0].rect = PaneRect {
            x: 0,
            y: 0,
            width: 20,
            height: 4,
        };
        model.panes[0].lines = vec!["underneath".to_string()];
        model.floats = vec![
            FloatingScreenModel {
                id: FloatingWindowId(1),
                content: FloatingContentRef::StaticLines { content_id: 1 },
                rect: PaneRect {
                    x: 1,
                    y: 0,
                    width: 10,
                    height: 2,
                },
                lines: vec!["low".to_string()],
                focusable: false,
                mouse: false,
                chrome: FloatingChrome {
                    border: FloatingBorder::None,
                },
                zindex: 40,
                creation_order: 1,
            },
            FloatingScreenModel {
                id: FloatingWindowId(2),
                content: FloatingContentRef::StaticLines { content_id: 2 },
                rect: PaneRect {
                    x: 1,
                    y: 0,
                    width: 10,
                    height: 2,
                },
                lines: vec!["top".to_string()],
                focusable: false,
                mouse: false,
                chrome: FloatingChrome {
                    border: FloatingBorder::None,
                },
                zindex: 100,
                creation_order: 2,
            },
        ];

        draw_workspace_frame(&mut terminal, &model, true, RenderTextMode::Plain)
            .expect("workspace render should succeed");

        let rendered = terminal.backend().buffer().content();
        let first_row = rendered
            .iter()
            .take(20)
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert_eq!(
            &first_row[1..4],
            "top",
            "topmost float should overwrite the overlapping pane text: {first_row:?}"
        );
        assert!(
            !first_row.contains("under"),
            "pane text must not remain under the resolved float area: {first_row:?}"
        );
    }

    #[test]
    fn workspace_render_draws_bordered_static_line_float() {
        let mut terminal =
            Terminal::new(TestBackend::new(20, 6)).expect("test terminal should initialize");
        let mut model = workspace_with_typed_message(None);
        model.floats = vec![FloatingScreenModel {
            id: FloatingWindowId(1),
            content: FloatingContentRef::StaticLines { content_id: 1 },
            rect: PaneRect {
                x: 1,
                y: 1,
                width: 10,
                height: 3,
            },
            lines: vec!["hover".to_string()],
            focusable: false,
            mouse: false,
            chrome: FloatingChrome {
                border: FloatingBorder::Single,
            },
            zindex: 40,
            creation_order: 1,
        }];

        draw_workspace_frame(&mut terminal, &model, true, RenderTextMode::Plain)
            .expect("workspace render should succeed");

        let rendered = format!("{}", terminal.backend());

        assert!(
            rendered.contains("┌────────┐"),
            "bordered float should draw a single border: {rendered:?}"
        );
        assert!(
            rendered.contains("│hover"),
            "bordered float should draw content inside the border: {rendered:?}"
        );
    }

    #[test]
    fn workspace_render_does_not_reserve_empty_global_message_row() {
        let mut terminal =
            Terminal::new(TestBackend::new(20, 4)).expect("test terminal should initialize");
        let model = WorkspaceScreenModel {
            panes: vec![ScreenModel {
                window_id: 1,
                buffer_id: 1,
                rect: PaneRect {
                    x: 0,
                    y: 0,
                    width: 20,
                    height: 4,
                },
                file_name: "alpha.txt".to_string(),
                mode_label: "NORMAL".to_string(),
                cursor_style: ScreenCursorStyle::Block,
                dirty: false,
                lines: vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()],
                line_projections: vec![],
                cursor_row: 2,
                cursor_col: 1,
                visual_selection: None,
                search_overlays: vec![],
                syntax_chunks: vec![],
                markdown_style_ranges: vec![],
                resolved_theme: crate::theme::ResolvedTheme::default(),
                message_line: None,
                command_cursor_col: None,
                is_active: true,
            }],
            floats: vec![],
            active_window_id: 1,
            message_line: resolve_workspace_message_line(Vec::<MessageLineCandidate>::new()),
            message_area_height: 5,
            message_scroll_offset: 0,
            prompt_line: None,
            pager_prompt: None,
            suppressed_prompt_hints: vec![],
            bell: None,
            command_line: None,
        };

        draw_workspace_frame(&mut terminal, &model, true, RenderTextMode::StyledTrueColor)
            .expect("workspace render should succeed");

        let rendered = format!("{}", terminal.backend());
        let rows: Vec<&str> = rendered.lines().collect();
        assert!(
            rows.get(3)
                .is_some_and(|row| row.contains("alpha.txt") && row.contains("NORMAL")),
            "message/command がない時は最下段まで local status line を使うこと: {:?}",
            rows
        );
    }

    fn assert_normal_redraw_removes_stale_message_area_after_dismiss(dismiss_key: &str) {
        let mut terminal =
            Terminal::new(TestBackend::new(20, 8)).expect("test terminal should initialize");
        let mut model = workspace_with_typed_message(Some("one\ntwo\nthree\nfour\nfive"));
        model.panes[0].lines = vec!["alpha".to_string()];

        draw_workspace_frame(&mut terminal, &model, true, RenderTextMode::StyledTrueColor)
            .expect("initial workspace render should succeed");
        let with_message = format!("{}", terminal.backend());
        assert!(
            with_message.contains("four") && with_message.contains("five"),
            "initial render should draw message area: {:?}",
            with_message
        );

        model.message_line = resolve_workspace_message_line(Vec::<MessageLineCandidate>::new());
        model.message_scroll_offset = 0;

        draw_workspace_frame(
            &mut terminal,
            &model,
            false,
            RenderTextMode::StyledTrueColor,
        )
        .expect("dismissed workspace render should succeed");

        let dismissed = format!("{}", terminal.backend());
        assert!(
            !dismissed.contains("four") && !dismissed.contains("five"),
            "normal redraw after {dismiss_key} dismissal must erase stale message area rows without terminal.clear: {:?}",
            dismissed
        );
        assert!(
            dismissed.contains("test.txt") && dismissed.contains("NORMAL"),
            "{dismiss_key} dismissed workspace should reclaim the bottom rows for the buffer/status area: {:?}",
            dismissed
        );
    }

    #[test]
    fn workspace_render_normal_redraw_removes_stale_message_area_after_enter_dismiss() {
        assert_normal_redraw_removes_stale_message_area_after_dismiss("Enter");
    }

    #[test]
    fn workspace_render_normal_redraw_removes_stale_message_area_after_escape_dismiss() {
        assert_normal_redraw_removes_stale_message_area_after_dismiss("Escape");
    }

    #[test]
    fn workspace_render_uses_single_bottom_row_for_command_line_without_message() {
        let mut terminal =
            Terminal::new(TestBackend::new(20, 4)).expect("test terminal should initialize");
        let model = WorkspaceScreenModel {
            panes: vec![ScreenModel {
                window_id: 1,
                buffer_id: 1,
                rect: PaneRect {
                    x: 0,
                    y: 0,
                    width: 20,
                    height: 3,
                },
                file_name: "alpha.txt".to_string(),
                mode_label: "NORMAL".to_string(),
                cursor_style: ScreenCursorStyle::Block,
                dirty: false,
                lines: vec!["alpha".to_string(), "beta".to_string()],
                line_projections: vec![],
                cursor_row: 0,
                cursor_col: 0,
                visual_selection: None,
                search_overlays: vec![],
                syntax_chunks: vec![],
                markdown_style_ranges: vec![],
                resolved_theme: crate::theme::ResolvedTheme::default(),
                message_line: None,
                command_cursor_col: None,
                is_active: true,
            }],
            floats: vec![],
            active_window_id: 1,
            message_line: resolve_workspace_message_line(Vec::<MessageLineCandidate>::new()),
            message_area_height: 5,
            message_scroll_offset: 0,
            prompt_line: None,
            pager_prompt: None,
            suppressed_prompt_hints: vec![],
            bell: None,
            command_line: Some(CommandLineModel {
                text: ":w".to_string(),
                cursor_col: 2,
            }),
        };

        draw_workspace_frame(&mut terminal, &model, true, RenderTextMode::StyledTrueColor)
            .expect("workspace render should succeed");

        let rendered = format!("{}", terminal.backend());
        let rows: Vec<&str> = rendered.lines().collect();
        assert!(
            rows.get(2)
                .is_some_and(|row| row.contains("alpha.txt") && row.contains("NORMAL")),
            "command line だけの時は status line の直下 1 行だけを予約すること: {:?}",
            rows
        );
        assert!(
            rows.get(3).is_some_and(|row| row.contains(":w")),
            "最下段に command line を描画すること: {:?}",
            rows
        );
    }

    #[test]
    fn command_line_overlay_update_appends_without_clearing_current_line() {
        let previous = CommandLineModel {
            text: ":syntax o".to_string(),
            cursor_col: 9,
        };
        let next = CommandLineModel {
            text: ":syntax on".to_string(),
            cursor_col: 10,
        };

        let update = command_line_overlay_update(Some(&previous), &next);

        assert_eq!(
            update,
            CommandLineOverlayUpdate {
                start_col: 9,
                cursor_col: 10,
                text: "n".to_string(),
                clear_current_line_first: false,
                clear_after_text: false,
            }
        );
    }

    #[test]
    fn command_line_overlay_update_clears_tail_only_when_text_shrinks() {
        let previous = CommandLineModel {
            text: ":syntax on".to_string(),
            cursor_col: 10,
        };
        let next = CommandLineModel {
            text: ":syntax o".to_string(),
            cursor_col: 9,
        };

        let update = command_line_overlay_update(Some(&previous), &next);

        assert_eq!(
            update,
            CommandLineOverlayUpdate {
                start_col: 9,
                cursor_col: 9,
                text: String::new(),
                clear_current_line_first: false,
                clear_after_text: true,
            }
        );
    }

    #[test]
    fn command_line_overlay_update_clears_current_line_only_for_first_overlay() {
        let next = CommandLineModel {
            text: ":".to_string(),
            cursor_col: 1,
        };

        let update = command_line_overlay_update(None, &next);

        assert_eq!(
            update,
            CommandLineOverlayUpdate {
                start_col: 0,
                cursor_col: 1,
                text: ":".to_string(),
                clear_current_line_first: true,
                clear_after_text: false,
            }
        );
    }

    #[test]
    fn workspace_layout_exposes_no_global_rows_when_message_and_command_are_absent() {
        let model = workspace_with_typed_message(None);

        let layout = compute_workspace_layout(
            Rect {
                x: 0,
                y: 0,
                width: 20,
                height: 4,
            },
            &model,
        );

        assert_eq!(layout.message_rect, None);
        assert_eq!(layout.command_rect, None);
        assert_eq!(layout.panes[0].rect.height, 3);
        assert_eq!(layout.cursor, Some((0, 0)));
    }

    #[test]
    fn workspace_layout_exposes_single_command_row_without_empty_message_row() {
        let mut pane = screen_model_with_message(None);
        pane.rect.height = 3;
        let model = WorkspaceScreenModel {
            panes: vec![pane],
            floats: vec![],
            active_window_id: 1,
            message_line: resolve_workspace_message_line(Vec::<MessageLineCandidate>::new()),
            message_area_height: 5,
            message_scroll_offset: 0,
            prompt_line: None,
            pager_prompt: None,
            suppressed_prompt_hints: vec![],
            bell: None,
            command_line: Some(CommandLineModel {
                text: ":w".to_string(),
                cursor_col: 2,
            }),
        };

        let layout = compute_workspace_layout(
            Rect {
                x: 0,
                y: 0,
                width: 20,
                height: 4,
            },
            &model,
        );

        assert_eq!(layout.message_rect, None);
        assert_eq!(
            layout.command_rect,
            Some(Rect {
                x: 0,
                y: 3,
                width: 20,
                height: 1,
            })
        );
        assert_eq!(layout.panes[0].rect.height, 3);
        assert_eq!(layout.cursor, None);
    }

    #[test]
    fn workspace_layout_does_not_reserve_row_for_empty_global_message() {
        let model = workspace_with_typed_message(Some(""));

        let layout = compute_workspace_layout(
            Rect {
                x: 0,
                y: 0,
                width: 20,
                height: 4,
            },
            &model,
        );

        assert_eq!(
            layout.message_rect, None,
            "空の message line では global row を予約しないこと"
        );
        assert_eq!(
            layout.panes[0].rect.height, 3,
            "空 message で pane body/status の高さを削らないこと"
        );
    }

    #[test]
    fn workspace_layout_stacks_message_above_command_without_overlap() {
        let mut model = workspace_with_typed_message(Some("saved"));
        model.command_line = Some(CommandLineModel {
            text: ":w".to_string(),
            cursor_col: 2,
        });

        let layout = compute_workspace_layout(
            Rect {
                x: 0,
                y: 0,
                width: 20,
                height: 5,
            },
            &model,
        );

        assert_eq!(
            layout.message_rect,
            Some(Rect {
                x: 0,
                y: 3,
                width: 20,
                height: 1,
            })
        );
        assert_eq!(
            layout.command_rect,
            Some(Rect {
                x: 0,
                y: 4,
                width: 20,
                height: 1,
            })
        );
        assert_eq!(layout.panes[0].rect.height, 3);
    }

    #[test]
    fn workspace_render_draws_message_prompt_pager_and_bell_rows_without_suppressed_hints() {
        let mut terminal =
            Terminal::new(TestBackend::new(32, 6)).expect("test terminal should initialize");
        let mut model = workspace_with_typed_message(Some("saved"));
        model.pager_prompt = Some(PagerPromptView {
            kind: CorePagerPromptKind::More,
            one_shot: true,
        });
        model.prompt_line = Some(InputPromptView {
            prompt: "Name:".to_string(),
            input: "abc".to_string(),
            correlation_id: 7,
            input_kind: CoreInputRequestKind::CommandLine,
            status: InputPromptStatus::Active,
        });
        model.suppressed_prompt_hints = vec![SuppressedPromptHint {
            pager_prompt: PagerPromptView {
                kind: CorePagerPromptKind::HitReturn,
                one_shot: true,
            },
            reason: PromptHintSuppressionReason::ActiveInputPrompt,
        }];
        model.bell = Some(BellIndication { count: 2 });

        draw_workspace_frame(&mut terminal, &model, true, RenderTextMode::StyledTrueColor)
            .expect("workspace render should succeed");

        let rendered = format!("{}", terminal.backend());
        let rows: Vec<&str> = rendered.lines().collect();
        assert!(
            rows.iter()
                .any(|row| row.contains("saved") && row.contains("[bell x2]")),
            "message row should include both visible message and bell marker: {:?}",
            rows
        );
        assert!(
            rows.iter().any(|row| row.contains("[pager: More]")),
            "pager hint should render as its own row: {:?}",
            rows
        );
        assert!(
            rows.iter().any(|row| row.contains("Name: abc")),
            "prompt line should render as its own row: {:?}",
            rows
        );
        assert!(
            !rows.iter().any(|row| row.contains("Confirm")),
            "suppressed prompt hints must stay headless-only: {:?}",
            rows
        );
    }

    #[test]
    fn workspace_layout_saturates_prompt_rows_on_small_terminal_without_overlap() {
        let mut model = workspace_with_typed_message(Some("saved"));
        model.pager_prompt = Some(PagerPromptView {
            kind: CorePagerPromptKind::More,
            one_shot: true,
        });
        model.prompt_line = Some(InputPromptView {
            prompt: "Name:".to_string(),
            input: "abc".to_string(),
            correlation_id: 7,
            input_kind: CoreInputRequestKind::CommandLine,
            status: InputPromptStatus::Active,
        });
        model.command_line = Some(CommandLineModel {
            text: ":w".to_string(),
            cursor_col: 2,
        });

        let layout = compute_workspace_layout(
            Rect {
                x: 0,
                y: 0,
                width: 20,
                height: 2,
            },
            &model,
        );

        let mut rows = Vec::new();
        rows.extend(layout.message_rect.map(|rect| rect.y));
        rows.extend(layout.pager_rect.map(|rect| rect.y));
        rows.extend(layout.prompt_rect.map(|rect| rect.y));
        rows.extend(layout.command_rect.map(|rect| rect.y));
        rows.sort_unstable();
        rows.dedup();

        assert_eq!(layout.command_rect.map(|rect| rect.y), Some(1));
        assert_eq!(layout.panes[0].rect.height, 1);
        assert_eq!(
            rows.len(),
            1,
            "small terminal must not overlap reserved rows"
        );
    }
}
