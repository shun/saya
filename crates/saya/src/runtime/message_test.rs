use super::*;

#[test]
fn runtime_callback_failure_message_formats_script_failure_for_ui() {
    let error = RuntimeDispatchError::CallbackFailed {
        event: RuntimeEventName::BufferOpen,
        handler_index: 0,
        error: RuntimeCallbackError::ScriptFailed {
            message: "boom".to_string(),
        },
    };

    assert_eq!(
        runtime_callback_failure_message(&error),
        Some("Runtime callback failed on bufferOpen handler 0: script error: boom".to_string())
    );
}

#[test]
fn runtime_callback_failure_message_formats_command_failure_for_ui() {
    let error = RuntimeDispatchError::CallbackFailed {
        event: RuntimeEventName::BufferWritePost,
        handler_index: 2,
        error: RuntimeCallbackError::Command(RuntimeCommandError::UnknownCommand {
            name: "missing".to_string(),
        }),
    };

    assert_eq!(
        runtime_callback_failure_message(&error),
        Some("Runtime callback failed on bufferWritePost handler 2: command error: unknown command: missing".to_string())
    );
}

#[test]
fn runtime_callback_failure_message_ignores_non_callback_errors() {
    assert_eq!(
        runtime_callback_failure_message(&RuntimeDispatchError::QueueClosed),
        None
    );
    assert_eq!(
        runtime_callback_failure_message(&RuntimeDispatchError::WorkerStopped),
        None
    );
}
