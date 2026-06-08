use crate::runtime::live::{
    RuntimeCallbackError, RuntimeCommandError, RuntimeDispatchError, RuntimeEventName,
};

/// runtime callback failure を application の transient message に変換する。
pub fn runtime_callback_failure_message(error: &RuntimeDispatchError) -> Option<String> {
    match error {
        RuntimeDispatchError::CallbackFailed {
            event,
            handler_index,
            error,
        } => {
            let message = format!(
                "Runtime callback failed on {} handler {}: {}",
                runtime_event_name_label(event),
                handler_index,
                runtime_callback_error_message(error),
            );
            log::debug!(
                "[runtime_message] projected runtime callback failure into application message: event={:?}, handler_index={}, message={}",
                event,
                handler_index,
                message
            );
            Some(message)
        }
        _ => None,
    }
}

fn runtime_callback_error_message(error: &RuntimeCallbackError) -> String {
    match error {
        RuntimeCallbackError::Command(command_error) => {
            format!(
                "command error: {}",
                runtime_command_error_message(command_error)
            )
        }
        RuntimeCallbackError::ScriptFailed { message } => {
            format!("script error: {}", message)
        }
    }
}

fn runtime_command_error_message(error: &RuntimeCommandError) -> String {
    match error {
        RuntimeCommandError::UnknownCommand { name } => format!("unknown command: {}", name),
        RuntimeCommandError::CommandFailed { name, message } => {
            format!("command {} failed: {}", name, message)
        }
        RuntimeCommandError::CircularCommand { name } => {
            format!("circular command: {}", name)
        }
    }
}

fn runtime_event_name_label(event: &RuntimeEventName) -> &'static str {
    match event {
        RuntimeEventName::BufferOpen => "bufferOpen",
        RuntimeEventName::BufferChanged => "bufferChanged",
        RuntimeEventName::BufferWritePost => "bufferWritePost",
        RuntimeEventName::BufferClosed => "bufferClosed",
    }
}

#[cfg(test)]
mod tests {
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
}
