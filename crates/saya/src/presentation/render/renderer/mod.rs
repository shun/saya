//! TUI レンダラ本体。
//!
//! WorkspaceScreenModel を入力として端末へフレームを描画する。
//! バックエンド型・TuiRenderer・ワークスペースフレーム描画とレイアウト計算を
//! 本モジュールが持ち、メッセージ欄・ペイン描画・行レンダリング・
//! スタイル解決は子モジュールへ分割している。

use crate::presentation::floating_window::{
    FloatingBorder, FloatingInlineStyle, FloatingInlineStyleKind, FloatingScreenModel,
};
#[cfg(test)]
use crate::presentation::screen_model::PaneRect;
use crate::presentation::screen_model::{
    CommandLineModel, ScreenCursorStyle, ScreenModel, ScreenSyntaxCategory, ScreenSyntaxModifier,
    ScreenTreeSitterSyntax, WorkspaceScreenModel,
};
use crate::presentation::theme::{
    ResolvedTextStyle, ResolvedTheme, ResolvedThemeColor, SyntaxSemanticStyleKey, UiStyleKey,
};
use crate::terminal::emulator::{TerminalCellStyle, TerminalColor};
use crate::terminal::lifecycle::TerminalBackend;
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

    fn enable_keyboard_enhancement(&mut self) -> io::Result<()> {
        execute!(
            io::stdout(),
            event::PushKeyboardEnhancementFlags(
                event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | event::KeyboardEnhancementFlags::REPORT_EVENT_TYPES
            )
        )
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

    fn disable_keyboard_enhancement(&mut self) -> io::Result<()> {
        execute!(io::stdout(), event::PopKeyboardEnhancementFlags)
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
) -> Result<(), B::Error> {
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
    previous_no_color: Option<std::ffi::OsString>,
}

impl CrosstermColorOutputGuard {
    fn for_text_mode(text_mode: RenderTextMode) -> Self {
        let previous_no_color = std::env::var_os("NO_COLOR");
        let should_force_color = colors_enabled(text_mode)
            && previous_no_color
                .as_ref()
                .is_some_and(|value| !value.is_empty());
        if should_force_color {
            log::debug!(
                "[tui_renderer] forcing crossterm color output for highlighted frame: text_mode={text_mode:?}, no_color_present=true"
            );
            log::debug!(
                "[saya-trace][renderer][color] force_color_output=true text_mode={text_mode:?} no_color_present=true"
            );
            style::force_color_output(true);
            unsafe {
                std::env::remove_var("NO_COLOR");
            }
        } else {
            log::debug!(
                "[tui_renderer] using crossterm color output default for frame: text_mode={text_mode:?}, no_color_present={}",
                previous_no_color
                    .as_ref()
                    .is_some_and(|value| !value.is_empty())
            );
        }
        Self {
            previous_no_color: if should_force_color {
                previous_no_color
            } else {
                None
            },
        }
    }
}

impl Drop for CrosstermColorOutputGuard {
    fn drop(&mut self) {
        if let Some(previous_no_color) = self.previous_no_color.as_ref() {
            log::debug!(
                "[tui_renderer] restoring crossterm NO_COLOR color suppression after frame"
            );
            log::debug!(
                "[saya-trace][renderer][color] force_color_output=false restore_no_color=true"
            );
            style::force_color_output(false);
            unsafe {
                std::env::set_var("NO_COLOR", previous_no_color);
            }
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

    if let Some((message_area, message_rect)) =
        message_area_text(model, size.width).zip(layout.message_rect)
    {
        f.render_widget(
            Paragraph::new(message_area).style(message_area_style(model, theme, text_mode)),
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

    let float_cursor = render_floats(f, &model.floats, text_mode, theme);

    if let Some((cursor_x, cursor_y)) = command_cursor {
        f.set_cursor_position((cursor_x, cursor_y));
        return;
    }

    if let Some((cursor_x, cursor_y)) = float_cursor {
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
) -> Option<(u16, u16)> {
    let mut sorted = floats.iter().collect::<Vec<_>>();
    sorted.sort_by_key(|float| (float.zindex, float.creation_order));
    let mut cursor = None;

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
        let base_style = ui_style(theme, UiStyleKey::Message, text_mode);
        let text = Text::from(
            float
                .lines
                .iter()
                .enumerate()
                .map(|(line_index, line)| {
                    Line::from(line_to_styled_spans(line, line_index, &float.inline_styles))
                })
                .collect::<Vec<_>>(),
        );
        // Block の `.style(base_style)` で内側の空セルを base_style で
        // 塗りつぶしつつ、`.border_style(...)` で border 文字には背景色
        // だけを base_style から引き継ぎ、前景色は端末 default のままに
        // する。これで base_style の前景が背景と同色になる theme でも
        // border 文字（│┌┐└┘─）が確実に視認できる。
        let border_only_style = match base_style.bg {
            Some(bg) => Style::default().bg(bg),
            None => Style::default(),
        };
        let paragraph = match float.chrome.border {
            FloatingBorder::None => Paragraph::new(text).style(base_style),
            FloatingBorder::Single => Paragraph::new(text).style(base_style).block(
                Block::bordered()
                    .style(base_style)
                    .border_style(border_only_style),
            ),
        };
        f.render_widget(paragraph, rect);
        if let Some(float_cursor) = float.cursor {
            let content_origin_x = rect.x
                + match float.chrome.border {
                    FloatingBorder::None => 0,
                    FloatingBorder::Single => 1,
                };
            let content_origin_y = rect.y
                + match float.chrome.border {
                    FloatingBorder::None => 0,
                    FloatingBorder::Single => 1,
                };
            cursor = Some((
                content_origin_x
                    .saturating_add(u16::try_from(float_cursor.column).unwrap_or(u16::MAX)),
                content_origin_y
                    .saturating_add(u16::try_from(float_cursor.line).unwrap_or(u16::MAX)),
            ));
        }
    }
    cursor
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
    let global_rows = workspace_global_rows(model, size.width);
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
        Some(message_area_row_count(model, size.width)).filter(|height| *height > 0),
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

fn workspace_global_rows(model: &WorkspaceScreenModel, width: u16) -> u16 {
    message_area_row_count(model, width)
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

mod line_render;
mod message_area;
mod pane;
mod style_resolve;

use line_render::*;
use message_area::*;
use pane::*;
use style_resolve::*;

#[cfg(test)]
mod tests;
