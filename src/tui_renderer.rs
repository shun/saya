use crate::screen_model::ScreenModel;
use crate::terminal_lifecycle::TerminalBackend;
use crossterm::{execute, terminal};
use ratatui::Terminal;
use ratatui::prelude::*;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Clear, Paragraph};
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
    needs_full_clear: bool,
}

impl TuiRenderer {
    pub fn new() -> io::Result<Self> {
        let backend = CrosstermBackend::new(io::stdout());
        let terminal = Terminal::new(backend)?;
        Ok(Self {
            terminal,
            needs_full_clear: true,
        })
    }

    pub fn draw(&mut self, model: &ScreenModel) -> io::Result<()> {
        draw_editor_frame(&mut self.terminal, model, self.needs_full_clear)?;
        self.needs_full_clear = false;
        Ok(())
    }
}

fn draw_editor_frame<B: Backend>(
    terminal: &mut Terminal<B>,
    model: &ScreenModel,
    force_full_clear: bool,
) -> io::Result<()> {
    if force_full_clear {
        terminal.clear()?;
    }
    terminal.draw(|f| render_editor_frame(f, model))?;
    Ok(())
}

fn render_editor_frame(f: &mut Frame<'_>, model: &ScreenModel) {
    let size = f.area();
    f.render_widget(Clear, size);

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

    let buffer_content = Paragraph::new(render_buffer_text(model, layout[0].width));
    trace_renderer_line(model, layout[0].width);
    f.render_widget(buffer_content, layout[0]);

    let status_bar = Paragraph::new(render_status_line(model))
        .style(Style::default().bg(Color::White).fg(Color::Black));
    f.render_widget(status_bar, layout[1]);
    f.render_widget(Paragraph::new(render_message_line(model)), layout[2]);

    if let Some(col) = model.command_cursor_col {
        f.set_cursor_position((col, layout[2].y));
    } else if model.cursor_row < layout[0].height {
        f.set_cursor_position((model.cursor_col, model.cursor_row));
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

fn render_buffer_text(model: &ScreenModel, width: u16) -> Text<'static> {
    let lines = model
        .lines
        .iter()
        .enumerate()
        .map(|(index, line)| render_line(model, index, line, width))
        .collect::<Vec<_>>();
    Text::from(lines)
}

fn trace_renderer_line(model: &ScreenModel, width: u16) {
    if std::env::var_os("SAYA_TRACE_RENDER").is_none() {
        return;
    }

    let line = model.lines.get(6).map(String::as_str).unwrap_or("");
    eprintln!(
        "[saya-trace][renderer] body_width={} rel_row=7 line={line:?}",
        width
    );
}

fn render_line(model: &ScreenModel, index: usize, line: &str, width: u16) -> Line<'static> {
    let Some(selection) = model.visual_selection else {
        return pad_line_to_width(Line::from(line.to_string()), width);
    };

    let row = u16::try_from(index).unwrap_or(u16::MAX);
    if row < selection.start_row || row > selection.end_row {
        return pad_line_to_width(Line::from(line.to_string()), width);
    }

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

    let (prefix, selected, suffix) =
        split_line_by_display_columns(line, start_col, end_col_exclusive);
    pad_line_to_width(
        Line::from(vec![
            Span::raw(prefix),
            Span::styled(selected, Style::default().add_modifier(Modifier::REVERSED)),
            Span::raw(suffix),
        ]),
        width,
    )
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
    use crate::bootstrap::prepare_launch;
    use crate::cli::LaunchRequest;
    use crate::editor_session::EditorSessionState;
    use crate::screen_model::ScreenSelection;
    use crate::screen_model::{ProjectionInput, project};
    use ratatui::backend::TestBackend;
    use ratatui::layout::Position;

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
                line_start_col: 0,
                end_row: 0,
                end_col_exclusive: 1,
            }),
            message_line: message_line.map(ToString::to_string),
            command_cursor_col: None,
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
    fn multiline_selection_does_not_highlight_line_number_gutter() {
        let model = ScreenModel {
            file_name: "test.txt".to_string(),
            mode_label: "V-LINE".to_string(),
            dirty: false,
            lines: vec![" 1 alpha".to_string(), " 2 beta".to_string()],
            cursor_row: 1,
            cursor_col: 3,
            visual_selection: Some(ScreenSelection {
                start_row: 0,
                start_col: 3,
                line_start_col: 3,
                end_row: 1,
                end_col_exclusive: 7,
            }),
            message_line: None,
            command_cursor_col: None,
        };

        let text = render_buffer_text(&model, 20);
        let second_line = &text.lines[1];

        assert_eq!(second_line.spans.len(), 4);
        assert_eq!(second_line.spans[0].content.as_ref(), " 2 ");
        assert_eq!(second_line.spans[1].content.as_ref(), "beta");
        assert_eq!(second_line.spans[2].content.as_ref(), "");
        assert!(
            second_line.spans[3]
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
    fn integrated_update_cycle_keeps_message_status_and_cursor_in_sync() {
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
}
