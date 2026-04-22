use saya::core_outcome::{
    ApplicationOutcomeState, NormalizedCoreOutcome, NormalizedDiagnosticOutcome,
    NormalizedHostDirective, NormalizedNotification, NormalizedOutcomeBatch, NormalizedPrompt,
    NormalizedStructuralOutcome, OutcomeOrigin, OutcomeTrace, PromptInputTransition,
    PromptResponseDisposition, PromptSessionStatus, core_event_raw_kind, core_host_action_raw_kind,
    fold_normalized_outcomes, normalize_core_event, normalize_host_action,
};
use vim_core_rs::{
    CoreEvent, CoreHostAction, CoreInputRequestKind, CoreJobStartRequest, CoreMessageCategory,
    CoreMessageEvent, CoreMessageSeverity, CorePagerPromptKind, CoreVfsRequest,
};

fn trace(sequence: u64, origin: OutcomeOrigin, raw_kind: &'static str) -> OutcomeTrace {
    OutcomeTrace {
        sequence,
        origin,
        raw_kind,
    }
}

fn user_message(content: &str) -> CoreMessageEvent {
    CoreMessageEvent {
        severity: CoreMessageSeverity::Info,
        category: CoreMessageCategory::UserVisible,
        content: content.to_string(),
    }
}

fn command_feedback(content: &str) -> CoreMessageEvent {
    CoreMessageEvent {
        severity: CoreMessageSeverity::Info,
        category: CoreMessageCategory::CommandFeedback,
        content: content.to_string(),
    }
}

#[test]
fn normalized_catalog_represents_scoped_outcomes_with_trace() {
    let batch = NormalizedOutcomeBatch::new(vec![
        NormalizedCoreOutcome::HostDirective(NormalizedHostDirective::Write {
            path: "notes.txt".to_string(),
            force: true,
            issued_after_revision: 7,
            trace: trace(
                1,
                OutcomeOrigin::TransactionHostAction,
                "CoreHostAction::Write",
            ),
        }),
        NormalizedCoreOutcome::HostDirective(NormalizedHostDirective::Quit {
            force: false,
            issued_after_revision: 8,
            trace: trace(
                2,
                OutcomeOrigin::TransactionHostAction,
                "CoreHostAction::Quit",
            ),
        }),
        NormalizedCoreOutcome::HostDirective(NormalizedHostDirective::VfsRequest {
            request: CoreVfsRequest::Resolve {
                request_id: 9,
                target_buf_id: 3,
                locator: "vfs://notes".to_string(),
            },
            trace: trace(
                3,
                OutcomeOrigin::TransactionHostAction,
                "CoreHostAction::VfsRequest",
            ),
        }),
        NormalizedCoreOutcome::Structural(NormalizedStructuralOutcome::RedrawRequested {
            full: true,
            clear_before_draw: true,
            trace: trace(
                4,
                OutcomeOrigin::TransactionHostAction,
                "CoreHostAction::Redraw",
            ),
        }),
        NormalizedCoreOutcome::Prompt(NormalizedPrompt::RequestInput {
            prompt: "Password:".to_string(),
            input_kind: CoreInputRequestKind::Secret,
            correlation_id: 42,
            trace: trace(
                5,
                OutcomeOrigin::TransactionHostAction,
                "CoreHostAction::RequestInput",
            ),
        }),
        NormalizedCoreOutcome::Notification(NormalizedNotification::Bell {
            trace: trace(
                6,
                OutcomeOrigin::TransactionHostAction,
                "CoreHostAction::Bell",
            ),
        }),
        NormalizedCoreOutcome::Notification(NormalizedNotification::Message {
            event: user_message("written"),
            trace: trace(7, OutcomeOrigin::TransactionEvent, "CoreEvent::Message"),
        }),
        NormalizedCoreOutcome::Prompt(NormalizedPrompt::PagerPrompt {
            kind: CorePagerPromptKind::More,
            trace: trace(8, OutcomeOrigin::TransactionEvent, "CoreEvent::PagerPrompt"),
        }),
        NormalizedCoreOutcome::Notification(NormalizedNotification::Bell {
            trace: trace(9, OutcomeOrigin::TransactionEvent, "CoreEvent::Bell"),
        }),
        NormalizedCoreOutcome::Structural(NormalizedStructuralOutcome::RedrawRequested {
            full: false,
            clear_before_draw: false,
            trace: trace(10, OutcomeOrigin::TransactionEvent, "CoreEvent::Redraw"),
        }),
        NormalizedCoreOutcome::Structural(NormalizedStructuralOutcome::BufferAdded {
            buf_id: 11,
            trace: trace(
                11,
                OutcomeOrigin::TransactionEvent,
                "CoreEvent::BufferAdded",
            ),
        }),
        NormalizedCoreOutcome::Structural(NormalizedStructuralOutcome::WindowCreated {
            win_id: 12,
            trace: trace(
                12,
                OutcomeOrigin::TransactionEvent,
                "CoreEvent::WindowCreated",
            ),
        }),
        NormalizedCoreOutcome::Structural(NormalizedStructuralOutcome::LayoutChanged {
            trace: trace(
                13,
                OutcomeOrigin::TransactionEvent,
                "CoreEvent::LayoutChanged",
            ),
        }),
        NormalizedCoreOutcome::Diagnostic(NormalizedDiagnosticOutcome::JobOutOfScope {
            raw_kind: "CoreHostAction::JobStart",
            trace: trace(
                14,
                OutcomeOrigin::TransactionHostAction,
                "CoreHostAction::JobStart",
            ),
        }),
        NormalizedCoreOutcome::Prompt(NormalizedPrompt::InputResponseAccepted {
            correlation_id: 42,
            disposition: PromptResponseDisposition::Submitted,
            trace: trace(
                15,
                OutcomeOrigin::BridgePromptResponse,
                "PromptResponse::Submit",
            ),
        }),
    ]);

    assert_eq!(batch.outcomes().len(), 15);
    assert!(
        batch
            .outcomes()
            .iter()
            .any(|outcome| matches!(outcome, NormalizedCoreOutcome::Diagnostic(_))),
        "job outcomes must stay observable as diagnostics"
    );
    assert!(
        batch
            .outcomes()
            .iter()
            .all(|outcome| outcome.trace().sequence > 0),
        "every normalized outcome must expose trace metadata"
    );
}

#[test]
fn mapping_functions_cover_scoped_host_actions_without_silent_drop() {
    let host_actions = vec![
        CoreHostAction::Write {
            path: "notes.txt".to_string(),
            force: false,
            issued_after_revision: 1,
        },
        CoreHostAction::Quit {
            force: true,
            issued_after_revision: 2,
        },
        CoreHostAction::VfsRequest(CoreVfsRequest::Exists {
            request_id: 3,
            locator: "vfs://notes".to_string(),
        }),
        CoreHostAction::Redraw {
            full: false,
            clear_before_draw: true,
        },
        CoreHostAction::RequestInput {
            prompt: "Name:".to_string(),
            input_kind: CoreInputRequestKind::CommandLine,
            correlation_id: 4,
        },
        CoreHostAction::Bell,
        CoreHostAction::JobStart(CoreJobStartRequest {
            job_id: 5,
            argv: vec!["make".to_string()],
            cwd: None,
            vfd_in: 0,
            vfd_out: 1,
            vfd_err: 2,
        }),
        CoreHostAction::JobWrite {
            vfd: 1,
            data: b"hello".to_vec(),
        },
        CoreHostAction::JobStop { job_id: 5 },
    ];

    let normalized = host_actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            normalize_host_action(
                action,
                trace(
                    index as u64 + 1,
                    OutcomeOrigin::TransactionHostAction,
                    core_host_action_raw_kind(action),
                ),
            )
        })
        .collect::<Vec<_>>();

    assert!(matches!(
        normalized[0],
        NormalizedCoreOutcome::HostDirective(NormalizedHostDirective::Write { .. })
    ));
    assert!(matches!(
        normalized[1],
        NormalizedCoreOutcome::HostDirective(NormalizedHostDirective::Quit { .. })
    ));
    assert!(matches!(
        normalized[2],
        NormalizedCoreOutcome::HostDirective(NormalizedHostDirective::VfsRequest { .. })
    ));
    assert!(matches!(
        normalized[3],
        NormalizedCoreOutcome::Structural(NormalizedStructuralOutcome::RedrawRequested {
            full: false,
            clear_before_draw: true,
            ..
        })
    ));
    assert!(matches!(
        normalized[4],
        NormalizedCoreOutcome::Prompt(NormalizedPrompt::RequestInput {
            correlation_id: 4,
            ..
        })
    ));
    assert!(matches!(
        normalized[5],
        NormalizedCoreOutcome::Notification(NormalizedNotification::Bell { .. })
    ));
    assert!(normalized[6..].iter().all(|outcome| matches!(
        outcome,
        NormalizedCoreOutcome::Diagnostic(NormalizedDiagnosticOutcome::JobOutOfScope { .. })
    )));
}

#[test]
fn mapping_functions_cover_scoped_core_events_without_silent_drop() {
    let events = vec![
        CoreEvent::Message(user_message("hello")),
        CoreEvent::PagerPrompt(CorePagerPromptKind::HitReturn),
        CoreEvent::Bell,
        CoreEvent::Redraw {
            full: true,
            clear_before_draw: true,
        },
        CoreEvent::BufferAdded { buf_id: 6 },
        CoreEvent::WindowCreated { win_id: 7 },
        CoreEvent::LayoutChanged,
    ];

    let normalized = events
        .iter()
        .enumerate()
        .map(|(index, event)| {
            normalize_core_event(
                event,
                trace(
                    index as u64 + 1,
                    OutcomeOrigin::TransactionEvent,
                    core_event_raw_kind(event),
                ),
            )
        })
        .collect::<Vec<_>>();

    assert!(matches!(
        normalized[0],
        NormalizedCoreOutcome::Notification(NormalizedNotification::Message { .. })
    ));
    assert!(matches!(
        normalized[1],
        NormalizedCoreOutcome::Prompt(NormalizedPrompt::PagerPrompt {
            kind: CorePagerPromptKind::HitReturn,
            ..
        })
    ));
    assert!(matches!(
        normalized[2],
        NormalizedCoreOutcome::Notification(NormalizedNotification::Bell { .. })
    ));
    assert!(matches!(
        normalized[3],
        NormalizedCoreOutcome::Structural(NormalizedStructuralOutcome::RedrawRequested {
            full: true,
            clear_before_draw: true,
            ..
        })
    ));
    assert!(matches!(
        normalized[4],
        NormalizedCoreOutcome::Structural(NormalizedStructuralOutcome::BufferAdded {
            buf_id: 6,
            ..
        })
    ));
    assert!(matches!(
        normalized[5],
        NormalizedCoreOutcome::Structural(NormalizedStructuralOutcome::WindowCreated {
            win_id: 7,
            ..
        })
    ));
    assert!(matches!(
        normalized[6],
        NormalizedCoreOutcome::Structural(NormalizedStructuralOutcome::LayoutChanged { .. })
    ));
}

#[test]
fn reducer_keeps_one_shot_effects_out_of_retained_state() {
    let batch = NormalizedOutcomeBatch::new(vec![
        NormalizedCoreOutcome::HostDirective(NormalizedHostDirective::Write {
            path: "notes.txt".to_string(),
            force: false,
            issued_after_revision: 2,
            trace: trace(
                1,
                OutcomeOrigin::TransactionHostAction,
                "CoreHostAction::Write",
            ),
        }),
        NormalizedCoreOutcome::HostDirective(NormalizedHostDirective::Quit {
            force: true,
            issued_after_revision: 2,
            trace: trace(
                2,
                OutcomeOrigin::TransactionHostAction,
                "CoreHostAction::Quit",
            ),
        }),
        NormalizedCoreOutcome::Notification(NormalizedNotification::Message {
            event: user_message("old"),
            trace: trace(3, OutcomeOrigin::TransactionEvent, "CoreEvent::Message"),
        }),
        NormalizedCoreOutcome::Notification(NormalizedNotification::Message {
            event: command_feedback("2 fewer lines"),
            trace: trace(4, OutcomeOrigin::TransactionEvent, "CoreEvent::Message"),
        }),
        NormalizedCoreOutcome::Notification(NormalizedNotification::Message {
            event: user_message("new"),
            trace: trace(5, OutcomeOrigin::TransactionEvent, "CoreEvent::Message"),
        }),
        NormalizedCoreOutcome::Notification(NormalizedNotification::Bell {
            trace: trace(6, OutcomeOrigin::TransactionEvent, "CoreEvent::Bell"),
        }),
        NormalizedCoreOutcome::Structural(NormalizedStructuralOutcome::RedrawRequested {
            full: false,
            clear_before_draw: true,
            trace: trace(7, OutcomeOrigin::TransactionEvent, "CoreEvent::Redraw"),
        }),
    ]);

    let folded = fold_normalized_outcomes(batch, ApplicationOutcomeState::default());

    assert_eq!(folded.effects.host_directives.len(), 2);
    assert_eq!(
        folded
            .effects
            .notification
            .latest_user_visible_message
            .as_ref()
            .map(|message| message.content.as_str()),
        Some("new")
    );
    assert_eq!(
        folded
            .effects
            .notification
            .latest_non_user_message
            .as_ref()
            .map(|message| message.content.as_str()),
        Some("2 fewer lines")
    );
    assert_eq!(folded.effects.notification.bell_count, 1);
    assert_eq!(
        folded
            .effects
            .structural
            .redraw
            .as_ref()
            .map(|redraw| redraw.clear_before_draw),
        Some(true)
    );

    let replay = fold_normalized_outcomes(NormalizedOutcomeBatch::default(), folded.state);

    assert!(replay.effects.host_directives.is_empty());
    assert!(
        replay
            .effects
            .notification
            .latest_user_visible_message
            .is_none()
    );
    assert!(replay.effects.structural.redraw.is_none());
}

#[test]
fn reducer_tracks_prompt_lifecycle_and_structural_redraw_escalation() {
    let requested = fold_normalized_outcomes(
        NormalizedOutcomeBatch::new(vec![NormalizedCoreOutcome::Prompt(
            NormalizedPrompt::RequestInput {
                prompt: "Name:".to_string(),
                input_kind: CoreInputRequestKind::CommandLine,
                correlation_id: 99,
                trace: trace(
                    1,
                    OutcomeOrigin::TransactionHostAction,
                    "CoreHostAction::RequestInput",
                ),
            },
        )]),
        ApplicationOutcomeState::default(),
    );

    let active = requested
        .state
        .prompt
        .active_input
        .as_ref()
        .expect("request input should become active");
    assert_eq!(active.status, PromptSessionStatus::WaitingForUser);
    assert!(matches!(
        requested.effects.prompt.input_transition,
        Some(PromptInputTransition::Requested { .. })
    ));

    let completed = fold_normalized_outcomes(
        NormalizedOutcomeBatch::new(vec![
            NormalizedCoreOutcome::Structural(NormalizedStructuralOutcome::BufferAdded {
                buf_id: 3,
                trace: trace(2, OutcomeOrigin::TransactionEvent, "CoreEvent::BufferAdded"),
            }),
            NormalizedCoreOutcome::Structural(NormalizedStructuralOutcome::WindowCreated {
                win_id: 5,
                trace: trace(
                    3,
                    OutcomeOrigin::TransactionEvent,
                    "CoreEvent::WindowCreated",
                ),
            }),
            NormalizedCoreOutcome::Structural(NormalizedStructuralOutcome::LayoutChanged {
                trace: trace(
                    4,
                    OutcomeOrigin::TransactionEvent,
                    "CoreEvent::LayoutChanged",
                ),
            }),
            NormalizedCoreOutcome::Prompt(NormalizedPrompt::InputResponseAccepted {
                correlation_id: 99,
                disposition: PromptResponseDisposition::Submitted,
                trace: trace(
                    5,
                    OutcomeOrigin::BridgePromptResponse,
                    "PromptResponse::Submit",
                ),
            }),
        ]),
        requested.state,
    );

    assert!(completed.state.prompt.active_input.is_none());
    assert!(matches!(
        completed.effects.prompt.input_transition,
        Some(PromptInputTransition::Submitted { correlation_id: 99 })
    ));
    assert_eq!(completed.effects.structural.invalidate_buffers, vec![3]);
    assert_eq!(completed.effects.structural.invalidate_windows, vec![5]);
    assert!(completed.effects.structural.layout_dirty);

    let redraw = completed
        .effects
        .structural
        .redraw
        .expect("structural invalidation should request redraw");
    assert!(redraw.full);
    assert!(redraw.required_by_structure_change);
}

#[test]
fn reducer_keeps_active_prompt_when_response_correlation_mismatches() {
    let requested = fold_normalized_outcomes(
        NormalizedOutcomeBatch::new(vec![NormalizedCoreOutcome::Prompt(
            NormalizedPrompt::RequestInput {
                prompt: "Name:".to_string(),
                input_kind: CoreInputRequestKind::CommandLine,
                correlation_id: 7,
                trace: trace(
                    1,
                    OutcomeOrigin::TransactionHostAction,
                    "CoreHostAction::RequestInput",
                ),
            },
        )]),
        ApplicationOutcomeState::default(),
    );

    let rejected = fold_normalized_outcomes(
        NormalizedOutcomeBatch::new(vec![NormalizedCoreOutcome::Prompt(
            NormalizedPrompt::InputResponseAccepted {
                correlation_id: 8,
                disposition: PromptResponseDisposition::Cancelled,
                trace: trace(
                    2,
                    OutcomeOrigin::BridgePromptResponse,
                    "PromptResponse::Cancel",
                ),
            },
        )]),
        requested.state,
    );

    assert_eq!(
        rejected
            .state
            .prompt
            .active_input
            .as_ref()
            .map(|session| session.correlation_id),
        Some(7)
    );
    assert_eq!(rejected.effects.diagnostics.len(), 1);
    assert_eq!(rejected.state.diagnostics.retained.len(), 1);
}

#[test]
fn downstream_consume_seam_exposes_folded_effects_and_retained_prompt_state() {
    let requested = fold_normalized_outcomes(
        NormalizedOutcomeBatch::new(vec![
            NormalizedCoreOutcome::Notification(NormalizedNotification::Message {
                event: user_message("old"),
                trace: trace(1, OutcomeOrigin::TransactionEvent, "CoreEvent::Message"),
            }),
            NormalizedCoreOutcome::Notification(NormalizedNotification::Message {
                event: user_message("new"),
                trace: trace(2, OutcomeOrigin::TransactionEvent, "CoreEvent::Message"),
            }),
            NormalizedCoreOutcome::Prompt(NormalizedPrompt::RequestInput {
                prompt: "Name:".to_string(),
                input_kind: CoreInputRequestKind::CommandLine,
                correlation_id: 42,
                trace: trace(
                    3,
                    OutcomeOrigin::TransactionHostAction,
                    "CoreHostAction::RequestInput",
                ),
            }),
            NormalizedCoreOutcome::Structural(NormalizedStructuralOutcome::RedrawRequested {
                full: false,
                clear_before_draw: true,
                trace: trace(4, OutcomeOrigin::TransactionEvent, "CoreEvent::Redraw"),
            }),
        ]),
        ApplicationOutcomeState::default(),
    );

    let seam = requested.downstream_consume_seam();

    assert_eq!(
        seam.notification()
            .latest_user_visible_message
            .as_ref()
            .map(|message| message.content.as_str()),
        Some("new")
    );
    assert!(matches!(
        seam.prompt_effect().input_transition,
        Some(PromptInputTransition::Requested { .. })
    ));
    assert_eq!(
        seam.prompt_state()
            .active_input
            .as_ref()
            .map(|session| session.correlation_id),
        Some(42)
    );
    assert_eq!(
        seam.structural()
            .redraw
            .as_ref()
            .map(|redraw| (redraw.full, redraw.clear_before_draw)),
        Some((false, true))
    );

    let completed = fold_normalized_outcomes(
        NormalizedOutcomeBatch::new(vec![NormalizedCoreOutcome::Prompt(
            NormalizedPrompt::InputResponseAccepted {
                correlation_id: 42,
                disposition: PromptResponseDisposition::Cancelled,
                trace: trace(
                    5,
                    OutcomeOrigin::BridgePromptResponse,
                    "PromptResponse::Cancel",
                ),
            },
        )]),
        requested.state,
    );
    let completed_seam = completed.downstream_consume_seam();

    assert!(completed_seam.prompt_state().active_input.is_none());
    assert!(matches!(
        completed_seam.prompt_effect().input_transition,
        Some(PromptInputTransition::Cancelled { correlation_id: 42 })
    ));

    let replay = fold_normalized_outcomes(NormalizedOutcomeBatch::default(), completed.state);
    let replay_seam = replay.downstream_consume_seam();

    assert!(
        replay_seam
            .notification()
            .latest_user_visible_message
            .is_none()
    );
    assert!(replay_seam.prompt_effect().input_transition.is_none());
    assert!(replay_seam.structural().redraw.is_none());
}
