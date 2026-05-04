use crate::input_router::KeyInput;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandLineEditAction {
    MoveLeft,
    MoveRight,
    MoveStart,
    MoveEnd,
    Backspace,
    Delete,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandLineEdit {
    buffer: String,
    cursor_byte_index: usize,
}

impl CommandLineEdit {
    pub fn from_buffer(buffer: &str) -> Self {
        Self {
            buffer: buffer.to_string(),
            cursor_byte_index: buffer.len(),
        }
    }

    pub fn buffer(&self) -> &str {
        &self.buffer
    }

    pub fn cursor_byte_index(&self) -> usize {
        self.cursor_byte_index
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        self.cursor_byte_index = 0;
        log::debug!("[command_line_editor] cleared command line edit buffer");
    }

    pub fn set_buffer_to_end(&mut self, buffer: String) {
        self.cursor_byte_index = buffer.len();
        self.buffer = buffer;
        log::debug!(
            "[command_line_editor] replaced buffer and moved cursor to end: buffer_len={}, cursor_byte_index={}",
            self.buffer.len(),
            self.cursor_byte_index
        );
    }

    pub fn insert_char(&mut self, ch: char) {
        self.buffer.insert(self.cursor_byte_index, ch);
        self.cursor_byte_index += ch.len_utf8();
        log::debug!(
            "[command_line_editor] inserted char: char_width_bytes={}, buffer_len={}, cursor_byte_index={}",
            ch.len_utf8(),
            self.buffer.len(),
            self.cursor_byte_index
        );
    }

    pub fn backspace(&mut self) -> bool {
        let Some(previous) = previous_char_boundary(&self.buffer, self.cursor_byte_index) else {
            log::debug!("[command_line_editor] backspace ignored at start of buffer");
            return false;
        };
        self.buffer.drain(previous..self.cursor_byte_index);
        self.cursor_byte_index = previous;
        log::debug!(
            "[command_line_editor] backspace removed char: buffer_len={}, cursor_byte_index={}",
            self.buffer.len(),
            self.cursor_byte_index
        );
        true
    }

    pub fn delete(&mut self) -> bool {
        if self.cursor_byte_index >= self.buffer.len() {
            log::debug!("[command_line_editor] delete ignored at end of buffer");
            return false;
        }
        let next =
            next_char_boundary(&self.buffer, self.cursor_byte_index).unwrap_or(self.buffer.len());
        self.buffer.drain(self.cursor_byte_index..next);
        log::debug!(
            "[command_line_editor] delete removed char: buffer_len={}, cursor_byte_index={}",
            self.buffer.len(),
            self.cursor_byte_index
        );
        true
    }

    pub fn move_left(&mut self) -> bool {
        let Some(previous) = previous_char_boundary(&self.buffer, self.cursor_byte_index) else {
            log::debug!("[command_line_editor] move left ignored at start of buffer");
            return false;
        };
        self.cursor_byte_index = previous;
        log::debug!(
            "[command_line_editor] moved cursor left: cursor_byte_index={}",
            self.cursor_byte_index
        );
        true
    }

    pub fn move_right(&mut self) -> bool {
        let Some(next) = next_char_boundary(&self.buffer, self.cursor_byte_index) else {
            log::debug!("[command_line_editor] move right ignored at end of buffer");
            return false;
        };
        self.cursor_byte_index = next;
        log::debug!(
            "[command_line_editor] moved cursor right: cursor_byte_index={}",
            self.cursor_byte_index
        );
        true
    }

    pub fn move_to_start(&mut self) -> bool {
        if self.cursor_byte_index == 0 {
            log::debug!("[command_line_editor] move start ignored at start of buffer");
            return false;
        }
        self.cursor_byte_index = 0;
        log::debug!("[command_line_editor] moved cursor to start");
        true
    }

    pub fn move_to_end(&mut self) -> bool {
        if self.cursor_byte_index == self.buffer.len() {
            log::debug!("[command_line_editor] move end ignored at end of buffer");
            return false;
        }
        self.cursor_byte_index = self.buffer.len();
        log::debug!(
            "[command_line_editor] moved cursor to end: cursor_byte_index={}",
            self.cursor_byte_index
        );
        true
    }

    pub fn apply_action(&mut self, action: CommandLineEditAction) -> bool {
        match action {
            CommandLineEditAction::MoveLeft => self.move_left(),
            CommandLineEditAction::MoveRight => self.move_right(),
            CommandLineEditAction::MoveStart => self.move_to_start(),
            CommandLineEditAction::MoveEnd => self.move_to_end(),
            CommandLineEditAction::Backspace => self.backspace(),
            CommandLineEditAction::Delete => self.delete(),
        }
    }
}

pub fn command_line_edit_action_for_key(key: &KeyInput) -> Option<CommandLineEditAction> {
    match key {
        KeyInput::Left | KeyInput::Ctrl('b') | KeyInput::Ctrl('B') => {
            Some(CommandLineEditAction::MoveLeft)
        }
        KeyInput::Right | KeyInput::Ctrl('f') | KeyInput::Ctrl('F') => {
            Some(CommandLineEditAction::MoveRight)
        }
        KeyInput::Home | KeyInput::Ctrl('a') | KeyInput::Ctrl('A') => {
            Some(CommandLineEditAction::MoveStart)
        }
        KeyInput::End | KeyInput::Ctrl('e') | KeyInput::Ctrl('E') => {
            Some(CommandLineEditAction::MoveEnd)
        }
        KeyInput::Backspace => Some(CommandLineEditAction::Backspace),
        KeyInput::Delete => Some(CommandLineEditAction::Delete),
        _ => None,
    }
}

fn previous_char_boundary(buffer: &str, cursor_byte_index: usize) -> Option<usize> {
    buffer[..cursor_byte_index]
        .char_indices()
        .last()
        .map(|(index, _)| index)
}

fn next_char_boundary(buffer: &str, cursor_byte_index: usize) -> Option<usize> {
    buffer[cursor_byte_index..]
        .chars()
        .next()
        .map(|ch| cursor_byte_index + ch.len_utf8())
}
