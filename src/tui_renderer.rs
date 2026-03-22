use crate::screen_model::ScreenModel;
use crate::terminal_lifecycle::TerminalBackend;
use crossterm::{execute, terminal};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use std::io::{self, Stdout};

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
                .constraints([Constraint::Min(1), Constraint::Length(1)].as_ref())
                .split(size);

            let text: String = model.lines.join("\n");
            let buffer_content = Paragraph::new(text);
            f.render_widget(buffer_content, layout[0]);

            let status_msg = if let Some(msg) = &model.status_message {
                format!("{} | {} | {}", model.file_name, model.mode_label, msg)
            } else {
                format!("{} | {}", model.file_name, model.mode_label)
            };
            let status_msg = if model.dirty {
                format!("{} [+]!", status_msg)
            } else {
                status_msg
            };

            let status_bar = Paragraph::new(status_msg)
                .style(Style::default().bg(Color::White).fg(Color::Black));
            f.render_widget(status_bar, layout[1]);

            // Set cursor
            if model.cursor_row < layout[0].height {
                f.set_cursor_position((model.cursor_col, model.cursor_row));
            }
        })?;
        Ok(())
    }
}
