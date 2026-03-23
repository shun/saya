use std::path::PathBuf;
use std::sync::Arc;

use saya::callback_registry_seed::CallbackRegistrySeed;
use saya::saya_live_runtime::{
    BoxFuture, HostCapabilityBridge, ReadonlyEditorSnapshot, ReadonlyWindowSnapshot,
    RuntimeCommandError, RuntimeMode,
};
use saya::saya_live_runtime::{
    BufferEventPayload, ReadonlyBufferSnapshot, RuntimeEventPayload, SayaLiveRuntime,
    runtime_forbidden_surface_names, runtime_public_surface_names,
};
use saya::startup_runtime::StartupRegistryEntry;
use saya::startup_runtime::{
    evaluate_startup_module, startup_forbidden_surface_names, startup_public_surface_names,
};

#[test]
fn startup_surface_excludes_filesystem_and_network_capabilities() {
    let surface = startup_public_surface_names();

    assert_eq!(surface, &["options", "keymap", "commands", "events"]);
    assert_eq!(
        startup_forbidden_surface_names(),
        &["filesystem", "network"]
    );
    assert!(!surface.contains(&"filesystem"));
    assert!(!surface.contains(&"network"));
}

#[tokio::test(flavor = "current_thread")]
async fn startup_runtime_does_not_expose_filesystem_or_network() {
    evaluate_startup_module(
        r#"
            if (typeof saya.filesystem !== "undefined") {
                throw new Error("filesystem capability leaked into startup surface");
            }
            if (typeof saya.network !== "undefined") {
                throw new Error("network capability leaked into startup surface");
            }
        "#,
    )
    .await
    .expect("startup surface should hide filesystem and network");
}

#[test]
fn runtime_surface_excludes_filesystem_and_network_capabilities() {
    let surface = runtime_public_surface_names();

    assert_eq!(surface, &["commands", "buffer", "window", "editor"]);
    assert_eq!(
        runtime_forbidden_surface_names(),
        &["filesystem", "network"]
    );
    assert!(!surface.contains(&"filesystem"));
    assert!(!surface.contains(&"network"));
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_does_not_expose_filesystem_or_network() {
    let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
        name: "bufferOpen".to_string(),
        callback_source: r#"
            (payload) => {
                if (typeof saya.filesystem !== "undefined") {
                    throw new Error("filesystem capability leaked into runtime surface");
                }
                if (typeof saya.network !== "undefined") {
                    throw new Error("network capability leaked into runtime surface");
                }
                return saya.commands.execute(`buffer:${payload.buffer.id}`);
            }
        "#
        .to_string(),
    }]);

    let runtime = SayaLiveRuntime::spawn_from_seed(Arc::new(NoopHostBridge), seed)
        .expect("runtime should initialize");
    let report = runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: ReadonlyBufferSnapshot {
                id: 7,
                path: Some(PathBuf::from("surface-guard.md")),
                line_count: 1,
            },
        }))
        .expect("dispatch queued")
        .await_result()
        .await
        .expect("dispatch result");

    assert_eq!(report.handler_count, 1);
}

struct NoopHostBridge;

impl HostCapabilityBridge for NoopHostBridge {
    fn execute_host_command(&self, _name: &str) -> BoxFuture<Result<(), RuntimeCommandError>> {
        Box::pin(async move { Ok(()) })
    }

    fn current_buffer(&self) -> BoxFuture<ReadonlyBufferSnapshot> {
        Box::pin(async move {
            ReadonlyBufferSnapshot {
                id: 1,
                path: None,
                line_count: 0,
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
