use crate::input_router::{KeyInput, NavigationKey};
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
}

struct TerminalFloatSession {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    _master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    output: Receiver<Vec<u8>>,
    _reader_thread: JoinHandle<()>,
    parser: vt100::Parser,
    viewport: TerminalViewportState,
    close_behavior: TerminalFloatCloseBehavior,
    detached: bool,
}

impl TerminalFloatManager {
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
        let reader_thread = std::thread::spawn(move || {
            let mut buffer = [0_u8; 4096];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => {
                        if sender.send(buffer[..read].to_vec()).is_err() {
                            break;
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
                _master: pair.master,
                writer,
                output: receiver,
                _reader_thread: reader_thread,
                parser: vt100::Parser::new(height, width, 2000),
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
                session.parser.process(&chunk);
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
        let visible_height = usize::from(session.viewport.height.max(1));
        let mut lines = session
            .parser
            .screen()
            .contents()
            .lines()
            .map(str::to_string)
            .collect::<Vec<_>>();
        if lines.is_empty() {
            lines.push(String::new());
        }
        let max_offset = lines.len().saturating_sub(visible_height);
        let offset = usize::from(session.viewport.scrollback_offset).min(max_offset);
        let start = lines
            .len()
            .saturating_sub(visible_height)
            .saturating_sub(offset);
        lines.into_iter().skip(start).take(visible_height).collect()
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
        let visible_height = usize::from(session.viewport.height.max(1));
        let line_count = session.parser.screen().contents().lines().count();
        let max_offset = line_count.saturating_sub(visible_height);
        let next = i32::from(session.viewport.scrollback_offset)
            .saturating_add(delta)
            .clamp(0, i32::try_from(max_offset).unwrap_or(i32::MAX));
        session.viewport.scrollback_offset = u16::try_from(next).unwrap_or(u16::MAX);
        log::debug!(
            "[terminal_float] terminal viewport scrolled: terminal_id={}, delta={}, offset={}, max_offset={}",
            terminal_id,
            delta,
            session.viewport.scrollback_offset,
            max_offset
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
