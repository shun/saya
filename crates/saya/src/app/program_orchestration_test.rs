use super::program_test_support::*;
use super::*;

#[test]
fn apply_runtime_dispatch_outcome_returns_shutdown_reason_from_runtime_intent() {
    let mut transient_msg = None;
    let mut need_redraw = false;
    let mut runtime_presentation_intents = Vec::new();

    let shutdown_reason = apply_runtime_dispatch_outcome(
        &mut transient_msg,
        &mut need_redraw,
        &mut runtime_presentation_intents,
        RuntimeDispatchOutcome {
            transient_message: Some("Saved successfully".to_string()),
            requires_redraw: true,
            shutdown_intent: Some(RuntimeShutdownIntent::UserQuit),
            presentation_intents: Vec::new(),
        },
    );

    assert_eq!(shutdown_reason, Some(ShutdownReason::UserQuit));
    assert_eq!(transient_msg, Some("Saved successfully".to_string()));
    assert!(need_redraw);
}

#[test]
fn save_snapshot_result_with_path_override_writes_to_explicit_host_path() {
    let original_path = unique_path("write-override-original");
    let alternate_path = unique_path("write-override-alternate");
    std::fs::write(&original_path, "original\n").expect("original file");
    let mut session_state =
        crate::app::session::EditorSessionState::new(Some(original_path.clone()));
    session_state.update_dirty(true);
    let alternate_path_string = alternate_path.display().to_string();

    let save_outcome = save_snapshot_result_with_path_override(
        "alternate\n",
        &mut session_state,
        Some(&alternate_path_string),
    );

    assert_eq!(
        save_outcome,
        SaveSnapshotOutcome {
            transient_message: Some("Saved successfully".to_string()),
            wrote: true,
            pending_directory_confirmation: false,
        }
    );
    assert_eq!(
        std::fs::read_to_string(&alternate_path).expect("alternate file should exist"),
        "alternate\n",
        "explicit host action path should receive the save contents"
    );
    assert_eq!(
        std::fs::read_to_string(&original_path).expect("original file should remain"),
        "original\n",
        "session target path should stay untouched when host action provides an explicit path"
    );
    assert!(
        !session_state.is_dirty(),
        "successful save should clear dirty"
    );

    std::fs::remove_file(&original_path).expect("cleanup original");
    std::fs::remove_file(&alternate_path).expect("cleanup alternate");
}

#[test]
fn save_family_host_actions_are_prioritized_by_revision_and_kind() {
    let trace = |sequence| crate::core::outcome::OutcomeTrace {
        sequence,
        origin: crate::core::outcome::OutcomeOrigin::TransactionHostAction,
        raw_kind: "test",
    };
    let directives = vec![
        NormalizedHostDirective::Quit {
            force: false,
            issued_after_revision: 9,
            trace: trace(1),
        },
        NormalizedHostDirective::Write {
            path: "stale.txt".to_string(),
            force: false,
            issued_after_revision: 8,
            trace: trace(2),
        },
        NormalizedHostDirective::Quit {
            force: false,
            issued_after_revision: 8,
            trace: trace(3),
        },
        NormalizedHostDirective::Write {
            path: "fresh.txt".to_string(),
            force: false,
            issued_after_revision: 9,
            trace: trace(4),
        },
    ];

    let prioritized = prioritize_save_family_host_directives(directives, 9);

    assert_eq!(
        prioritized,
        vec![
            NormalizedHostDirective::Write {
                path: "fresh.txt".to_string(),
                force: false,
                issued_after_revision: 9,
                trace: trace(4),
            },
            NormalizedHostDirective::Quit {
                force: false,
                issued_after_revision: 9,
                trace: trace(1),
            },
        ]
    );
}

#[test]
fn prompt_revision_changes_when_search_buffer_text_changes_with_same_length() {
    let alpha = resolve_prompt_revision(Some('/'), "ab");
    let omega = resolve_prompt_revision(Some('/'), "cd");

    assert_ne!(alpha, omega);
}

#[test]
fn runtime_current_window_id_keeps_explicit_failure_when_snapshot_has_no_active_window() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let bridge = crate::core::bridge::CoreBridge::new("alpha\nbeta\n").expect("core bridge");
    let mut snapshot = bridge.snapshot();
    snapshot.windows[0].id = 42;
    snapshot.windows[0].is_active = false;

    assert_eq!(
        resolve_runtime_current_window_id(&snapshot),
        None,
        "runtime current window は固定 fallback を返さず explicit failure を保つこと"
    );
}

#[test]
fn workspace_redraw_transaction_rolls_back_to_last_successful_model_with_failure_message() {
    let mut last_successful_workspace_model = Some(WorkspaceScreenModel {
        panes: vec![crate::presentation::screen_model::ScreenModel {
            window_id: 1,
            buffer_id: 1,
            rect: crate::presentation::screen_model::PaneRect {
                x: 0,
                y: 0,
                width: 20,
                height: 3,
            },
            file_name: "alpha.txt".to_string(),
            mode_label: "NORMAL".to_string(),
            status_line: "test.txt | NORMAL".to_string(),
            cursor_style: ScreenCursorStyle::Block,
            dirty: false,
            lines: vec!["alpha".to_string()],
            line_projections: vec![],
            cursor_row: 0,
            cursor_col: 0,
            visual_selection: None,
            search_overlays: vec![],
            syntax_chunks: vec![],
            markdown_style_ranges: vec![],
            filer_style_ranges: vec![],
            resolved_theme: crate::presentation::theme::ResolvedTheme::default(),
            message_line: None,
            command_cursor_col: None,
            is_active: true,
        }],
        floats: vec![],
        active_window_id: 1,
        message_line: crate::core::notification_prompt::resolve_workspace_message_line(Vec::<
            crate::core::notification_prompt::MessageLineCandidate,
        >::new(
        )),
        message_area_height: 5,
        message_scroll_offset: 0,
        prompt_line: None,
        pager_prompt: None,
        suppressed_prompt_hints: vec![],
        bell: None,
        command_line: None,
    });

    let output = apply_workspace_redraw_transaction(
        &mut last_successful_workspace_model,
        Err(WorkspaceRedrawError::Projection(
            WorkspaceProjectionError::ActiveWindowMissing,
        )),
    )
    .expect("rollback should return the previous successful model");

    assert_eq!(output.model.active_window_id, 1);
    assert_eq!(output.model.panes.len(), 1);
    assert_eq!(
        output.model.visible_message_text(),
        Some("workspace projection failed: active window could not be resolved")
    );
    assert_eq!(
        output.failure_message.as_deref(),
        output.model.visible_message_text()
    );
    assert_eq!(
        last_successful_workspace_model
            .as_ref()
            .expect("last successful model should be retained")
            .visible_message_text(),
        None
    );
}

#[test]
fn prompt_revision_distinguishes_search_and_command_prompts() {
    let search = resolve_prompt_revision(Some('/'), "word");
    let command = resolve_prompt_revision(Some(':'), "word");

    assert_ne!(search, command);
}

#[test]
fn prompt_revision_is_none_when_prompt_is_inactive() {
    assert_eq!(resolve_prompt_revision(None, "word"), None);
}

#[test]
fn command_line_only_render_reuses_last_workspace_for_colon_prompt() {
    let mut last_workspace = main_test_workspace();
    last_workspace.panes[0].lines = vec!["keep full projection".to_string()];

    let rendered =
        build_command_line_only_workspace(Some(&last_workspace), Some(':'), "write", 5, 4)
            .expect("colon command preview should use command-line-only workspace");

    assert_eq!(rendered.panes, last_workspace.panes);
    assert_eq!(
        rendered.command_line,
        Some(crate::presentation::screen_model::CommandLineModel {
            text: ":write".to_string(),
            cursor_col: 6,
        })
    );
}

#[test]
fn command_line_only_render_uses_command_line_edit_cursor_position() {
    let last_workspace = main_test_workspace();

    let rendered =
        build_command_line_only_workspace(Some(&last_workspace), Some(':'), "write", 1, 4)
            .expect("colon command preview should use command-line-only workspace");

    assert_eq!(
        rendered.command_line,
        Some(crate::presentation::screen_model::CommandLineModel {
            text: ":write".to_string(),
            cursor_col: 2,
        })
    );
}

#[test]
fn command_line_only_render_is_not_used_for_search_prompt() {
    let last_workspace = main_test_workspace();

    let rendered =
        build_command_line_only_workspace(Some(&last_workspace), Some('/'), "pattern", 7, 4);

    assert_eq!(rendered, None);
}

#[test]
fn command_line_only_render_is_not_used_for_substitute_live_preview() {
    let last_workspace = main_test_workspace();

    for command in [
        "%s/foo/bar",
        "s/foo/bar",
        "substitute/foo/bar",
        "10,20s/foo/bar",
    ] {
        let rendered =
            build_command_line_only_workspace(Some(&last_workspace), Some(':'), command, 3, 4);

        assert_eq!(
            rendered, None,
            "substitute input should rebuild workspace overlays instead of reusing stale projection: {command}"
        );
    }
}

#[test]
fn command_line_only_render_requires_existing_workspace() {
    let rendered = build_command_line_only_workspace(None, Some(':'), "write", 5, 4);

    assert_eq!(rendered, None);
}

#[test]
fn colon_command_input_uses_overlay_without_workspace_redraw_traces() {
    let _guard = redraw_trace_observation_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    reset_test_redraw_trace_diagnostic_counts();
    let mut coordinator = TuiRenderCoordinator::new_headless(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    );
    let mut writer = RecordingOverlayWriter::default();
    let mut last_workspace = Some(main_test_workspace());

    for buffer in ["w", "wq"] {
        let result = render_command_line_only_redraw_if_possible(
            &mut coordinator,
            Some(&mut writer),
            &mut last_workspace,
            None,
            false,
            Some(':'),
            buffer,
            buffer.len(),
            4,
        );

        assert_eq!(result, CommandLineOnlyRedraw::Rendered);
    }

    let counts = test_redraw_trace_diagnostic_counts();
    assert_eq!(counts.command_line_only_overlay, 2);
    assert_eq!(
        counts.workspace_render_build_started, 0,
        "colon command typing must not rebuild the workspace projection"
    );
    assert_eq!(
        counts.renderer_frame_requested, 0,
        "colon command typing must not request a full workspace frame"
    );
    assert_eq!(counts.command_line_overlay_fallback, 0);
    assert_eq!(
        last_workspace
            .as_ref()
            .and_then(|workspace| workspace.command_line.as_ref())
            .map(|command_line| command_line.text.as_str()),
        Some(":wq")
    );
    assert_eq!(
        writer.cursor_styles,
        vec![ScreenCursorStyle::SteadyBar],
        "unchanged command-line cursor style should be written only once"
    );
}

#[test]
fn search_prompt_does_not_use_command_line_only_overlay_trace() {
    let _guard = redraw_trace_observation_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    reset_test_redraw_trace_diagnostic_counts();
    let mut coordinator = TuiRenderCoordinator::new_headless(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    );
    let mut writer = RecordingOverlayWriter::default();
    let mut last_workspace = Some(main_test_workspace());

    let result = render_command_line_only_redraw_if_possible(
        &mut coordinator,
        Some(&mut writer),
        &mut last_workspace,
        None,
        false,
        Some('/'),
        "word",
        4,
        4,
    );

    assert_eq!(result, CommandLineOnlyRedraw::NotApplicable);
    assert_eq!(
        test_redraw_trace_diagnostic_counts(),
        RedrawTraceCounts::default()
    );
    assert!(writer.cursor_styles.is_empty());
}

#[test]
fn command_line_only_render_is_not_used_when_workspace_projection_is_dirty() {
    let _guard = redraw_trace_observation_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    reset_test_redraw_trace_diagnostic_counts();
    let mut coordinator = TuiRenderCoordinator::new_headless(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    );
    let mut writer = RecordingOverlayWriter::default();
    let mut last_workspace = Some(main_test_workspace());

    let result = render_command_line_only_redraw_if_possible(
        &mut coordinator,
        Some(&mut writer),
        &mut last_workspace,
        None,
        true,
        Some(':'),
        "",
        0,
        4,
    );

    assert_eq!(result, CommandLineOnlyRedraw::NotApplicable);
    assert_eq!(
        test_redraw_trace_diagnostic_counts().command_line_only_overlay,
        0
    );
    assert!(writer.cursor_styles.is_empty());
    assert!(
        last_workspace
            .as_ref()
            .is_some_and(|workspace| workspace.command_line.is_none()),
        "dirty workspace projection must not reuse the stale workspace for ':'"
    );
}

#[test]
fn command_line_overlay_failure_is_observable_before_workspace_fallback() {
    let _guard = redraw_trace_observation_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    reset_test_redraw_trace_diagnostic_counts();
    let mut coordinator = TuiRenderCoordinator::new_headless(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    );
    let mut writer = FailingOverlayWriter;
    let mut last_workspace = Some(main_test_workspace());

    let result = render_command_line_only_redraw_if_possible(
        &mut coordinator,
        Some(&mut writer),
        &mut last_workspace,
        None,
        false,
        Some(':'),
        "write",
        5,
        4,
    );

    let counts = test_redraw_trace_diagnostic_counts();
    assert_eq!(result, CommandLineOnlyRedraw::Fallback);
    assert_eq!(counts.command_line_only_overlay, 1);
    assert_eq!(counts.command_line_overlay_fallback, 1);
    assert_eq!(counts.workspace_render_build_started, 0);
    assert_eq!(counts.renderer_frame_requested, 0);
}

struct FailingOverlayWriter;

impl OverlayTerminalWriter for FailingOverlayWriter {
    fn write_overlay_bytes(&mut self, _bytes: &[u8]) -> Result<(), String> {
        Ok(())
    }

    fn set_cursor_style(&mut self, _style: ScreenCursorStyle) -> Result<(), String> {
        Err("forced cursor style failure".to_string())
    }
}

#[test]
fn latest_user_visible_message_returns_last_user_visible_message() {
    let messages = vec![
        CoreMessageEvent {
            severity: vim_core_rs::CoreMessageSeverity::Info,
            category: vim_core_rs::CoreMessageCategory::UserVisible,
            content: "first".to_string(),
        },
        CoreMessageEvent {
            severity: vim_core_rs::CoreMessageSeverity::Error,
            category: vim_core_rs::CoreMessageCategory::UserVisible,
            content: "second".to_string(),
        },
    ];

    assert_eq!(
        latest_user_visible_message(messages),
        Some("second".to_string())
    );
}

#[test]
fn latest_user_visible_message_ignores_undo_command_feedback() {
    let messages = vec![
        CoreMessageEvent {
            severity: vim_core_rs::CoreMessageSeverity::Info,
            category: vim_core_rs::CoreMessageCategory::CommandFeedback,
            content: "2 fewer lines; before #2  4 seconds ago".to_string(),
        },
        CoreMessageEvent {
            severity: vim_core_rs::CoreMessageSeverity::Info,
            category: vim_core_rs::CoreMessageCategory::CommandFeedback,
            content: "1 change; after #3  1 second ago".to_string(),
        },
    ];

    assert_eq!(latest_user_visible_message(messages), None);
}

#[test]
fn latest_user_visible_message_skips_command_feedback_and_keeps_visible_notice() {
    let messages = vec![
        CoreMessageEvent {
            severity: vim_core_rs::CoreMessageSeverity::Info,
            category: vim_core_rs::CoreMessageCategory::CommandFeedback,
            content: "2 fewer lines; before #2  4 seconds ago".to_string(),
        },
        CoreMessageEvent {
            severity: vim_core_rs::CoreMessageSeverity::Warning,
            category: vim_core_rs::CoreMessageCategory::UserVisible,
            content: "visible warning".to_string(),
        },
    ];

    assert_eq!(
        latest_user_visible_message(messages),
        Some("visible warning".to_string())
    );
}

#[test]
fn core_screen_size_sync_does_not_enqueue_redraw_when_size_is_unchanged() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut outcome =
        crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest::default())
            .expect("launch should succeed");
    let mut last_synced_terminal_size = None;
    let terminal_size = TerminalSize {
        columns: 80,
        rows: 24,
    };

    assert!(sync_core_screen_size_if_changed(
        &mut outcome,
        &mut last_synced_terminal_size,
        terminal_size,
    ));
    let first_batch = outcome.core_bridge.take_normalized_outcomes();
    assert!(
        !first_batch.is_empty(),
        "first screen-size sync should expose the core layout redraw"
    );

    assert!(!sync_core_screen_size_if_changed(
        &mut outcome,
        &mut last_synced_terminal_size,
        terminal_size,
    ));
    assert!(
        outcome.core_bridge.take_normalized_outcomes().is_empty(),
        "unchanged screen-size sync must not leave a stale redraw for the next keypress"
    );
}

#[test]
fn core_screen_size_sync_updates_when_size_changes() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut outcome =
        crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest::default())
            .expect("launch should succeed");
    let mut last_synced_terminal_size = Some(TerminalSize {
        columns: 80,
        rows: 24,
    });

    assert!(sync_core_screen_size_if_changed(
        &mut outcome,
        &mut last_synced_terminal_size,
        TerminalSize {
            columns: 100,
            rows: 30,
        },
    ));
    assert_eq!(
        last_synced_terminal_size,
        Some(TerminalSize {
            columns: 100,
            rows: 30,
        })
    );
    assert!(
        !outcome.core_bridge.take_normalized_outcomes().is_empty(),
        "changed size should still request the necessary layout redraw"
    );
}

#[test]
fn rendered_structural_refresh_no_longer_blocks_command_line_overlay() {
    let mut accumulator = MainOutcomeAccumulator {
        last_structural_refresh: Some(StructuralRefresh::from_folded_effects(
            &crate::core::outcome::StructuralEffectSet {
                redraw: Some(crate::core::outcome::RedrawEffect {
                    full: true,
                    clear_before_draw: false,
                    required_by_structure_change: true,
                    coalesced_count: 1,
                }),
                invalidate_buffers: vec![],
                invalidate_windows: vec![],
                layout_dirty: true,
            },
        )),
        ..MainOutcomeAccumulator::default()
    };

    mark_structural_refresh_rendered(&mut accumulator);

    let refresh = accumulator
        .last_structural_refresh
        .as_ref()
        .expect("rendered refresh should leave a neutral diagnostic state");
    assert!(
        structural_refresh_is_idle(Some(refresh)),
        "a structural refresh that has already been rendered must not force the next command-line key into a full redraw"
    );
}

#[test]
fn terminal_display_invalidation_forces_full_clear_redraw_without_core_changes() {
    let redraw_plan =
        effective_workspace_redraw_plan(None, Some(&terminal_display_invalidated_redraw_plan()));

    assert!(redraw_plan.requested);
    assert!(redraw_plan.full);
    assert!(redraw_plan.clear_before_draw);
    assert_eq!(
        redraw_plan.source,
        crate::presentation::structural_refresh::RedrawPlanSource::TerminalDisplayInvalidation
    );
}

#[test]
fn terminal_display_invalidation_overrides_idle_structural_refresh_for_resume() {
    let idle_refresh =
        StructuralRefresh::from_folded_effects(&crate::core::outcome::StructuralEffectSet {
            redraw: None,
            invalidate_buffers: vec![],
            invalidate_windows: vec![],
            layout_dirty: false,
        });

    let redraw_plan = effective_workspace_redraw_plan(
        Some(&idle_refresh),
        Some(&terminal_display_invalidated_redraw_plan()),
    );

    assert!(redraw_plan.requested);
    assert!(redraw_plan.full);
    assert!(redraw_plan.clear_before_draw);
    assert_eq!(
        redraw_plan.source,
        crate::presentation::structural_refresh::RedrawPlanSource::TerminalDisplayInvalidation
    );
}

#[test]
fn consume_core_outcomes_marks_need_redraw_when_bridge_has_pending_redraw() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut bridge = crate::core::bridge::CoreBridge::new("hello\n").expect("core bridge");
    let mut accumulator = MainOutcomeAccumulator::default();
    let mut need_redraw = false;

    bridge
        .apply_ex_command(":redraw")
        .expect(":redraw should succeed");
    consume_core_outcomes_from_core(&mut bridge, &mut accumulator, &mut need_redraw);

    assert!(
        need_redraw,
        "pending redraw from core should mark need_redraw"
    );
    assert!(
        bridge.take_normalized_outcomes().is_empty(),
        "normalized outcomes should be drained after helper runs"
    );
}

#[test]
fn consume_core_outcomes_replaces_stale_structural_refresh_on_empty_batch() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut bridge = crate::core::bridge::CoreBridge::new("hello\n").expect("core bridge");
    assert!(
        bridge.take_normalized_outcomes().is_empty(),
        "new bridge should not start with pending normalized outcomes"
    );
    let stale_full_refresh =
        StructuralRefresh::from_folded_effects(&crate::core::outcome::StructuralEffectSet {
            redraw: Some(crate::core::outcome::RedrawEffect {
                full: true,
                clear_before_draw: true,
                required_by_structure_change: true,
                coalesced_count: 1,
            }),
            invalidate_buffers: vec![1],
            invalidate_windows: vec![1],
            layout_dirty: true,
        });
    let mut accumulator = MainOutcomeAccumulator {
        last_structural_refresh: Some(stale_full_refresh),
        ..MainOutcomeAccumulator::default()
    };
    let mut need_redraw = false;

    consume_core_outcomes_from_core(&mut bridge, &mut accumulator, &mut need_redraw);

    assert!(
        !need_redraw,
        "empty batch should leave redraw scheduling to the caller policy"
    );
    let refresh = accumulator
        .last_structural_refresh
        .expect("empty batch should record a neutral structural refresh");
    assert!(!refresh.redraw_plan.requested);
    assert!(!refresh.redraw_plan.full);
    assert!(!refresh.redraw_plan.clear_before_draw);
    assert_eq!(
        refresh.redraw_plan.source,
        crate::presentation::structural_refresh::RedrawPlanSource::None
    );
}

#[test]
fn consume_core_outcomes_tracks_active_prompt_in_projection_state() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut bridge = crate::core::bridge::CoreBridge::new("hello\n").expect("core bridge");
    let mut accumulator = MainOutcomeAccumulator::default();
    let mut need_redraw = false;

    bridge
        .apply_ex_command(":input Name")
        .expect("input request should succeed");
    consume_core_outcomes_from_core(&mut bridge, &mut accumulator, &mut need_redraw);

    assert_eq!(
        accumulator
            .projection
            .prompt()
            .active_input()
            .map(|view| view.correlation_id),
        Some(1)
    );
    assert_eq!(
        accumulator
            .last_projection_frame
            .as_ref()
            .and_then(|frame| frame.input_prompt.as_ref())
            .map(|view| view.prompt.as_str()),
        Some("Name")
    );
}

#[test]
fn prompt_response_success_is_routed_through_bridge_and_closes_only_after_folded_batch() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut bridge = crate::core::bridge::CoreBridge::new("hello\n").expect("core bridge");
    let mut accumulator = MainOutcomeAccumulator::default();
    let mut need_redraw = false;

    bridge
        .apply_ex_command(":input Name")
        .expect("input request should succeed");
    consume_core_outcomes_from_core(&mut bridge, &mut accumulator, &mut need_redraw);
    assert_eq!(
        accumulator
            .projection
            .prompt()
            .active_input()
            .map(|view| view.correlation_id),
        Some(1)
    );

    let action = crate::core::notification_prompt::handle_prompt_key(
        &mut accumulator.projection,
        &KeyInput::Enter,
    );
    let command = match action {
        crate::core::notification_prompt::PromptInputAction::Submit(command) => command,
        other => panic!("expected submit action, got {other:?}"),
    };

    dispatch_prompt_response_command(&mut bridge, &mut accumulator, command, &mut need_redraw);

    assert!(need_redraw);
    assert!(accumulator.projection.prompt().active_input().is_none());
    assert_eq!(
        accumulator
            .projection
            .prompt()
            .last_transition()
            .map(|transition| transition.kind),
        Some(crate::core::notification_prompt::PromptTransitionKind::Submitted)
    );
}

#[test]
fn prompt_response_end_to_end_preserves_typed_input_value() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut bridge = crate::core::bridge::CoreBridge::new("hello\n").expect("core bridge");
    let mut accumulator = MainOutcomeAccumulator::default();
    let mut need_redraw = false;

    bridge
        .apply_ex_command(":input Name")
        .expect("input request should succeed");
    consume_core_outcomes_from_core(&mut bridge, &mut accumulator, &mut need_redraw);

    assert!(matches!(
        crate::core::notification_prompt::handle_prompt_key(
            &mut accumulator.projection,
            &KeyInput::Char('a'),
        ),
        crate::core::notification_prompt::PromptInputAction::Consumed
    ));
    assert!(matches!(
        crate::core::notification_prompt::handle_prompt_key(
            &mut accumulator.projection,
            &KeyInput::Char('b'),
        ),
        crate::core::notification_prompt::PromptInputAction::Consumed
    ));
    let action = crate::core::notification_prompt::handle_prompt_key(
        &mut accumulator.projection,
        &KeyInput::Enter,
    );
    let command = match action {
        crate::core::notification_prompt::PromptInputAction::Submit(command) => command,
        other => panic!("expected submit action, got {other:?}"),
    };

    dispatch_prompt_response_command(&mut bridge, &mut accumulator, command, &mut need_redraw);

    assert!(accumulator.projection.prompt().active_input().is_none());
    assert_eq!(
        accumulator
            .projection
            .prompt()
            .last_transition()
            .map(|transition| (transition.kind, transition.input_len)),
        Some((
            crate::core::notification_prompt::PromptTransitionKind::Submitted,
            2
        ))
    );
}

#[test]
fn structural_redraw_does_not_close_active_prompt() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut bridge = crate::core::bridge::CoreBridge::new("hello\n").expect("core bridge");
    let mut accumulator = MainOutcomeAccumulator::default();
    let mut need_redraw = false;

    bridge
        .apply_ex_command(":input Name")
        .expect("input request should succeed");
    consume_core_outcomes_from_core(&mut bridge, &mut accumulator, &mut need_redraw);
    assert_eq!(
        accumulator
            .projection
            .prompt()
            .active_input()
            .map(|view| view.correlation_id),
        Some(1)
    );

    need_redraw = false;
    bridge
        .apply_ex_command(":redraw")
        .expect(":redraw should succeed");
    consume_core_outcomes_from_core(&mut bridge, &mut accumulator, &mut need_redraw);

    assert!(
        need_redraw,
        "structural redraw should still request a redraw"
    );
    assert_eq!(
        accumulator
            .projection
            .prompt()
            .active_input()
            .map(|view| view.correlation_id),
        Some(1),
        "redraw-only dispatch must not close the active prompt"
    );
}

#[test]
fn prompt_response_error_restores_active_prompt_and_preserves_buffer() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut bridge = crate::core::bridge::CoreBridge::new("hello\n").expect("core bridge");
    let mut accumulator = MainOutcomeAccumulator::default();
    let mut need_redraw = false;

    bridge
        .apply_ex_command(":input Name")
        .expect("input request should succeed");
    consume_core_outcomes_from_core(&mut bridge, &mut accumulator, &mut need_redraw);
    assert!(matches!(
        crate::core::notification_prompt::handle_prompt_key(
            &mut accumulator.projection,
            &KeyInput::Char('x'),
        ),
        crate::core::notification_prompt::PromptInputAction::Consumed
    ));
    let action = crate::core::notification_prompt::handle_prompt_key(
        &mut accumulator.projection,
        &KeyInput::Enter,
    );
    let command = match action {
        crate::core::notification_prompt::PromptInputAction::Submit(command) => command,
        other => panic!("expected submit action, got {other:?}"),
    };

    dispatch_prompt_response_command(
        &mut bridge,
        &mut accumulator,
        crate::core::prompt::PromptResponseCommand::Submit {
            correlation_id: command.correlation_id() + 1,
            value: "ignored".to_string(),
        },
        &mut need_redraw,
    );

    assert!(matches!(
        accumulator
            .projection
            .prompt()
            .active_input()
            .map(|view| view.status),
        Some(crate::core::notification_prompt::InputPromptStatus::Active)
    ));
    assert_eq!(
        accumulator
            .projection
            .prompt()
            .active_input()
            .map(|view| view.input.as_str()),
        Some("x")
    );
    assert!(
        accumulator
            .projection
            .prompt()
            .last_response_error()
            .is_some_and(|message| message.contains("expected=1"))
    );
}

#[tokio::test(flavor = "current_thread")]
async fn selector_accept_action_opens_selected_rg_location_and_hides_reopenable_selector() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("selector-rg-jump").with_extension("txt");
    std::fs::write(&target_path, "first\nabcdef\nthird\n").expect("target fixture");
    let target_literal = serde_json::to_string(&target_path.to_string_lossy()).expect("path JSON");
    let seed = crate::runtime::callback_registry_seed::CallbackRegistrySeed::from_startup_entries(
        vec![crate::runtime::config::StartupRegistryEntry::Event {
            name: "bufferOpen".to_string(),
            callback_source: format!(
                r#"
                        async () => {{
                            await saya.selector.open({{
                                source: {{
                                    kind: "static",
                                    items: [
                                        {{
                                            id: "rg-target",
                                            value: "target.txt:2:4:abcdef",
                                            kind: "rg",
                                            detail: {{ path: {target_literal}, line: 2, column: 4, text: "abcdef" }},
                                        }},
                                    ],
                                }},
                                matcher: "substringAnd",
                                query: "target",
                            }});
                        }}
                    "#
            ),
        }],
    );
    let mut runtime_session =
        RuntimeSessionOwner::spawn(seed).expect("runtime owner should initialize");
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();

    {
        let mut host_session = MainRuntimeHostSession::new(&mut outcome, &mut session_state);
        runtime_session
            .dispatch(
                crate::runtime::live::RuntimeEventPayload::BufferOpen(
                    crate::runtime::live::BufferEventPayload {
                        buffer: host_session.current_buffer_snapshot(),
                    },
                ),
                &mut host_session,
            )
            .await;
    }
    let selector_model = runtime_session
        .selector_tui_projection_sink()
        .current_model()
        .expect("selector should be active before Enter");

    let dispatch_outcome = handle_selector_accept_action(
        &mut runtime_session,
        &selector_model,
        &mut outcome,
        &mut session_state,
    )
    .await;

    assert!(dispatch_outcome.requires_redraw);
    assert_eq!(session_state.target_path(), Some(&target_path));
    assert_eq!(outcome.target_path, Some(target_path.clone()));
    let snapshot = outcome.core_bridge.light_snapshot();
    assert_eq!(snapshot.cursor_row, 1, "rg line is 1-based");
    assert_eq!(snapshot.cursor_col, 3, "rg column is 1-based");
    let hidden_model = runtime_session
        .selector_tui_projection_sink()
        .current_model()
        .expect("selector hide should publish model");
    assert!(hidden_model.hidden);
    assert!(!hidden_model.cancelled);
    assert_eq!(
        hidden_model
            .selected_row
            .as_ref()
            .map(|row| row.item.id.as_str()),
        Some("rg-target")
    );
    {
        let mut host_session = MainRuntimeHostSession::new(&mut outcome, &mut session_state);
        let reopen_outcome = runtime_session
            .control_selector(
                hidden_model.session_id,
                RuntimeSelectorControllerCommand::Show,
                &mut host_session,
            )
            .await;
        assert!(reopen_outcome.requires_redraw);
    }
    let reopened_model = runtime_session
        .selector_tui_projection_sink()
        .current_model()
        .expect("selector show should publish model");
    assert!(!reopened_model.hidden);
    assert!(!reopened_model.cancelled);
    assert_eq!(
        reopened_model
            .selected_row
            .as_ref()
            .map(|row| row.item.id.as_str()),
        Some("rg-target")
    );
    assert!(
        outcome.core_bridge.buffer_text().contains("abcdef"),
        "Enter must open the selected file instead of leaking into normal editing"
    );

    std::fs::remove_file(target_path).expect("cleanup target fixture");
}

#[tokio::test(flavor = "current_thread")]
async fn selector_accept_action_reports_invalid_rg_detail_without_normal_enter_leak() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let seed =
        crate::runtime::callback_registry_seed::CallbackRegistrySeed::from_startup_entries(vec![
            crate::runtime::config::StartupRegistryEntry::Event {
                name: "bufferOpen".to_string(),
                callback_source: r#"
                    async () => {
                        await saya.selector.open({
                            source: {
                                kind: "static",
                                items: [
                                    {
                                        id: "rg-invalid",
                                        value: "broken",
                                        kind: "rg",
                                        detail: { path: "missing.txt", line: 1 },
                                    },
                                ],
                            },
                            matcher: "substringAnd",
                            query: "broken",
                        });
                    }
                "#
                .to_string(),
            },
        ]);
    let mut runtime_session =
        RuntimeSessionOwner::spawn(seed).expect("runtime owner should initialize");
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();

    {
        let mut host_session = MainRuntimeHostSession::new(&mut outcome, &mut session_state);
        runtime_session
            .dispatch(
                crate::runtime::live::RuntimeEventPayload::BufferOpen(
                    crate::runtime::live::BufferEventPayload {
                        buffer: host_session.current_buffer_snapshot(),
                    },
                ),
                &mut host_session,
            )
            .await;
    }
    let before_text = outcome.core_bridge.buffer_text();
    let selector_model = runtime_session
        .selector_tui_projection_sink()
        .current_model()
        .expect("selector should be active before Enter");

    let dispatch_outcome = handle_selector_accept_action(
        &mut runtime_session,
        &selector_model,
        &mut outcome,
        &mut session_state,
    )
    .await;

    assert!(dispatch_outcome.requires_redraw);
    assert!(
        dispatch_outcome
            .transient_message
            .as_deref()
            .is_some_and(|message| message.contains("detail.column")),
        "invalid detail should report a user-visible failure: {:?}",
        dispatch_outcome.transient_message
    );
    assert_eq!(
        outcome.core_bridge.buffer_text(),
        before_text,
        "invalid selector Enter must be consumed without normal Enter editing"
    );
    assert_eq!(session_state.target_path(), None);
    let current_model = runtime_session
        .selector_tui_projection_sink()
        .current_model()
        .expect("failed action should retain selector state");
    assert!(!current_model.cancelled);
}

#[test]
fn markdown_metadata_collection_is_limited_to_markdown_target_paths() {
    assert!(is_markdown_target_path(Some(&PathBuf::from("notes.md"))));
    assert!(is_markdown_target_path(Some(&PathBuf::from(
        "notes.markdown"
    ))));
    assert!(is_markdown_target_path(Some(&PathBuf::from("notes.MDOWN"))));
    assert!(!is_markdown_target_path(Some(&PathBuf::from("notes.txt"))));
    assert!(!is_markdown_target_path(None));
}
