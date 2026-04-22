use saya::core_prompt::{PromptResponseCommand, PromptResponseError, PromptResponseRejection};

#[test]
fn prompt_response_command_exposes_submit_and_cancel_without_raw_core_types() {
    let submit = PromptResponseCommand::Submit {
        correlation_id: 10,
        value: "alice".to_string(),
    };
    let cancel = PromptResponseCommand::Cancel { correlation_id: 11 };

    assert_eq!(submit.correlation_id(), 10);
    assert_eq!(cancel.correlation_id(), 11);
    assert!(matches!(
        submit,
        PromptResponseCommand::Submit {
            correlation_id: 10,
            ..
        }
    ));
    assert!(matches!(
        cancel,
        PromptResponseCommand::Cancel { correlation_id: 11 }
    ));
}

#[test]
fn prompt_response_errors_distinguish_application_and_core_rejection_cases() {
    let no_active = PromptResponseError::NoActivePrompt;
    let mismatch = PromptResponseError::CorrelationMismatch {
        expected: 10,
        actual: 11,
    };
    let rejected =
        PromptResponseError::CoreRejected(PromptResponseRejection::CoreCorrelationMismatch {
            expected: 12,
            actual: 13,
        });

    assert!(matches!(no_active, PromptResponseError::NoActivePrompt));
    assert!(matches!(
        mismatch,
        PromptResponseError::CorrelationMismatch {
            expected: 10,
            actual: 11
        }
    ));
    assert!(matches!(
        rejected,
        PromptResponseError::CoreRejected(PromptResponseRejection::CoreCorrelationMismatch {
            expected: 12,
            actual: 13
        })
    ));
}
