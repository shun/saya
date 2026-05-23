use std::path::PathBuf;
use std::sync::Arc;

use saya::runtime::callback_registry_seed::CallbackRegistrySeed;
use saya::runtime::live::{
    BoxFuture, BufferEventPayload, HostCapabilityBridge, ReadonlyBufferSnapshot,
    ReadonlyEditorSnapshot, ReadonlyWindowSnapshot, RuntimeCommandError, RuntimeEventPayload,
    RuntimeMode,
};
use saya::runtime::startup::StartupRegistryEntry;
use tokio::sync::Mutex;

#[derive(Default)]
pub struct CommandRecordingHostBridge {
    executed_commands: Arc<Mutex<Vec<String>>>,
}

impl CommandRecordingHostBridge {
    pub fn new() -> Self {
        Self::default()
    }
}

impl HostCapabilityBridge for CommandRecordingHostBridge {
    fn execute_host_command(&self, name: &str) -> BoxFuture<Result<(), RuntimeCommandError>> {
        let executed_commands = self.executed_commands.clone();
        let name = name.to_string();
        Box::pin(async move {
            executed_commands.lock().await.push(name);
            Ok(())
        })
    }

    fn current_buffer(&self) -> BoxFuture<ReadonlyBufferSnapshot> {
        Box::pin(async move {
            ReadonlyBufferSnapshot {
                id: 1,
                path: None,
                line_count: 0,
                cursor_row: 0,
                cursor_col: 0,
                current_line: String::new(),
                text: String::new(),
            }
        })
    }

    fn current_window(&self) -> BoxFuture<ReadonlyWindowSnapshot> {
        Box::pin(async move { ReadonlyWindowSnapshot { id: 1 } })
    }

    fn current_editor(&self) -> BoxFuture<ReadonlyEditorSnapshot> {
        Box::pin(async move {
            ReadonlyEditorSnapshot {
                mode: RuntimeMode::Normal,
            }
        })
    }
}

pub fn buffer_open_payload(path: impl Into<PathBuf>) -> RuntimeEventPayload {
    RuntimeEventPayload::BufferOpen(BufferEventPayload {
        buffer: ReadonlyBufferSnapshot {
            id: 1,
            path: Some(path.into()),
            line_count: 1,
            cursor_row: 0,
            cursor_col: 0,
            current_line: String::new(),
            text: String::new(),
        },
    })
}

pub fn seed_with_buffer_open_handler(handler_source: &str) -> CallbackRegistrySeed {
    CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
        name: "bufferOpen".to_string(),
        callback_source: handler_source.to_string(),
    }])
}
