use crate::bootstrap::{BootstrapError, BootstrapOutcome, prepare_launch};
use crate::cli::LaunchRequest;
use crate::runtime_integration::RuntimeSessionOwner;
use crate::saya_live_runtime::RuntimeInitError;
use crate::terminal_capability::{TerminalCapabilityProbeService, TerminalCapabilityProfile};
use crate::terminal_io_broker::{TerminalIoBroker, TerminalIoBrokerError};
use crate::terminal_lifecycle::{TerminalBackend, TerminalStartError};
use crate::ui_surface::{
    PolicyViolation, UiFeatureRequest, UiSurfacePolicy, UiSurfacePolicyService,
};

#[derive(Debug)]
pub enum LaunchStartError {
    Bootstrap(BootstrapError),
    Terminal(TerminalStartError),
    Policy(PolicyViolation),
}

#[derive(Debug)]
pub enum TuiStartupContextError {
    Launch(LaunchStartError),
    CapabilityProbe(TerminalIoBrokerError),
}

pub struct PreparedTuiStartup<'a, B: TerminalBackend> {
    pub outcome: BootstrapOutcome,
    pub terminal_broker: TerminalIoBroker<'a, B>,
    pub capability_profile: TerminalCapabilityProfile,
    pub runtime_session: Option<RuntimeSessionOwner>,
    pub runtime_init_message: Option<String>,
}

pub fn prepare_launch_and_start_terminal<'a, B: TerminalBackend>(
    request: LaunchRequest,
    backend: &'a mut B,
) -> Result<(BootstrapOutcome, TerminalIoBroker<'a, B>), LaunchStartError> {
    let policy = UiSurfacePolicy;
    let surface_mode = policy.resolve_mode();
    log::debug!(
        "[app_startup] resolved startup UI surface policy: mode={surface_mode:?}"
    );
    for feature_request in [
        UiFeatureRequest::CoreEditing,
        UiFeatureRequest::StyledText,
        UiFeatureRequest::InlineGraphics,
    ] {
        if let Err(error) = policy.validate_feature_request(feature_request) {
            log::debug!(
                "[app_startup] startup surface request rejected before bootstrap: request={feature_request:?}, error={error}"
            );
            return Err(LaunchStartError::Policy(error));
        }
    }

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

    log::debug!("[app_startup] bootstrap succeeded, starting terminal I/O broker");
    let terminal_broker = match TerminalIoBroker::begin_session(backend, surface_mode) {
        Ok(broker) => broker,
        Err(error) => {
            log::debug!(
                "[app_startup] terminal I/O broker initialization failed after bootstrap: {:?}",
                error
            );
            let TerminalIoBrokerError::TerminalStart(error) = error else {
                unreachable!("begin_session only fails with terminal start errors")
            };
            return Err(LaunchStartError::Terminal(error));
        }
    };

    log::debug!(
        "[app_startup] launch preparation and terminal broker initialization completed: mode={surface_mode:?}"
    );
    Ok((outcome, terminal_broker))
}

pub fn prepare_tui_startup_context<'a, B, P>(
    request: LaunchRequest,
    backend: &'a mut B,
    probe: &mut P,
) -> Result<PreparedTuiStartup<'a, B>, TuiStartupContextError>
where
    B: TerminalBackend,
    P: TerminalCapabilityProbeService,
{
    let (outcome, mut terminal_broker) =
        prepare_launch_and_start_terminal(request, backend).map_err(TuiStartupContextError::Launch)?;
    let capability_profile = terminal_broker
        .run_probe(probe)
        .map_err(TuiStartupContextError::CapabilityProbe)?;
    log::debug!(
        "[app_startup] composed TUI startup context after capability probe: mode={:?}, profile={:?}",
        terminal_broker.surface_mode(),
        capability_profile
    );

    let (runtime_session, runtime_init_message) =
        match RuntimeSessionOwner::spawn(outcome.callback_registry.clone()) {
            Ok(runtime_session) => (Some(runtime_session), None),
            Err(error) => {
                let message = format_runtime_init_error(&error);
                log::debug!(
                    "[app_startup] runtime session owner initialization degraded during startup composition: {:?}",
                    error
                );
                (None, Some(message))
            }
        };

    Ok(PreparedTuiStartup {
        outcome,
        terminal_broker,
        capability_profile,
        runtime_session,
        runtime_init_message,
    })
}

fn format_runtime_init_error(error: &RuntimeInitError) -> String {
    match error {
        RuntimeInitError::WorkerStartFailed { message } => {
            format!("Runtime initialization failed: {}", message)
        }
        RuntimeInitError::UnsupportedEvent { name } => {
            format!("Runtime initialization failed: unsupported event {}", name)
        }
        RuntimeInitError::BootstrapFailed { message } => {
            format!("Runtime initialization failed: {}", message)
        }
    }
}
