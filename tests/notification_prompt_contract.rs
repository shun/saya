use std::sync::{Mutex, OnceLock};

use saya::core_notification_prompt::{
    BellIndication, InputPromptStatus, InputPromptView, MessageLineCandidate, MessageLineSource,
    NotificationPromptProjectionState, PagerPromptView, ProjectionFrame,
    PromptHintSuppressionReason, PromptInputAction, PromptTransitionKind,
    RetainedPromptState, SuppressedPromptHint, handle_prompt_key,
    record_prompt_response_error, resolve_workspace_message_line,
};
use saya::core_outcome::{
    ApplicationOutcomeState, NormalizedCoreOutcome, NormalizedOutcomeBatch, NormalizedPrompt,
    OutcomeOrigin, OutcomeTrace, PromptResponseDisposition, fold_normalized_outcomes,
};
use saya::core_prompt::PromptResponseError;
use saya::input_router::KeyInput;
use vim_core_rs::{CoreInputRequestKind, CorePagerPromptKind};

struct TestLogger {
    lines: Mutex<Vec<String>>,
}

fn test_logger_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

impl TestLogger {
    fn init() -> &'static Self {
        static LOGGER: OnceLock<TestLogger> = OnceLock::new();
        let logger = LOGGER.get_or_init(|| TestLogger {
            lines: Mutex::new(Vec::new()),
        });
        let _ = log::set_logger(logger);
        log::set_max_level(log::LevelFilter::Debug);
        logger.clear();
        logger
    }

    fn clear(&self) {
        self.lines
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }

    fn lines(&self) -> Vec<String> {
        self.lines
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl log::Log for TestLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Debug
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            self.lines
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(format!("{}", record.args()));
        }
    }

    fn flush(&self) {}
}

#[test]
fn projection_frame_resolves_visible_source_and_retains_suppressed_candidates() {
    let frame = ProjectionFrame {
        sequence: 7,
        replayed: false,
        response_error: None,
        bell: Some(BellIndication { count: 2 }),
        message_line: resolve_workspace_message_line(vec![
            MessageLineCandidate::legacy(MessageLineSource::TransientInfo, "saved"),
            MessageLineCandidate::legacy(MessageLineSource::CoreNotification, "core note"),
            MessageLineCandidate::legacy(MessageLineSource::SystemWarning, "warning"),
            MessageLineCandidate::legacy(MessageLineSource::CommandPreview, ":%s/foo/bar"),
        ]),
        prompt: RetainedPromptState::default(),
        pager_prompt: Some(PagerPromptView {
            kind: CorePagerPromptKind::More,
            one_shot: true,
        }),
        suppressed_prompt_hints: vec![SuppressedPromptHint {
            pager_prompt: PagerPromptView {
                kind: CorePagerPromptKind::More,
                one_shot: true,
            },
            reason: PromptHintSuppressionReason::ActiveInputPrompt,
        }],
        input_prompt: None,
    };

    assert_eq!(
        frame.message_line.visible_source(),
        Some(MessageLineSource::CommandPreview)
    );
    assert_eq!(frame.message_line.visible_text(), Some(":%s/foo/bar"));
    assert_eq!(
        frame.message_line.suppressed_sources(),
        vec![
            MessageLineSource::SystemWarning,
            MessageLineSource::CoreNotification,
            MessageLineSource::TransientInfo,
        ]
    );
    assert!(!frame.message_line.is_empty());
    assert_eq!(frame.bell.as_ref().map(|bell| bell.count), Some(2));
    assert!(frame.pager_prompt.is_some());
    assert_eq!(frame.suppressed_prompt_hints.len(), 1);

    let workspace_view = frame.workspace_view();
    assert_eq!(
        workspace_view
            .notification_message
            .as_ref()
            .map(|message| message.text.as_str()),
        Some(":%s/foo/bar")
    );
    assert_eq!(workspace_view.bell.as_ref().map(|bell| bell.count), Some(2));
}

#[test]
fn retained_prompt_state_tracks_requested_superseded_submitted_and_cancelled_without_leaking_values()
 {
    let _guard = test_logger_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _logger = TestLogger::init();

    let mut prompt = RetainedPromptState::default();
    let requested = prompt.requested(InputPromptView {
        prompt: "Name:".to_string(),
        input: "alpha".to_string(),
        correlation_id: 11,
        input_kind: CoreInputRequestKind::CommandLine,
        status: InputPromptStatus::Active,
    });
    assert_eq!(requested.kind, PromptTransitionKind::Requested);
    assert_eq!(
        prompt.active_input().map(|view| view.correlation_id),
        Some(11)
    );

    let superseded = prompt.superseded(InputPromptView {
        prompt: "Password:".to_string(),
        input: "beta".to_string(),
        correlation_id: 12,
        input_kind: CoreInputRequestKind::Secret,
        status: InputPromptStatus::Active,
    });
    assert_eq!(superseded.kind, PromptTransitionKind::Superseded);
    assert_eq!(
        prompt.active_input().map(|view| view.correlation_id),
        Some(12)
    );

    let submitted = prompt.submitted(12, "secret-value");
    assert_eq!(submitted.kind, PromptTransitionKind::Submitted);
    assert!(prompt.active_input().is_none());
    assert!(prompt.last_transition().is_some());
    assert!(prompt.last_response_error().is_none());

    let mut cancelled_prompt = RetainedPromptState::default();
    let _ = cancelled_prompt.requested(InputPromptView {
        prompt: "Token:".to_string(),
        input: "gamma".to_string(),
        correlation_id: 13,
        input_kind: CoreInputRequestKind::Secret,
        status: InputPromptStatus::Active,
    });
    let cancelled = cancelled_prompt.cancelled(13);
    assert_eq!(cancelled.kind, PromptTransitionKind::Cancelled);
    assert!(cancelled_prompt.active_input().is_none());
    assert_eq!(cancelled.input_len, 0);
    assert_eq!(cancelled.correlation_id, 13);
    assert!(cancelled_prompt.last_response_error().is_none());
}

#[test]
fn projection_frame_logs_metadata_without_prompt_input_values() {
    let _guard = test_logger_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let logger = TestLogger::init();

    let mut prompt = RetainedPromptState::default();
    let _ = prompt.requested(InputPromptView {
        prompt: "Enter password:".to_string(),
        input: "redacted-value".to_string(),
        correlation_id: 21,
        input_kind: CoreInputRequestKind::Secret,
        status: InputPromptStatus::Active,
    });
    let _ = prompt.submitted(21, "never-log-me");

    let frame = ProjectionFrame {
        sequence: 42,
        replayed: true,
        response_error: Some("core rejected prompt response".to_string()),
        bell: Some(BellIndication { count: 1 }),
        message_line: resolve_workspace_message_line(vec![
            MessageLineCandidate::legacy(MessageLineSource::CoreNotification, "visible"),
            MessageLineCandidate::legacy(MessageLineSource::TransientInfo, "fallback"),
        ]),
        prompt,
        pager_prompt: None,
        suppressed_prompt_hints: vec![],
        input_prompt: None,
    };
    frame.log_metadata();

    let logs = logger.lines();
    assert!(
        logs.iter().any(|line| line.contains("sequence=42")),
        "frame log should include the frame sequence"
    );
    assert!(
        logs.iter()
            .any(|line| line.contains("visible_source=Some(CommandPreview)")
                || line.contains("visible_source=Some(CoreNotification)")),
        "frame log should include the visible source"
    );
    assert!(
        logs.iter().any(|line| line.contains("suppressed_sources=")),
        "frame log should include suppressed source metadata"
    );
    assert!(
        logs.iter().any(|line| line.contains("correlation_id=21")),
        "prompt transition logs should include the correlation id"
    );
    assert!(
        logs.iter().any(|line| line.contains("input_kind=Secret")),
        "prompt transition logs should include the input kind"
    );
    assert!(
        logs.iter()
            .any(|line| line.contains("input_len=12") || line.contains("input_len=14")),
        "prompt transition logs should redact values into length metadata"
    );
    for forbidden in ["redacted-value", "never-log-me"] {
        assert!(
            !logs.iter().any(|line| line.contains(forbidden)),
            "logs must not contain prompt values: {forbidden}"
        );
    }
}

#[test]
fn projection_frame_logs_prompt_transition_kind_for_failure_tracing() {
    let _guard = test_logger_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let logger = TestLogger::init();

    let mut prompt = RetainedPromptState::default();
    let _ = prompt.requested(InputPromptView {
        prompt: "Enter password:".to_string(),
        input: "redacted-value".to_string(),
        correlation_id: 22,
        input_kind: CoreInputRequestKind::Secret,
        status: InputPromptStatus::Active,
    });
    let _ = prompt.submitted(22, "never-log-me");
    logger.clear();

    let frame = ProjectionFrame {
        sequence: 43,
        replayed: false,
        response_error: Some("core rejected prompt response".to_string()),
        bell: Some(BellIndication { count: 1 }),
        message_line: resolve_workspace_message_line(vec![MessageLineCandidate::legacy(
            MessageLineSource::CoreNotification,
            "visible",
        )]),
        prompt,
        pager_prompt: None,
        suppressed_prompt_hints: vec![],
        input_prompt: None,
    };
    frame.log_metadata();

    let logs = logger.lines();
    assert!(
        logs.iter()
            .any(|line| line.contains("transition_kind=Submitted")),
        "frame log should include the prompt transition kind for lifecycle tracing"
    );
}

#[test]
fn empty_message_candidates_are_suppressed_but_observable_in_logs() {
    let _guard = test_logger_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let logger = TestLogger::init();
    let state = resolve_workspace_message_line(vec![
        MessageLineCandidate::legacy(MessageLineSource::CoreNotification, "  "),
        MessageLineCandidate::legacy(MessageLineSource::TransientInfo, " "),
    ]);

    assert!(state.visible.is_none());
    assert!(state.suppressed.is_empty());

    let logs = logger.lines();
    assert!(
        logs.iter()
            .any(|line| line.contains("empty message candidate")),
        "empty message suppression should be recorded in logs"
    );
}

fn trace(sequence: u64, raw_kind: &'static str) -> OutcomeTrace {
    OutcomeTrace {
        sequence,
        origin: OutcomeOrigin::TransactionEvent,
        raw_kind,
    }
}

fn apply_projection(
    state: &mut NotificationPromptProjectionState,
    application_state: &mut ApplicationOutcomeState,
    batch: NormalizedOutcomeBatch,
) -> ProjectionFrame {
    let folded = fold_normalized_outcomes(batch, std::mem::take(application_state));
    *application_state = folded.state.clone();
    state.apply_seam(folded.downstream_consume_seam())
}

#[test]
fn apply_seam_projects_request_input_and_non_replayed_pager_prompt() {
    let mut state = NotificationPromptProjectionState::default();
    let mut application_state = ApplicationOutcomeState::default();

    let request_frame = apply_projection(
        &mut state,
        &mut application_state,
        NormalizedOutcomeBatch::new(vec![
            NormalizedCoreOutcome::Prompt(NormalizedPrompt::PagerPrompt {
                kind: CorePagerPromptKind::More,
                trace: trace(1, "CoreEvent::PagerPrompt"),
            }),
            NormalizedCoreOutcome::Prompt(NormalizedPrompt::RequestInput {
                prompt: "Name:".to_string(),
                input_kind: CoreInputRequestKind::CommandLine,
                correlation_id: 41,
                trace: trace(2, "CoreHostAction::RequestInput"),
            }),
        ]),
    );

    assert_eq!(request_frame.sequence, 1);
    assert_eq!(
        request_frame.input_prompt.as_ref().map(|view| view.correlation_id),
        Some(41)
    );
    assert_eq!(request_frame.pager_prompt, None);
    assert_eq!(request_frame.suppressed_prompt_hints.len(), 1);
    assert_eq!(
        request_frame.suppressed_prompt_hints[0].reason,
        PromptHintSuppressionReason::ActiveInputPrompt
    );

    let replay_frame = apply_projection(
        &mut state,
        &mut application_state,
        NormalizedOutcomeBatch::new(vec![]),
    );
    assert_eq!(replay_frame.sequence, 2);
    assert_eq!(
        replay_frame.input_prompt.as_ref().map(|view| view.correlation_id),
        Some(41)
    );
    assert!(replay_frame.pager_prompt.is_none());
    assert!(
        replay_frame.suppressed_prompt_hints.is_empty(),
        "pager prompt must stay one-shot and must not replay on the next dispatch"
    );
}

#[test]
fn apply_seam_closes_prompt_only_after_folded_submission_transition() {
    let mut state = NotificationPromptProjectionState::default();
    let mut application_state = ApplicationOutcomeState::default();

    let requested = apply_projection(
        &mut state,
        &mut application_state,
        NormalizedOutcomeBatch::new(vec![NormalizedCoreOutcome::Prompt(
            NormalizedPrompt::RequestInput {
                prompt: "Name:".to_string(),
                input_kind: CoreInputRequestKind::CommandLine,
                correlation_id: 7,
                trace: trace(1, "CoreHostAction::RequestInput"),
            },
        )]),
    );
    assert_eq!(
        requested.input_prompt.as_ref().map(|view| view.correlation_id),
        Some(7)
    );

    match handle_prompt_key(&mut state, &KeyInput::Char('a')) {
        PromptInputAction::Consumed => {}
        other => panic!("expected prompt char input to be consumed, got {other:?}"),
    }
    let submit = handle_prompt_key(&mut state, &KeyInput::Enter);
    match submit {
        PromptInputAction::Submit(command) => {
            assert_eq!(command.correlation_id(), 7);
        }
        other => panic!("expected submit action, got {other:?}"),
    }
    assert!(matches!(
        state.prompt().active_input().map(|view| view.status),
        Some(InputPromptStatus::AwaitingCore { .. })
    ));

    let completed = apply_projection(
        &mut state,
        &mut application_state,
        NormalizedOutcomeBatch::new(vec![NormalizedCoreOutcome::Prompt(
            NormalizedPrompt::InputResponseAccepted {
                correlation_id: 7,
                disposition: PromptResponseDisposition::Submitted,
                trace: trace(2, "PromptResponse::Submit"),
            },
        )]),
    );

    assert!(completed.input_prompt.is_none());
    assert!(state.prompt().active_input().is_none());
    assert_eq!(
        state.prompt().last_transition().map(|transition| transition.kind),
        Some(PromptTransitionKind::Submitted)
    );
}

#[test]
fn handle_prompt_key_and_response_error_restore_active_prompt_without_losing_buffer() {
    let mut state = NotificationPromptProjectionState::default();
    let mut application_state = ApplicationOutcomeState::default();
    let _ = apply_projection(
        &mut state,
        &mut application_state,
        NormalizedOutcomeBatch::new(vec![NormalizedCoreOutcome::Prompt(
            NormalizedPrompt::RequestInput {
                prompt: "Token:".to_string(),
                input_kind: CoreInputRequestKind::Secret,
                correlation_id: 19,
                trace: trace(1, "CoreHostAction::RequestInput"),
            },
        )]),
    );

    for key in [KeyInput::Char(':'), KeyInput::Char('/'), KeyInput::Char('x')] {
        match handle_prompt_key(&mut state, &key) {
            PromptInputAction::Consumed => {}
            other => panic!("expected prompt input to stay inside prompt controller, got {other:?}"),
        }
    }

    let submit = handle_prompt_key(&mut state, &KeyInput::Enter);
    match submit {
        PromptInputAction::Submit(command) => {
            assert_eq!(command.correlation_id(), 19);
        }
        other => panic!("expected submit action, got {other:?}"),
    }
    assert_eq!(
        state.prompt().active_input().map(|view| view.input.as_str()),
        Some(":/x")
    );

    record_prompt_response_error(
        &mut state,
        PromptResponseError::CorrelationMismatch {
            expected: 19,
            actual: 20,
        },
    );

    assert!(matches!(
        state.prompt().active_input().map(|view| view.status),
        Some(InputPromptStatus::Active)
    ));
    assert_eq!(
        state.prompt().active_input().map(|view| view.input.as_str()),
        Some(":/x")
    );
    assert!(
        state
            .prompt()
            .last_response_error()
            .is_some_and(|message| message.contains("expected=19"))
    );

    match handle_prompt_key(&mut state, &KeyInput::Escape) {
        PromptInputAction::Cancel(command) => {
            assert_eq!(command.correlation_id(), 19);
        }
        other => panic!("expected cancel action after restore, got {other:?}"),
    }
}
