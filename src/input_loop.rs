use crate::event_loop::{EventSender, UiEvent};
use crate::input_router::KeyInput;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
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

        log::debug!(
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
            log::debug!("[input_loop] poll timed out without event");
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
                    log::debug!("[input_loop] forwarding key event: {:?}", key);
                    if sender.blocking_send(UiEvent::Input(key)).is_err() {
                        log::debug!("[input_loop] receiver closed while sending key event");
                        break;
                    }
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
            other => {
                log::debug!("[input_loop] ignoring unsupported event: {:?}", other);
            }
        }
    }

    log::debug!("[input_loop] input thread finished");
}

fn map_key_input(key_event: KeyEvent) -> Option<KeyInput> {
    match key_event.code {
        KeyCode::Char(c) => {
            if key_event.modifiers.contains(KeyModifiers::CONTROL) {
                Some(KeyInput::Ctrl(c))
            } else {
                Some(KeyInput::Char(c))
            }
        }
        KeyCode::Esc => Some(KeyInput::Escape),
        KeyCode::Enter => Some(KeyInput::Enter),
        KeyCode::Backspace => Some(KeyInput::Backspace),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState};
    use tokio::sync::mpsc;

    struct MockEventSource {
        polls: Vec<io::Result<bool>>,
        reads: Vec<io::Result<Event>>,
    }

    impl MockEventSource {
        fn new(polls: Vec<io::Result<bool>>, reads: Vec<io::Result<Event>>) -> Self {
            Self { polls, reads }
        }
    }

    impl TerminalEventSource for MockEventSource {
        fn poll(&mut self, _timeout: Duration) -> io::Result<bool> {
            if self.polls.is_empty() {
                return Ok(false);
            }
            self.polls.remove(0)
        }

        fn read(&mut self) -> io::Result<Event> {
            self.reads.remove(0)
        }
    }

    fn ctrl_char_event(ch: char) -> Event {
        Event::Key(KeyEvent {
            code: KeyCode::Char(ch),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        })
    }

    #[test]
    fn input_loop_exits_when_stop_requested_during_idle_polling() {
        let (sender, _receiver) = mpsc::channel(4);
        let stop_requested = Arc::new(AtomicBool::new(true));
        let mut source = MockEventSource::new(Vec::new(), Vec::new());

        run_terminal_input_loop(&mut source, sender, stop_requested);
    }

    #[test]
    fn input_loop_forwards_supported_keys_and_resize_events() {
        let (sender, mut receiver) = mpsc::channel(4);
        let stop_requested = Arc::new(AtomicBool::new(false));
        let mut source = MockEventSource::new(
            vec![Ok(true), Ok(true), Ok(false), Ok(false)],
            vec![Ok(ctrl_char_event('c')), Ok(Event::Resize(120, 40))],
        );

        let stop_for_thread = stop_requested.clone();
        let handle = std::thread::spawn(move || {
            run_terminal_input_loop(&mut source, sender, stop_for_thread);
        });

        let first = receiver.blocking_recv().expect("first event");
        let second = receiver.blocking_recv().expect("second event");
        stop_requested.store(true, Ordering::Relaxed);
        handle.join().expect("input loop thread should stop");

        assert_eq!(first, UiEvent::Input(KeyInput::Ctrl('c')));
        assert_eq!(
            second,
            UiEvent::Resize {
                columns: 120,
                rows: 40
            }
        );
    }

    #[test]
    fn input_loop_exits_when_receiver_is_closed() {
        let (sender, receiver) = mpsc::channel(4);
        drop(receiver);

        let stop_requested = Arc::new(AtomicBool::new(false));
        let mut source = MockEventSource::new(vec![Ok(true)], vec![Ok(ctrl_char_event('x'))]);

        run_terminal_input_loop(&mut source, sender, stop_requested);
    }
}
