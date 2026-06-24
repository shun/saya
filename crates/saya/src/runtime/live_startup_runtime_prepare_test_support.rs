use super::*;
use crate::runtime::startup::{
    PreparedStartupModule, StartupModulePrepareResult, prepare_init_module,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum StartupRuntimePrepareError {
    ReadFailed { path: PathBuf, message: String },
    TranspileFailed { path: PathBuf, message: String },
}

pub(super) struct StartupRuntimePrepareEvaluator {
    config_path: PathBuf,
    current_dir: PathBuf,
}

impl SayaStartupPhaseEvaluator for StartupRuntimePrepareEvaluator {
    type Output = PreparedStartupModule;
    type Error = StartupRuntimePrepareError;

    fn evaluate(&self) -> BoxFuture<Result<Self::Output, Self::Error>> {
        let config_path = self.config_path.clone();
        let current_dir = self.current_dir.clone();
        Box::pin(async move {
            log::debug!(
                "[saya_live_runtime] prepare startup runtime on worker: config_path={}, current_dir={}",
                config_path.display(),
                current_dir.display()
            );
            match prepare_init_module(&config_path, &current_dir) {
                StartupModulePrepareResult::Success(module) => Ok(module),
                StartupModulePrepareResult::ReadFailed { path, message } => {
                    Err(StartupRuntimePrepareError::ReadFailed { path, message })
                }
                StartupModulePrepareResult::TranspileFailed { path, message } => {
                    Err(StartupRuntimePrepareError::TranspileFailed { path, message })
                }
            }
        })
    }
}

pub(super) fn spawn_startup_runtime_prepare_runner(
    config_path: PathBuf,
    current_dir: PathBuf,
) -> SayaStartupPhaseRunner<StartupRuntimePrepareEvaluator> {
    log::debug!(
        "[saya_live_runtime] spawn startup runtime prepare runner: config_path={}, current_dir={}",
        config_path.display(),
        current_dir.display()
    );
    SayaStartupPhaseRunner::new(Arc::new(StartupRuntimePrepareEvaluator {
        config_path,
        current_dir,
    }))
}
