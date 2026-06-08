use saya::app::event_loop::{EventLoopCoordinator, LoopAction};
use saya::input::router::KeyInput;
use saya::presentation::floating_window::{
    FloatingBorder, FloatingChrome, FloatingContentRef, FloatingInlineStyleKind, FloatingPlacement,
    FloatingSize, FloatingWindowManager, FloatingZIndex,
};
use saya::terminal::emulator::{
    TerminalCellStyle, TerminalColor, TerminalEmulator, Vt100TerminalEmulator,
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
fn vt100_terminal_emulator_projects_cell_styles_and_cursor() {
    let mut emulator = Vt100TerminalEmulator::new(12, 3, 16);

    emulator.feed(b"\x1b[31;44;1;4mA\x1b[7mB\x1b[0m\nplain\x1b[2;6H");

    let snapshot = emulator.screen();
    assert_eq!(snapshot.cursor_row, 1);
    assert_eq!(snapshot.cursor_col, 5);
    assert!(snapshot.cursor_visible);
    assert_eq!(snapshot.rows[0][0].text, "A");
    assert_eq!(
        snapshot.rows[0][0].style,
        TerminalCellStyle {
            foreground: Some(TerminalColor::Indexed(1)),
            background: Some(TerminalColor::Indexed(4)),
            bold: true,
            underline: true,
            inverse: false,
        }
    );
    assert_eq!(snapshot.rows[0][1].text, "B");
    assert!(snapshot.rows[0][1].style.inverse);
}

#[test]
fn terminal_snapshot_projects_lines_and_terminal_inline_styles() {
    let mut emulator = Vt100TerminalEmulator::new(8, 2, 16);

    emulator.feed(b"\x1b[32mOK\x1b[0m \x1b[7m!\x1b[0m");

    let snapshot = emulator.screen();

    assert_eq!(snapshot.rendered_lines(), vec!["OK !    ", "        "]);
    assert_eq!(snapshot.inline_styles().len(), 2);
    assert!(snapshot.inline_styles().iter().any(|style| {
        matches!(
            style.kind,
            FloatingInlineStyleKind::TerminalCell(TerminalCellStyle {
                foreground: Some(TerminalColor::Indexed(2)),
                background: None,
                bold: false,
                underline: false,
                inverse: false,
            })
        ) && style.line == 0
            && style.column_start == 0
            && style.column_end == 2
    }));
    assert!(snapshot.inline_styles().iter().any(|style| {
        matches!(
            style.kind,
            FloatingInlineStyleKind::TerminalCell(TerminalCellStyle {
                foreground: None,
                background: None,
                bold: false,
                underline: false,
                inverse: true,
            })
        ) && style.line == 0
            && style.column_start == 3
            && style.column_end == 4
    }));
}

#[test]
fn vt100_terminal_emulator_does_not_render_wide_continuation_cells_as_spaces() {
    let mut emulator = Vt100TerminalEmulator::new(12, 2, 16);

    emulator.feed("このリポ".as_bytes());

    let snapshot = emulator.screen();
    assert_eq!(snapshot.cursor_row, 0);
    assert_eq!(snapshot.cursor_col, 8);
    assert!(
        snapshot.rendered_lines()[0].starts_with("このリポ"),
        "wide-character continuation cells must not insert extra spaces: {:?}",
        snapshot.rendered_lines()[0]
    );
    assert!(
        !snapshot.rendered_lines()[0].starts_with("こ の リ ポ "),
        "wide-character continuation cells were rendered as visible spaces: {:?}",
        snapshot.rendered_lines()[0]
    );
}

#[test]
fn vt100_terminal_emulator_resize_updates_snapshot_dimensions() {
    let mut emulator = Vt100TerminalEmulator::new(4, 2, 16);

    emulator.resize(6, 3);

    let snapshot = emulator.screen();
    assert_eq!(snapshot.rows.len(), 3);
    assert_eq!(snapshot.rows[0].len(), 6);
}

#[tokio::test(flavor = "current_thread")]
async fn terminal_output_requests_redraw_event() {
    let (mut coordinator, redraw_sender) = EventLoopCoordinator::with_capacity(8);
    let mut terminals = TerminalFloatManager::default();
    terminals.set_redraw_sender(redraw_sender);

    let terminal_id = terminals
        .spawn(TerminalFloatSpawnRequest {
            command: "sh".to_string(),
            args: vec!["-lc".to_string(), "printf 'redraw-from-pty\\n'".to_string()],
            width: 24,
            height: 4,
            close_behavior: TerminalFloatCloseBehavior::KillOnClose,
        })
        .expect("terminal session should spawn");

    let action = tokio::time::timeout(Duration::from_secs(5), coordinator.next_action())
        .await
        .expect("terminal output should wake the UI loop with redraw");

    assert_eq!(action, LoopAction::NeedRedraw);
    wait_until(|| {
        terminals.drain();
        terminals
            .rendered_lines(terminal_id)
            .iter()
            .any(|line| line.contains("redraw-from-pty"))
    });
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
fn pty_terminal_float_projects_cell_styles_and_resize() {
    let mut terminals = TerminalFloatManager::default();

    let terminal_id = terminals
        .spawn(TerminalFloatSpawnRequest {
            command: "sh".to_string(),
            args: vec![
                "-lc".to_string(),
                "printf '\\033[31;44;1;4mstyled\\033[0m\\n'; sleep 30".to_string(),
            ],
            width: 16,
            height: 4,
            close_behavior: TerminalFloatCloseBehavior::KillOnClose,
        })
        .expect("terminal session should spawn");

    wait_until(|| {
        terminals.drain();
        terminals
            .screen_snapshot(terminal_id)
            .is_some_and(|snapshot| snapshot.rendered_lines()[0].contains("styled"))
    });

    let snapshot = terminals
        .screen_snapshot(terminal_id)
        .expect("terminal snapshot should be available");
    let styled = snapshot
        .inline_styles()
        .into_iter()
        .find(|style| style.line == 0 && style.column_start == 0 && style.column_end >= 6)
        .expect("styled terminal cells should project to inline styles");
    assert!(matches!(
        styled.kind,
        FloatingInlineStyleKind::TerminalCell(TerminalCellStyle {
            foreground: Some(TerminalColor::Indexed(1)),
            background: Some(TerminalColor::Indexed(4)),
            bold: true,
            underline: true,
            inverse: false,
        })
    ));

    terminals
        .resize(terminal_id, 20, 6)
        .expect("terminal resize should propagate to PTY and emulator");
    let resized = terminals
        .screen_snapshot(terminal_id)
        .expect("resized terminal snapshot should be available");
    assert_eq!(resized.rows.len(), 6);
    assert_eq!(resized.rows[0].len(), 20);

    terminals
        .kill(terminal_id)
        .expect("test cleanup should kill resized terminal");
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
