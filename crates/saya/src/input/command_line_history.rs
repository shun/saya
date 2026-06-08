use crate::input::router::KeyInput;
use crate::support::paths;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const COMMAND_LINE_HISTORY_FILE_NAME: &str = "command-line-history.json";

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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PersistedCommandLineHistories {
    #[serde(default)]
    ex_commands: Vec<String>,
    #[serde(default)]
    searches: Vec<String>,
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

    fn from_persisted(persisted: PersistedCommandLineHistories) -> Self {
        let mut histories = Self::default();
        for entry in persisted.ex_commands {
            histories.ex_commands.record(&entry);
        }
        for entry in persisted.searches {
            histories.searches.record(&entry);
        }
        histories.reset_navigation();
        log::debug!(
            "[command_line_history] restored histories from persisted entries: ex_count={}, search_count={}",
            histories.ex_commands.entries.len(),
            histories.searches.entries.len()
        );
        histories
    }

    fn to_persisted(&self) -> PersistedCommandLineHistories {
        PersistedCommandLineHistories {
            ex_commands: self.ex_commands.entries.clone(),
            searches: self.searches.entries.clone(),
        }
    }
}

pub fn default_history_path() -> Option<PathBuf> {
    paths::cache_dir().map(|dir| dir.join(COMMAND_LINE_HISTORY_FILE_NAME))
}

pub fn load_histories_from_default_cache() -> CommandLineHistories {
    let Some(path) = default_history_path() else {
        log::debug!("[command_line_history] cache path unavailable; starting with empty histories");
        return CommandLineHistories::default();
    };

    match load_histories_from_path(&path) {
        Ok(histories) => histories,
        Err(error) => {
            log::debug!(
                "[command_line_history] failed to load cache; starting with empty histories: path={}, error={}",
                path.display(),
                error
            );
            CommandLineHistories::default()
        }
    }
}

pub fn save_histories_to_default_cache(histories: &CommandLineHistories) {
    let Some(path) = default_history_path() else {
        log::debug!("[command_line_history] cache path unavailable; skipped history save");
        return;
    };

    if let Err(error) = save_histories_to_path(histories, &path) {
        log::debug!(
            "[command_line_history] failed to save cache: path={}, error={}",
            path.display(),
            error
        );
    }
}

pub fn record_history_and_save_to_default_cache(
    histories: &mut CommandLineHistories,
    prompt: char,
    buffer: &str,
) {
    let Some(path) = default_history_path() else {
        log::debug!(
            "[command_line_history] cache path unavailable; recording in memory only: prompt={}, buffer_len={}",
            prompt,
            buffer.len()
        );
        histories.record(prompt, buffer);
        return;
    };

    if let Err(error) = record_history_and_save_to_path(histories, prompt, buffer, &path) {
        log::debug!(
            "[command_line_history] failed to persist recorded history entry: path={}, prompt={}, buffer_len={}, error={}",
            path.display(),
            prompt,
            buffer.len(),
            error
        );
    }
}

pub fn record_history_and_save_to_path(
    histories: &mut CommandLineHistories,
    prompt: char,
    buffer: &str,
    path: &Path,
) -> io::Result<()> {
    log::debug!(
        "[command_line_history] recording history entry before cache save: path={}, prompt={}, buffer_len={}",
        path.display(),
        prompt,
        buffer.len()
    );
    histories.record(prompt, buffer);
    save_histories_to_path(histories, path)
}

pub fn load_histories_from_path(path: &Path) -> io::Result<CommandLineHistories> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            log::debug!(
                "[command_line_history] cache file missing; starting empty: path={}",
                path.display()
            );
            return Ok(CommandLineHistories::default());
        }
        Err(error) => return Err(error),
    };

    let persisted: PersistedCommandLineHistories =
        serde_json::from_str(&content).map_err(io::Error::other)?;
    log::debug!(
        "[command_line_history] loaded cache file: path={}, bytes={}",
        path.display(),
        content.len()
    );
    Ok(CommandLineHistories::from_persisted(persisted))
}

pub fn save_histories_to_path(histories: &CommandLineHistories, path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let encoded =
        serde_json::to_string_pretty(&histories.to_persisted()).map_err(io::Error::other)?;
    fs::write(path, encoded.as_bytes())?;
    log::debug!(
        "[command_line_history] saved cache file: path={}, bytes={}, ex_count={}, search_count={}",
        path.display(),
        encoded.len(),
        histories.ex_commands.entries.len(),
        histories.searches.entries.len()
    );
    Ok(())
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
