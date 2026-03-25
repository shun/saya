use crate::screen_model::ScreenModel;
use crate::terminal_lifecycle::TerminalBackend;
use crossterm::{execute, terminal};
use ratatui::prelude::*;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;
use std::io::{self, Stdout};
use unicode_width::UnicodeWidthChar;

pub struct CrosstermBackendImpl;

impl TerminalBackend for CrosstermBackendImpl {
    fn enable_raw_mode(&mut self) -> io::Result<()> {
        terminal::enable_raw_mode()
    }

    fn enter_alternate_screen(&mut self) -> io::Result<()> {
        execute!(io::stdout(), terminal::EnterAlternateScreen)
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
}

impl TuiRenderer {
    pub fn new() -> io::Result<Self> {
        let backend = CrosstermBackend::new(io::stdout());
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal })
    }

    pub fn draw(&mut self, model: &ScreenModel) -> io::Result<()> {
        self.terminal.draw(|f| {
            let size = f.area();

            let layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints(
                    [
                        Constraint::Min(1),
                        Constraint::Length(1),
                        Constraint::Length(1),
                    ]
                    .as_ref(),
                )
                .split(size);

            let buffer_content = Paragraph::new(render_buffer_text(model));
            f.render_widget(buffer_content, layout[0]);

            let status_bar = Paragraph::new(render_status_line(model))
                .style(Style::default().bg(Color::White).fg(Color::Black));
            f.render_widget(status_bar, layout[1]);
            f.render_widget(Paragraph::new(render_message_line(model)), layout[2]);

            // Set cursor
            if model.cursor_row < layout[0].height {
                f.set_cursor_position((model.cursor_col, model.cursor_row));
            }
        })?;
        Ok(())
    }
}

fn render_status_line(model: &ScreenModel) -> String {
    let status = format!("{} | {}", model.file_name, model.mode_label);
    if model.dirty {
        format!("{status} [+]!")
    } else {
        status
    }
}

fn render_message_line(model: &ScreenModel) -> &str {
    model.message_line.as_deref().unwrap_or("")
}

fn render_buffer_text(model: &ScreenModel) -> Text<'static> {
    let lines = model
        .lines
        .iter()
        .enumerate()
        .map(|(index, line)| render_line(model, index, line))
        .collect::<Vec<_>>();
    Text::from(lines)
}

fn render_line(model: &ScreenModel, index: usize, line: &str) -> Line<'static> {
    let Some(selection) = model.visual_selection else {
        return Line::from(line.to_string());
    };

    let row = u16::try_from(index).unwrap_or(u16::MAX);
    if row < selection.start_row || row > selection.end_row {
        return Line::from(line.to_string());
    }

    let start_col = if row == selection.start_row {
        usize::from(selection.start_col)
    } else {
        0
    };
    let end_col_exclusive = if row == selection.end_row {
        usize::from(selection.end_col_exclusive)
    } else {
        display_width(line)
    };

    let (prefix, selected, suffix) =
        split_line_by_display_columns(line, start_col, end_col_exclusive);
    Line::from(vec![
        Span::raw(prefix),
        Span::styled(selected, Style::default().add_modifier(Modifier::REVERSED)),
        Span::raw(suffix),
    ])
}

fn split_line_by_display_columns(
    line: &str,
    start_col: usize,
    end_col_exclusive: usize,
) -> (String, String, String) {
    let mut prefix = String::new();
    let mut selected = String::new();
    let mut suffix = String::new();
    let mut display_col = 0usize;

    for ch in line.chars() {
        let width = ch.width().unwrap_or(0);
        let target = if display_col < start_col {
            &mut prefix
        } else if display_col < end_col_exclusive {
            &mut selected
        } else {
            &mut suffix
        };
        target.push(ch);
        display_col = display_col.saturating_add(width);
    }

    (prefix, selected, suffix)
}

fn display_width(text: &str) -> usize {
    text.chars().map(|ch| ch.width().unwrap_or(0)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen_model::ScreenSelection;

    fn screen_model_with_message(message_line: Option<&str>) -> ScreenModel {
        ScreenModel {
            file_name: "test.txt".to_string(),
            mode_label: "NORMAL".to_string(),
            dirty: true,
            lines: vec!["hello".to_string()],
            cursor_row: 0,
            cursor_col: 0,
            visual_selection: Some(ScreenSelection {
                start_row: 0,
                start_col: 0,
                end_row: 0,
                end_col_exclusive: 1,
            }),
            message_line: message_line.map(ToString::to_string),
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
}
