use super::*;
use std::io;

#[derive(Default)]
struct RecordingBackend {
    calls: Vec<&'static str>,
    fail_on_enter_alternate_screen: bool,
    fail_on_enable_mouse_capture: bool,
    fail_on_enable_bracketed_paste: bool,
    fail_on_enable_keyboard_enhancement: bool,
    fail_on_disable_keyboard_enhancement: bool,
    fail_on_disable_bracketed_paste: bool,
    fail_on_disable_mouse_capture: bool,
    fail_on_leave_alternate_screen: bool,
    fail_on_disable_raw_mode: bool,
    fail_on_reset_cursor_style: bool,
}

impl TerminalBackend for RecordingBackend {
    fn enable_raw_mode(&mut self) -> io::Result<()> {
        self.calls.push("enable_raw_mode");
        Ok(())
    }

    fn enter_alternate_screen(&mut self) -> io::Result<()> {
        self.calls.push("enter_alternate_screen");
        if self.fail_on_enter_alternate_screen {
            Err(io::Error::other("alternate screen failed"))
        } else {
            Ok(())
        }
    }

    fn enable_mouse_capture(&mut self) -> io::Result<()> {
        self.calls.push("enable_mouse_capture");
        if self.fail_on_enable_mouse_capture {
            Err(io::Error::other("mouse capture failed"))
        } else {
            Ok(())
        }
    }

    fn enable_bracketed_paste(&mut self) -> io::Result<()> {
        self.calls.push("enable_bracketed_paste");
        if self.fail_on_enable_bracketed_paste {
            Err(io::Error::other("bracketed paste failed"))
        } else {
            Ok(())
        }
    }

    fn enable_keyboard_enhancement(&mut self) -> io::Result<()> {
        self.calls.push("enable_keyboard_enhancement");
        if self.fail_on_enable_keyboard_enhancement {
            Err(io::Error::other("keyboard enhancement failed"))
        } else {
            Ok(())
        }
    }

    fn set_cursor_style(&mut self, style: ScreenCursorStyle) -> io::Result<()> {
        self.calls.push(match style {
            ScreenCursorStyle::Block => "set_cursor_style_block",
            ScreenCursorStyle::SteadyBar => "set_cursor_style_steady_bar",
            ScreenCursorStyle::UnderScore => "set_cursor_style_underscore",
        });
        Ok(())
    }

    fn reset_cursor_style(&mut self) -> io::Result<()> {
        self.calls.push("reset_cursor_style");
        if self.fail_on_reset_cursor_style {
            Err(io::Error::other("reset cursor style failed"))
        } else {
            Ok(())
        }
    }

    fn disable_bracketed_paste(&mut self) -> io::Result<()> {
        self.calls.push("disable_bracketed_paste");
        if self.fail_on_disable_bracketed_paste {
            Err(io::Error::other("disable bracketed paste failed"))
        } else {
            Ok(())
        }
    }

    fn disable_keyboard_enhancement(&mut self) -> io::Result<()> {
        self.calls.push("disable_keyboard_enhancement");
        if self.fail_on_disable_keyboard_enhancement {
            Err(io::Error::other("disable keyboard enhancement failed"))
        } else {
            Ok(())
        }
    }

    fn disable_mouse_capture(&mut self) -> io::Result<()> {
        self.calls.push("disable_mouse_capture");
        if self.fail_on_disable_mouse_capture {
            Err(io::Error::other("disable mouse capture failed"))
        } else {
            Ok(())
        }
    }

    fn leave_alternate_screen(&mut self) -> io::Result<()> {
        self.calls.push("leave_alternate_screen");
        if self.fail_on_leave_alternate_screen {
            Err(io::Error::other("leave alternate screen failed"))
        } else {
            Ok(())
        }
    }

    fn disable_raw_mode(&mut self) -> io::Result<()> {
        self.calls.push("disable_raw_mode");
        if self.fail_on_disable_raw_mode {
            Err(io::Error::other("disable raw mode failed"))
        } else {
            Ok(())
        }
    }
}

#[test]
fn start_edit_mode_enables_raw_mode_before_alternate_screen() {
    let mut backend = RecordingBackend::default();

    let session = TerminalLifecycle::start(&mut backend).expect("terminal start");

    assert!(session.is_raw_mode_enabled());
    assert!(session.is_alternate_screen_enabled());
    let restore_result = session.restore();
    assert!(restore_result.is_ok());
    assert_eq!(
        backend.calls,
        vec![
            "enable_raw_mode",
            "enter_alternate_screen",
            "enable_mouse_capture",
            "enable_bracketed_paste",
            "enable_keyboard_enhancement",
            "disable_keyboard_enhancement",
            "disable_bracketed_paste",
            "disable_mouse_capture",
            "leave_alternate_screen",
            "disable_raw_mode",
        ]
    );
}

#[test]
fn start_edit_mode_disables_raw_mode_if_alternate_screen_fails() {
    let mut backend = RecordingBackend {
        fail_on_enter_alternate_screen: true,
        ..RecordingBackend::default()
    };

    let result = TerminalLifecycle::start(&mut backend);

    assert!(matches!(
        result,
        Err(TerminalStartError::AlternateScreenFailed { .. })
    ));
    drop(result);
    assert_eq!(
        backend.calls,
        vec![
            "enable_raw_mode",
            "enter_alternate_screen",
            "disable_raw_mode",
        ]
    );
}

#[test]
fn start_edit_mode_rolls_back_alternate_screen_and_raw_mode_if_mouse_capture_fails() {
    let mut backend = RecordingBackend {
        fail_on_enable_mouse_capture: true,
        ..RecordingBackend::default()
    };

    let result = TerminalLifecycle::start(&mut backend);

    assert!(matches!(
        result,
        Err(TerminalStartError::MouseCaptureFailed { .. })
    ));
    drop(result);
    assert_eq!(
        backend.calls,
        vec![
            "enable_raw_mode",
            "enter_alternate_screen",
            "enable_mouse_capture",
            "leave_alternate_screen",
            "disable_raw_mode",
        ]
    );
}

#[test]
fn start_edit_mode_rolls_back_extended_input_if_bracketed_paste_fails() {
    let mut backend = RecordingBackend {
        fail_on_enable_bracketed_paste: true,
        ..RecordingBackend::default()
    };

    let result = TerminalLifecycle::start(&mut backend);

    assert!(matches!(
        result,
        Err(TerminalStartError::BracketedPasteFailed { .. })
    ));
    drop(result);
    assert_eq!(
        backend.calls,
        vec![
            "enable_raw_mode",
            "enter_alternate_screen",
            "enable_mouse_capture",
            "enable_bracketed_paste",
            "disable_mouse_capture",
            "leave_alternate_screen",
            "disable_raw_mode",
        ]
    );
}

#[test]
fn restore_leaves_alternate_screen_before_disabling_raw_mode() {
    let mut backend = RecordingBackend::default();

    let session = TerminalLifecycle::start(&mut backend).expect("terminal start");
    let restore_result = session.restore();

    assert!(restore_result.is_ok());
    assert_eq!(
        backend.calls,
        vec![
            "enable_raw_mode",
            "enter_alternate_screen",
            "enable_mouse_capture",
            "enable_bracketed_paste",
            "enable_keyboard_enhancement",
            "disable_keyboard_enhancement",
            "disable_bracketed_paste",
            "disable_mouse_capture",
            "leave_alternate_screen",
            "disable_raw_mode",
        ]
    );
}

#[test]
fn restore_resets_cursor_style_before_cleanup_steps() {
    let mut backend = RecordingBackend::default();

    let mut session = TerminalLifecycle::start(&mut backend).expect("terminal start");
    session
        .set_cursor_style(ScreenCursorStyle::SteadyBar)
        .expect("cursor style should apply");
    let restore_result = session.restore();

    assert!(restore_result.is_ok());
    assert_eq!(
        backend.calls,
        vec![
            "enable_raw_mode",
            "enter_alternate_screen",
            "enable_mouse_capture",
            "enable_bracketed_paste",
            "enable_keyboard_enhancement",
            "set_cursor_style_steady_bar",
            "reset_cursor_style",
            "disable_keyboard_enhancement",
            "disable_bracketed_paste",
            "disable_mouse_capture",
            "leave_alternate_screen",
            "disable_raw_mode",
        ]
    );
}

#[test]
fn suspend_releases_terminal_without_marking_session_restored() {
    let mut backend = RecordingBackend::default();

    let mut session = TerminalLifecycle::start(&mut backend).expect("terminal start");
    session
        .suspend_for_job_control()
        .expect("job-control suspend should release terminal");

    assert!(!session.is_raw_mode_enabled());
    assert!(!session.is_alternate_screen_enabled());

    session
        .resume_after_job_control()
        .expect("resume should reclaim terminal");
    assert!(session.is_raw_mode_enabled());
    assert!(session.is_alternate_screen_enabled());
    assert!(session.take_redraw_request());
    session
        .restore()
        .expect("restore after resume should succeed");

    assert_eq!(
        backend.calls,
        vec![
            "enable_raw_mode",
            "enter_alternate_screen",
            "enable_mouse_capture",
            "enable_bracketed_paste",
            "enable_keyboard_enhancement",
            "disable_keyboard_enhancement",
            "disable_bracketed_paste",
            "disable_mouse_capture",
            "leave_alternate_screen",
            "disable_raw_mode",
            "enable_raw_mode",
            "enter_alternate_screen",
            "enable_mouse_capture",
            "enable_bracketed_paste",
            "enable_keyboard_enhancement",
            "disable_keyboard_enhancement",
            "disable_bracketed_paste",
            "disable_mouse_capture",
            "leave_alternate_screen",
            "disable_raw_mode",
        ]
    );
}

#[test]
fn resume_reapplies_cursor_style_and_requests_redraw() {
    let mut backend = RecordingBackend::default();

    let mut session = TerminalLifecycle::start(&mut backend).expect("terminal start");
    session
        .set_cursor_style(ScreenCursorStyle::SteadyBar)
        .expect("cursor style should apply");
    session
        .suspend_for_job_control()
        .expect("job-control suspend should release terminal");
    session
        .resume_after_job_control()
        .expect("resume should reclaim terminal");

    assert!(session.is_redraw_requested());
    session
        .restore()
        .expect("restore after resume should succeed");
    assert_eq!(
        backend.calls,
        vec![
            "enable_raw_mode",
            "enter_alternate_screen",
            "enable_mouse_capture",
            "enable_bracketed_paste",
            "enable_keyboard_enhancement",
            "set_cursor_style_steady_bar",
            "reset_cursor_style",
            "disable_keyboard_enhancement",
            "disable_bracketed_paste",
            "disable_mouse_capture",
            "leave_alternate_screen",
            "disable_raw_mode",
            "enable_raw_mode",
            "enter_alternate_screen",
            "enable_mouse_capture",
            "enable_bracketed_paste",
            "enable_keyboard_enhancement",
            "set_cursor_style_steady_bar",
            "reset_cursor_style",
            "disable_keyboard_enhancement",
            "disable_bracketed_paste",
            "disable_mouse_capture",
            "leave_alternate_screen",
            "disable_raw_mode",
        ]
    );
}

#[test]
fn record_resize_updates_latest_size_and_requests_redraw() {
    let mut backend = RecordingBackend::default();

    let mut session = TerminalLifecycle::start(&mut backend).expect("terminal start");

    assert_eq!(session.latest_size(), None);
    assert!(!session.is_redraw_requested());

    session.record_resize(TerminalSize {
        columns: 120,
        rows: 40,
    });

    assert_eq!(
        session.latest_size(),
        Some(TerminalSize {
            columns: 120,
            rows: 40,
        })
    );
    assert!(session.is_redraw_requested());
    assert!(session.take_redraw_request());
    assert!(!session.is_redraw_requested());
}

#[test]
fn record_resize_keeps_the_most_recent_size() {
    let mut backend = RecordingBackend::default();

    let mut session = TerminalLifecycle::start(&mut backend).expect("terminal start");

    session.record_resize(TerminalSize {
        columns: 80,
        rows: 24,
    });
    assert_eq!(
        session.latest_size(),
        Some(TerminalSize {
            columns: 80,
            rows: 24,
        })
    );
    assert!(session.take_redraw_request());

    session.record_resize(TerminalSize {
        columns: 100,
        rows: 50,
    });

    assert_eq!(
        session.latest_size(),
        Some(TerminalSize {
            columns: 100,
            rows: 50,
        })
    );
    assert!(session.is_redraw_requested());
}

#[test]
fn dropping_session_restores_terminal_with_same_path() {
    let mut backend = RecordingBackend::default();

    {
        let _session = TerminalLifecycle::start(&mut backend).expect("terminal start");
    }

    assert_eq!(
        backend.calls,
        vec![
            "enable_raw_mode",
            "enter_alternate_screen",
            "enable_mouse_capture",
            "enable_bracketed_paste",
            "enable_keyboard_enhancement",
            "disable_keyboard_enhancement",
            "disable_bracketed_paste",
            "disable_mouse_capture",
            "leave_alternate_screen",
            "disable_raw_mode",
        ]
    );
}

#[test]
fn restore_attempts_both_cleanup_steps_when_leave_alternate_screen_fails() {
    let mut backend = RecordingBackend {
        fail_on_leave_alternate_screen: true,
        ..RecordingBackend::default()
    };

    let session = TerminalLifecycle::start(&mut backend).expect("terminal start");
    let restore_error = session.restore().expect_err("restore should fail");

    assert_eq!(
        restore_error,
        TerminalRestoreError {
            reset_cursor_style: None,
            disable_bracketed_paste: None,
            disable_keyboard_enhancement: None,
            disable_mouse_capture: None,
            leave_alternate_screen: Some("leave alternate screen failed".to_string()),
            disable_raw_mode: None,
        }
    );
    assert_eq!(
        backend.calls,
        vec![
            "enable_raw_mode",
            "enter_alternate_screen",
            "enable_mouse_capture",
            "enable_bracketed_paste",
            "enable_keyboard_enhancement",
            "disable_keyboard_enhancement",
            "disable_bracketed_paste",
            "disable_mouse_capture",
            "leave_alternate_screen",
            "disable_raw_mode",
        ]
    );
}

#[test]
fn restore_attempts_all_cleanup_steps_when_extended_input_disable_fails() {
    let mut backend = RecordingBackend {
        fail_on_disable_bracketed_paste: true,
        fail_on_disable_keyboard_enhancement: true,
        fail_on_disable_mouse_capture: true,
        fail_on_leave_alternate_screen: true,
        fail_on_disable_raw_mode: true,
        ..RecordingBackend::default()
    };

    let session = TerminalLifecycle::start(&mut backend).expect("terminal start");
    let restore_error = session.restore().expect_err("restore should fail");

    assert_eq!(
        restore_error,
        TerminalRestoreError {
            reset_cursor_style: None,
            disable_bracketed_paste: Some("disable bracketed paste failed".to_string()),
            disable_keyboard_enhancement: Some("disable keyboard enhancement failed".to_string()),
            disable_mouse_capture: Some("disable mouse capture failed".to_string()),
            leave_alternate_screen: Some("leave alternate screen failed".to_string()),
            disable_raw_mode: Some("disable raw mode failed".to_string()),
        }
    );
    assert_eq!(
        backend.calls,
        vec![
            "enable_raw_mode",
            "enter_alternate_screen",
            "enable_mouse_capture",
            "enable_bracketed_paste",
            "enable_keyboard_enhancement",
            "disable_keyboard_enhancement",
            "disable_bracketed_paste",
            "disable_mouse_capture",
            "leave_alternate_screen",
            "disable_raw_mode",
        ]
    );
}
