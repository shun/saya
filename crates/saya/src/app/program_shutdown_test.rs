use super::program_test_support::*;
use super::*;

#[test]
fn shutdown_reason_maps_clean_quit_to_user_quit() {
    let mut transient_msg = None;

    let reason = shutdown_reason_from_quit_decision(QuitDecision::Allow, false, &mut transient_msg);

    assert_eq!(reason, Some(ShutdownReason::UserQuit));
    assert_eq!(transient_msg, None);
}

#[test]
fn shutdown_reason_maps_forced_quit_to_force_quit() {
    let mut transient_msg = None;

    let reason =
        shutdown_reason_from_quit_decision(QuitDecision::ForceQuit, true, &mut transient_msg);

    assert_eq!(reason, Some(ShutdownReason::UserForceQuit));
    assert_eq!(transient_msg, None);
}

#[test]
fn shutdown_reason_keeps_loop_running_when_quit_is_rejected() {
    let mut transient_msg = None;

    let reason =
        shutdown_reason_from_quit_decision(QuitDecision::WarnUnsaved, false, &mut transient_msg);

    assert_eq!(reason, None);
    assert_eq!(
        transient_msg,
        Some(normal_quit_warning_message().to_string())
    );
}

#[test]
fn normal_and_force_quit_messages_remain_distinct() {
    let mut normal_transient_msg = None;
    let normal_reason = shutdown_reason_from_quit_decision(
        QuitDecision::WarnUnsaved,
        false,
        &mut normal_transient_msg,
    );

    let mut force_transient_msg = None;
    let force_reason =
        shutdown_reason_from_quit_decision(QuitDecision::ForceQuit, true, &mut force_transient_msg);

    assert_eq!(normal_reason, None);
    assert_eq!(force_reason, Some(ShutdownReason::UserForceQuit));
    assert_eq!(
        normal_transient_msg,
        Some("No write since last change (add ! to override)".to_string())
    );
    assert_eq!(force_transient_msg, None);
    assert_ne!(normal_transient_msg, force_transient_msg);
}

#[test]
fn merge_shutdown_reason_prefers_force_quit_over_clean_quit() {
    let mut shutdown_reason = Some(ShutdownReason::UserQuit);

    merge_shutdown_reason(&mut shutdown_reason, Some(ShutdownReason::UserForceQuit));

    assert_eq!(shutdown_reason, Some(ShutdownReason::UserForceQuit));
}

#[test]
fn save_error_message_reports_read_only_mode() {
    let message = save_error_message(&SaveRequestError::ReadOnly);

    assert_eq!(message, "Read-only option is set; add ! to override");
}

#[test]
fn write_host_action_updates_transient_message_on_failure() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("write-failure");
    std::fs::write(&target_path, "initial\n").expect("test file");

    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::File(target_path.clone()),
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let bad_path = PathBuf::from("/nonexistent/dir/file.txt");
    let mut session_state = crate::app::session::EditorSessionState::new(Some(bad_path));

    outcome.core_bridge.dispatch_key("i").unwrap();
    outcome.core_bridge.dispatch_key("X").unwrap();
    outcome.core_bridge.dispatch_key("\x1b").unwrap();
    sync_session_dirty_from_core(&mut session_state, &outcome.core_bridge);

    outcome
        .core_bridge
        .apply_ex_command(":w")
        .expect(":w command should succeed");

    let mut outcome_accumulator = MainOutcomeAccumulator::default();
    let mut transient_msg = None;
    let mut system_warning = None;
    let mut need_redraw = false;
    let mut host_action_runtime = HostActionRuntime::default();
    consume_core_outcomes_from_core(
        &mut outcome.core_bridge,
        &mut outcome_accumulator,
        &mut need_redraw,
    );
    assert!(
        matches!(
            outcome_accumulator.host_directives.as_slice(),
            [NormalizedHostDirective::Write { .. }]
        ),
        ":w 後に normalized write directive が 1 件発行されること: {:?}",
        outcome_accumulator.host_directives
    );
    let shutdown = process_pending_host_actions_without_runtime(
        &mut outcome,
        &mut outcome_accumulator,
        &mut session_state,
        &mut transient_msg,
        &mut system_warning,
        &mut host_action_runtime,
    );
    let expected_error = session_state
        .last_save_error()
        .expect("save failure should be recorded")
        .to_string();
    let expected_message = format!("Save failed: {}", expected_error);

    assert_eq!(shutdown, None);
    assert_eq!(transient_msg, Some(expected_message.clone()));
    assert_eq!(transient_msg.as_deref(), Some(expected_message.as_str()));
    assert!(session_state.is_dirty());

    std::fs::remove_file(&target_path).expect("cleanup");
}

#[test]
fn write_host_action_creates_missing_named_file_without_quit_warning() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("write-missing-named-file.md");
    assert!(
        !target_path.exists(),
        "test starts with a nonexistent target"
    );

    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::File(target_path.clone()),
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();

    outcome.core_bridge.dispatch_key("i").unwrap();
    outcome.core_bridge.dispatch_key("hello").unwrap();
    outcome.core_bridge.dispatch_key("\x1b").unwrap();
    sync_session_dirty_from_core(&mut session_state, &outcome.core_bridge);

    let mut outcome_accumulator = MainOutcomeAccumulator::default();
    let mut transient_msg = None;
    let mut system_warning = None;
    let mut need_redraw = false;
    let mut host_action_runtime = HostActionRuntime::default();

    outcome
        .core_bridge
        .apply_ex_command(":q")
        .expect(":q command should succeed");
    consume_core_outcomes_from_core(
        &mut outcome.core_bridge,
        &mut outcome_accumulator,
        &mut need_redraw,
    );
    let quit_shutdown = process_pending_host_actions_without_runtime(
        &mut outcome,
        &mut outcome_accumulator,
        &mut session_state,
        &mut transient_msg,
        &mut system_warning,
        &mut host_action_runtime,
    );
    assert_eq!(quit_shutdown, None);
    assert_eq!(
        system_warning,
        Some(normal_quit_warning_message().to_string())
    );

    outcome
        .core_bridge
        .apply_ex_command(":w")
        .expect(":w command should succeed");
    consume_core_outcomes_from_core(
        &mut outcome.core_bridge,
        &mut outcome_accumulator,
        &mut need_redraw,
    );
    let write_shutdown = process_pending_host_actions_without_runtime(
        &mut outcome,
        &mut outcome_accumulator,
        &mut session_state,
        &mut transient_msg,
        &mut system_warning,
        &mut host_action_runtime,
    );

    assert_eq!(write_shutdown, None);
    assert_eq!(system_warning, None);
    assert_eq!(transient_msg, Some("Saved successfully".to_string()));
    assert_eq!(
        std::fs::read_to_string(&target_path).expect("missing target should be created"),
        outcome.core_bridge.snapshot().text
    );
    assert!(!session_state.is_dirty());

    std::fs::remove_file(&target_path).expect("cleanup");
}

#[test]
fn directory_buffer_write_prepares_operation_preview_without_filesystem_mutation() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("directory-write-plan");
    let alpha_path = root_path.join("alpha.md");
    let beta_path = root_path.join("beta.md");
    let renamed_path = root_path.join("renamed.md");
    std::fs::create_dir_all(&root_path).expect("test directory");
    std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
    std::fs::write(&beta_path, "beta\n").expect("beta file");
    let mut session_state = crate::app::session::EditorSessionState::new(Some(root_path.clone()));

    let save_outcome = save_snapshot_result("renamed.md\nbeta.md\n", &mut session_state);

    let preview = session_state
        .pending_directory_operation_preview()
        .expect("plain write should prepare a preview");
    assert_eq!(
        save_outcome,
        SaveSnapshotOutcome {
            transient_message: Some(format!(
                "Apply 1 dired operation(s) (0 high-risk)? y/Enter=OK n/Esc=Cancel id={}",
                preview.id
            )),
            wrote: false,
            pending_directory_confirmation: true,
        }
    );
    assert!(alpha_path.exists(), "rename must not be applied in phase 7");
    assert!(!renamed_path.exists(), "rename target is only planned");
    assert!(beta_path.exists());

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[test]
fn directory_buffer_write_reports_validation_error_without_filesystem_mutation() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("directory-write-validation");
    let alpha_path = root_path.join("alpha.md");
    std::fs::create_dir_all(&root_path).expect("test directory");
    std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
    let mut session_state = crate::app::session::EditorSessionState::new(Some(root_path.clone()));

    let save_outcome =
        save_snapshot_result("alpha.md\n\n../escape.md\nalpha.md\n", &mut session_state);

    assert_eq!(
        save_outcome,
        SaveSnapshotOutcome {
            transient_message: Some(
                "Directory operation plan failed validation: 4 error(s)".to_string()
            ),
            wrote: false,
            pending_directory_confirmation: false,
        }
    );
    assert!(
        alpha_path.exists(),
        "invalid writable directory edits must not mutate the filesystem"
    );

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[test]
fn directory_buffer_plain_write_previews_delete_without_filesystem_mutation() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("directory-write-preview-delete");
    let alpha_path = root_path.join("alpha.md");
    let beta_path = root_path.join("beta.md");
    std::fs::create_dir_all(&root_path).expect("test directory");
    std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
    std::fs::write(&beta_path, "beta\n").expect("beta file");
    let mut session_state = crate::app::session::EditorSessionState::new(Some(root_path.clone()));

    let save_outcome = save_snapshot_result("beta.md\n", &mut session_state);
    let preview = session_state
        .pending_directory_operation_preview()
        .expect("plain write should prepare a pending preview");

    assert_eq!(preview.operation_count, 1);
    assert_eq!(preview.high_risk_count, 1);
    assert_eq!(
        save_outcome.transient_message,
        Some(format!(
            "Apply 1 dired operation(s) (1 high-risk)? y/Enter=OK n/Esc=Cancel id={}",
            preview.id
        ))
    );
    assert!(!save_outcome.wrote);
    assert!(
        session_state.directory_operation_confirmation_dialog_active(),
        "plain :write should open the confirmation dialog"
    );
    assert!(alpha_path.exists(), "plain :write must not delete files");
    assert!(beta_path.exists());

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[test]
fn directory_buffer_write_confirmation_ok_applies_delete_and_refreshes_metadata() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("directory-write-dialog-ok");
    let alpha_path = root_path.join("alpha.md");
    let beta_path = root_path.join("beta.md");
    std::fs::create_dir_all(&root_path).expect("test directory");
    std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
    std::fs::write(&beta_path, "beta\n").expect("beta file");
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    execute_runtime_host_command(
        &format!("edit {}", root_path.display()),
        &mut outcome,
        &mut session_state,
    )
    .expect("open directory listing");
    outcome
        .core_bridge
        .dispatch_key("dd")
        .expect("delete current listing line");

    let preview_effect = execute_runtime_host_command("write", &mut outcome, &mut session_state)
        .expect("plain write should prepare confirmation dialog");
    assert!(
        preview_effect
            .transient_message
            .as_deref()
            .is_some_and(|message| message.contains("y/Enter=OK n/Esc=Cancel")),
        "plain write should ask for confirmation: {:?}",
        preview_effect.transient_message
    );
    assert!(alpha_path.exists());

    let mut transient_msg = None;
    let mut need_redraw = false;
    let applied = handle_directory_operation_confirmation_key_without_runtime(
        &KeyInput::Char('y'),
        &mut outcome,
        &mut session_state,
        &mut transient_msg,
        &mut need_redraw,
    );

    assert_eq!(
        applied,
        Some(None),
        "y should confirm the pending directory write without requesting shutdown"
    );
    assert_eq!(
        transient_msg,
        Some("Directory operations applied: 1 operation(s)".to_string())
    );
    assert!(need_redraw);
    assert!(!alpha_path.exists(), "OK should delete alpha");
    assert!(beta_path.exists());
    assert_eq!(outcome.core_bridge.snapshot().text, "beta.md\n");
    assert!(
        !session_state.directory_operation_confirmation_dialog_active(),
        "confirmed dialog should be cleared"
    );

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[test]
fn directory_buffer_write_confirmation_cancel_keeps_filesystem_unchanged() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("directory-write-dialog-cancel");
    let alpha_path = root_path.join("alpha.md");
    let beta_path = root_path.join("beta.md");
    std::fs::create_dir_all(&root_path).expect("test directory");
    std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
    std::fs::write(&beta_path, "beta\n").expect("beta file");
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    execute_runtime_host_command(
        &format!("edit {}", root_path.display()),
        &mut outcome,
        &mut session_state,
    )
    .expect("open directory listing");
    outcome
        .core_bridge
        .dispatch_key("dd")
        .expect("delete current listing line");
    execute_runtime_host_command("write", &mut outcome, &mut session_state)
        .expect("plain write should prepare confirmation dialog");

    let mut transient_msg = None;
    let mut need_redraw = false;
    let cancelled = handle_directory_operation_confirmation_key_without_runtime(
        &KeyInput::Escape,
        &mut outcome,
        &mut session_state,
        &mut transient_msg,
        &mut need_redraw,
    );

    assert_eq!(
        cancelled,
        Some(None),
        "Esc should cancel the pending directory write without requesting shutdown"
    );
    assert_eq!(
        transient_msg,
        Some("Directory operation cancelled; no filesystem changes were applied".to_string())
    );
    assert!(need_redraw);
    assert!(alpha_path.exists(), "cancel must not delete alpha");
    assert!(beta_path.exists());
    assert!(
        session_state
            .pending_directory_operation_preview()
            .is_none()
    );

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[test]
fn directory_buffer_force_write_applies_latest_preview_and_refreshes_metadata() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("directory-write-apply");
    let alpha_path = root_path.join("alpha.md");
    let beta_path = root_path.join("beta.md");
    let renamed_path = root_path.join("renamed.md");
    let created_path = root_path.join("notes.md");
    let created_dir_path = root_path.join("src");
    std::fs::create_dir_all(&root_path).expect("test directory");
    std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
    std::fs::write(&beta_path, "beta\n").expect("beta file");
    let mut session_state = crate::app::session::EditorSessionState::new(Some(root_path.clone()));

    let edited_text = "renamed.md\nnotes.md\nsrc/\n";
    let preview_outcome = save_snapshot_result(edited_text, &mut session_state);
    assert!(!preview_outcome.wrote);

    let apply_outcome =
        save_snapshot_result_with_confirmation(edited_text, &mut session_state, None, true, None);

    assert_eq!(
        apply_outcome.transient_message,
        Some("Directory operations applied: 3 operation(s)".to_string())
    );
    assert!(apply_outcome.wrote);
    assert!(!alpha_path.exists(), "rename source should be moved");
    assert!(renamed_path.is_file(), "rename target should exist");
    assert!(created_path.is_file(), "create file plan should be applied");
    assert!(
        created_dir_path.is_dir(),
        "create directory plan should be applied"
    );
    assert!(
        session_state
            .pending_directory_operation_preview()
            .is_none(),
        "successful apply should clear the pending preview"
    );
    let entries = session_state
        .directory_buffer()
        .expect("directory metadata should remain active")
        .entries
        .iter()
        .map(|entry| entry.display_text.clone())
        .collect::<Vec<_>>();
    assert!(entries.contains(&"renamed.md".to_string()));
    assert!(entries.contains(&"notes.md".to_string()));
    assert!(entries.contains(&"src/".to_string()));

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[test]
fn directory_buffer_force_write_applies_confirmed_delete_and_refreshes_metadata() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("directory-write-apply-delete");
    let alpha_path = root_path.join("alpha.md");
    let beta_path = root_path.join("beta.md");
    std::fs::create_dir_all(&root_path).expect("test directory");
    std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
    std::fs::write(&beta_path, "beta\n").expect("beta file");
    let mut session_state = crate::app::session::EditorSessionState::new(Some(root_path.clone()));

    let preview_outcome = save_snapshot_result("beta.md\n", &mut session_state);
    let preview = session_state
        .pending_directory_operation_preview()
        .expect("plain write should prepare delete preview");
    assert!(!preview_outcome.wrote);
    assert_eq!(preview.operation_count, 1);
    assert_eq!(preview.high_risk_count, 1);
    assert!(alpha_path.exists());

    let apply_outcome =
        save_snapshot_result_with_confirmation("beta.md\n", &mut session_state, None, true, None);

    assert_eq!(
        apply_outcome.transient_message,
        Some("Directory operations applied: 1 operation(s)".to_string())
    );
    assert!(apply_outcome.wrote);
    assert!(!alpha_path.exists(), "confirmed delete should remove alpha");
    assert!(beta_path.is_file());
    let entries = session_state
        .directory_buffer()
        .expect("directory metadata should refresh after delete")
        .entries
        .iter()
        .map(|entry| entry.display_text.clone())
        .collect::<Vec<_>>();
    assert_eq!(entries, vec!["beta.md".to_string()]);

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[test]
fn directory_buffer_write_host_action_previews_confirms_deletes_and_refreshes_listing() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("directory-write-host-action-delete");
    let alpha_path = root_path.join("alpha.md");
    let beta_path = root_path.join("beta.md");
    let gamma_path = root_path.join("gamma.md");
    std::fs::create_dir_all(&root_path).expect("test directory");
    std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
    std::fs::write(&beta_path, "beta\n").expect("beta file");
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    execute_runtime_host_command(
        &format!("edit {}", root_path.display()),
        &mut outcome,
        &mut session_state,
    )
    .expect("open directory listing");
    assert_eq!(outcome.core_bridge.snapshot().text, "alpha.md\nbeta.md\n");

    outcome
        .core_bridge
        .dispatch_key("dd")
        .expect("delete current listing line");
    let mut outcome_accumulator = MainOutcomeAccumulator::default();
    let mut transient_msg = None;
    let mut system_warning = None;
    let mut need_redraw = false;
    let mut host_action_runtime = HostActionRuntime::default();
    consume_core_outcomes_from_core(
        &mut outcome.core_bridge,
        &mut outcome_accumulator,
        &mut need_redraw,
    );
    outcome
        .core_bridge
        .apply_ex_command(":write")
        .expect(":write should produce a preview host action");
    consume_core_outcomes_from_core(
        &mut outcome.core_bridge,
        &mut outcome_accumulator,
        &mut need_redraw,
    );
    assert!(
        !outcome_accumulator.host_directives.is_empty(),
        ":write should emit a host directive before preview processing"
    );

    let preview_shutdown = process_pending_host_actions_without_runtime(
        &mut outcome,
        &mut outcome_accumulator,
        &mut session_state,
        &mut transient_msg,
        &mut system_warning,
        &mut host_action_runtime,
    );
    let preview = session_state
        .pending_directory_operation_preview()
        .unwrap_or_else(|| {
            panic!(
                "plain :write should prepare a directory operation preview; transient={transient_msg:?}, target={:?}, directory_buffer_present={}",
                session_state.target_path(),
                session_state.directory_buffer().is_some()
            )
        });
    assert_eq!(preview_shutdown, None);
    assert_eq!(preview.operation_count, 1);
    assert_eq!(preview.high_risk_count, 1);
    assert!(
        transient_msg
            .as_deref()
            .is_some_and(|message| message.starts_with("Apply 1 dired operation")),
        "plain :write should report the pending preview: {transient_msg:?}"
    );
    assert!(
        alpha_path.exists(),
        "unconfirmed preview must not delete files"
    );
    std::fs::write(&gamma_path, "gamma\n").expect("external file before confirmed apply");

    outcome
        .core_bridge
        .apply_ex_command(":write!")
        .expect(":write! should produce a confirmed host action");
    consume_core_outcomes_from_core(
        &mut outcome.core_bridge,
        &mut outcome_accumulator,
        &mut need_redraw,
    );
    let apply_shutdown = process_pending_host_actions_without_runtime(
        &mut outcome,
        &mut outcome_accumulator,
        &mut session_state,
        &mut transient_msg,
        &mut system_warning,
        &mut host_action_runtime,
    );

    assert_eq!(apply_shutdown, None);
    assert_eq!(
        transient_msg,
        Some("Directory operations applied: 1 operation(s)".to_string())
    );
    assert!(
        !alpha_path.exists(),
        "confirmed :write! should delete alpha"
    );
    assert!(beta_path.exists());
    assert!(gamma_path.exists());
    assert_eq!(outcome.core_bridge.snapshot().text, "beta.md\ngamma.md\n");
    assert!(
        session_state
            .pending_directory_operation_preview()
            .is_none(),
        "confirmed apply should clear the pending preview"
    );

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[test]
fn directory_buffer_wq_waits_for_preview_confirmation_then_quits() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("directory-wq-delete");
    let alpha_path = root_path.join("alpha.md");
    let beta_path = root_path.join("beta.md");
    std::fs::create_dir_all(&root_path).expect("test directory");
    std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
    std::fs::write(&beta_path, "beta\n").expect("beta file");
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    execute_runtime_host_command(
        &format!("edit {}", root_path.display()),
        &mut outcome,
        &mut session_state,
    )
    .expect("open directory listing");

    outcome
        .core_bridge
        .dispatch_key("dd")
        .expect("delete current listing line");
    let mut outcome_accumulator = MainOutcomeAccumulator::default();
    let mut transient_msg = None;
    let mut system_warning = None;
    let mut need_redraw = false;
    let mut host_action_runtime = HostActionRuntime::default();
    consume_core_outcomes_from_core(
        &mut outcome.core_bridge,
        &mut outcome_accumulator,
        &mut need_redraw,
    );
    sync_session_dirty_from_core(&mut session_state, &outcome.core_bridge);

    outcome
        .core_bridge
        .apply_ex_command(":wq")
        .expect(":wq should produce write then quit host actions");
    consume_core_outcomes_from_core(
        &mut outcome.core_bridge,
        &mut outcome_accumulator,
        &mut need_redraw,
    );
    let preview_shutdown = process_pending_host_actions_without_runtime(
        &mut outcome,
        &mut outcome_accumulator,
        &mut session_state,
        &mut transient_msg,
        &mut system_warning,
        &mut host_action_runtime,
    );

    assert_eq!(
        preview_shutdown, None,
        ":wq should wait for directory operation confirmation"
    );
    assert!(
        alpha_path.exists(),
        "unconfirmed preview must not delete files"
    );
    assert!(
        session_state
            .pending_directory_operation_preview()
            .is_some(),
        ":wq should keep a pending dired preview"
    );

    let handled = handle_directory_operation_confirmation_key_without_runtime(
        &KeyInput::Enter,
        &mut outcome,
        &mut session_state,
        &mut transient_msg,
        &mut need_redraw,
    );

    assert_eq!(
        handled,
        Some(Some(ShutdownReason::UserQuit)),
        "confirmation key should apply and resume the pending quit"
    );
    assert!(
        !alpha_path.exists(),
        "confirmed :wq should apply the dired operation"
    );
    assert!(beta_path.exists());
    assert_eq!(outcome.core_bridge.snapshot().text, "beta.md\n");

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[test]
fn directory_buffer_force_write_rejects_stale_preview_without_mutation() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("directory-write-stale-preview");
    let alpha_path = root_path.join("alpha.md");
    let beta_path = root_path.join("beta.md");
    std::fs::create_dir_all(&root_path).expect("test directory");
    std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
    std::fs::write(&beta_path, "beta\n").expect("beta file");
    let mut session_state = crate::app::session::EditorSessionState::new(Some(root_path.clone()));

    let preview_outcome = save_snapshot_result("beta.md\n", &mut session_state);
    assert!(!preview_outcome.wrote);

    let apply_outcome =
        save_snapshot_result_with_confirmation("alpha.md\n", &mut session_state, None, true, None);

    assert_eq!(
        apply_outcome.transient_message,
        Some("Directory operation preview is stale; run :write again before :write!".to_string())
    );
    assert!(!apply_outcome.wrote);
    assert!(alpha_path.exists(), "stale preview must not delete files");
    assert!(beta_path.exists());

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[test]
fn directory_buffer_cancel_command_clears_preview_without_mutation() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("directory-write-cancel-preview");
    let alpha_path = root_path.join("alpha.md");
    let beta_path = root_path.join("beta.md");
    std::fs::create_dir_all(&root_path).expect("test directory");
    std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
    std::fs::write(&beta_path, "beta\n").expect("beta file");
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    execute_runtime_host_command(
        &format!("edit {}", root_path.display()),
        &mut outcome,
        &mut session_state,
    )
    .expect("open directory listing");
    outcome
        .core_bridge
        .dispatch_key("dd")
        .expect("delete current listing line");

    let preview_effect = execute_runtime_host_command("write", &mut outcome, &mut session_state)
        .expect("plain write should prepare preview");
    let preview_id = session_state
        .pending_directory_operation_preview()
        .expect("preview should be pending")
        .id
        .clone();
    assert!(
        preview_effect
            .transient_message
            .as_deref()
            .is_some_and(|message| message.contains("y/Enter=OK n/Esc=Cancel")),
        "preview message should show the cancel command: {:?}",
        preview_effect.transient_message
    );

    let cancel_effect =
        execute_runtime_host_command("dired-cancel", &mut outcome, &mut session_state)
            .expect("cancel command should be host-handled");

    assert_eq!(
        cancel_effect.transient_message,
        Some(format!(
            "Directory operation preview cancelled: 1 operation(s), preview_id={preview_id}"
        ))
    );
    assert!(
        session_state
            .pending_directory_operation_preview()
            .is_none()
    );
    assert!(alpha_path.exists(), "cancel must not delete alpha");
    assert!(beta_path.exists());

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[test]
fn directory_buffer_transaction_applies_multiple_renames_without_collision() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("directory-transaction-rename-collision");
    let alpha_path = root_path.join("alpha.md");
    let beta_path = root_path.join("beta.md");
    let gamma_path = root_path.join("gamma.md");
    std::fs::create_dir_all(&root_path).expect("test directory");
    std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
    std::fs::write(&beta_path, "beta\n").expect("beta file");
    let mut session_state = crate::app::session::EditorSessionState::new(Some(root_path.clone()));
    let plan = crate::app::session::DirectoryBufferOperationPlan {
        root_path: root_path.clone(),
        operations: vec![
            DirectoryBufferPlannedOperation::Rename {
                from: alpha_path.clone(),
                to: beta_path.clone(),
                from_name: "alpha.md".to_string(),
                to_name: "beta.md".to_string(),
                kind: crate::app::session::DirectoryBufferEntryKind::File,
            },
            DirectoryBufferPlannedOperation::Rename {
                from: beta_path.clone(),
                to: gamma_path.clone(),
                from_name: "beta.md".to_string(),
                to_name: "gamma.md".to_string(),
                kind: crate::app::session::DirectoryBufferEntryKind::File,
            },
        ],
    };

    let applied_count = apply_directory_buffer_operation_plan(&mut session_state, &plan)
        .expect("transaction should avoid rename target collisions");

    assert_eq!(applied_count, 2);
    assert!(!alpha_path.exists());
    assert_eq!(
        std::fs::read_to_string(&beta_path).expect("beta target"),
        "alpha\n"
    );
    assert_eq!(
        std::fs::read_to_string(&gamma_path).expect("gamma target"),
        "beta\n"
    );
    let entries = session_state
        .directory_buffer()
        .expect("directory metadata should refresh after transaction")
        .entries
        .iter()
        .map(|entry| entry.display_text.clone())
        .collect::<Vec<_>>();
    assert_eq!(entries, vec!["beta.md".to_string(), "gamma.md".to_string()]);

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[test]
fn directory_buffer_transaction_reports_partial_failure_and_refreshes_metadata() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("directory-transaction-partial-failure");
    let created_path = root_path.join("created.md");
    let non_empty_dir = root_path.join("non-empty");
    std::fs::create_dir_all(&non_empty_dir).expect("test directory");
    std::fs::write(non_empty_dir.join("child.md"), "child\n").expect("child file");
    let mut session_state = crate::app::session::EditorSessionState::new(Some(root_path.clone()));
    let plan = crate::app::session::DirectoryBufferOperationPlan {
        root_path: root_path.clone(),
        operations: vec![
            DirectoryBufferPlannedOperation::CreateFile {
                path: created_path.clone(),
                name: "created.md".to_string(),
            },
            DirectoryBufferPlannedOperation::Delete {
                path: non_empty_dir.clone(),
                name: "non-empty".to_string(),
                kind: crate::app::session::DirectoryBufferEntryKind::Directory,
            },
        ],
    };

    let error = apply_directory_buffer_operation_plan(&mut session_state, &plan)
        .expect_err("non-empty directory delete should report a partial failure");

    match error {
        RuntimeFilerError::OperationFailed { message, .. } => {
            assert!(
                message.contains("successful=1")
                    && message.contains("failed=1")
                    && message.contains("manual_recovery_required=1"),
                "partial failure report should include structured counts: {message}"
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
    assert!(
        created_path.is_file(),
        "successful operation should be left in place and reported"
    );
    assert!(
        non_empty_dir.is_dir(),
        "failed delete should leave the original directory"
    );
    let entries = session_state
        .directory_buffer()
        .expect("directory metadata should refresh even after partial failure")
        .entries
        .iter()
        .map(|entry| entry.display_text.clone())
        .collect::<Vec<_>>();
    assert!(
        entries.contains(&"created.md".to_string()) && entries.contains(&"non-empty/".to_string()),
        "refreshed metadata should reflect the real filesystem: {entries:?}"
    );

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[test]
fn directory_buffer_confirm_failure_reports_recovery_hint_and_keeps_real_listing() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("directory-write-recovery-hint");
    let non_empty_dir = root_path.join("non-empty");
    std::fs::create_dir_all(&non_empty_dir).expect("test directory");
    std::fs::write(non_empty_dir.join("child.md"), "child\n").expect("child file");
    let mut session_state = crate::app::session::EditorSessionState::new(Some(root_path.clone()));

    let preview_outcome = save_snapshot_result("", &mut session_state);
    assert!(!preview_outcome.wrote);

    let apply_outcome =
        save_snapshot_result_with_confirmation("", &mut session_state, None, true, None);

    let message = apply_outcome
        .transient_message
        .expect("failed directory apply should report a message");
    assert!(
        message.contains("Directory operation apply failed")
            && message.contains("Recovery:")
            && message.contains("inspect the listing before retrying"),
        "failed apply should include a recovery hint: {message}"
    );
    assert!(!apply_outcome.wrote);
    assert!(
        non_empty_dir.is_dir(),
        "failed delete must not remove directory"
    );
    let entries = session_state
        .directory_buffer()
        .expect("directory metadata should remain active")
        .entries
        .iter()
        .map(|entry| entry.display_text.clone())
        .collect::<Vec<_>>();
    assert_eq!(entries, vec!["non-empty/".to_string()]);

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[test]
fn directory_buffer_transaction_rejects_existing_create_target_before_mutation() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("directory-transaction-create-conflict");
    let existing_path = root_path.join("existing.md");
    let later_path = root_path.join("later.md");
    std::fs::create_dir_all(&root_path).expect("test directory");
    std::fs::write(&existing_path, "existing\n").expect("existing file");
    let mut session_state = crate::app::session::EditorSessionState::new(Some(root_path.clone()));
    let plan = crate::app::session::DirectoryBufferOperationPlan {
        root_path: root_path.clone(),
        operations: vec![
            DirectoryBufferPlannedOperation::CreateFile {
                path: existing_path.clone(),
                name: "existing.md".to_string(),
            },
            DirectoryBufferPlannedOperation::CreateFile {
                path: later_path.clone(),
                name: "later.md".to_string(),
            },
        ],
    };

    let error = apply_directory_buffer_operation_plan(&mut session_state, &plan)
        .expect_err("existing create target should be rejected before execution");

    assert!(matches!(
        error,
        RuntimeFilerError::OperationFailed {
            kind: RuntimeFilerErrorKind::AlreadyExists,
            ..
        }
    ));
    assert_eq!(
        std::fs::read_to_string(&existing_path).expect("existing file"),
        "existing\n"
    );
    assert!(
        !later_path.exists(),
        "conflict check must stop before later operations mutate the filesystem"
    );

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[test]
fn directory_buffer_transaction_rejects_missing_rename_source_before_mutation() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("directory-transaction-missing-source");
    let missing_path = root_path.join("missing.md");
    let renamed_path = root_path.join("renamed.md");
    let later_path = root_path.join("later.md");
    std::fs::create_dir_all(&root_path).expect("test directory");
    let mut session_state = crate::app::session::EditorSessionState::new(Some(root_path.clone()));
    let plan = crate::app::session::DirectoryBufferOperationPlan {
        root_path: root_path.clone(),
        operations: vec![
            DirectoryBufferPlannedOperation::Rename {
                from: missing_path.clone(),
                to: renamed_path.clone(),
                from_name: "missing.md".to_string(),
                to_name: "renamed.md".to_string(),
                kind: crate::app::session::DirectoryBufferEntryKind::File,
            },
            DirectoryBufferPlannedOperation::CreateFile {
                path: later_path.clone(),
                name: "later.md".to_string(),
            },
        ],
    };

    let error = apply_directory_buffer_operation_plan(&mut session_state, &plan)
        .expect_err("missing rename source should be rejected before execution");

    assert!(matches!(
        error,
        RuntimeFilerError::OperationFailed {
            kind: RuntimeFilerErrorKind::NotFound,
            ..
        }
    ));
    assert!(!renamed_path.exists());
    assert!(
        !later_path.exists(),
        "conflict check must stop before later operations mutate the filesystem"
    );

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[test]
fn directory_buffer_transaction_rejects_special_file_entries_before_mutation() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("directory-transaction-special-file");
    let special_path = root_path.join("special");
    let later_path = root_path.join("later.md");
    std::fs::create_dir_all(&root_path).expect("test directory");
    std::fs::write(&special_path, "special\n").expect("special placeholder");
    let mut session_state = crate::app::session::EditorSessionState::new(Some(root_path.clone()));
    let plan = crate::app::session::DirectoryBufferOperationPlan {
        root_path: root_path.clone(),
        operations: vec![
            DirectoryBufferPlannedOperation::Delete {
                path: special_path.clone(),
                name: "special".to_string(),
                kind: crate::app::session::DirectoryBufferEntryKind::Other,
            },
            DirectoryBufferPlannedOperation::CreateFile {
                path: later_path.clone(),
                name: "later.md".to_string(),
            },
        ],
    };

    let error = apply_directory_buffer_operation_plan(&mut session_state, &plan)
        .expect_err("special entries should be rejected before execution");

    assert!(matches!(
        error,
        RuntimeFilerError::OperationFailed {
            kind: RuntimeFilerErrorKind::Unsupported,
            ..
        }
    ));
    assert!(special_path.is_file());
    assert!(
        !later_path.exists(),
        "unsupported special entry must stop before later operations mutate the filesystem"
    );

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[cfg(unix)]
#[test]
fn directory_buffer_transaction_rejects_unwritable_parent_before_mutation() {
    use std::os::unix::fs::PermissionsExt;

    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("directory-transaction-permission-conflict");
    let create_path = root_path.join("created.md");
    std::fs::create_dir_all(&root_path).expect("test directory");
    let original_permissions = std::fs::metadata(&root_path)
        .expect("root metadata")
        .permissions();
    std::fs::set_permissions(&root_path, std::fs::Permissions::from_mode(0o555))
        .expect("make root read-only");
    let mut session_state = crate::app::session::EditorSessionState::new(Some(root_path.clone()));
    let plan = crate::app::session::DirectoryBufferOperationPlan {
        root_path: root_path.clone(),
        operations: vec![DirectoryBufferPlannedOperation::CreateFile {
            path: create_path.clone(),
            name: "created.md".to_string(),
        }],
    };

    let error = apply_directory_buffer_operation_plan(&mut session_state, &plan)
        .expect_err("unwritable parent should be rejected before execution");

    assert!(matches!(
        error,
        RuntimeFilerError::OperationFailed {
            kind: RuntimeFilerErrorKind::PermissionDenied,
            ..
        }
    ));
    assert!(!create_path.exists());

    std::fs::set_permissions(&root_path, original_permissions).expect("restore permissions");
    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}
