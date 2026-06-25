use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use vim_core_rs::{
    CoreCommandOutcome, CoreCommandTransaction, CoreEvent, CoreHostAction, CoreMessageCategory,
    CoreMessageEvent, CoreMessageSeverity, CoreMode, CorePendingInput,
};

use super::{CoreBridge, completion_replacement_end};
use crate::core::outcome::{
    ApplicationOutcomeState, NormalizedCoreOutcome, NormalizedHostDirective,
    NormalizedNotification, NormalizedOutcomeBatch, NormalizedPrompt, NormalizedStructuralOutcome,
    OutcomeOrigin, PromptInputTransition, PromptResponseDisposition, fold_normalized_outcomes,
};
use crate::core::prompt::{PromptResponseCommand, PromptResponseError};
use crate::features::completion::session::{
    CompletionPosition, CompletionRange, CompletionTextEdit,
};

use crate::app::test_support::launch_serial_lock as session_test_lock;

fn unique_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-core-bridge-{name}-{nanos}.txt"))
}

#[test]
fn initializes_vim_core_session_and_returns_initial_snapshot() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("").expect("core bridge should initialize");
    let snapshot = bridge.snapshot();

    assert_eq!(snapshot.text, "\n");
    assert_eq!(snapshot.revision, 0);
    assert!(!snapshot.dirty);
    assert_eq!(snapshot.mode, CoreMode::Normal);
}

#[test]
fn syntax_enabled_tracks_vim_syntax_on_and_off() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("fn main() {}\n").expect("core bridge should initialize");

    assert!(
        !bridge.is_syntax_enabled(),
        "syntax should start disabled until Vim :syntax on is executed"
    );

    bridge
        .apply_ex_command("syntax on")
        .expect("syntax on should be accepted by Vim core");
    assert!(
        bridge.is_syntax_enabled(),
        "syntax on should enable syntax-dependent highlighting"
    );

    bridge
        .apply_ex_command("syntax off")
        .expect("syntax off should be accepted by Vim core");
    assert!(
        !bridge.is_syntax_enabled(),
        "syntax off should disable syntax-dependent highlighting"
    );
}

#[cfg(feature = "tree-sitter-syntax")]
#[test]
fn tree_sitter_request_poll_and_query_reads_committed_cache_without_worker() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("fn main() {}\n").expect("core bridge should initialize");
    let snapshot = bridge.snapshot();
    let buffer = snapshot
        .buffers
        .iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist");
    let range = vim_core_rs::CoreTextRange {
        start: vim_core_rs::CoreTextPosition { row: 0, col: 0 },
        end: vim_core_rs::CoreTextPosition { row: 1, col: 0 },
    };

    let preparation = bridge
        .request_tree_sitter_syntax_preparation(vim_core_rs::CoreTreeSitterPreparationRequest {
            buffer_id: buffer.id,
            source_revision: Some(buffer.source_revision),
            range,
            vim_filetype: None,
            buffer_name: Some("src/main.rs".to_string()),
            host_language_hint: None,
            snapshot_policy: vim_core_rs::CoreTreeSitterSnapshotPolicy::default(),
        })
        .expect("Tree-sitter preparation should be requested through vim-core-rs");
    assert_eq!(preparation.source_revision, buffer.source_revision);

    let completed = bridge
        .poll_tree_sitter_preparation()
        .expect("synchronous vim-core-rs MVP should complete preparation");
    assert_eq!(completed.request_id, preparation.request_id);
    assert_eq!(
        completed.syntax.status,
        vim_core_rs::CoreTreeSitterStatus::Prepared
    );

    let queried = bridge
        .query_tree_sitter_syntax_range(buffer.id, buffer.source_revision, range)
        .expect("committed Tree-sitter cache should be queryable");
    assert_eq!(queried.source_revision, buffer.source_revision);
    assert_eq!(queried.status, vim_core_rs::CoreTreeSitterStatus::Prepared);
    assert!(
        queried
            .chunks
            .iter()
            .any(|chunk| chunk.category == vim_core_rs::CoreSyntaxCategory::Keyword),
        "saya should consume normalized category data from vim-core-rs: {:?}",
        queried.chunks
    );
}

#[test]
fn starts_with_no_pending_host_actions() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("buffer text").expect("core bridge should initialize");

    assert!(bridge.take_pending_host_actions().is_empty());
}

#[test]
fn starts_with_no_pending_core_messages() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("buffer text").expect("core bridge should initialize");

    assert!(bridge.take_pending_messages().is_empty());
}

#[test]
fn starts_with_no_pending_redraw_requests() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("buffer text").expect("core bridge should initialize");

    assert!(bridge.take_pending_redraw_requests().is_empty());
}

#[test]
fn normalized_outcomes_preserve_transaction_total_order_and_sequence() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("buffer text").expect("core bridge should initialize");
    let tx = CoreCommandTransaction {
        outcome: CoreCommandOutcome::NoChange,
        snapshot: bridge.snapshot(),
        host_actions: vec![
            CoreHostAction::Write {
                path: "notes.txt".to_string(),
                force: false,
                issued_after_revision: 1,
            },
            CoreHostAction::Bell,
        ],
        events: vec![
            CoreEvent::Message(CoreMessageEvent {
                severity: CoreMessageSeverity::Info,
                category: CoreMessageCategory::UserVisible,
                content: "written".to_string(),
            }),
            CoreEvent::Redraw {
                full: false,
                clear_before_draw: true,
            },
        ],
    };

    bridge.queue_transaction_artifacts(&tx);
    let batch = bridge.take_normalized_outcomes();

    let traces = batch
        .outcomes()
        .iter()
        .map(|outcome| *outcome.trace())
        .collect::<Vec<_>>();
    assert_eq!(
        traces
            .iter()
            .map(|trace| trace.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4]
    );
    assert_eq!(
        traces.iter().map(|trace| trace.origin).collect::<Vec<_>>(),
        vec![
            OutcomeOrigin::TransactionHostAction,
            OutcomeOrigin::TransactionHostAction,
            OutcomeOrigin::TransactionEvent,
            OutcomeOrigin::TransactionEvent,
        ]
    );
    assert_eq!(
        traces
            .iter()
            .map(|trace| trace.raw_kind)
            .collect::<Vec<_>>(),
        vec![
            "CoreHostAction::Write",
            "CoreHostAction::Bell",
            "CoreEvent::Message",
            "CoreEvent::Redraw",
        ]
    );
    assert!(matches!(
        batch.outcomes()[0],
        NormalizedCoreOutcome::HostDirective(NormalizedHostDirective::Write { .. })
    ));
    assert!(matches!(
        batch.outcomes()[1],
        NormalizedCoreOutcome::Notification(NormalizedNotification::Bell { .. })
    ));
    assert!(matches!(
        batch.outcomes()[3],
        NormalizedCoreOutcome::Structural(NormalizedStructuralOutcome::RedrawRequested {
            full: false,
            clear_before_draw: true,
            ..
        })
    ));
}

#[test]
fn normalized_outcomes_append_pending_session_state_after_transaction_payload() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("buffer text").expect("core bridge should initialize");
    let tx = CoreCommandTransaction {
        outcome: CoreCommandOutcome::NoChange,
        snapshot: bridge.snapshot(),
        host_actions: vec![CoreHostAction::Write {
            path: "notes.txt".to_string(),
            force: false,
            issued_after_revision: 1,
        }],
        events: vec![CoreEvent::Message(CoreMessageEvent {
            severity: CoreMessageSeverity::Info,
            category: CoreMessageCategory::UserVisible,
            content: "transaction message".to_string(),
        })],
    };

    bridge.queue_transaction_artifacts(&tx);
    bridge.queue_host_action(
        &CoreHostAction::Quit {
            force: true,
            issued_after_revision: 2,
        },
        OutcomeOrigin::PendingSessionHostAction,
    );
    bridge.queue_core_event(
        &CoreEvent::Redraw {
            full: true,
            clear_before_draw: false,
        },
        OutcomeOrigin::PendingSessionEvent,
    );

    let batch = bridge.take_normalized_outcomes();
    let traces = batch
        .outcomes()
        .iter()
        .map(|outcome| *outcome.trace())
        .collect::<Vec<_>>();

    assert_eq!(
        traces
            .iter()
            .map(|trace| trace.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4]
    );
    assert_eq!(
        traces.iter().map(|trace| trace.origin).collect::<Vec<_>>(),
        vec![
            OutcomeOrigin::TransactionHostAction,
            OutcomeOrigin::TransactionEvent,
            OutcomeOrigin::PendingSessionHostAction,
            OutcomeOrigin::PendingSessionEvent,
        ]
    );
}

#[test]
fn take_normalized_outcomes_moves_batch_and_clears_bridge_queue() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("buffer text").expect("core bridge should initialize");
    let tx = CoreCommandTransaction {
        outcome: CoreCommandOutcome::NoChange,
        snapshot: bridge.snapshot(),
        host_actions: vec![CoreHostAction::Quit {
            force: true,
            issued_after_revision: 1,
        }],
        events: vec![],
    };

    bridge.queue_transaction_artifacts(&tx);

    assert_eq!(bridge.take_normalized_outcomes().outcomes().len(), 1);
    assert!(
        bridge.take_normalized_outcomes().is_empty(),
        "normalized outcome drain must clear the bridge queue"
    );
}

#[test]
fn respond_to_prompt_returns_completion_batch_that_folds_active_prompt_closed() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("buffer text").expect("core bridge should initialize");

    bridge
        .apply_ex_command(":input Name")
        .expect("input request should be queued");
    let request_batch = bridge.take_normalized_outcomes();
    assert!(matches!(
        request_batch.outcomes().first(),
        Some(NormalizedCoreOutcome::Prompt(
            NormalizedPrompt::RequestInput {
                correlation_id: 1,
                ..
            }
        ))
    ));
    let requested = fold_normalized_outcomes(request_batch, ApplicationOutcomeState::default());
    assert!(matches!(
        requested.effects.prompt.input_transition,
        Some(PromptInputTransition::Requested { .. })
    ));

    let response_batch = bridge
        .respond_to_prompt(PromptResponseCommand::Submit {
            correlation_id: 1,
            value: "alice".to_string(),
        })
        .expect("prompt response should be accepted");

    assert!(matches!(
        response_batch.outcomes().first(),
        Some(NormalizedCoreOutcome::Prompt(
            NormalizedPrompt::InputResponseAccepted {
                correlation_id: 1,
                disposition: PromptResponseDisposition::Submitted,
                ..
            }
        ))
    ));

    let completed = fold_normalized_outcomes(response_batch, requested.state);
    assert!(completed.state.prompt.active_input.is_none());
    assert!(matches!(
        completed.effects.prompt.input_transition,
        Some(PromptInputTransition::Submitted { correlation_id: 1 })
    ));
    assert!(matches!(
        bridge.respond_to_prompt(PromptResponseCommand::Cancel { correlation_id: 1 }),
        Err(PromptResponseError::NoActivePrompt)
    ));
}

#[test]
fn respond_to_prompt_rejects_missing_and_mismatched_active_prompt_without_clearing_it() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("buffer text").expect("core bridge should initialize");

    assert!(matches!(
        bridge.respond_to_prompt(PromptResponseCommand::Cancel { correlation_id: 1 }),
        Err(PromptResponseError::NoActivePrompt)
    ));

    bridge
        .apply_ex_command(":input Name")
        .expect("input request should be queued");
    let _ = bridge.take_normalized_outcomes();

    assert!(matches!(
        bridge.respond_to_prompt(PromptResponseCommand::Cancel { correlation_id: 2 }),
        Err(PromptResponseError::CorrelationMismatch {
            expected: 1,
            actual: 2
        })
    ));

    assert!(
        bridge
            .respond_to_prompt(PromptResponseCommand::Cancel { correlation_id: 1 })
            .is_ok(),
        "mismatch must not clear the bridge active input correlation"
    );
}

#[test]
fn legacy_message_projection_consumes_only_projected_normalized_outcome() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("buffer text").expect("core bridge should initialize");
    let tx = CoreCommandTransaction {
        outcome: CoreCommandOutcome::NoChange,
        snapshot: bridge.snapshot(),
        host_actions: vec![CoreHostAction::Write {
            path: "notes.txt".to_string(),
            force: false,
            issued_after_revision: 1,
        }],
        events: vec![CoreEvent::Message(CoreMessageEvent {
            severity: CoreMessageSeverity::Info,
            category: CoreMessageCategory::UserVisible,
            content: "written".to_string(),
        })],
    };

    bridge.queue_transaction_artifacts(&tx);

    let messages = bridge.take_pending_messages();
    assert_eq!(messages.len(), 1);

    let remaining = bridge.take_normalized_outcomes();
    assert_eq!(
        remaining.outcomes().len(),
        1,
        "legacy message projection must not duplicate consumed messages or drop host directives"
    );
    assert!(matches!(
        remaining.outcomes()[0],
        NormalizedCoreOutcome::HostDirective(NormalizedHostDirective::Write { .. })
    ));
}

#[test]
fn legacy_host_action_projection_consumes_host_directives_from_normalized_queue() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("buffer text").expect("core bridge should initialize");
    let tx = CoreCommandTransaction {
        outcome: CoreCommandOutcome::NoChange,
        snapshot: bridge.snapshot(),
        host_actions: vec![CoreHostAction::Quit {
            force: true,
            issued_after_revision: 1,
        }],
        events: vec![CoreEvent::Redraw {
            full: true,
            clear_before_draw: false,
        }],
    };

    bridge.queue_transaction_artifacts(&tx);

    let actions = bridge.take_pending_host_actions();
    assert_eq!(actions.len(), 1);

    let remaining = bridge.take_normalized_outcomes();
    assert_eq!(
        remaining.outcomes().len(),
        1,
        "legacy host projection must leave non-host normalized outcomes queued"
    );
    assert!(matches!(
        remaining.outcomes()[0],
        NormalizedCoreOutcome::Structural(NormalizedStructuralOutcome::RedrawRequested {
            full: true,
            ..
        })
    ));
}

#[test]
fn headless_application_regression_folds_core_foundation_without_effect_replay() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("buffer text").expect("core bridge should initialize");
    let mut folded_state = ApplicationOutcomeState::default();

    bridge
        .apply_ex_command(":input Name")
        .expect("input request should be queued");
    let requested = fold_normalized_outcomes(bridge.take_normalized_outcomes(), folded_state);
    folded_state = requested.state;
    assert!(matches!(
        requested.effects.prompt.input_transition,
        Some(PromptInputTransition::Requested { .. })
    ));
    assert_eq!(
        folded_state
            .prompt
            .active_input
            .as_ref()
            .map(|session| session.correlation_id),
        Some(1)
    );

    let response_batch = bridge
        .respond_to_prompt(PromptResponseCommand::Cancel { correlation_id: 1 })
        .expect("prompt cancel should be accepted");
    let cancelled = fold_normalized_outcomes(response_batch, folded_state);
    folded_state = cancelled.state;
    assert!(folded_state.prompt.active_input.is_none());
    assert!(matches!(
        cancelled.effects.prompt.input_transition,
        Some(PromptInputTransition::Cancelled { correlation_id: 1 })
    ));

    let tx = CoreCommandTransaction {
        outcome: CoreCommandOutcome::NoChange,
        snapshot: bridge.snapshot(),
        host_actions: vec![
            CoreHostAction::Write {
                path: "notes.txt".to_string(),
                force: false,
                issued_after_revision: 1,
            },
            CoreHostAction::Quit {
                force: false,
                issued_after_revision: 1,
            },
        ],
        events: vec![
            CoreEvent::Message(CoreMessageEvent {
                severity: CoreMessageSeverity::Info,
                category: CoreMessageCategory::UserVisible,
                content: "saved".to_string(),
            }),
            CoreEvent::Redraw {
                full: false,
                clear_before_draw: true,
            },
            CoreEvent::BufferAdded { buf_id: 3 },
            CoreEvent::LayoutChanged,
        ],
    };

    bridge.queue_transaction_artifacts(&tx);
    let folded = fold_normalized_outcomes(bridge.take_normalized_outcomes(), folded_state);

    assert!(matches!(
        folded.effects.host_directives.as_slice(),
        [
            NormalizedHostDirective::Write { .. },
            NormalizedHostDirective::Quit { .. }
        ]
    ));
    assert_eq!(
        folded
            .effects
            .notification
            .latest_user_visible_message
            .as_ref()
            .map(|message| message.content.as_str()),
        Some("saved")
    );
    assert_eq!(folded.effects.structural.invalidate_buffers, vec![3]);
    assert!(folded.effects.structural.layout_dirty);
    assert_eq!(
        folded.effects.structural.redraw.map(|redraw| {
            (
                redraw.full,
                redraw.clear_before_draw,
                redraw.required_by_structure_change,
            )
        }),
        Some((true, true, true))
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
    assert!(replay.effects.prompt.input_transition.is_none());
    assert!(replay.effects.structural.redraw.is_none());
    assert!(
        bridge.take_normalized_outcomes().is_empty(),
        "headless application regression must leave the bridge drain contract clear"
    );
}

#[test]
fn initializes_empty_buffer_for_new_file_scenario() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("").expect("core bridge should initialize with empty text");
    let snapshot = bridge.snapshot();

    assert_eq!(snapshot.text, "\n");
    assert!(!snapshot.dirty);
    assert_eq!(snapshot.mode, CoreMode::Normal);
}

#[test]
fn allows_attaching_target_path_to_empty_buffer_after_creation() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let save_path = unique_path("new-file-save");

    let mut bridge = CoreBridge::new("").expect("core bridge should initialize");
    bridge
        .attach_target_path(&save_path)
        .expect("should attach target path to empty buffer");
    let snapshot = bridge.snapshot();

    let active_buffer = snapshot
        .buffers
        .iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist");

    assert_eq!(active_buffer.name, save_path.display().to_string());
    assert_eq!(snapshot.text, "\n");
    assert!(!snapshot.dirty);
}

#[test]
fn initializes_existing_file_session_with_target_path_as_buffer_name() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("target file");

    let bridge = CoreBridge::new_with_target_path(&target_path, "buffer text\n")
        .expect("core bridge should initialize existing file session");
    let snapshot = bridge.snapshot();
    let active_buffer = snapshot
        .buffers
        .iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist");

    assert_eq!(active_buffer.name, target_path.display().to_string());
    assert_eq!(snapshot.text, "buffer text\n");
    assert!(!snapshot.dirty);
    assert_eq!(snapshot.mode, CoreMode::Normal);
}

#[test]
fn message_handler_captures_echoerr_messages() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("hello\n").expect("core bridge should initialize");

    bridge
        .apply_ex_command("echoerr 'test error message'")
        .expect("echoerr command should complete");
    let messages = bridge.take_pending_messages();

    assert!(
        messages.iter().any(|message| {
            message.severity == CoreMessageSeverity::Error
                && message.category == CoreMessageCategory::UserVisible
                && message.content.contains("test error message")
        }),
        "echoerr message should be queued: {:?}",
        messages
    );
}

#[test]
fn message_handler_captures_echom_messages() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("hello\n").expect("core bridge should initialize");

    bridge
        .apply_ex_command("echom \"one\\ntwo\\nthree\\nfour\\nfive\\nsix\"")
        .expect("echom command should complete");
    let messages = bridge.take_pending_messages();

    assert!(
        messages.iter().any(|message| {
            message.category == CoreMessageCategory::UserVisible
                && message.content.contains("one")
                && message.content.contains("six")
        }),
        "echom message should be queued: {:?}",
        messages
    );
}

#[test]
fn dispatch_key_queues_pending_redraw_request() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("hello\n").expect("core bridge should initialize");

    bridge
        .apply_ex_command(":redraw")
        .expect(":redraw should succeed");
    let redraws = bridge.take_pending_redraw_requests();

    assert!(
        !redraws.is_empty(),
        ":redraw should enqueue at least one redraw request: {:?}",
        redraws
    );
}

#[test]
fn dirty_ctrl_c_queues_upstream_exit_guidance_message() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("hello\n").expect("core bridge should initialize");
    bridge.dispatch_key("i").expect("insert mode");
    bridge.dispatch_key("X").expect("typed input");
    bridge.dispatch_key("\x1b").expect("normal mode");

    bridge
        .dispatch_key("\u{3}")
        .expect("ctrl-c should dispatch");
    let messages = bridge.take_pending_messages();

    assert!(
        messages
            .iter()
            .any(|message| message.content.contains(":qa!")),
        "ctrl-c guidance should mention :qa!: {:?}",
        messages
    );
}

// ---- タスク 4.1: モード遷移テスト ----

#[test]
fn starts_in_normal_mode() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let bridge = CoreBridge::new("hello\n").expect("core bridge should initialize");
    let snapshot = bridge.snapshot();

    assert_eq!(
        snapshot.mode,
        CoreMode::Normal,
        "起動時はノーマルモードであること"
    );
}

#[test]
fn transitions_to_insert_mode_with_i_key() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("hello\n").expect("core bridge should initialize");

    let result = bridge.dispatch_key("i");
    assert!(result.is_ok(), "i キーの dispatch は成功すること");

    let snapshot = bridge.snapshot();
    assert_eq!(
        snapshot.mode,
        CoreMode::Insert,
        "i キーでインサートモードに遷移すること"
    );
}

#[test]
fn transitions_back_to_normal_mode_with_escape() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("hello\n").expect("core bridge should initialize");

    bridge.dispatch_key("i").expect("i キーで insert 遷移");
    assert_eq!(bridge.snapshot().mode, CoreMode::Insert);

    bridge
        .dispatch_key("\x1b")
        .expect("Escape キーで normal 復帰");
    let snapshot = bridge.snapshot();
    assert_eq!(
        snapshot.mode,
        CoreMode::Normal,
        "Escape でノーマルモードに復帰すること"
    );
}

#[test]
fn current_mode_is_available_from_snapshot_after_dispatch() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("test\n").expect("core bridge should initialize");

    // ノーマルモード確認
    assert_eq!(bridge.snapshot().mode, CoreMode::Normal);

    // インサートモードへ
    bridge.dispatch_key("i").expect("insert mode");
    assert_eq!(bridge.snapshot().mode, CoreMode::Insert);

    // ノーマルモードへ戻る
    bridge.dispatch_key("\x1b").expect("normal mode");
    assert_eq!(bridge.snapshot().mode, CoreMode::Normal);
}

// ---- タスク 4.2: カーソル移動テスト ----

#[test]
fn cursor_moves_down_with_j_key() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge =
        CoreBridge::new("first line\nsecond line\n").expect("core bridge should initialize");
    let initial = bridge.snapshot();
    assert_eq!(initial.cursor_row, 0, "初期カーソル行は 0");
    assert_eq!(initial.cursor_col, 0, "初期カーソル列は 0");

    bridge.dispatch_key("j").expect("j キーで下移動");
    let snapshot = bridge.snapshot();
    assert_eq!(
        snapshot.cursor_row, 1,
        "j キーでカーソルが 1 行下に移動すること"
    );
    assert_eq!(snapshot.cursor_col, 0, "j キーで列は変わらないこと");
}

#[test]
fn cursor_moves_right_with_l_key() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("hello\n").expect("core bridge should initialize");

    bridge.dispatch_key("l").expect("l キーで右移動");
    let snapshot = bridge.snapshot();
    assert_eq!(snapshot.cursor_col, 1, "l キーでカーソルが右に移動すること");
    assert_eq!(snapshot.cursor_row, 0, "l キーで行は変わらないこと");
}

#[test]
fn cursor_moves_left_with_h_key() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("hello\n").expect("core bridge should initialize");

    // まず右に移動してから左に戻る
    bridge.dispatch_key("ll").expect("l で右に 2 回移動");
    assert_eq!(bridge.snapshot().cursor_col, 2);

    bridge.dispatch_key("h").expect("h キーで左移動");
    let snapshot = bridge.snapshot();
    assert_eq!(snapshot.cursor_col, 1, "h キーでカーソルが左に移動すること");
}

#[test]
fn cursor_moves_up_with_k_key() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge =
        CoreBridge::new("first\nsecond\nthird\n").expect("core bridge should initialize");

    bridge.dispatch_key("jj").expect("j で 2 行下に移動");
    assert_eq!(bridge.snapshot().cursor_row, 2);
    bridge.dispatch_key("ll").expect("l で 2 列右に移動");
    assert_eq!(bridge.snapshot().cursor_col, 2);

    bridge.dispatch_key("k").expect("k キーで上移動");
    let snapshot = bridge.snapshot();
    assert_eq!(snapshot.cursor_row, 1, "k キーでカーソルが上に移動すること");
}

#[test]
fn cursor_position_reflected_in_snapshot_after_multiple_moves() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge =
        CoreBridge::new("abcde\nfghij\nklmno\n").expect("core bridge should initialize");

    bridge
        .dispatch_key("jll")
        .expect("j で 1 行下、ll で 2 列右");
    let snapshot = bridge.snapshot();
    assert_eq!(snapshot.cursor_row, 1, "複合移動後の行位置");
    assert_eq!(snapshot.cursor_col, 2, "複合移動後の列位置");
}

// ---- タスク 4.3: インサートモード文字入力テスト ----

#[test]
fn insert_mode_text_input_appears_in_buffer() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("").expect("core bridge should initialize");

    // i でインサートモードに入り、文字を入力して Esc で戻る
    bridge.dispatch_key("i").expect("insert mode");
    bridge.dispatch_key("H").expect("H を入力");
    bridge.dispatch_key("i").expect("i を入力");
    bridge
        .dispatch_key("\x1b")
        .expect("Escape でノーマルモードに復帰");

    let snapshot = bridge.snapshot();
    assert_eq!(snapshot.mode, CoreMode::Normal, "ノーマルモードに復帰");
    assert!(
        snapshot.text.contains("Hi"),
        "入力した文字 'Hi' がバッファに含まれること: actual={:?}",
        snapshot.text
    );
}

#[test]
fn insert_mode_marks_buffer_dirty() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("").expect("core bridge should initialize");

    assert!(!bridge.snapshot().dirty, "初期状態は dirty でないこと");

    bridge.dispatch_key("i").expect("insert mode");
    bridge.dispatch_key("a").expect("a を入力");
    bridge.dispatch_key("\x1b").expect("normal mode");

    let snapshot = bridge.snapshot();
    assert!(
        snapshot.dirty,
        "インサートモードで文字入力後は dirty になること"
    );
}

#[test]
fn insert_mode_text_input_updates_buffer_for_redraw() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("line1\n").expect("core bridge should initialize");

    bridge.dispatch_key("i").expect("insert mode");
    bridge.dispatch_key("X").expect("X を入力");
    bridge.dispatch_key("\x1b").expect("normal mode");

    let snapshot = bridge.snapshot();
    assert!(
        snapshot.text.starts_with("X"),
        "先頭に X が挿入されること: actual={:?}",
        snapshot.text
    );
    log::debug!(
        "[test] insert 後のバッファ内容（再描画用）: {:?}",
        snapshot.text
    );
}

// ---- タスク 4.4: 削除操作テスト ----

#[test]
fn x_key_deletes_character_at_cursor() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("abcde\n").expect("core bridge should initialize");

    bridge.dispatch_key("x").expect("x キーで文字削除");
    let snapshot = bridge.snapshot();
    assert_eq!(
        snapshot.text, "bcde\n",
        "x キーで先頭の 'a' が削除されること"
    );
}

#[test]
fn x_key_marks_buffer_dirty() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("hello\n").expect("core bridge should initialize");

    assert!(!bridge.snapshot().dirty, "初期状態は dirty でないこと");

    bridge.dispatch_key("x").expect("x キーで削除");
    let snapshot = bridge.snapshot();
    assert!(snapshot.dirty, "削除後は dirty になること");
}

#[test]
fn dd_deletes_entire_line() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge =
        CoreBridge::new("first\nsecond\nthird\n").expect("core bridge should initialize");

    bridge.dispatch_key("dd").expect("dd で行削除");
    let snapshot = bridge.snapshot();
    assert_eq!(
        snapshot.text, "second\nthird\n",
        "dd で最初の行が削除されること"
    );
    assert!(snapshot.dirty, "dd 後は dirty になること");
}

#[test]
fn sequential_multi_key_pending_input_is_forwarded_through_core_dispatch() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge =
        CoreBridge::new("first\nsecond\nthird\n").expect("core bridge should initialize");

    let first = bridge
        .dispatch_key("y")
        .expect("first key should be forwarded to core");
    assert_eq!(first, CoreCommandOutcome::NoChange);
    assert_eq!(
        bridge.snapshot().pending_input.pending_keys,
        "y",
        "pending input state should come from vim-core-rs"
    );

    bridge
        .dispatch_key("y")
        .expect("second key should complete the sequence");

    let snapshot = bridge.snapshot();
    assert_eq!(
        snapshot.text, "first\nsecond\nthird\n",
        "yy itself should not change the buffer"
    );
    assert_eq!(
        snapshot.pending_input,
        CorePendingInput::none(),
        "completed sequence should leave no bridge-side pending parser state"
    );
}

#[test]
fn ctrl_c_cancels_core_owned_pending_input_without_showing_exit_guidance() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("first\nsecond\n").expect("core bridge should initialize");

    bridge.dispatch_key("d").expect("enter operator pending");
    assert!(
        bridge.snapshot().pending_input.is_pending(),
        "pending input should be reported by vim-core-rs before ctrl-c"
    );

    let outcome = bridge
        .dispatch_key("\u{3}")
        .expect("ctrl-c should cancel pending input");
    assert_eq!(outcome, CoreCommandOutcome::NoChange);
    assert_eq!(
        bridge.snapshot().pending_input,
        CorePendingInput::none(),
        "ctrl-c should clear core pending input via escape dispatch"
    );
    assert!(
        bridge.take_pending_messages().is_empty(),
        "canceling pending input should not enqueue exit guidance"
    );
}

#[test]
fn ctrl_w_prefix_is_coalesced_across_separate_dispatch_calls() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge =
        CoreBridge::new("first\nsecond\nthird\n").expect("core bridge should initialize");
    bridge.set_screen_size(24, 80);

    let first = bridge
        .dispatch_key("\u{17}")
        .expect("ctrl-w prefix should be accepted");
    assert_eq!(first, CoreCommandOutcome::NoChange);
    assert_eq!(
        bridge.snapshot().windows.len(),
        1,
        "buffering the transport prefix alone should not mutate layout yet"
    );

    bridge
        .dispatch_key("s")
        .expect("second key should complete the ctrl-w sequence");

    let snapshot = bridge.snapshot();
    assert_eq!(
        snapshot.windows.len(),
        2,
        "Ctrl-w followed by s in separate dispatch calls should create a split"
    );
}

#[test]
fn new_configures_high_report_threshold_to_suppress_bulk_edit_messages() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge =
        CoreBridge::new("first\nsecond\nthird\n").expect("core bridge should initialize");
    assert!(
        bridge
            .session
            .eval_string("&report")
            .as_deref()
            .is_some_and(|report| report.trim() == "999999"),
        "複数行操作の報告メッセージ抑制のため report が引き上げられていること"
    );
}

#[test]
fn sequential_dd_deletes_current_line_via_core_owned_pending_input() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge =
        CoreBridge::new("first\nsecond\nthird\n").expect("core bridge should initialize");

    bridge
        .dispatch_key("d")
        .expect("first d should enter operator pending");
    bridge
        .dispatch_key("d")
        .expect("second d should delete current line");

    let snapshot = bridge.snapshot();
    assert_eq!(
        snapshot.text, "second\nthird\n",
        "dd を逐次入力しても現在行が削除されること"
    );
}

#[test]
fn insert_mode_keeps_literal_text_literal_at_bridge_boundary() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("").expect("core bridge should initialize");

    bridge.dispatch_key("i").expect("enter insert mode");
    bridge
        .dispatch_key("2")
        .expect("insert literal count digit");
    bridge
        .dispatch_key("d")
        .expect("insert literal operator key");
    bridge
        .dispatch_key("g")
        .expect("insert literal normal prefix key");
    bridge.dispatch_key("\x1b").expect("leave insert mode");

    let snapshot = bridge.snapshot();
    assert_eq!(snapshot.mode, CoreMode::Normal);
    assert_eq!(snapshot.text, "2dg\n");
}

#[test]
fn delete_maintains_dirty_state_across_operations() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("abc\ndef\n").expect("core bridge should initialize");

    bridge.dispatch_key("x").expect("最初の x で削除");
    assert!(bridge.snapshot().dirty, "最初の削除後は dirty");

    bridge.dispatch_key("x").expect("2 回目の x で削除");
    let snapshot = bridge.snapshot();
    assert!(
        snapshot.dirty,
        "複数回の削除操作後も dirty 状態が維持されること"
    );
    assert_eq!(snapshot.text, "c\ndef\n", "2 文字削除後のバッファ内容");
}

// ---- タスク 5.1: 保存要求（:w）でホストアクション Write が発行されるテスト ----

#[test]
fn write_command_produces_write_host_action() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("write-host-action");

    let mut bridge = CoreBridge::new_with_target_path(&target_path, "content\n")
        .expect("core bridge should initialize");

    // :w を実行
    bridge
        .apply_ex_command(":w")
        .expect(":w コマンドは成功すること");

    let actions = bridge.take_pending_host_actions();
    log::debug!("[test] host actions after :w: {:?}", actions);
    let has_write = actions
        .iter()
        .any(|a| matches!(a, vim_core_rs::CoreHostAction::Write { .. }));
    assert!(
        has_write,
        ":w 実行後に Write ホストアクションが発行されること: actions={:?}",
        actions
    );
}

#[test]
fn buffer_text_returns_current_contents() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("hello\n").expect("core bridge should initialize");

    let text = bridge.buffer_text();
    assert_eq!(text, "hello\n", "buffer_text が現在の内容を返すこと");

    // 編集後もテキストが更新されること
    bridge.dispatch_key("i").expect("insert mode");
    bridge.dispatch_key("X").expect("X を入力");
    bridge.dispatch_key("\x1b").expect("normal mode");

    let text_after = bridge.buffer_text();
    assert!(
        text_after.contains("X"),
        "編集後の buffer_text に入力文字が含まれること: {:?}",
        text_after
    );
}

#[test]
fn completion_replace_range_moves_insert_cursor_to_replacement_end() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("ty\n").expect("core bridge should initialize");
    bridge.dispatch_key("A").expect("insert mode at line end");
    bridge
        .apply_completion_replace_range(
            &CompletionRange {
                start: CompletionPosition {
                    line: 0,
                    character: 0,
                },
                end: CompletionPosition {
                    line: 0,
                    character: 2,
                },
            },
            "type",
        )
        .expect("completion replacement should apply");

    let snapshot = bridge.snapshot();
    assert_eq!(snapshot.text, "type\n");
    assert_eq!(snapshot.mode, CoreMode::Insert);
    assert_eq!(snapshot.cursor_row, 0);
    assert_eq!(
        snapshot.cursor_col, 4,
        "accepted completion should leave the insert cursor after the inserted text"
    );
}

#[test]
fn completion_replace_range_applies_additional_text_edits_before_cursor() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("package main\n\nfunc main() {\n\tlog.Pri\n}\n")
        .expect("core bridge should initialize");
    bridge
        .dispatch_key("GkA")
        .expect("insert mode at completion line end");
    bridge
        .apply_completion_replace_range_with_additional_text_edits(
            &CompletionRange {
                start: CompletionPosition {
                    line: 3,
                    character: 5,
                },
                end: CompletionPosition {
                    line: 3,
                    character: 8,
                },
            },
            "Printf",
            &[CompletionTextEdit {
                range: CompletionRange {
                    start: CompletionPosition {
                        line: 2,
                        character: 0,
                    },
                    end: CompletionPosition {
                        line: 2,
                        character: 0,
                    },
                },
                new_text: "import \"log\"\n\n".to_string(),
            }],
        )
        .expect("completion replacement and import edit should apply");

    let snapshot = bridge.snapshot();
    assert_eq!(
        snapshot.text,
        "package main\n\nimport \"log\"\n\nfunc main() {\n\tlog.Printf\n}\n"
    );
    assert_eq!(snapshot.mode, CoreMode::Insert);
    assert_eq!(snapshot.cursor_row, 5);
    assert_eq!(snapshot.cursor_col, "\tlog.Printf".len());
}

#[test]
fn completion_replacement_end_tracks_multiline_insert_text() {
    let range = CompletionRange {
        start: CompletionPosition {
            line: 2,
            character: 3,
        },
        end: CompletionPosition {
            line: 2,
            character: 5,
        },
    };

    assert_eq!(completion_replacement_end(&range, "abc"), (2, 6));
    assert_eq!(completion_replacement_end(&range, "ab\ncd"), (3, 2));
}

// ---- タスク 5.4: 終了要求（:q, :q!）でホストアクション Quit が発行されるテスト ----

#[test]
fn quit_command_produces_quit_host_action() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("content\n").expect("core bridge should initialize");

    bridge
        .apply_ex_command(":q")
        .expect(":q コマンドは成功すること");

    let actions = bridge.take_pending_host_actions();
    log::debug!("[test] host actions after :q: {:?}", actions);
    let has_quit = actions
        .iter()
        .any(|a| matches!(a, vim_core_rs::CoreHostAction::Quit { force: false, .. }));
    assert!(
        has_quit,
        ":q 実行後に Quit(force=false) ホストアクションが発行されること: actions={:?}",
        actions
    );
}

#[test]
fn force_quit_command_produces_force_quit_host_action() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut bridge = CoreBridge::new("content\n").expect("core bridge should initialize");

    bridge
        .apply_ex_command(":q!")
        .expect(":q! コマンドは成功すること");

    let actions = bridge.take_pending_host_actions();
    log::debug!("[test] host actions after :q!: {:?}", actions);
    let has_force_quit = actions
        .iter()
        .any(|a| matches!(a, vim_core_rs::CoreHostAction::Quit { force: true, .. }));
    assert!(
        has_force_quit,
        ":q! 実行後に Quit(force=true) ホストアクションが発行されること: actions={:?}",
        actions
    );
}
