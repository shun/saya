use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;

use super::{
    HostCapabilityBridge, ReadonlyBufferSnapshot, ReadonlyEditorSnapshot, ReadonlyWindowSnapshot,
    RuntimeCommandError, RuntimeMode, SayaStartupPhaseEvaluator,
};

pub(super) fn unique_path(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-live-runtime-{name}-{nanos}"))
}

pub(super) struct RecordingHostBridge {
    pub(super) executed_commands: Arc<Mutex<Vec<String>>>,
    command_results: HashMap<String, RuntimeCommandError>,
}

impl RecordingHostBridge {
    pub(super) fn new() -> Self {
        Self {
            executed_commands: Arc::new(Mutex::new(Vec::new())),
            command_results: HashMap::new(),
        }
    }

    pub(super) fn with_command_error(name: &str, error: RuntimeCommandError) -> Self {
        let mut command_results = HashMap::new();
        command_results.insert(name.to_string(), error);
        Self {
            executed_commands: Arc::new(Mutex::new(Vec::new())),
            command_results,
        }
    }
}

impl HostCapabilityBridge for RecordingHostBridge {
    fn execute_host_command(
        &self,
        name: &str,
    ) -> super::BoxFuture<Result<(), RuntimeCommandError>> {
        let executed_commands = self.executed_commands.clone();
        let name = name.to_string();
        let result = self.command_results.get(&name).cloned();
        Box::pin(async move {
            if let Some(error) = result {
                return Err(error);
            }
            executed_commands.lock().await.push(name);
            Ok(())
        })
    }

    fn current_buffer(&self) -> super::BoxFuture<ReadonlyBufferSnapshot> {
        Box::pin(async move {
            ReadonlyBufferSnapshot {
                id: 7,
                path: Some(PathBuf::from("notes.md")),
                line_count: 3,
                cursor_row: 0,
                cursor_col: 0,
                current_line: String::new(),
                text: String::new(),
            }
        })
    }

    fn current_window(&self) -> super::BoxFuture<ReadonlyWindowSnapshot> {
        Box::pin(async move { ReadonlyWindowSnapshot { id: 9 } })
    }

    fn current_editor(&self) -> super::BoxFuture<ReadonlyEditorSnapshot> {
        Box::pin(async move {
            ReadonlyEditorSnapshot {
                mode: RuntimeMode::Normal,
            }
        })
    }
}

pub(super) struct SleepingStartupEvaluator;

impl SayaStartupPhaseEvaluator for SleepingStartupEvaluator {
    type Output = &'static str;
    type Error = &'static str;

    fn evaluate(&self) -> super::BoxFuture<Result<Self::Output, Self::Error>> {
        Box::pin(async move {
            tokio::time::sleep(Duration::from_millis(40)).await;
            Ok("startup-ready")
        })
    }
}
