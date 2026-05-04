use crate::input_router::KeyInput;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandLineHistoryDirection {
    Previous,
    Next,
}

#[derive(Debug, Clone, Default)]
pub struct CommandLineHistory {
    entries: Vec<String>,
    cursor: Option<usize>,
    draft: Option<String>,
    filter_prefix: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CommandLineHistories {
    ex_commands: CommandLineHistory,
    searches: CommandLineHistory,
}

impl CommandLineHistories {
    pub fn record(&mut self, prompt: char, buffer: &str) {
        let line = format!("{prompt}{buffer}");
        match prompt {
            ':' => self.ex_commands.record(&line),
            '/' => self.searches.record(&line),
            _ => {
                log::debug!(
                    "[command_line_history] skipped unsupported prompt history record: prompt={}",
                    prompt
                );
            }
        }
    }

    pub fn navigate(
        &mut self,
        prompt: char,
        buffer: &str,
        direction: CommandLineHistoryDirection,
    ) -> Option<String> {
        let current_line = format!("{prompt}{buffer}");
        let selected = match (prompt, direction) {
            (':', CommandLineHistoryDirection::Previous) => {
                self.ex_commands.previous(&current_line)
            }
            (':', CommandLineHistoryDirection::Next) => self.ex_commands.next(&current_line),
            ('/', CommandLineHistoryDirection::Previous) => self.searches.previous(&current_line),
            ('/', CommandLineHistoryDirection::Next) => self.searches.next(&current_line),
            _ => None,
        }?;
        let selected_buffer = selected
            .strip_prefix(prompt)
            .unwrap_or(selected)
            .to_string();
        log::debug!(
            "[command_line_history] applied history selection: prompt={}, direction={:?}, selected_len={}, buffer_len={}",
            prompt,
            direction,
            selected.len(),
            selected_buffer.len()
        );
        Some(selected_buffer)
    }

    pub fn reset_navigation(&mut self) {
        self.ex_commands.reset_navigation();
        self.searches.reset_navigation();
    }
}

impl CommandLineHistory {
    pub fn record(&mut self, line: &str) {
        let line = line.trim_end_matches(['\r', '\n']);
        if line.len() <= 1 {
            log::debug!(
                "[command_line_history] skipped empty history entry: line_len={}",
                line.len()
            );
            self.reset_navigation();
            return;
        }

        if self.entries.last().is_some_and(|entry| entry == line) {
            log::debug!(
                "[command_line_history] skipped adjacent duplicate history entry: line_len={}, entry_count={}",
                line.len(),
                self.entries.len()
            );
            self.reset_navigation();
            return;
        }

        self.entries.push(line.to_string());
        log::debug!(
            "[command_line_history] recorded history entry: line_len={}, entry_count={}",
            line.len(),
            self.entries.len()
        );
        self.reset_navigation();
    }

    pub fn previous<'a>(&'a mut self, current_line: &str) -> Option<&'a str> {
        if self.entries.is_empty() {
            log::debug!("[command_line_history] previous ignored because history is empty");
            return None;
        }

        if self.cursor.is_none() {
            self.draft = Some(current_line.to_string());
            self.filter_prefix = Some(current_line.to_string());
        }

        let prefix = self.filter_prefix.as_deref().unwrap_or(current_line);
        let mut candidate = match self.cursor {
            Some(0) => None,
            Some(cursor) => cursor.checked_sub(1),
            None => {
                if self.entries.is_empty() {
                    None
                } else {
                    Some(self.entries.len() - 1)
                }
            }
        };

        while let Some(index) = candidate {
            if self.entries[index].starts_with(prefix) {
                self.cursor = Some(index);
                log::debug!(
                    "[command_line_history] previous selected entry: cursor={}, entry_count={}, prefix_len={}, current_len={}",
                    index,
                    self.entries.len(),
                    prefix.len(),
                    current_line.len()
                );
                return Some(self.entries[index].as_str());
            }
            candidate = index.checked_sub(1);
        }

        log::debug!(
            "[command_line_history] previous stopped before matching entry: entry_count={}, prefix_len={}, current_len={}",
            self.entries.len(),
            prefix.len(),
            current_line.len()
        );
        None
    }

    pub fn next<'a>(&'a mut self, current_line: &str) -> Option<&'a str> {
        let Some(cursor) = self.cursor else {
            log::debug!("[command_line_history] next ignored because navigation is inactive");
            return None;
        };

        let prefix = self.filter_prefix.as_deref().unwrap_or_default();
        let mut candidate = cursor + 1;
        while candidate < self.entries.len() {
            if self.entries[candidate].starts_with(prefix) {
                self.cursor = Some(candidate);
                log::debug!(
                    "[command_line_history] next selected entry: cursor={}, entry_count={}, prefix_len={}, current_len={}",
                    candidate,
                    self.entries.len(),
                    prefix.len(),
                    current_line.len()
                );
                return Some(self.entries[candidate].as_str());
            }
            candidate += 1;
        }

        self.cursor = None;
        let draft = self.draft.get_or_insert_with(String::new);
        log::debug!(
            "[command_line_history] next restored draft: draft_len={}, current_len={}",
            draft.len(),
            current_line.len()
        );
        Some(draft.as_str())
    }

    pub fn reset_navigation(&mut self) {
        self.cursor = None;
        self.draft = None;
        self.filter_prefix = None;
    }
}

pub fn history_direction_for_key(key: &KeyInput) -> Option<CommandLineHistoryDirection> {
    match key {
        KeyInput::Up | KeyInput::Ctrl('p') | KeyInput::Ctrl('P') => {
            Some(CommandLineHistoryDirection::Previous)
        }
        KeyInput::Down | KeyInput::Ctrl('n') | KeyInput::Ctrl('N') => {
            Some(CommandLineHistoryDirection::Next)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vim_style_history_keys_map_to_navigation_directions() {
        assert_eq!(
            history_direction_for_key(&KeyInput::Up),
            Some(CommandLineHistoryDirection::Previous)
        );
        assert_eq!(
            history_direction_for_key(&KeyInput::Ctrl('p')),
            Some(CommandLineHistoryDirection::Previous)
        );
        assert_eq!(
            history_direction_for_key(&KeyInput::Down),
            Some(CommandLineHistoryDirection::Next)
        );
        assert_eq!(
            history_direction_for_key(&KeyInput::Ctrl('n')),
            Some(CommandLineHistoryDirection::Next)
        );
        assert_eq!(history_direction_for_key(&KeyInput::Char('p')), None);
    }
}
