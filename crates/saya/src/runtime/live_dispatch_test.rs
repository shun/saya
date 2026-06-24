use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use super::live_test_support::RecordingHostBridge;
use super::{
    BufferEventPayload, CallbackRegistryBuilder, ReadonlyBufferSnapshot, RuntimeEventPayload,
    SayaLiveRuntime,
};

#[tokio::test(flavor = "current_thread")]
async fn runtime_dispatch_preserves_registration_order_without_blocking_sender() {
    let trace = Arc::new(Mutex::new(Vec::new()));
    let first_trace = trace.clone();
    let second_trace = trace.clone();

    let registry = CallbackRegistryBuilder::default()
        .on_buffer_open(move |_, payload| {
            let first_trace = first_trace.clone();
            Box::pin(async move {
                first_trace
                    .lock()
                    .await
                    .push(format!("first:{:?}", payload.buffer.path));
                tokio::time::sleep(Duration::from_millis(30)).await;
                first_trace.lock().await.push("first:done".to_string());
                Ok(())
            })
        })
        .on_buffer_open(move |_, payload| {
            let second_trace = second_trace.clone();
            Box::pin(async move {
                second_trace
                    .lock()
                    .await
                    .push(format!("second:{:?}", payload.buffer.path));
                Ok(())
            })
        })
        .build();

    let runtime = SayaLiveRuntime::spawn(Arc::new(RecordingHostBridge::new()), registry);
    let payload = RuntimeEventPayload::BufferOpen(BufferEventPayload {
        buffer: ReadonlyBufferSnapshot {
            id: 11,
            path: Some(PathBuf::from("article.md")),
            line_count: 8,
            cursor_row: 0,
            cursor_col: 0,
            current_line: String::new(),
            text: String::new(),
        },
    });

    let started_at = Instant::now();
    let receipt = runtime
        .dispatch_event(payload)
        .expect("dispatch should succeed");

    assert!(
        started_at.elapsed() < Duration::from_millis(20),
        "dispatch should queue work without waiting for handlers"
    );

    let report = receipt.await_result().await.expect("dispatch result");
    assert_eq!(report.handler_count, 2);

    let trace = trace.lock().await.clone();
    assert_eq!(
        trace,
        vec![
            "first:Some(\"article.md\")".to_string(),
            "first:done".to_string(),
            "second:Some(\"article.md\")".to_string(),
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_callback_can_execute_registered_command_and_read_typed_state() {
    let host_bridge = Arc::new(RecordingHostBridge::new());
    let observed = Arc::new(Mutex::new(Vec::new()));
    let observed_in_command = observed.clone();
    let observed_in_event = observed.clone();

    let registry = CallbackRegistryBuilder::default()
        .register_command("writeCurrent", move |ctx| {
            let observed_in_command = observed_in_command.clone();
            Box::pin(async move {
                let buffer = ctx.buffer().current().await;
                let editor_mode = ctx.editor().mode().await;
                observed_in_command
                    .lock()
                    .await
                    .push(format!("command:{:?}:{:?}", buffer.path, editor_mode));
                ctx.commands().execute("write").await
            })
        })
        .on_buffer_open(move |ctx, payload| {
            let observed_in_event = observed_in_event.clone();
            Box::pin(async move {
                observed_in_event.lock().await.push(format!(
                    "event:{}:{:?}",
                    payload.buffer.id, payload.buffer.path
                ));
                ctx.commands().execute("writeCurrent").await?;
                Ok(())
            })
        })
        .build();

    let runtime = SayaLiveRuntime::spawn(host_bridge.clone(), registry);
    let receipt = runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: ReadonlyBufferSnapshot {
                id: 21,
                path: Some(PathBuf::from("typed.md")),
                line_count: 5,
                cursor_row: 0,
                cursor_col: 0,
                current_line: String::new(),
                text: String::new(),
            },
        }))
        .expect("dispatch should succeed");

    let report = receipt.await_result().await.expect("dispatch report");
    assert_eq!(report.handler_count, 1);

    assert_eq!(
        observed.lock().await.clone(),
        vec![
            "event:21:Some(\"typed.md\")".to_string(),
            "command:Some(\"notes.md\"):Normal".to_string(),
        ]
    );
    assert_eq!(
        host_bridge.executed_commands.lock().await.clone(),
        vec!["write".to_string()]
    );
}
