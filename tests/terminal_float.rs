use saya::input::router::KeyInput;
use saya::presentation::floating_window::{
    FloatingBorder, FloatingChrome, FloatingContentRef, FloatingPlacement, FloatingSize,
    FloatingWindowManager, FloatingZIndex,
};
use saya::terminal::float::{
    TerminalFloatCloseBehavior, TerminalFloatManager, TerminalFloatSpawnRequest,
};
use std::time::{Duration, Instant};

fn wait_until(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if predicate() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(predicate(), "condition did not become true before timeout");
}

#[test]
fn pty_terminal_float_renders_command_output_inside_floating_window() {
    let mut floating = FloatingWindowManager::default();
    let mut terminals = TerminalFloatManager::default();

    let terminal_id = terminals
        .spawn(TerminalFloatSpawnRequest {
            command: "sh".to_string(),
            args: vec![
                "-lc".to_string(),
                "printf 'phase7-terminal\\nsecond-line\\n'".to_string(),
            ],
            width: 24,
            height: 4,
            close_behavior: TerminalFloatCloseBehavior::KillOnClose,
        })
        .expect("terminal session should spawn");
    let float_id = floating.open_terminal(
        terminal_id,
        FloatingPlacement::editor_at(1, 2),
        FloatingSize {
            width: 28,
            height: 6,
        },
        FloatingChrome {
            border: FloatingBorder::Single,
        },
        FloatingZIndex::User,
        true,
    );
    floating.focus_float(float_id);

    wait_until(|| {
        terminals.drain();
        let lines = terminals.rendered_lines(terminal_id);
        lines.iter().any(|line| line.contains("phase7-terminal"))
            && lines.iter().any(|line| line.contains("second-line"))
    });

    let request = floating
        .terminal_float_view_requests()
        .into_iter()
        .next()
        .expect("terminal float should request rendered terminal lines");
    assert_eq!(request.terminal_id, terminal_id);
    assert_eq!(request.content_height, 4);

    let lines = terminals.rendered_lines(terminal_id);
    assert!(floating.replace_terminal_lines(float_id, lines));
    let floats = floating.resolve_screen_models(80, 24, &[], None);

    assert_eq!(
        floats[0].content,
        FloatingContentRef::Terminal { terminal_id }
    );
    assert!(
        floats[0]
            .lines
            .iter()
            .any(|line| line.contains("phase7-terminal"))
    );
}

#[test]
fn focused_terminal_float_routes_input_and_keeps_close_policy_explicit() {
    let mut floating = FloatingWindowManager::default();
    let mut terminals = TerminalFloatManager::default();

    let terminal_id = terminals
        .spawn(TerminalFloatSpawnRequest {
            command: "sh".to_string(),
            args: vec![
                "-lc".to_string(),
                "read line; printf \"echo:%s\\n\" \"$line\"; sleep 30".to_string(),
            ],
            width: 32,
            height: 4,
            close_behavior: TerminalFloatCloseBehavior::DetachOnClose,
        })
        .expect("terminal session should spawn");
    let float_id = floating.open_terminal(
        terminal_id,
        FloatingPlacement::editor_at(1, 2),
        FloatingSize {
            width: 36,
            height: 6,
        },
        FloatingChrome {
            border: FloatingBorder::Single,
        },
        FloatingZIndex::User,
        true,
    );
    floating.focus_float(float_id);

    let focused_terminal = floating
        .focused_terminal_id()
        .expect("focused terminal float should expose terminal id");
    terminals
        .write_key(focused_terminal, &KeyInput::Char('o'))
        .expect("input should be written to terminal");
    terminals
        .write_key(focused_terminal, &KeyInput::Char('k'))
        .expect("input should be written to terminal");
    terminals
        .write_key(focused_terminal, &KeyInput::Enter)
        .expect("enter should be written to terminal");

    wait_until(|| {
        terminals.drain();
        terminals
            .rendered_lines(terminal_id)
            .iter()
            .any(|line| line.contains("echo:ok"))
    });

    assert!(floating.close(float_id));
    terminals
        .close_view(terminal_id)
        .expect("close policy should apply");
    assert!(
        terminals.is_alive(terminal_id),
        "detach policy keeps the terminal session alive after closing the float view"
    );
    terminals
        .kill(terminal_id)
        .expect("test cleanup should kill detached session");
}
