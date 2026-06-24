use crate::app::event_loop::{EventSender, UiEvent};
use crate::input::router::{KeyInput, NavigationKey};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(50);

pub trait TerminalEventSource {
    fn poll(&mut self, timeout: Duration) -> io::Result<bool>;
    fn read(&mut self) -> io::Result<Event>;
}

pub struct CrosstermEventSource;

impl TerminalEventSource for CrosstermEventSource {
    fn poll(&mut self, timeout: Duration) -> io::Result<bool> {
        crossterm::event::poll(timeout)
    }

    fn read(&mut self) -> io::Result<Event> {
        crossterm::event::read()
    }
}

pub fn run_terminal_input_loop<S: TerminalEventSource>(
    source: &mut S,
    sender: EventSender,
    stop_requested: Arc<AtomicBool>,
) {
    log::debug!("[input_loop] input thread started");

    loop {
        if stop_requested.load(Ordering::Relaxed) {
            log::debug!("[input_loop] stop requested before poll");
            break;
        }

        log::trace!(
            "[input_loop] polling terminal events: timeout_ms={}",
            INPUT_POLL_INTERVAL.as_millis()
        );

        let event_ready = match source.poll(INPUT_POLL_INTERVAL) {
            Ok(ready) => ready,
            Err(error) => {
                log::debug!("[input_loop] poll failed: {}", error);
                break;
            }
        };

        if !event_ready {
            log::trace!("[input_loop] poll timed out without event");
            continue;
        }

        let event = match source.read() {
            Ok(event) => event,
            Err(error) => {
                log::debug!("[input_loop] read failed: {}", error);
                break;
            }
        };
        log::debug!("[input_loop] raw event: {:?}", event);

        match event {
            Event::Key(key_event) => {
                if let Some(key) = map_key_input(key_event) {
                    log::debug!(
                        "[input_loop] mapped terminal key to internal input: {:?}",
                        key
                    );
                    log::debug!("[input_loop] forwarding key event: {:?}", key);
                    if sender.blocking_send(UiEvent::Input(key)).is_err() {
                        log::debug!("[input_loop] receiver closed while sending key event");
                        break;
                    }
                } else {
                    log::debug!(
                        "[input_loop] ignoring unsupported or non-dispatchable key event: {:?}",
                        key_event
                    );
                }
            }
            Event::Resize(columns, rows) => {
                log::debug!(
                    "[input_loop] forwarding resize event: columns={}, rows={}",
                    columns,
                    rows
                );
                if sender
                    .blocking_send(UiEvent::Resize { columns, rows })
                    .is_err()
                {
                    log::debug!("[input_loop] receiver closed while sending resize event");
                    break;
                }
            }
            Event::Mouse(mouse_event) => match mouse_event.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    log::debug!(
                        "[input_loop] forwarding left mouse click: column={}, row={}",
                        mouse_event.column,
                        mouse_event.row
                    );
                    if sender
                        .blocking_send(UiEvent::MouseClick {
                            column: mouse_event.column,
                            row: mouse_event.row,
                        })
                        .is_err()
                    {
                        log::debug!("[input_loop] receiver closed while sending mouse event");
                        break;
                    }
                }
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                    let vertical_delta = match mouse_event.kind {
                        MouseEventKind::ScrollUp => -1,
                        MouseEventKind::ScrollDown => 1,
                        _ => 0,
                    };
                    let (delta_x, delta_y) = if mouse_event.modifiers.contains(KeyModifiers::SHIFT)
                    {
                        (vertical_delta, 0)
                    } else {
                        (0, vertical_delta)
                    };
                    log::debug!(
                        "[input_loop] forwarding mouse wheel: column={}, row={}, delta=({}, {})",
                        mouse_event.column,
                        mouse_event.row,
                        delta_x,
                        delta_y
                    );
                    if sender
                        .blocking_send(UiEvent::MouseWheel {
                            column: mouse_event.column,
                            row: mouse_event.row,
                            delta_x,
                            delta_y,
                        })
                        .is_err()
                    {
                        log::debug!("[input_loop] receiver closed while sending wheel event");
                        break;
                    }
                }
                _ => {}
            },
            Event::Paste(text) => {
                if !text.is_empty() {
                    log::debug!(
                        "[input_loop] forwarding pasted text event: chars={}",
                        text.chars().count()
                    );
                    if sender.blocking_send(UiEvent::PastedText(text)).is_err() {
                        log::debug!("[input_loop] receiver closed while sending paste event");
                        break;
                    }
                }
            }
            other => {
                log::debug!("[input_loop] ignoring unsupported event: {:?}", other);
            }
        }
    }

    log::debug!("[input_loop] input thread finished");
}

fn map_key_input(key_event: KeyEvent) -> Option<KeyInput> {
    if key_event.kind == KeyEventKind::Release {
        return None;
    }

    let modifiers = key_event.modifiers;
    match key_event.code {
        KeyCode::F(number @ 1..=12) => Some(KeyInput::F(number)),
        KeyCode::F(_) => None,
        KeyCode::Char(c) => {
            if modifiers.contains(KeyModifiers::ALT) && modifiers.contains(KeyModifiers::CONTROL) {
                None
            } else if modifiers.contains(KeyModifiers::ALT) {
                Some(KeyInput::Alt(c))
            } else if modifiers.contains(KeyModifiers::CONTROL) {
                Some(KeyInput::Ctrl(c))
            } else {
                Some(KeyInput::Char(c))
            }
        }
        KeyCode::Tab => Some(KeyInput::Tab),
        KeyCode::BackTab => Some(KeyInput::BackTab),
        KeyCode::Left => map_navigation_input(NavigationKey::Left, KeyInput::Left, modifiers),
        KeyCode::Right => map_navigation_input(NavigationKey::Right, KeyInput::Right, modifiers),
        KeyCode::Up => map_navigation_input(NavigationKey::Up, KeyInput::Up, modifiers),
        KeyCode::Down => map_navigation_input(NavigationKey::Down, KeyInput::Down, modifiers),
        KeyCode::Home => map_navigation_input(NavigationKey::Home, KeyInput::Home, modifiers),
        KeyCode::End => map_navigation_input(NavigationKey::End, KeyInput::End, modifiers),
        KeyCode::PageUp => map_navigation_input(NavigationKey::PageUp, KeyInput::PageUp, modifiers),
        KeyCode::PageDown => {
            map_navigation_input(NavigationKey::PageDown, KeyInput::PageDown, modifiers)
        }
        KeyCode::Delete => Some(KeyInput::Delete),
        KeyCode::Insert => Some(KeyInput::Insert),
        KeyCode::Esc => Some(KeyInput::Escape),
        KeyCode::Enter if modifiers == KeyModifiers::SHIFT => Some(KeyInput::ShiftEnter),
        KeyCode::Enter if modifiers == KeyModifiers::NONE => Some(KeyInput::Enter),
        KeyCode::Enter => None,
        KeyCode::Backspace => Some(KeyInput::Backspace),
        _ => None,
    }
}

fn map_navigation_input(
    navigation: NavigationKey,
    plain: KeyInput,
    modifiers: KeyModifiers,
) -> Option<KeyInput> {
    if modifiers == KeyModifiers::NONE {
        return Some(plain);
    }

    if modifiers == KeyModifiers::SHIFT {
        return Some(KeyInput::ShiftedNav(navigation));
    }

    if modifiers == KeyModifiers::CONTROL
        && matches!(
            navigation,
            NavigationKey::Left | NavigationKey::Right | NavigationKey::Up | NavigationKey::Down
        )
    {
        return Some(KeyInput::CtrlNav(navigation));
    }

    None
}

#[cfg(test)]
#[path = "input_loop_test.rs"]
mod tests;
