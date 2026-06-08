use crate::presentation::screen_model::ScreenCursorStyle;
use std::io;

#[derive(Debug, Default)]
pub struct TerminalLifecycle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalSize {
    pub columns: u16,
    pub rows: u16,
}

#[derive(Debug)]
pub struct TerminalSession<'a, B: TerminalBackend> {
    backend: &'a mut B,
    raw_mode_enabled: bool,
    alternate_screen_enabled: bool,
    mouse_capture_enabled: bool,
    bracketed_paste_enabled: bool,
    keyboard_enhancement_enabled: bool,
    current_cursor_style: Option<ScreenCursorStyle>,
    suspended: bool,
    restored: bool,
    latest_size: Option<TerminalSize>,
    redraw_requested: bool,
}

impl<'a, B: TerminalBackend> TerminalSession<'a, B> {
    pub fn is_raw_mode_enabled(&self) -> bool {
        self.raw_mode_enabled
    }

    pub fn is_alternate_screen_enabled(&self) -> bool {
        self.alternate_screen_enabled
    }

    pub fn is_mouse_capture_enabled(&self) -> bool {
        self.mouse_capture_enabled
    }

    pub fn is_bracketed_paste_enabled(&self) -> bool {
        self.bracketed_paste_enabled
    }

    pub fn is_keyboard_enhancement_enabled(&self) -> bool {
        self.keyboard_enhancement_enabled
    }

    pub fn latest_size(&self) -> Option<TerminalSize> {
        self.latest_size
    }

    pub fn is_redraw_requested(&self) -> bool {
        self.redraw_requested
    }

    pub fn take_redraw_request(&mut self) -> bool {
        let requested = self.redraw_requested;
        self.redraw_requested = false;
        requested
    }

    pub fn record_resize(&mut self, size: TerminalSize) {
        log::debug!(
            "[terminal] resize observed: columns={}, rows={}",
            size.columns,
            size.rows
        );
        self.latest_size = Some(size);
        self.redraw_requested = true;
    }

    pub fn set_cursor_style(&mut self, style: ScreenCursorStyle) -> io::Result<()> {
        log::debug!("[terminal] applying cursor style: style={style:?}");
        self.backend.set_cursor_style(style)?;
        self.current_cursor_style = Some(style);
        Ok(())
    }

    pub fn suspend_for_job_control(&mut self) -> Result<(), TerminalRestoreError> {
        if self.restored {
            log::debug!("[terminal] job-control suspend skipped: already restored");
            return Ok(());
        }
        if self.suspended {
            log::debug!("[terminal] job-control suspend skipped: already suspended");
            return Ok(());
        }

        log::debug!("[terminal] releasing terminal for job-control suspend");
        self.release_terminal_modes()?;
        self.suspended = true;
        Ok(())
    }

    pub fn resume_after_job_control(&mut self) -> Result<(), TerminalStartError> {
        if self.restored {
            log::debug!("[terminal] job-control resume rejected: session already restored");
            return Err(TerminalStartError::RawModeFailed {
                message: "terminal session already restored".to_string(),
            });
        }
        if !self.suspended {
            log::debug!(
                "[terminal] job-control resume requested while terminal is already claimed"
            );
            self.redraw_requested = true;
            return Ok(());
        }

        log::debug!("[terminal] reclaiming terminal after job-control resume");
        self.backend
            .enable_raw_mode()
            .map_err(|error| TerminalStartError::RawModeFailed {
                message: error.to_string(),
            })?;
        self.raw_mode_enabled = true;
        log::debug!("[terminal] raw mode re-enabled after resume");

        if let Err(error) = self.backend.enter_alternate_screen() {
            log::debug!(
                "[terminal] resume alternate screen failed, rolling back raw mode: {}",
                error
            );
            let _ = self.backend.disable_raw_mode();
            self.raw_mode_enabled = false;
            return Err(TerminalStartError::AlternateScreenFailed {
                message: error.to_string(),
            });
        }
        self.alternate_screen_enabled = true;
        log::debug!("[terminal] alternate screen re-entered after resume");

        if let Err(error) = self.backend.enable_mouse_capture() {
            log::debug!(
                "[terminal] resume mouse capture failed, rolling back alternate screen and raw mode: {}",
                error
            );
            let _ = self.backend.leave_alternate_screen();
            let _ = self.backend.disable_raw_mode();
            self.alternate_screen_enabled = false;
            self.raw_mode_enabled = false;
            return Err(TerminalStartError::MouseCaptureFailed {
                message: error.to_string(),
            });
        }
        self.mouse_capture_enabled = true;
        log::debug!("[terminal] mouse capture re-enabled after resume");

        if let Err(error) = self.backend.enable_bracketed_paste() {
            log::debug!(
                "[terminal] resume bracketed paste failed, rolling back terminal claim: {}",
                error
            );
            let _ = self.backend.disable_mouse_capture();
            let _ = self.backend.leave_alternate_screen();
            let _ = self.backend.disable_raw_mode();
            self.mouse_capture_enabled = false;
            self.alternate_screen_enabled = false;
            self.raw_mode_enabled = false;
            return Err(TerminalStartError::BracketedPasteFailed {
                message: error.to_string(),
            });
        }
        self.bracketed_paste_enabled = true;
        log::debug!("[terminal] bracketed paste re-enabled after resume");

        if let Err(error) = self.backend.enable_keyboard_enhancement() {
            log::debug!(
                "[terminal] resume keyboard enhancement failed, rolling back terminal claim: {}",
                error
            );
            let _ = self.backend.disable_bracketed_paste();
            let _ = self.backend.disable_mouse_capture();
            let _ = self.backend.leave_alternate_screen();
            let _ = self.backend.disable_raw_mode();
            self.bracketed_paste_enabled = false;
            self.mouse_capture_enabled = false;
            self.alternate_screen_enabled = false;
            self.raw_mode_enabled = false;
            return Err(TerminalStartError::KeyboardEnhancementFailed {
                message: error.to_string(),
            });
        }
        self.keyboard_enhancement_enabled = true;
        log::debug!("[terminal] keyboard enhancement re-enabled after resume");

        if let Some(style) = self.current_cursor_style {
            self.backend.set_cursor_style(style).map_err(|error| {
                TerminalStartError::CursorStyleFailed {
                    message: error.to_string(),
                }
            })?;
            log::debug!("[terminal] cursor style re-applied after resume: style={style:?}");
        }

        self.suspended = false;
        self.redraw_requested = true;
        Ok(())
    }

    pub fn restore(mut self) -> Result<(), TerminalRestoreError> {
        self.restore_inner()
    }

    fn restore_inner(&mut self) -> Result<(), TerminalRestoreError> {
        if self.restored {
            log::debug!("[terminal] restore skipped: already restored");
            return Ok(());
        }

        log::debug!("[terminal] restoring terminal lifecycle");
        self.release_terminal_modes()?;
        self.restored = true;
        log::debug!("[terminal] terminal lifecycle restored");
        Ok(())
    }

    fn release_terminal_modes(&mut self) -> Result<(), TerminalRestoreError> {
        let reset_cursor_style_error = if self.current_cursor_style.is_some() {
            log::debug!("[terminal] resetting cursor style");
            self.backend.reset_cursor_style().err().map(|error| {
                log::debug!("[terminal] reset cursor style failed: {}", error);
                error.to_string()
            })
        } else {
            None
        };

        let disable_keyboard_enhancement_error = if self.keyboard_enhancement_enabled {
            log::debug!("[terminal] disabling keyboard enhancement");
            self.backend
                .disable_keyboard_enhancement()
                .err()
                .map(|error| {
                    log::debug!("[terminal] disable keyboard enhancement failed: {}", error);
                    error.to_string()
                })
        } else {
            None
        };

        let disable_bracketed_paste_error = if self.bracketed_paste_enabled {
            log::debug!("[terminal] disabling bracketed paste");
            self.backend.disable_bracketed_paste().err().map(|error| {
                log::debug!("[terminal] disable bracketed paste failed: {}", error);
                error.to_string()
            })
        } else {
            None
        };

        let disable_mouse_capture_error = if self.mouse_capture_enabled {
            log::debug!("[terminal] disabling mouse capture");
            self.backend.disable_mouse_capture().err().map(|error| {
                log::debug!("[terminal] disable mouse capture failed: {}", error);
                error.to_string()
            })
        } else {
            None
        };

        let leave_alternate_screen_error = if self.alternate_screen_enabled {
            log::debug!("[terminal] leaving alternate screen");
            self.backend.leave_alternate_screen().err().map(|error| {
                log::debug!("[terminal] leave alternate screen failed: {}", error);
                error.to_string()
            })
        } else {
            None
        };

        let disable_raw_mode_error = if self.raw_mode_enabled {
            log::debug!("[terminal] disabling raw mode");
            self.backend.disable_raw_mode().err().map(|error| {
                log::debug!("[terminal] disable raw mode failed: {}", error);
                error.to_string()
            })
        } else {
            None
        };

        self.bracketed_paste_enabled = false;
        self.keyboard_enhancement_enabled = false;
        self.mouse_capture_enabled = false;
        self.alternate_screen_enabled = false;
        self.raw_mode_enabled = false;

        match (
            disable_bracketed_paste_error,
            disable_keyboard_enhancement_error,
            disable_mouse_capture_error,
            leave_alternate_screen_error,
            disable_raw_mode_error,
            reset_cursor_style_error,
        ) {
            (None, None, None, None, None, None) => Ok(()),
            (
                disable_bracketed_paste,
                disable_keyboard_enhancement,
                disable_mouse_capture,
                leave_alternate_screen,
                disable_raw_mode,
                reset_cursor_style,
            ) => {
                log::debug!(
                    "[terminal] terminal restore failed: reset_cursor_style={reset_cursor_style:?}, disable_keyboard_enhancement={disable_keyboard_enhancement:?}, disable_bracketed_paste={disable_bracketed_paste:?}, disable_mouse_capture={disable_mouse_capture:?}, leave_alternate_screen={leave_alternate_screen:?}, disable_raw_mode={disable_raw_mode:?}"
                );
                Err(TerminalRestoreError {
                    reset_cursor_style,
                    disable_bracketed_paste,
                    disable_keyboard_enhancement,
                    disable_mouse_capture,
                    leave_alternate_screen,
                    disable_raw_mode,
                })
            }
        }
    }
}

impl<B: TerminalBackend> Drop for TerminalSession<'_, B> {
    fn drop(&mut self) {
        if let Err(error) = self.restore_inner() {
            log::debug!("[terminal] drop restore failed: {error:?}");
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalStartError {
    RawModeFailed { message: String },
    AlternateScreenFailed { message: String },
    MouseCaptureFailed { message: String },
    BracketedPasteFailed { message: String },
    KeyboardEnhancementFailed { message: String },
    CursorStyleFailed { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalRestoreError {
    pub reset_cursor_style: Option<String>,
    pub disable_bracketed_paste: Option<String>,
    pub disable_keyboard_enhancement: Option<String>,
    pub disable_mouse_capture: Option<String>,
    pub leave_alternate_screen: Option<String>,
    pub disable_raw_mode: Option<String>,
}

pub trait TerminalBackend {
    fn enable_raw_mode(&mut self) -> io::Result<()>;
    fn enter_alternate_screen(&mut self) -> io::Result<()>;
    fn enable_mouse_capture(&mut self) -> io::Result<()>;
    fn enable_bracketed_paste(&mut self) -> io::Result<()>;
    fn enable_keyboard_enhancement(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn set_cursor_style(&mut self, _style: ScreenCursorStyle) -> io::Result<()> {
        Ok(())
    }
    fn reset_cursor_style(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn disable_bracketed_paste(&mut self) -> io::Result<()>;
    fn disable_keyboard_enhancement(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn disable_mouse_capture(&mut self) -> io::Result<()>;
    fn leave_alternate_screen(&mut self) -> io::Result<()>;
    fn disable_raw_mode(&mut self) -> io::Result<()>;
}

impl TerminalLifecycle {
    pub fn start<B: TerminalBackend>(
        backend: &mut B,
    ) -> Result<TerminalSession<'_, B>, TerminalStartError> {
        log::debug!("[terminal] starting edit-mode terminal lifecycle");
        backend
            .enable_raw_mode()
            .map_err(|error| TerminalStartError::RawModeFailed {
                message: error.to_string(),
            })?;
        log::debug!("[terminal] raw mode enabled");

        if let Err(error) = backend.enter_alternate_screen() {
            log::debug!(
                "[terminal] alternate screen failed, rolling back raw mode: {}",
                error
            );
            let _ = backend.disable_raw_mode();
            return Err(TerminalStartError::AlternateScreenFailed {
                message: error.to_string(),
            });
        }

        log::debug!("[terminal] alternate screen entered");

        if let Err(error) = backend.enable_mouse_capture() {
            log::debug!(
                "[terminal] mouse capture failed, rolling back alternate screen and raw mode: {}",
                error
            );
            if let Err(rollback_error) = backend.leave_alternate_screen() {
                log::debug!(
                    "[terminal] rollback leave alternate screen failed after mouse capture failure: {}",
                    rollback_error
                );
            }
            if let Err(rollback_error) = backend.disable_raw_mode() {
                log::debug!(
                    "[terminal] rollback disable raw mode failed after mouse capture failure: {}",
                    rollback_error
                );
            }
            return Err(TerminalStartError::MouseCaptureFailed {
                message: error.to_string(),
            });
        }

        log::debug!("[terminal] mouse capture enabled");

        if let Err(error) = backend.enable_bracketed_paste() {
            log::debug!(
                "[terminal] bracketed paste failed, rolling back mouse capture, alternate screen, and raw mode: {}",
                error
            );
            if let Err(rollback_error) = backend.disable_mouse_capture() {
                log::debug!(
                    "[terminal] rollback disable mouse capture failed after bracketed paste failure: {}",
                    rollback_error
                );
            }
            if let Err(rollback_error) = backend.leave_alternate_screen() {
                log::debug!(
                    "[terminal] rollback leave alternate screen failed after bracketed paste failure: {}",
                    rollback_error
                );
            }
            if let Err(rollback_error) = backend.disable_raw_mode() {
                log::debug!(
                    "[terminal] rollback disable raw mode failed after bracketed paste failure: {}",
                    rollback_error
                );
            }
            return Err(TerminalStartError::BracketedPasteFailed {
                message: error.to_string(),
            });
        }

        log::debug!("[terminal] bracketed paste enabled");

        if let Err(error) = backend.enable_keyboard_enhancement() {
            log::debug!(
                "[terminal] keyboard enhancement failed, rolling back bracketed paste, mouse capture, alternate screen, and raw mode: {}",
                error
            );
            if let Err(rollback_error) = backend.disable_bracketed_paste() {
                log::debug!(
                    "[terminal] rollback disable bracketed paste failed after keyboard enhancement failure: {}",
                    rollback_error
                );
            }
            if let Err(rollback_error) = backend.disable_mouse_capture() {
                log::debug!(
                    "[terminal] rollback disable mouse capture failed after keyboard enhancement failure: {}",
                    rollback_error
                );
            }
            if let Err(rollback_error) = backend.leave_alternate_screen() {
                log::debug!(
                    "[terminal] rollback leave alternate screen failed after keyboard enhancement failure: {}",
                    rollback_error
                );
            }
            if let Err(rollback_error) = backend.disable_raw_mode() {
                log::debug!(
                    "[terminal] rollback disable raw mode failed after keyboard enhancement failure: {}",
                    rollback_error
                );
            }
            return Err(TerminalStartError::KeyboardEnhancementFailed {
                message: error.to_string(),
            });
        }

        log::debug!("[terminal] keyboard enhancement enabled");

        Ok(TerminalSession {
            backend,
            raw_mode_enabled: true,
            alternate_screen_enabled: true,
            mouse_capture_enabled: true,
            bracketed_paste_enabled: true,
            keyboard_enhancement_enabled: true,
            current_cursor_style: None,
            suspended: false,
            restored: false,
            latest_size: None,
            redraw_requested: false,
        })
    }
}

#[cfg(test)]
mod tests {
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
                disable_keyboard_enhancement: Some(
                    "disable keyboard enhancement failed".to_string()
                ),
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
}
