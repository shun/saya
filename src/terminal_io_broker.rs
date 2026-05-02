use crate::event_loop::EventSender;
use crate::input_loop::{TerminalEventSource, run_terminal_input_loop};
use crate::terminal_capability::{TerminalCapabilityProbeService, TerminalCapabilityProfile};
use crate::terminal_lifecycle::{
    TerminalBackend, TerminalRestoreError, TerminalSession, TerminalSize, TerminalStartError,
};
use crate::ui_surface::UiSurfaceMode;
use std::fmt;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::task::JoinHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalIoPhase {
    Probe,
    Interactive,
    Restored,
}

#[derive(Debug)]
pub enum TerminalIoBrokerError {
    TerminalStart(TerminalStartError),
    TerminalRestore(TerminalRestoreError),
    InvalidPhaseTransition {
        phase: TerminalIoPhase,
        attempted: &'static str,
    },
}

impl fmt::Display for TerminalIoBrokerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TerminalIoBrokerError::TerminalStart(error) => {
                write!(f, "terminal broker failed to start: {error:?}")
            }
            TerminalIoBrokerError::TerminalRestore(error) => {
                write!(f, "terminal broker failed to restore: {error:?}")
            }
            TerminalIoBrokerError::InvalidPhaseTransition { phase, attempted } => {
                write!(
                    f,
                    "terminal broker rejected {attempted} while in phase {phase:?}; probe must complete before interactive input"
                )
            }
        }
    }
}

pub struct TerminalIoBroker<'a, B: TerminalBackend> {
    surface_mode: UiSurfaceMode,
    phase: TerminalIoPhase,
    session: Option<TerminalSession<'a, B>>,
    stop_requested: Arc<AtomicBool>,
    input_task: Option<JoinHandle<()>>,
    capability_profile: Option<TerminalCapabilityProfile>,
}

impl<'a, B: TerminalBackend> TerminalIoBroker<'a, B> {
    pub fn begin_session(
        backend: &'a mut B,
        surface_mode: UiSurfaceMode,
    ) -> Result<Self, TerminalIoBrokerError> {
        let session = crate::terminal_lifecycle::TerminalLifecycle::start(backend)
            .map_err(TerminalIoBrokerError::TerminalStart)?;
        log::debug!(
            "[terminal_io_broker] terminal session started in probe phase: surface_mode={surface_mode:?}"
        );
        Ok(Self {
            surface_mode,
            phase: TerminalIoPhase::Probe,
            session: Some(session),
            stop_requested: Arc::new(AtomicBool::new(false)),
            input_task: None,
            capability_profile: None,
        })
    }

    pub fn surface_mode(&self) -> UiSurfaceMode {
        self.surface_mode
    }

    pub fn phase(&self) -> TerminalIoPhase {
        self.phase
    }

    pub fn is_raw_mode_enabled(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(TerminalSession::is_raw_mode_enabled)
    }

    pub fn is_alternate_screen_enabled(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(TerminalSession::is_alternate_screen_enabled)
    }

    pub fn capability_profile(&self) -> Option<&TerminalCapabilityProfile> {
        self.capability_profile.as_ref()
    }

    pub fn run_probe<P: TerminalCapabilityProbeService>(
        &mut self,
        probe: &mut P,
    ) -> Result<TerminalCapabilityProfile, TerminalIoBrokerError> {
        if self.phase != TerminalIoPhase::Probe || self.capability_profile.is_some() {
            return Err(TerminalIoBrokerError::InvalidPhaseTransition {
                phase: self.phase,
                attempted: "capability probe",
            });
        }

        let profile = probe.detect();
        log::debug!("[terminal_io_broker] capability probe completed: profile={profile:?}");
        self.capability_profile = Some(profile.clone());
        Ok(profile)
    }

    pub fn start_interactive_input<S: TerminalEventSource + Send + 'static>(
        &mut self,
        sender: EventSender,
        mut source: S,
    ) -> Result<(), TerminalIoBrokerError> {
        if self.phase != TerminalIoPhase::Probe || self.capability_profile.is_none() {
            return Err(TerminalIoBrokerError::InvalidPhaseTransition {
                phase: self.phase,
                attempted: "interactive input",
            });
        }

        let stop_requested = self.stop_requested.clone();
        self.input_task = Some(tokio::task::spawn_blocking(move || {
            run_terminal_input_loop(&mut source, sender, stop_requested);
        }));
        self.phase = TerminalIoPhase::Interactive;
        log::debug!("[terminal_io_broker] interactive input phase started");
        Ok(())
    }

    pub fn request_shutdown(&self) {
        log::debug!(
            "[terminal_io_broker] shutdown requested: phase={:?}",
            self.phase
        );
        self.stop_requested.store(true, Ordering::Relaxed);
    }

    pub fn record_resize(&mut self, size: TerminalSize) {
        if let Some(session) = self.session.as_mut() {
            session.record_resize(size);
        }
    }

    pub fn write_overlay_bytes(&mut self, bytes: &[u8]) -> Result<(), TerminalIoBrokerError> {
        if self.phase != TerminalIoPhase::Interactive {
            return Err(TerminalIoBrokerError::InvalidPhaseTransition {
                phase: self.phase,
                attempted: "overlay write",
            });
        }
        let mut stdout = std::io::stdout();
        stdout.write_all(bytes).map_err(terminal_transport_error)?;
        stdout.flush().map_err(terminal_transport_error)?;
        log::debug!(
            "[terminal_io_broker] wrote optional overlay bytes during interactive phase: bytes={}",
            bytes.len()
        );
        Ok(())
    }

    pub fn write_bell(&mut self, count: usize) -> Result<(), TerminalIoBrokerError> {
        if self.phase != TerminalIoPhase::Interactive {
            return Err(TerminalIoBrokerError::InvalidPhaseTransition {
                phase: self.phase,
                attempted: "terminal bell",
            });
        }
        if count == 0 {
            return Ok(());
        }
        let mut stdout = std::io::stdout();
        stdout
            .write_all(&vec![b'\x07'; count])
            .map_err(terminal_transport_error)?;
        stdout.flush().map_err(terminal_transport_error)?;
        log::debug!("[terminal_io_broker] wrote terminal bell signal: count={count}");
        Ok(())
    }

    pub fn latest_size(&self) -> Option<TerminalSize> {
        self.session.as_ref().and_then(TerminalSession::latest_size)
    }

    pub async fn shutdown(mut self) -> Result<(), TerminalIoBrokerError> {
        self.request_shutdown();
        if let Some(handle) = self.input_task.take() {
            if let Err(error) = handle.await {
                log::debug!("[terminal_io_broker] input task join failed: {error}");
            }
        }

        self.restore_session()
    }

    fn restore_session(&mut self) -> Result<(), TerminalIoBrokerError> {
        if self.phase == TerminalIoPhase::Restored {
            return Ok(());
        }

        let Some(session) = self.session.take() else {
            self.phase = TerminalIoPhase::Restored;
            return Ok(());
        };

        session
            .restore()
            .map_err(TerminalIoBrokerError::TerminalRestore)?;
        self.phase = TerminalIoPhase::Restored;
        log::debug!("[terminal_io_broker] terminal session restored");
        Ok(())
    }
}

impl<B: TerminalBackend> Drop for TerminalIoBroker<'_, B> {
    fn drop(&mut self) {
        self.stop_requested.store(true, Ordering::Relaxed);
        if let Some(handle) = self.input_task.take() {
            handle.abort();
        }
        if let Err(error) = self.restore_session() {
            log::debug!("[terminal_io_broker] drop restore failed: {error}");
        }
    }
}

fn terminal_transport_error(error: std::io::Error) -> TerminalIoBrokerError {
    TerminalIoBrokerError::TerminalRestore(TerminalRestoreError {
        disable_bracketed_paste: None,
        disable_mouse_capture: None,
        leave_alternate_screen: None,
        disable_raw_mode: Some(error.to_string()),
    })
}
