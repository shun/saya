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

fn shift_mouse_event(kind: MouseEventKind, column: u16, row: u16) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::SHIFT,
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
fn map_key_input_preserves_shift_enter_as_modified_enter() {
    let mapped = map_key_input(key_event_with_modifiers(
        KeyCode::Enter,
        KeyModifiers::SHIFT,
    ));

    assert_eq!(mapped, Some(KeyInput::ShiftEnter));
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
            Ok(false),
            Ok(false),
        ],
        vec![
            Ok(mouse_event(MouseEventKind::Down(MouseButton::Right), 1, 1)),
            Ok(mouse_event(MouseEventKind::Down(MouseButton::Middle), 1, 1)),
            Ok(mouse_event(MouseEventKind::Drag(MouseButton::Left), 1, 1)),
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
fn input_loop_forwards_mouse_wheel_events() {
    let (sender, mut receiver) = mpsc::channel(4);
    let stop_requested = Arc::new(AtomicBool::new(false));
    let mut source = MockEventSource::new(
        vec![Ok(true), Ok(true), Ok(true), Ok(false), Ok(false)],
        vec![
            Ok(mouse_event(MouseEventKind::ScrollDown, 2, 3)),
            Ok(mouse_event(MouseEventKind::ScrollUp, 4, 5)),
            Ok(shift_mouse_event(MouseEventKind::ScrollDown, 6, 7)),
        ],
    );

    let stop_for_thread = stop_requested.clone();
    let handle = std::thread::spawn(move || {
        run_terminal_input_loop(&mut source, sender, stop_for_thread);
    });

    let first = receiver
        .blocking_recv()
        .expect("first wheel event should be forwarded");
    let second = receiver
        .blocking_recv()
        .expect("second wheel event should be forwarded");
    let third = receiver
        .blocking_recv()
        .expect("shift wheel event should be forwarded as horizontal pan");
    stop_requested.store(true, Ordering::Relaxed);
    handle.join().expect("input loop thread should stop");

    assert_eq!(
        first,
        UiEvent::MouseWheel {
            column: 2,
            row: 3,
            delta_x: 0,
            delta_y: 1,
        }
    );
    assert_eq!(
        second,
        UiEvent::MouseWheel {
            column: 4,
            row: 5,
            delta_x: 0,
            delta_y: -1,
        }
    );
    assert_eq!(
        third,
        UiEvent::MouseWheel {
            column: 6,
            row: 7,
            delta_x: 1,
            delta_y: 0,
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
