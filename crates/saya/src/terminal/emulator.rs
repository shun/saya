use crate::presentation::floating_window::{FloatingInlineStyle, FloatingInlineStyleKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TerminalColor {
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TerminalCellStyle {
    pub foreground: Option<TerminalColor>,
    pub background: Option<TerminalColor>,
    pub bold: bool,
    pub underline: bool,
    pub inverse: bool,
}

impl TerminalCellStyle {
    pub fn is_default(self) -> bool {
        self == Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalCell {
    pub text: String,
    pub style: TerminalCellStyle,
}

impl Default for TerminalCell {
    fn default() -> Self {
        Self {
            text: " ".to_string(),
            style: TerminalCellStyle::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalCursor {
    pub row: u16,
    pub col: u16,
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalScreenSnapshot {
    pub rows: Vec<Vec<TerminalCell>>,
    pub cursor_row: u16,
    pub cursor_col: u16,
    pub cursor_visible: bool,
}

impl TerminalScreenSnapshot {
    pub fn rendered_lines(&self) -> Vec<String> {
        self.rows
            .iter()
            .map(|row| row.iter().map(|cell| cell.text.as_str()).collect())
            .collect()
    }

    pub fn inline_styles(&self) -> Vec<FloatingInlineStyle> {
        let mut styles = Vec::new();
        for (line, row) in self.rows.iter().enumerate() {
            let mut byte_column = 0usize;
            let mut segment_start: Option<usize> = None;
            let mut segment_style = TerminalCellStyle::default();
            for cell in row {
                let cell_start = byte_column;
                byte_column = byte_column.saturating_add(cell.text.len());
                if cell.style.is_default() {
                    if let Some(start) = segment_start.take() {
                        styles.push(FloatingInlineStyle {
                            kind: FloatingInlineStyleKind::TerminalCell(segment_style),
                            line,
                            column_start: start,
                            column_end: cell_start,
                        });
                    }
                    continue;
                }
                if segment_start.is_some() && cell.style == segment_style {
                    continue;
                }
                if let Some(start) = segment_start.replace(cell_start) {
                    styles.push(FloatingInlineStyle {
                        kind: FloatingInlineStyleKind::TerminalCell(segment_style),
                        line,
                        column_start: start,
                        column_end: cell_start,
                    });
                }
                segment_style = cell.style;
            }
            if let Some(start) = segment_start {
                styles.push(FloatingInlineStyle {
                    kind: FloatingInlineStyleKind::TerminalCell(segment_style),
                    line,
                    column_start: start,
                    column_end: byte_column,
                });
            }
        }
        styles
    }
}

pub trait TerminalEmulator {
    fn feed(&mut self, bytes: &[u8]);
    fn resize(&mut self, cols: u16, rows: u16);
    fn screen(&self) -> TerminalScreenSnapshot;
    fn cursor(&self) -> TerminalCursor;
    fn set_scrollback(&mut self, rows: usize);
    fn scrollback(&self) -> usize;
}

pub struct Vt100TerminalEmulator {
    parser: vt100::Parser,
}

impl Vt100TerminalEmulator {
    pub fn new(cols: u16, rows: u16, scrollback_len: usize) -> Self {
        let cols = cols.max(1);
        let rows = rows.max(1);
        Self {
            parser: vt100::Parser::new(rows, cols, scrollback_len),
        }
    }
}

impl TerminalEmulator for Vt100TerminalEmulator {
    fn feed(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    fn resize(&mut self, cols: u16, rows: u16) {
        self.parser.set_size(rows.max(1), cols.max(1));
    }

    fn screen(&self) -> TerminalScreenSnapshot {
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();
        let mut snapshot_rows = Vec::with_capacity(usize::from(rows));
        for row in 0..rows {
            let mut snapshot_row = Vec::with_capacity(usize::from(cols));
            for col in 0..cols {
                let cell = screen
                    .cell(row, col)
                    .map_or_else(TerminalCell::default, |cell| {
                        if cell.is_wide_continuation() {
                            TerminalCell {
                                text: String::new(),
                                style: TerminalCellStyle::default(),
                            }
                        } else {
                            TerminalCell {
                                text: if cell.has_contents() {
                                    cell.contents()
                                } else {
                                    " ".to_string()
                                },
                                style: TerminalCellStyle {
                                    foreground: terminal_color(cell.fgcolor()),
                                    background: terminal_color(cell.bgcolor()),
                                    bold: cell.bold(),
                                    underline: cell.underline(),
                                    inverse: cell.inverse(),
                                },
                            }
                        }
                    });
                snapshot_row.push(cell);
            }
            snapshot_rows.push(snapshot_row);
        }
        let cursor = self.cursor();
        TerminalScreenSnapshot {
            rows: snapshot_rows,
            cursor_row: cursor.row,
            cursor_col: cursor.col,
            cursor_visible: cursor.visible,
        }
    }

    fn cursor(&self) -> TerminalCursor {
        let (row, col) = self.parser.screen().cursor_position();
        TerminalCursor {
            row,
            col,
            visible: !self.parser.screen().hide_cursor(),
        }
    }

    fn set_scrollback(&mut self, rows: usize) {
        self.parser.set_scrollback(rows);
    }

    fn scrollback(&self) -> usize {
        self.parser.screen().scrollback()
    }
}

fn terminal_color(color: vt100::Color) -> Option<TerminalColor> {
    match color {
        vt100::Color::Default => None,
        vt100::Color::Idx(index) => Some(TerminalColor::Indexed(index)),
        vt100::Color::Rgb(red, green, blue) => Some(TerminalColor::Rgb(red, green, blue)),
    }
}
