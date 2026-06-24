use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::runtime::callback_registry_seed::CallbackRegistrySeed;
use crate::runtime::config::StartupRegistryEntry;

use super::live_test_support::{RecordingHostBridge, unique_path};
use super::{
    BufferEventPayload, ReadonlyBufferSnapshot, RuntimeCallbackError, RuntimeCommandError,
    RuntimeDispatchError, RuntimeEventName, RuntimeEventPayload, SayaLiveRuntime,
};

fn buffer_snapshot(id: u64, path: &str, line_count: usize) -> ReadonlyBufferSnapshot {
    ReadonlyBufferSnapshot {
        id,
        path: Some(PathBuf::from(path)),
        line_count,
        cursor_row: 0,
        cursor_col: 0,
        current_line: String::new(),
        text: String::new(),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn seed_runtime_can_read_buffer_window_editor_state_and_typed_payload() {
    let host_bridge = Arc::new(RecordingHostBridge::new());
    let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
        name: "bufferOpen".to_string(),
        callback_source: r#"
                async (payload) => {
                    const buffer = await saya.buffer.current();
                    const window = await saya.window.current();
                    const mode = await saya.editor.mode();
                    await saya.commands.execute(`state:${payload.buffer.id}:${buffer.id}:${window.id}:${mode}`);
                }
            "#
        .to_string(),
    }]);

    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), seed)
        .expect("seed runtime should initialize");

    let report = runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: buffer_snapshot(51, "typed-payload.md", 12),
        }))
        .expect("dispatch queued")
        .await_result()
        .await
        .expect("dispatch result");

    assert_eq!(report.handler_count, 1);
    assert_eq!(
        host_bridge.executed_commands.lock().await.clone(),
        vec!["state:51:7:9:Normal".to_string()]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn seed_runtime_can_dispatch_buffer_write_post_payload() {
    let host_bridge = Arc::new(RecordingHostBridge::new());
    let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
        name: "bufferWritePost".to_string(),
        callback_source:
            "(payload) => saya.commands.execute(`write-post:${payload.buffer.id}:${payload.buffer.lineCount}`)"
                .to_string(),
    }]);

    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), seed)
        .expect("seed runtime should initialize");

    let report = runtime
        .dispatch_event(RuntimeEventPayload::BufferWritePost(BufferEventPayload {
            buffer: buffer_snapshot(61, "write-post.md", 14),
        }))
        .expect("dispatch queued")
        .await_result()
        .await
        .expect("dispatch result");

    assert_eq!(report.handler_count, 1);
    assert_eq!(
        host_bridge.executed_commands.lock().await.clone(),
        vec!["write-post:61:14".to_string()]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn seed_runtime_can_list_filer_entries_for_typescript_plugin() {
    let root_path = unique_path("seed-runtime-filer-list");
    std::fs::create_dir_all(&root_path).expect("root directory");
    std::fs::write(root_path.join("alpha.txt"), "alpha\n").expect("alpha file");
    std::fs::write(root_path.join("beta.txt"), "beta\n").expect("beta file");

    let root_for_script = root_path.to_string_lossy().replace('\\', "\\\\");
    let host_bridge = Arc::new(RecordingHostBridge::new());
    let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
        name: "bufferOpen".to_string(),
        callback_source: format!(
            r#"
                async () => {{
                    const entries = await saya.filer.list("{root_for_script}", {{ sortBy: "name" }});
                    await saya.commands.execute(`filer:${{entries.map((entry) => entry.name).join(",")}}`);
                }}
            "#
        ),
    }]);

    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), seed)
        .expect("seed runtime should initialize");

    let report = runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: buffer_snapshot(62, "filer-list.md", 1),
        }))
        .expect("dispatch queued")
        .await_result()
        .await
        .expect("dispatch result");

    assert_eq!(report.handler_count, 1);
    assert_eq!(
        host_bridge.executed_commands.lock().await.clone(),
        vec!["filer:alpha.txt,beta.txt".to_string()]
    );

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[tokio::test(flavor = "current_thread")]
async fn seed_runtime_reports_unknown_command_as_structured_error() {
    let host_bridge = Arc::new(RecordingHostBridge::with_command_error(
        "missing",
        RuntimeCommandError::UnknownCommand {
            name: "missing".to_string(),
        },
    ));
    let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
        name: "bufferOpen".to_string(),
        callback_source: "(payload) => saya.commands.execute(\"missing\")".to_string(),
    }]);

    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge, seed)
        .expect("seed runtime should initialize");

    let error = runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: buffer_snapshot(71, "unknown-command.md", 2),
        }))
        .expect("dispatch queued")
        .await_result()
        .await
        .expect_err("unknown command should be surfaced");

    assert_eq!(
        error,
        RuntimeDispatchError::CallbackFailed {
            event: RuntimeEventName::BufferOpen,
            handler_index: 0,
            error: RuntimeCallbackError::Command(RuntimeCommandError::UnknownCommand {
                name: "missing".to_string(),
            }),
        }
    );
}

#[tokio::test(flavor = "current_thread")]
async fn seed_runtime_reports_script_failure_as_structured_error() {
    let host_bridge = Arc::new(RecordingHostBridge::new());
    let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
        name: "bufferOpen".to_string(),
        callback_source: "(payload) => { throw new Error(\"boom\"); }".to_string(),
    }]);

    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge, seed)
        .expect("seed runtime should initialize");

    let error = runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: buffer_snapshot(81, "script-failure.md", 6),
        }))
        .expect("dispatch queued")
        .await_result()
        .await
        .expect_err("script failure should be surfaced");

    assert!(
        matches!(
            error,
            RuntimeDispatchError::CallbackFailed {
                event: RuntimeEventName::BufferOpen,
                handler_index: 0,
                error: RuntimeCallbackError::ScriptFailed { ref message },
            } if message.contains("boom")
        ),
        "script failure should stay structured: {error:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn seed_runtime_dispatch_queues_without_blocking_sender() {
    let host_bridge = Arc::new(RecordingHostBridge::new());
    let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
        name: "bufferOpen".to_string(),
        callback_source: "(payload) => saya.commands.execute(\"write\")".to_string(),
    }]);

    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge, seed)
        .expect("seed runtime should initialize");

    let started_at = Instant::now();
    let receipt = runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: buffer_snapshot(31, "seed.md", 4),
        }))
        .expect("dispatch should be queued");

    assert!(
        started_at.elapsed() < Duration::from_millis(20),
        "dispatch should return quickly even for seed runtime"
    );

    let report = receipt.await_result().await.expect("dispatch report");
    assert_eq!(report.handler_count, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn seed_runtime_dispatches_event_handlers_in_registration_order() {
    let host_bridge = Arc::new(RecordingHostBridge::new());
    let seed = CallbackRegistrySeed::from_startup_entries(vec![
        StartupRegistryEntry::Command {
            name: "writeCurrent".to_string(),
            callback_source: "() => saya.commands.execute(\"write\")".to_string(),
        },
        StartupRegistryEntry::Event {
            name: "bufferOpen".to_string(),
            callback_source: "(payload) => saya.commands.execute(\"writeCurrent\")".to_string(),
        },
        StartupRegistryEntry::Event {
            name: "bufferOpen".to_string(),
            callback_source: "(payload) => saya.commands.execute(\"write!\")".to_string(),
        },
    ]);

    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), seed)
        .expect("seed runtime should initialize");

    let report = runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: buffer_snapshot(41, "ordered.md", 9),
        }))
        .expect("dispatch queued")
        .await_result()
        .await
        .expect("dispatch result");

    assert_eq!(report.handler_count, 2);
    assert_eq!(
        host_bridge.executed_commands.lock().await.clone(),
        vec!["write".to_string(), "write!".to_string()]
    );
}
