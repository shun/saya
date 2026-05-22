use crate::app::event_loop::{EventSender, UiEvent};
use crate::input::router::{KeyInput, NavigationKey};
use crate::terminal::emulator::{TerminalEmulator, TerminalScreenSnapshot, Vt100TerminalEmulator};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalFloatCloseBehavior {
    KillOnClose,
    DetachOnClose,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalFloatSpawnRequest {
    pub command: String,
    pub args: Vec<String>,
    pub width: u16,
    pub height: u16,
    pub close_behavior: TerminalFloatCloseBehavior,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalFloatError {
    EmptyCommand,
    SpawnFailed(String),
    WriteFailed(String),
    MissingTerminal { terminal_id: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalViewportState {
    pub width: u16,
    pub height: u16,
    pub scrollback_offset: u16,
}

#[derive(Default)]
pub struct TerminalFloatManager {
    next_id: u64,
    sessions: BTreeMap<u64, TerminalFloatSession>,
    redraw_sender: Option<EventSender>,
}

struct TerminalFloatSession {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    output: Receiver<Vec<u8>>,
    _reader_thread: JoinHandle<()>,
    emulator: Box<dyn TerminalEmulator + Send>,
    viewport: TerminalViewportState,
    close_behavior: TerminalFloatCloseBehavior,
    detached: bool,
}

impl TerminalFloatManager {
    pub fn set_redraw_sender(&mut self, sender: EventSender) {
        self.redraw_sender = Some(sender);
    }

    pub fn spawn(&mut self, request: TerminalFloatSpawnRequest) -> Result<u64, TerminalFloatError> {
        if request.command.trim().is_empty() {
            return Err(TerminalFloatError::EmptyCommand);
        }
        let terminal_id = self.allocate_id();
        let width = request.width.max(1);
        let height = request.height.max(1);
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: height,
                cols: width,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| TerminalFloatError::SpawnFailed(error.to_string()))?;
        let mut command = CommandBuilder::new(request.command.clone());
        command.args(request.args.iter().map(String::as_str));
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| TerminalFloatError::SpawnFailed(error.to_string()))?;
        drop(pair.slave);
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| TerminalFloatError::SpawnFailed(error.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| TerminalFloatError::SpawnFailed(error.to_string()))?;
        let (sender, receiver) = mpsc::channel();
        let redraw_sender = self.redraw_sender.clone();
        let reader_thread = std::thread::spawn(move || {
            let mut buffer = [0_u8; 4096];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => {
                        if sender.send(buffer[..read].to_vec()).is_err() {
                            break;
                        }
                        if let Some(redraw_sender) = redraw_sender.as_ref() {
                            let _ = redraw_sender.try_send(UiEvent::Redraw);
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
        });
        self.sessions.insert(
            terminal_id,
            TerminalFloatSession {
                child,
                master: pair.master,
                writer,
                output: receiver,
                _reader_thread: reader_thread,
                emulator: Box::new(Vt100TerminalEmulator::new(width, height, 2000)),
                viewport: TerminalViewportState {
                    width,
                    height,
                    scrollback_offset: 0,
                },
                close_behavior: request.close_behavior,
                detached: false,
            },
        );
        log::debug!(
            "[terminal_float] spawned PTY terminal session: terminal_id={}, command={}, args={:?}, size=({},{}), close_behavior={:?}",
            terminal_id,
            request.command,
            request.args,
            width,
            height,
            request.close_behavior
        );
        Ok(terminal_id)
    }

    pub fn drain(&mut self) {
        for (terminal_id, session) in &mut self.sessions {
            let mut chunks = 0_u64;
            let mut bytes = 0_usize;
            while let Ok(chunk) = session.output.try_recv() {
                bytes = bytes.saturating_add(chunk.len());
                chunks = chunks.saturating_add(1);
                session.emulator.feed(&chunk);
            }
            if chunks > 0 {
                log::debug!(
                    "[terminal_float] drained PTY output: terminal_id={}, chunks={}, bytes={}",
                    terminal_id,
                    chunks,
                    bytes
                );
            }
        }
    }

    pub fn rendered_lines(&self, terminal_id: u64) -> Vec<String> {
        let Some(session) = self.sessions.get(&terminal_id) else {
            return Vec::new();
        };
        let mut lines = session.emulator.screen().rendered_lines();
        if lines.is_empty() {
            lines.push(String::new());
        }
        let visible_height = usize::from(session.viewport.height.max(1));
        lines.into_iter().take(visible_height).collect()
    }

    pub fn cursor_position(&self, terminal_id: u64) -> Option<(u16, u16)> {
        self.sessions.get(&terminal_id).map(|session| {
            let cursor = session.emulator.cursor();
            (cursor.row, cursor.col)
        })
    }

    pub fn screen_snapshot(&self, terminal_id: u64) -> Option<TerminalScreenSnapshot> {
        self.sessions
            .get(&terminal_id)
            .map(|session| session.emulator.screen())
    }

    pub fn resize(
        &mut self,
        terminal_id: u64,
        width: u16,
        height: u16,
    ) -> Result<(), TerminalFloatError> {
        let session = self
            .sessions
            .get_mut(&terminal_id)
            .ok_or(TerminalFloatError::MissingTerminal { terminal_id })?;
        let width = width.max(1);
        let height = height.max(1);
        if session.viewport.width == width && session.viewport.height == height {
            return Ok(());
        }
        session
            .master
            .resize(PtySize {
                rows: height,
                cols: width,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| TerminalFloatError::WriteFailed(error.to_string()))?;
        session.emulator.resize(width, height);
        session.viewport.width = width;
        session.viewport.height = height;
        session.viewport.scrollback_offset = 0;
        log::debug!(
            "[terminal_float] resized PTY terminal session: terminal_id={}, size=({},{})",
            terminal_id,
            width,
            height
        );
        Ok(())
    }

    pub fn write_key(
        &mut self,
        terminal_id: u64,
        key: &KeyInput,
    ) -> Result<(), TerminalFloatError> {
        let bytes = terminal_key_bytes(key);
        self.write_bytes(terminal_id, bytes.as_bytes())
    }

    pub fn write_bytes(
        &mut self,
        terminal_id: u64,
        bytes: &[u8],
    ) -> Result<(), TerminalFloatError> {
        let session = self
            .sessions
            .get_mut(&terminal_id)
            .ok_or(TerminalFloatError::MissingTerminal { terminal_id })?;
        session
            .writer
            .write_all(bytes)
            .and_then(|_| session.writer.flush())
            .map_err(|error| TerminalFloatError::WriteFailed(error.to_string()))?;
        session.viewport.scrollback_offset = 0;
        log::debug!(
            "[terminal_float] wrote input to PTY terminal: terminal_id={}, bytes={}",
            terminal_id,
            bytes.len()
        );
        Ok(())
    }

    pub fn scroll(&mut self, terminal_id: u64, delta: i32) -> Result<(), TerminalFloatError> {
        let session = self
            .sessions
            .get_mut(&terminal_id)
            .ok_or(TerminalFloatError::MissingTerminal { terminal_id })?;
        let next = i32::try_from(session.emulator.scrollback())
            .unwrap_or(i32::MAX)
            .saturating_add(delta)
            .max(0);
        session
            .emulator
            .set_scrollback(usize::try_from(next).unwrap_or(usize::MAX));
        session.viewport.scrollback_offset =
            u16::try_from(session.emulator.scrollback()).unwrap_or(u16::MAX);
        log::debug!(
            "[terminal_float] terminal viewport scrolled: terminal_id={}, delta={}, offset={}",
            terminal_id,
            delta,
            session.viewport.scrollback_offset
        );
        Ok(())
    }

    pub fn close_view(&mut self, terminal_id: u64) -> Result<(), TerminalFloatError> {
        let close_behavior = self
            .sessions
            .get(&terminal_id)
            .ok_or(TerminalFloatError::MissingTerminal { terminal_id })?
            .close_behavior;
        match close_behavior {
            TerminalFloatCloseBehavior::KillOnClose => self.kill(terminal_id),
            TerminalFloatCloseBehavior::DetachOnClose => {
                if let Some(session) = self.sessions.get_mut(&terminal_id) {
                    session.detached = true;
                }
                log::debug!(
                    "[terminal_float] detached terminal float view without killing PTY: terminal_id={}",
                    terminal_id
                );
                Ok(())
            }
        }
    }

    pub fn kill(&mut self, terminal_id: u64) -> Result<(), TerminalFloatError> {
        let mut session = self
            .sessions
            .remove(&terminal_id)
            .ok_or(TerminalFloatError::MissingTerminal { terminal_id })?;
        match session.child.try_wait() {
            Ok(Some(status)) => {
                log::debug!(
                    "[terminal_float] terminal already exited before kill: terminal_id={}, status={:?}",
                    terminal_id,
                    status
                );
            }
            Ok(None) => {
                session
                    .child
                    .kill()
                    .map_err(|error| TerminalFloatError::WriteFailed(error.to_string()))?;
                log::debug!(
                    "[terminal_float] killed PTY terminal session: terminal_id={}",
                    terminal_id
                );
            }
            Err(error) => {
                log::debug!(
                    "[terminal_float] terminal status check failed before kill: terminal_id={}, error={}",
                    terminal_id,
                    error
                );
            }
        }
        Ok(())
    }

    pub fn is_alive(&mut self, terminal_id: u64) -> bool {
        let Some(session) = self.sessions.get_mut(&terminal_id) else {
            return false;
        };
        matches!(session.child.try_wait(), Ok(None))
    }

    pub fn is_detached(&self, terminal_id: u64) -> bool {
        self.sessions
            .get(&terminal_id)
            .is_some_and(|session| session.detached)
    }

    fn allocate_id(&mut self) -> u64 {
        self.next_id = self.next_id.saturating_add(1);
        self.next_id
    }
}

fn terminal_key_bytes(key: &KeyInput) -> String {
    match key {
        KeyInput::Char(ch) => ch.to_string(),
        KeyInput::Ctrl(ch) => String::from(((*ch as u8) & 0x1f) as char),
        KeyInput::Tab => "\t".to_string(),
        KeyInput::BackTab => "\x1b[Z".to_string(),
        KeyInput::Left => "\x1b[D".to_string(),
        KeyInput::Right => "\x1b[C".to_string(),
        KeyInput::Up => "\x1b[A".to_string(),
        KeyInput::Down => "\x1b[B".to_string(),
        KeyInput::Home => "\x1b[H".to_string(),
        KeyInput::End => "\x1b[F".to_string(),
        KeyInput::PageUp => "\x1b[5~".to_string(),
        KeyInput::PageDown => "\x1b[6~".to_string(),
        KeyInput::Delete => "\x1b[3~".to_string(),
        KeyInput::Insert => "\x1b[2~".to_string(),
        KeyInput::Escape => "\x1b".to_string(),
        KeyInput::Enter => "\r".to_string(),
        KeyInput::ShiftEnter => "\x1b[13;2u".to_string(),
        KeyInput::Backspace => "\x08".to_string(),
        KeyInput::F(number) => function_key_sequence(*number).to_string(),
        KeyInput::Alt(ch) => format!("\x1b{ch}"),
        KeyInput::ShiftedNav(nav) => modified_navigation_sequence(*nav, 2).to_string(),
        KeyInput::CtrlNav(nav) => modified_navigation_sequence(*nav, 5).to_string(),
    }
}

fn function_key_sequence(number: u8) -> &'static str {
    match number {
        1 => "\x1bOP",
        2 => "\x1bOQ",
        3 => "\x1bOR",
        4 => "\x1bOS",
        5 => "\x1b[15~",
        6 => "\x1b[17~",
        7 => "\x1b[18~",
        8 => "\x1b[19~",
        9 => "\x1b[20~",
        10 => "\x1b[21~",
        11 => "\x1b[23~",
        12 => "\x1b[24~",
        _ => "",
    }
}

fn modified_navigation_sequence(nav: NavigationKey, modifier: u8) -> &'static str {
    match (nav, modifier) {
        (NavigationKey::Up, 2) => "\x1b[1;2A",
        (NavigationKey::Down, 2) => "\x1b[1;2B",
        (NavigationKey::Right, 2) => "\x1b[1;2C",
        (NavigationKey::Left, 2) => "\x1b[1;2D",
        (NavigationKey::Home, 2) => "\x1b[1;2H",
        (NavigationKey::End, 2) => "\x1b[1;2F",
        (NavigationKey::PageUp, 2) => "\x1b[5;2~",
        (NavigationKey::PageDown, 2) => "\x1b[6;2~",
        (NavigationKey::Up, 5) => "\x1b[1;5A",
        (NavigationKey::Down, 5) => "\x1b[1;5B",
        (NavigationKey::Right, 5) => "\x1b[1;5C",
        (NavigationKey::Left, 5) => "\x1b[1;5D",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_key_bytes_preserves_shift_enter_as_modified_enter_sequence() {
        assert_eq!(terminal_key_bytes(&KeyInput::ShiftEnter), "\x1b[13;2u");
    }
}
