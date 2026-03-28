use crate::bootstrap::{BootstrapError, BootstrapOutcome, prepare_launch};
use crate::cli::LaunchRequest;
use crate::terminal_lifecycle::{
    TerminalBackend, TerminalLifecycle, TerminalSession, TerminalStartError,
};

#[derive(Debug)]
pub enum LaunchStartError {
    Bootstrap(BootstrapError),
    Terminal(TerminalStartError),
}

pub fn prepare_launch_and_start_terminal<'a, B: TerminalBackend>(
    request: LaunchRequest,
    backend: &'a mut B,
) -> Result<(BootstrapOutcome, TerminalSession<'a, B>), LaunchStartError> {
    log::debug!("[app_startup] preparing launch before terminal initialization");
    let outcome = match prepare_launch(request) {
        Ok(outcome) => outcome,
        Err(error) => {
            log::debug!(
                "[app_startup] bootstrap failed before terminal initialization: {:?}",
                error
            );
            return Err(LaunchStartError::Bootstrap(error));
        }
    };

    log::debug!("[app_startup] bootstrap succeeded, starting terminal lifecycle");
    let terminal_session = match TerminalLifecycle::start(backend) {
        Ok(session) => session,
        Err(error) => {
            log::debug!(
                "[app_startup] terminal lifecycle initialization failed after bootstrap: {:?}",
                error
            );
            return Err(LaunchStartError::Terminal(error));
        }
    };

    log::debug!("[app_startup] launch preparation and terminal initialization completed");
    Ok((outcome, terminal_session))
}
