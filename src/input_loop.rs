use crate::event_loop::{EventSender, UiEvent};
use crate::input_router::{KeyInput, NavigationKey};
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
        KeyCode::Enter => Some(KeyInput::Enter),
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
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState, MouseButton, MouseEvent, MouseEventKind};
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

    fn key_event(code: KeyCode) -> Event {
        Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        })
    }

    fn key_event_with_modifiers(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn mouse_event(kind: MouseEventKind, column: u16, row: u16) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    }

    #[test]
    fn map_key_input_accepts_function_keys_one_through_twelve() {
        for number in 1..=12 {
            let mapped = map_key_input(key_event_with_modifiers(
                KeyCode::F(number),
                KeyModifiers::NONE,
            ));

            assert_eq!(mapped, Some(KeyInput::F(number)));
        }
    }

    #[test]
    fn map_key_input_rejects_unsupported_function_keys() {
        for number in [13, 24] {
            let mapped = map_key_input(key_event_with_modifiers(
                KeyCode::F(number),
                KeyModifiers::NONE,
            ));

            assert_eq!(mapped, None);
        }
    }

    #[test]
    fn map_key_input_accepts_supported_modified_navigation() {
        let cases = [
            (
                KeyCode::Up,
                KeyModifiers::SHIFT,
                KeyInput::ShiftedNav(NavigationKey::Up),
            ),
            (
                KeyCode::Down,
                KeyModifiers::SHIFT,
                KeyInput::ShiftedNav(NavigationKey::Down),
            ),
            (
                KeyCode::Right,
                KeyModifiers::SHIFT,
                KeyInput::ShiftedNav(NavigationKey::Right),
            ),
            (
                KeyCode::Left,
                KeyModifiers::SHIFT,
                KeyInput::ShiftedNav(NavigationKey::Left),
            ),
            (
                KeyCode::Home,
                KeyModifiers::SHIFT,
                KeyInput::ShiftedNav(NavigationKey::Home),
            ),
            (
                KeyCode::End,
                KeyModifiers::SHIFT,
                KeyInput::ShiftedNav(NavigationKey::End),
            ),
            (
                KeyCode::PageUp,
                KeyModifiers::SHIFT,
                KeyInput::ShiftedNav(NavigationKey::PageUp),
            ),
            (
                KeyCode::PageDown,
                KeyModifiers::SHIFT,
                KeyInput::ShiftedNav(NavigationKey::PageDown),
            ),
            (
                KeyCode::Up,
                KeyModifiers::CONTROL,
                KeyInput::CtrlNav(NavigationKey::Up),
            ),
            (
                KeyCode::Down,
                KeyModifiers::CONTROL,
                KeyInput::CtrlNav(NavigationKey::Down),
            ),
            (
                KeyCode::Right,
                KeyModifiers::CONTROL,
                KeyInput::CtrlNav(NavigationKey::Right),
            ),
            (
                KeyCode::Left,
                KeyModifiers::CONTROL,
                KeyInput::CtrlNav(NavigationKey::Left),
            ),
        ];

        for (code, modifiers, expected) in cases {
            let mapped = map_key_input(key_event_with_modifiers(code, modifiers));
            assert_eq!(mapped, Some(expected));
        }
    }

    #[test]
    fn map_key_input_rejects_unsupported_modified_navigation() {
        let cases = [
            (KeyCode::Left, KeyModifiers::SHIFT | KeyModifiers::CONTROL),
            (KeyCode::Home, KeyModifiers::CONTROL),
            (KeyCode::End, KeyModifiers::CONTROL),
            (KeyCode::PageUp, KeyModifiers::CONTROL),
            (KeyCode::PageDown, KeyModifiers::CONTROL),
        ];

        for (code, modifiers) in cases {
            let mapped = map_key_input(key_event_with_modifiers(code, modifiers));
            assert_eq!(mapped, None);
        }
    }

    #[test]
    fn map_key_input_accepts_alt_character_without_control() {
        let ascii = map_key_input(key_event_with_modifiers(
            KeyCode::Char('x'),
            KeyModifiers::ALT,
        ));
        let multibyte = map_key_input(key_event_with_modifiers(
            KeyCode::Char('あ'),
            KeyModifiers::ALT,
        ));

        assert_eq!(ascii, Some(KeyInput::Alt('x')));
        assert_eq!(multibyte, Some(KeyInput::Alt('あ')));
    }

    #[test]
    fn map_key_input_rejects_alt_control_character() {
        let mapped = map_key_input(key_event_with_modifiers(
            KeyCode::Char('x'),
            KeyModifiers::ALT | KeyModifiers::CONTROL,
        ));

        assert_eq!(mapped, None);
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
    fn input_loop_preserves_ctrl_w_prefix_and_following_key_as_separate_inputs() {
        let (sender, mut receiver) = mpsc::channel(4);
        let stop_requested = Arc::new(AtomicBool::new(false));
        let mut source = MockEventSource::new(
            vec![Ok(true), Ok(true), Ok(false), Ok(false)],
            vec![
                Ok(ctrl_char_event('w')),
                Ok(Event::Key(KeyEvent {
                    code: KeyCode::Char('s'),
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                })),
            ],
        );

        let stop_for_thread = stop_requested.clone();
        let handle = std::thread::spawn(move || {
            run_terminal_input_loop(&mut source, sender, stop_for_thread);
        });

        let first = receiver.blocking_recv().expect("first event");
        let second = receiver.blocking_recv().expect("second event");
        stop_requested.store(true, Ordering::Relaxed);
        handle.join().expect("input loop thread should stop");

        assert_eq!(first, UiEvent::Input(KeyInput::Ctrl('w')));
        assert_eq!(second, UiEvent::Input(KeyInput::Char('s')));
    }

    #[test]
    fn input_loop_forwards_common_terminal_navigation_keys() {
        let (sender, mut receiver) = mpsc::channel(16);
        let stop_requested = Arc::new(AtomicBool::new(false));
        let mut source = MockEventSource::new(
            vec![
                Ok(true),
                Ok(true),
                Ok(true),
                Ok(true),
                Ok(true),
                Ok(true),
                Ok(true),
                Ok(true),
                Ok(true),
                Ok(true),
                Ok(true),
                Ok(true),
            ],
            vec![
                Ok(key_event(KeyCode::Tab)),
                Ok(key_event(KeyCode::BackTab)),
                Ok(key_event(KeyCode::Left)),
                Ok(key_event(KeyCode::Right)),
                Ok(key_event(KeyCode::Up)),
                Ok(key_event(KeyCode::Down)),
                Ok(key_event(KeyCode::Home)),
                Ok(key_event(KeyCode::End)),
                Ok(key_event(KeyCode::PageUp)),
                Ok(key_event(KeyCode::PageDown)),
                Ok(key_event(KeyCode::Delete)),
                Ok(key_event(KeyCode::Insert)),
            ],
        );

        let stop_for_thread = stop_requested.clone();
        let handle = std::thread::spawn(move || {
            run_terminal_input_loop(&mut source, sender, stop_for_thread);
        });

        let mut forwarded = Vec::new();
        for _ in 0..12 {
            forwarded.push(receiver.blocking_recv().expect("navigation event"));
        }
        stop_requested.store(true, Ordering::Relaxed);
        handle.join().expect("input loop thread should stop");

        assert_eq!(
            forwarded,
            vec![
                UiEvent::Input(KeyInput::Tab),
                UiEvent::Input(KeyInput::BackTab),
                UiEvent::Input(KeyInput::Left),
                UiEvent::Input(KeyInput::Right),
                UiEvent::Input(KeyInput::Up),
                UiEvent::Input(KeyInput::Down),
                UiEvent::Input(KeyInput::Home),
                UiEvent::Input(KeyInput::End),
                UiEvent::Input(KeyInput::PageUp),
                UiEvent::Input(KeyInput::PageDown),
                UiEvent::Input(KeyInput::Delete),
                UiEvent::Input(KeyInput::Insert),
            ]
        );
    }

    #[test]
    fn input_loop_forwards_left_mouse_click_and_non_empty_paste_events() {
        let (sender, mut receiver) = mpsc::channel(4);
        let stop_requested = Arc::new(AtomicBool::new(false));
        let mut source = MockEventSource::new(
            vec![Ok(true), Ok(true), Ok(false), Ok(false)],
            vec![
                Ok(mouse_event(MouseEventKind::Down(MouseButton::Left), 7, 8)),
                Ok(Event::Paste("alpha\nbeta".to_string())),
            ],
        );

        let stop_for_thread = stop_requested.clone();
        let handle = std::thread::spawn(move || {
            run_terminal_input_loop(&mut source, sender, stop_for_thread);
        });

        let first = receiver.blocking_recv().expect("mouse event");
        let second = receiver.blocking_recv().expect("paste event");
        stop_requested.store(true, Ordering::Relaxed);
        handle.join().expect("input loop thread should stop");

        assert_eq!(first, UiEvent::MouseClick { column: 7, row: 8 });
        assert_eq!(second, UiEvent::PastedText("alpha\nbeta".to_string()));
    }

    #[test]
    fn input_loop_ignores_unsupported_mouse_events_and_empty_paste() {
        let (sender, mut receiver) = mpsc::channel(4);
        let stop_requested = Arc::new(AtomicBool::new(false));
        let mut source = MockEventSource::new(
            vec![
                Ok(true),
                Ok(true),
                Ok(true),
                Ok(true),
                Ok(true),
                Ok(true),
                Ok(false),
                Ok(false),
            ],
            vec![
                Ok(mouse_event(MouseEventKind::Down(MouseButton::Right), 1, 1)),
                Ok(mouse_event(MouseEventKind::Down(MouseButton::Middle), 1, 1)),
                Ok(mouse_event(MouseEventKind::Drag(MouseButton::Left), 1, 1)),
                Ok(mouse_event(MouseEventKind::ScrollDown, 1, 1)),
                Ok(mouse_event(MouseEventKind::Up(MouseButton::Left), 1, 1)),
                Ok(Event::Paste(String::new())),
            ],
        );

        let stop_for_thread = stop_requested.clone();
        let handle = std::thread::spawn(move || {
            run_terminal_input_loop(&mut source, sender, stop_for_thread);
        });

        std::thread::sleep(Duration::from_millis(20));
        stop_requested.store(true, Ordering::Relaxed);
        handle.join().expect("input loop thread should stop");

        assert!(
            receiver.try_recv().is_err(),
            "要件外の mouse/paste event は receiver に現れないこと"
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
