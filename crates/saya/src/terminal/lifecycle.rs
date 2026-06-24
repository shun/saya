use crate::presentation::screen_model::ScreenCursorStyle;
use std::io;

/// 現在のターミナルサイズ（列, 行）を返す。取得に失敗した場合は 80x24 を返す。
pub fn current_terminal_size() -> (u16, u16) {
    crossterm::terminal::size().unwrap_or((80, 24))
}

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
#[path = "lifecycle_test.rs"]
mod tests;
