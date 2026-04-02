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
    let row = u16::try_from(index).unwrap_or(u16::MAX);
    let overlays = collect_render_overlays(model, row, line);
    if overlays.is_empty() {
        return pad_line_to_width(Line::from(line.to_string()), width);
    }

    render_layered_line(line, &overlays, width)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenderOverlayKind {
    VisualSelection,
    Search(crate::search_query::SearchMatchKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RenderOverlayRange {
    start_col: usize,
    end_col_exclusive: usize,
    kind: RenderOverlayKind,
}

fn collect_render_overlays(model: &ScreenModel, row: u16, line: &str) -> Vec<RenderOverlayRange> {
    let mut overlays = Vec::new();

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
            overlay_kind_rank(overlay.kind),
        )
    });
    overlays
}

fn render_layered_line(line: &str, overlays: &[RenderOverlayRange], width: u16) -> Line<'static> {
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
            .max_by_key(|overlay| overlay_kind_rank(overlay.kind))
            .map(|overlay| style_for_overlay_kind(overlay.kind))
            .unwrap_or_default();
        if style == Style::default() {
            spans.push(Span::raw(text));
        } else {
            spans.push(Span::styled(text, style));
        }
    }

    pad_line_to_width(Line::from(spans), width)
}

fn overlay_kind_rank(kind: RenderOverlayKind) -> usize {
    match kind {
        RenderOverlayKind::VisualSelection => 3,
        RenderOverlayKind::Search(crate::search_query::SearchMatchKind::Current) => 2,
        RenderOverlayKind::Search(crate::search_query::SearchMatchKind::Incremental) => 1,
        RenderOverlayKind::Search(crate::search_query::SearchMatchKind::Regular) => 0,
    }
}

fn style_for_overlay_kind(kind: RenderOverlayKind) -> Style {
    match kind {
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
    use crate::bootstrap::prepare_launch;
    use crate::cli::LaunchRequest;
    use crate::editor_session::EditorSessionState;
    use crate::screen_model::ScreenSelection;
    use crate::screen_model::{ProjectionInput, ScreenSearchOverlay, project};
    use crate::search_query::SearchMatchKind;
    use crate::session_guard::test_lock as session_test_lock;
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
            search_overlays: vec![],
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
    fn search_overlay_precedence_prefers_current_over_incremental_and_regular() {
        let model = ScreenModel {
            file_name: "test.txt".to_string(),
            mode_label: "NORMAL".to_string(),
            dirty: false,
            lines: vec!["abcdef".to_string()],
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
            message_line: None,
            command_cursor_col: None,
        };

        let text = render_buffer_text(&model, 6);
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
    fn visual_selection_overrides_search_overlay_when_ranges_overlap() {
        let model = ScreenModel {
            file_name: "test.txt".to_string(),
            mode_label: "VISUAL".to_string(),
            dirty: false,
            lines: vec!["abcdef".to_string()],
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
            message_line: None,
            command_cursor_col: None,
        };

        let text = render_buffer_text(&model, 6);
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
            file_name: "test.txt".to_string(),
            mode_label: "NORMAL".to_string(),
            dirty: false,
            lines: vec!["xあx".to_string()],
            cursor_row: 0,
            cursor_col: 0,
            visual_selection: None,
            search_overlays: vec![ScreenSearchOverlay {
                row: 0,
                start_col: 1,
                end_col_exclusive: 3,
                kind: SearchMatchKind::Regular,
            }],
            message_line: None,
            command_cursor_col: None,
        };

        let text = render_buffer_text(&model, 6);
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
            search_overlays: vec![],
            message_line: None,
            command_cursor_col: None,
        };

        let text = render_buffer_text(&model, 20);
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
}
