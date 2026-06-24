use super::*;
use crate::runtime::live::{RuntimeCallbackError, RuntimeEventName};

#[test]
fn runtime_outcome_projector_requests_redraw_for_callback_failure() {
    let projected = RuntimeOutcomeProjector::project_error(&RuntimeDispatchError::CallbackFailed {
        event: RuntimeEventName::BufferOpen,
        handler_index: 0,
        error: RuntimeCallbackError::ScriptFailed {
            message: "boom".to_string(),
        },
    });

    assert_eq!(
        projected,
        RuntimeDispatchOutcome {
            transient_message: Some(
                "Runtime callback failed on bufferOpen handler 0: script error: boom".to_string()
            ),
            requires_redraw: true,
            shutdown_intent: None,
            presentation_intents: Vec::new(),
        }
    );
}
