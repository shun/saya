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
#[path = "message_test.rs"]
mod tests;
