use std::path::PathBuf;

use super::*;

fn unique_test_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "saya-editor-session-{name}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos()
    ))
}

// ---- タスク 5.1: 保存要求の生成テスト ----

#[test]
fn build_save_request_returns_request_with_path_and_contents() {
    let state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
    let buffer_contents = "hello world\n";

    let request = state
        .build_save_request(buffer_contents)
        .expect("保存要求の生成に成功すること");

    assert_eq!(request.path, PathBuf::from("/tmp/test.txt"));
    assert_eq!(request.contents, "hello world\n");
}

#[test]
fn build_save_request_fails_when_no_target_path() {
    let state = EditorSessionState::new(None);

    let result = state.build_save_request("data");

    assert_eq!(
        result,
        Err(SaveRequestError::NoTargetPath),
        "target_path が未設定の場合は NoTargetPath エラーになること"
    );
}

#[test]
fn build_save_request_extracts_current_buffer_contents() {
    let state = EditorSessionState::new(Some(PathBuf::from("/tmp/file.txt")));
    let contents = "line1\nline2\nline3\n";

    let request = state.build_save_request(contents).unwrap();

    assert_eq!(
        request.contents, contents,
        "buffer の現在内容がそのまま保存要求に含まれること"
    );
}

// ---- タスク 5.2: 保存成功時の clean 状態テスト ----

#[test]
fn record_save_success_clears_dirty_state() {
    let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
    state.update_dirty(true);
    assert!(state.is_dirty(), "保存前は dirty であること");

    state.record_save_success();

    assert!(!state.is_dirty(), "保存成功後は dirty が解除されること");
}

#[test]
fn saved_core_revision_keeps_stale_dirty_projection_clean() {
    let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
    state.update_dirty_at_revision(true, Some(7));
    assert!(state.is_dirty(), "保存前は dirty であること");

    state.record_save_success_at_revision(Some(7));
    state.update_dirty_at_revision(true, Some(7));

    assert!(
        !state.is_dirty(),
        "保存済み revision の core dirty は stale として無視すること"
    );
}

#[test]
fn newer_core_revision_can_dirty_after_save_success() {
    let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
    state.update_dirty_at_revision(true, Some(7));
    state.record_save_success_at_revision(Some(7));

    state.update_dirty_at_revision(true, Some(8));

    assert!(
        state.is_dirty(),
        "保存後に進んだ revision の dirty は新しい編集として反映すること"
    );
}

#[test]
fn record_save_success_clears_last_save_error() {
    let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
    state.record_save_failure("previous error".to_string());
    assert!(state.last_save_error().is_some());

    state.record_save_success();

    assert_eq!(
        state.last_save_error(),
        None,
        "保存成功後はエラーメッセージがクリアされること"
    );
}

#[test]
fn quit_evaluates_to_allow_after_save_success() {
    let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
    state.update_dirty(true);
    state.record_save_success();

    let decision = state.evaluate_quit(false);

    assert_eq!(
        decision,
        QuitDecision::Allow,
        "保存成功後の通常終了は Allow であること"
    );
}

// ---- タスク 5.3: 保存失敗時の編集継続テスト ----

#[test]
fn record_save_failure_preserves_dirty_state() {
    let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
    state.update_dirty(true);

    state.record_save_failure("disk full".to_string());

    assert!(state.is_dirty(), "保存失敗後も dirty 状態が維持されること");
}

#[test]
fn record_save_failure_stores_error_message_for_display() {
    let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));

    state.record_save_failure("permission denied".to_string());

    assert_eq!(
        state.last_save_error(),
        Some("permission denied"),
        "保存失敗メッセージが表示用に保持されること"
    );
}

#[test]
fn save_failure_does_not_prevent_further_editing() {
    let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
    state.update_dirty(true);
    state.record_save_failure("write error".to_string());

    // 保存失敗後も dirty 更新が可能であること
    state.update_dirty(true);
    assert!(
        state.is_dirty(),
        "保存失敗後も編集状態の更新が可能であること"
    );

    // 再度保存要求を生成できること
    let request = state.build_save_request("updated content");
    assert!(request.is_ok(), "保存失敗後も保存要求を再生成できること");
}

// ---- タスク 5.4: 通常終了と強制終了の分岐テスト ----

#[test]
fn evaluate_quit_allows_when_clean() {
    let state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));

    let decision = state.evaluate_quit(false);

    assert_eq!(
        decision,
        QuitDecision::Allow,
        "clean 状態での通常終了は Allow であること"
    );
}

#[test]
fn evaluate_quit_warns_when_dirty_and_not_forced() {
    let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
    state.update_dirty(true);

    let decision = state.evaluate_quit(false);

    assert_eq!(
        decision,
        QuitDecision::WarnUnsaved,
        "dirty 状態での通常終了は WarnUnsaved であること"
    );
}

#[test]
fn evaluate_quit_force_quits_even_when_dirty() {
    let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
    state.update_dirty(true);

    let decision = state.evaluate_quit(true);

    assert_eq!(
        decision,
        QuitDecision::ForceQuit,
        "dirty 状態でも force=true なら ForceQuit であること"
    );
}

#[test]
fn evaluate_quit_force_quits_when_clean() {
    let state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));

    let decision = state.evaluate_quit(true);

    assert_eq!(
        decision,
        QuitDecision::ForceQuit,
        "clean 状態でも force=true なら ForceQuit であること"
    );
}

// ---- タスク 5.5: 未保存状態での終了警告テスト ----

#[test]
fn dirty_state_triggers_warn_unsaved_on_normal_quit() {
    let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
    state.update_dirty(true);

    let decision = state.evaluate_quit(false);

    assert_eq!(
        decision,
        QuitDecision::WarnUnsaved,
        "dirty 状態の通常終了は警告に切り替わること"
    );
}

#[test]
fn warn_unsaved_does_not_terminate_session() {
    let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
    state.update_dirty(true);

    let decision = state.evaluate_quit(false);
    assert_eq!(decision, QuitDecision::WarnUnsaved);

    // 警告後もセッション状態は維持される
    assert!(
        state.is_dirty(),
        "警告後も dirty 状態が維持されること（即時終了しない）"
    );

    // 警告後も保存要求を生成できる
    let request = state.build_save_request("content");
    assert!(request.is_ok(), "警告後も保存操作が可能であること");
}

#[test]
fn force_quit_bypasses_unsaved_warning() {
    let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
    state.update_dirty(true);

    // 通常終了では警告になる
    assert_eq!(state.evaluate_quit(false), QuitDecision::WarnUnsaved);

    // 強制終了では即時終了
    assert_eq!(
        state.evaluate_quit(true),
        QuitDecision::ForceQuit,
        "強制終了は未保存警告をバイパスすること"
    );
}

#[test]
fn clean_state_after_save_allows_normal_quit() {
    let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
    state.update_dirty(true);

    // 保存前は警告
    assert_eq!(state.evaluate_quit(false), QuitDecision::WarnUnsaved);

    // 保存成功
    state.record_save_success();

    // 保存後は通常終了可能
    assert_eq!(
        state.evaluate_quit(false),
        QuitDecision::Allow,
        "保存成功後は通常終了が許可されること"
    );
}

#[test]
fn new_with_tab_size_preserves_requested_value() {
    let state = EditorSessionState::new_with_tab_size(None, 4);

    assert_eq!(state.tab_size(), 4);
}

#[test]
fn new_with_tab_size_clamps_zero_to_one() {
    let state = EditorSessionState::new_with_tab_size(None, 0);

    assert_eq!(state.tab_size(), 1);
}

#[test]
fn new_with_number_width_defaults_to_four() {
    let state = EditorSessionState::new(None);

    assert_eq!(state.number_width(), 4);
}

#[test]
fn set_line_numbers_enables_projection_flag() {
    let mut state = EditorSessionState::new(None);
    assert!(!state.line_numbers(), "既定値は false であること");

    state.set_line_numbers(true);

    assert!(state.line_numbers(), "行番号表示が有効になること");
}

#[test]
fn set_line_numbers_disables_projection_flag() {
    let mut state = EditorSessionState::new_with_tab_size_and_line_numbers(None, 8, true);
    assert!(state.line_numbers(), "初期状態は true であること");

    state.set_line_numbers(false);

    assert!(!state.line_numbers(), "行番号表示が無効になること");
}

#[test]
fn mermaid_preview_auto_option_can_be_disabled_and_manual_state_is_explicitly_cleared() {
    let mut state = EditorSessionState::new(None);
    assert!(state.mermaid_preview_auto());

    state
        .apply_presentation_option(
            SayaOptionName::MermaidPreview,
            SayaOptionValue::Boolean(false),
        )
        .expect("mermaidpreview option should apply");
    assert!(!state.mermaid_preview_auto());

    state.request_mermaid_preview();
    assert!(state.mermaid_preview_manual_active());
    state.clear_mermaid_preview_manual("test");
    assert!(!state.mermaid_preview_manual_active());
}

#[test]
fn mermaid_preview_view_state_tracks_focus_zoom_and_pan_separately_from_auto_option() {
    let mut state = EditorSessionState::new(None);

    state.request_mermaid_preview();
    assert!(state.mermaid_preview_focused());
    assert_eq!(state.mermaid_preview_zoom(), MermaidPreviewZoom::Fit);

    state.zoom_mermaid_preview_in();
    assert_eq!(
        state.mermaid_preview_zoom(),
        MermaidPreviewZoom::Percent(125)
    );
    state.pan_mermaid_preview(7, 11);
    assert_eq!(state.mermaid_preview_pan(), (7, 11));

    state.zoom_mermaid_preview_fit();
    assert_eq!(state.mermaid_preview_zoom(), MermaidPreviewZoom::Fit);
    assert_eq!(state.mermaid_preview_pan(), (0, 0));

    state.zoom_mermaid_preview_actual_size();
    assert_eq!(
        state.mermaid_preview_zoom(),
        MermaidPreviewZoom::Percent(100)
    );

    state.close_mermaid_preview("test");
    assert!(!state.mermaid_preview_manual_active());
    assert!(!state.mermaid_preview_focused());
    assert!(state.mermaid_preview_closed());
    state.reopen_mermaid_preview_if_closed("test");
    assert!(!state.mermaid_preview_closed());
}

#[test]
fn set_number_width_updates_projection_width() {
    let mut state = EditorSessionState::new(None);

    state.set_number_width(6);

    assert_eq!(state.number_width(), 6);
}

#[test]
fn set_number_width_clamps_zero_to_one() {
    let mut state = EditorSessionState::new(None);

    state.set_number_width(0);

    assert_eq!(state.number_width(), 1);
}

#[test]
fn message_area_height_defaults_to_five() {
    let state = EditorSessionState::new(None);

    assert_eq!(state.message_area_height(), 5);
    assert_eq!(state.message_scroll_offset(), 0);
}

#[test]
fn scroll_message_area_by_clamps_to_available_range() {
    let mut state = EditorSessionState::new(None);

    assert!(state.scroll_message_area_by(2, 3));
    assert_eq!(state.message_scroll_offset(), 2);
    assert!(state.scroll_message_area_by(2, 3));
    assert_eq!(state.message_scroll_offset(), 3);
    assert!(state.scroll_message_area_by(-1, 3));
    assert_eq!(state.message_scroll_offset(), 2);
    assert!(state.scroll_message_area_by(-9, 3));
    assert_eq!(state.message_scroll_offset(), 0);
}

#[test]
fn message_pager_enters_more_state_for_overflowing_message_and_uses_vim_keys() {
    let mut state = EditorSessionState::new(None);

    assert!(state.sync_message_pager("one\ntwo\nthree\nfour", 2));
    assert!(state.message_pager_active());
    assert_eq!(
        state.message_pager_prompt_kind(),
        Some(vim_core_rs::CorePagerPromptKind::More)
    );

    assert!(state.handle_message_pager_key(&crate::input::router::KeyInput::Enter));
    assert_eq!(state.message_scroll_offset(), 1);
    assert!(state.handle_message_pager_key(&crate::input::router::KeyInput::Char(' ')));
    assert_eq!(state.message_scroll_offset(), 2);
    assert_eq!(
        state.message_pager_prompt_kind(),
        Some(vim_core_rs::CorePagerPromptKind::HitReturn)
    );
    assert!(state.handle_message_pager_key(&crate::input::router::KeyInput::Char('j')));
    assert!(state.message_pager_active());
    assert_eq!(state.message_scroll_offset(), 2);
    assert!(state.handle_message_pager_key(&crate::input::router::KeyInput::Enter));
    assert!(!state.message_pager_active());
    assert!(state.message_pager_hides_message("one\ntwo\nthree\nfour"));
}

#[test]
fn message_pager_supports_backward_keys_without_leaving_message_mode() {
    let mut state = EditorSessionState::new(None);
    state
        .apply_presentation_option(SayaOptionName::MessageHeight, SayaOptionValue::Number(2))
        .expect("message height option should apply");

    assert!(state.sync_message_pager("one\ntwo\nthree\nfour\nfive\nsix", 2));
    assert!(state.handle_message_pager_key(&crate::input::router::KeyInput::Char('G')));
    assert_eq!(state.message_scroll_offset(), 4);
    assert!(state.handle_message_pager_key(&crate::input::router::KeyInput::Char('k')));
    assert_eq!(state.message_scroll_offset(), 3);
    assert!(state.handle_message_pager_key(&crate::input::router::KeyInput::Char('b')));
    assert_eq!(state.message_scroll_offset(), 1);
    assert!(state.handle_message_pager_key(&crate::input::router::KeyInput::Char('g')));
    assert_eq!(state.message_scroll_offset(), 0);
    assert!(state.message_pager_active());
}

#[test]
fn message_pager_can_be_reopened_after_hit_return_dismisses_it() {
    let mut state = EditorSessionState::new(None);
    assert!(state.sync_message_pager("one\ntwo\nthree\nfour", 2));
    assert!(state.handle_message_pager_key(&crate::input::router::KeyInput::Char('G')));
    assert_eq!(
        state.message_pager_prompt_kind(),
        Some(vim_core_rs::CorePagerPromptKind::HitReturn)
    );
    assert!(state.handle_message_pager_key(&crate::input::router::KeyInput::Enter));
    assert!(!state.message_pager_active());

    let message = state
        .reopen_message_pager()
        .expect("dismissed message pager should be reopenable");

    assert_eq!(message, "one\ntwo\nthree\nfour");
    assert_eq!(state.message_scroll_offset(), 0);
    assert_eq!(
        state.message_pager_prompt_kind(),
        Some(vim_core_rs::CorePagerPromptKind::More)
    );
}

#[test]
fn message_pager_escape_dismisses_and_marks_current_message_hidden() {
    let mut state = EditorSessionState::new(None);
    assert!(state.sync_message_pager("one\ntwo\nthree\nfour", 2));

    assert!(state.handle_message_pager_key(&crate::input::router::KeyInput::Escape));

    assert!(!state.message_pager_active());
    assert!(state.message_pager_hides_message("one\ntwo\nthree\nfour"));
}

#[test]
fn message_pager_enter_dismisses_multiline_message_that_fits_visible_height() {
    let mut state = EditorSessionState::new(None);
    assert!(state.sync_message_pager("one\ntwo\nthree\nfour\nfive", 5));
    assert_eq!(
        state.message_pager_prompt_kind(),
        Some(vim_core_rs::CorePagerPromptKind::HitReturn)
    );

    assert!(state.handle_message_pager_key(&crate::input::router::KeyInput::Enter));

    assert!(!state.message_pager_active());
    assert!(state.message_pager_hides_message("one\ntwo\nthree\nfour\nfive"));
}

#[test]
fn message_pager_ctrl_left_bracket_dismisses_like_escape() {
    let mut state = EditorSessionState::new(None);
    assert!(state.sync_message_pager("one\ntwo\nthree\nfour", 2));

    assert!(state.handle_message_pager_key(&crate::input::router::KeyInput::Ctrl('[')));

    assert!(!state.message_pager_active());
    assert!(state.message_pager_hides_message("one\ntwo\nthree\nfour"));
}

#[test]
fn build_save_request_fails_when_session_is_read_only() {
    let state = EditorSessionState::new_with_options(None, 8, false, 4, true);

    let result = state.build_save_request("content");

    assert_eq!(result, Err(SaveRequestError::ReadOnly));
    assert!(state.read_only());
}

#[test]
fn build_save_request_fails_when_target_path_is_directory() {
    let dir_path = std::env::temp_dir().join(format!(
        "saya-editor-session-directory-buffer-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir_path).expect("test directory");
    let state = EditorSessionState::new(Some(dir_path.clone()));

    let result = state.build_save_request("README.md\n");

    assert_eq!(result, Err(SaveRequestError::DirectoryBuffer));
    std::fs::remove_dir(dir_path).expect("cleanup directory");
}

#[test]
fn directory_buffer_metadata_tracks_display_root_entries_and_ids() {
    let root_path = std::env::temp_dir().join(format!(
        "saya-editor-session-directory-metadata-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos()
    ));
    let nested_path = root_path.join("src");
    let readme_path = root_path.join("README.md");
    std::fs::create_dir_all(&nested_path).expect("nested directory");
    std::fs::write(&readme_path, "hello\n").expect("test file");

    let state = EditorSessionState::new(Some(root_path.clone()));
    let directory_buffer = state
        .directory_buffer()
        .expect("directory target should initialize directory buffer metadata");

    assert_eq!(directory_buffer.root_path, root_path);
    assert_eq!(directory_buffer.display_text, "src/\nREADME.md\n");
    assert_eq!(directory_buffer.entries.len(), 2);
    assert_eq!(directory_buffer.entries[0].name, "src");
    assert_eq!(
        directory_buffer.entries[0].kind,
        DirectoryBufferEntryKind::Directory
    );
    assert_eq!(directory_buffer.entries[0].display_text, "src/");
    assert_ne!(
        directory_buffer.entries[0].id, directory_buffer.entries[1].id,
        "entry ids should distinguish entries in the same directory"
    );
    assert_eq!(directory_buffer.entries[1].name, "README.md");
    assert_eq!(directory_buffer.entries[1].path, readme_path);
    assert_eq!(
        directory_buffer.entries[1].kind,
        DirectoryBufferEntryKind::File
    );
    assert_eq!(directory_buffer.entries[1].display_text, "README.md");

    std::fs::remove_dir_all(directory_buffer.root_path.clone()).expect("cleanup directory");
}

#[cfg(unix)]
#[test]
fn directory_buffer_kind_sort_groups_directories_then_sorts_other_entries_by_name() {
    let root_path = std::env::temp_dir().join(format!(
        "saya-editor-session-directory-kind-sort-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos()
    ));
    let directory_path = root_path.join("middle-dir");
    let file_path = root_path.join("z-file.txt");
    let target_path = root_path.join("target.md");
    let link_path = root_path.join("a-link.md");
    std::fs::create_dir_all(&directory_path).expect("nested directory");
    std::fs::write(&file_path, "file\n").expect("file entry");
    std::fs::write(&target_path, "target\n").expect("symlink target");
    std::os::unix::fs::symlink(&target_path, &link_path).expect("symlink");

    let directory_buffer = read_directory_buffer_state_with_options(
        &root_path,
        DirectoryBufferListingOptions {
            show_hidden: true,
            sort_by: DirectoryBufferSortKey::Kind,
            filter: None,
        },
    )
    .expect("directory listing");

    assert_eq!(
        directory_buffer
            .entries
            .iter()
            .map(|entry| entry.display_text.as_str())
            .collect::<Vec<_>>(),
        vec!["middle-dir/", "a-link.md@", "target.md", "z-file.txt"]
    );

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[cfg(unix)]
#[test]
fn directory_buffer_display_marks_symlink_entries() {
    let root_path = std::env::temp_dir().join(format!(
        "saya-editor-session-directory-symlink-display-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos()
    ));
    let target_path = root_path.join("target.md");
    let link_path = root_path.join("linked.md");
    std::fs::create_dir_all(&root_path).expect("test directory");
    std::fs::write(&target_path, "target\n").expect("target file");
    std::os::unix::fs::symlink(&target_path, &link_path).expect("symlink");

    let state = EditorSessionState::new(Some(root_path.clone()));
    let directory_buffer = state
        .directory_buffer()
        .expect("directory target should initialize directory buffer metadata");

    let link = directory_buffer
        .entries
        .iter()
        .find(|entry| entry.name == "linked.md")
        .expect("symlink entry should exist");
    assert_eq!(link.kind, DirectoryBufferEntryKind::Symlink);
    assert_eq!(link.display_text, "linked.md@");
    assert!(
        directory_buffer.display_text.contains("linked.md@\n"),
        "rendered dired listing should identify symlinks: {:?}",
        directory_buffer.display_text
    );

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[test]
fn directory_buffer_dirty_update_is_projected_but_kept_out_of_regular_save_flow() {
    let dir_path = std::env::temp_dir().join(format!(
        "saya-editor-session-directory-dirty-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir_path).expect("test directory");
    let mut state = EditorSessionState::new(Some(dir_path.clone()));

    state.update_dirty(true);

    assert!(
        state.is_dirty(),
        "directory buffer edits should still project a modified state"
    );
    assert_eq!(
        state.build_save_request("mutated listing"),
        Err(SaveRequestError::DirectoryBuffer)
    );

    std::fs::remove_dir(dir_path).expect("cleanup directory");
}

#[test]
fn directory_buffer_preview_prompt_emphasizes_risky_operations_for_headless_ui() {
    let root_path = unique_test_dir("preview-prompt");
    let alpha_path = root_path.join("alpha.md");
    let beta_path = root_path.join("beta.md");
    std::fs::create_dir_all(&root_path).expect("test directory");
    std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
    std::fs::write(&beta_path, "beta\n").expect("beta file");
    let mut state = EditorSessionState::new(Some(root_path.clone()));

    let preview = state
        .prepare_directory_buffer_operation_preview("beta.md\n")
        .expect("deleted listing line should prepare a preview");
    let prompt = state
        .directory_buffer_operation_prompt()
        .expect("prepared preview should expose a prompt state");

    assert_eq!(prompt.preview_id, preview.id);
    assert!(prompt.status_line.contains("Apply 1 dired operation"));
    assert!(prompt.status_line.contains("1 high-risk"));
    assert!(prompt.status_line.contains("y/Enter=OK"));
    assert!(prompt.status_line.contains("n/Esc=Cancel"));
    assert!(
        prompt.detail_lines.iter().any(|line| {
            line.contains("HIGH RISK") && line.contains("delete") && line.contains("alpha.md")
        }),
        "delete preview should be clearly marked as high risk: {:?}",
        prompt.detail_lines
    );
    assert_eq!(prompt.confirm_command, "OK");
    assert_eq!(prompt.cancel_command, "Cancel");
    assert!(prompt.recovery_hint.contains("No filesystem changes"));
    assert!(
        alpha_path.exists(),
        "preview must not mutate the filesystem"
    );

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[test]
fn directory_buffer_preview_cancel_clears_state_without_filesystem_mutation() {
    let root_path = unique_test_dir("preview-cancel");
    let alpha_path = root_path.join("alpha.md");
    let beta_path = root_path.join("beta.md");
    std::fs::create_dir_all(&root_path).expect("test directory");
    std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
    std::fs::write(&beta_path, "beta\n").expect("beta file");
    let mut state = EditorSessionState::new(Some(root_path.clone()));

    state
        .prepare_directory_buffer_operation_preview("beta.md\n")
        .expect("deleted listing line should prepare a preview");
    let cancelled = state
        .cancel_directory_buffer_operation_preview()
        .expect("pending preview should be cancellable");

    assert_eq!(cancelled.operation_count, 1);
    assert!(state.pending_directory_operation_preview().is_none());
    assert!(state.directory_buffer_operation_prompt().is_none());
    assert!(alpha_path.exists(), "cancel must not delete files");
    assert!(beta_path.exists());

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[test]
fn directory_buffer_mark_state_survives_refresh_and_tracks_rename() {
    let root_path = std::env::temp_dir().join(format!(
        "saya-editor-session-directory-mark-refresh-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos()
    ));
    let alpha_path = root_path.join("alpha.md");
    let beta_path = root_path.join("beta.md");
    let renamed_path = root_path.join("renamed.md");
    std::fs::create_dir_all(&root_path).expect("test directory");
    std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
    std::fs::write(&beta_path, "beta\n").expect("beta file");
    let mut state = EditorSessionState::new(Some(root_path.clone()));

    let alpha = state
        .current_directory_entry(0)
        .expect("alpha should be first")
        .clone();
    state.mark_directory_entry(&alpha);
    state
        .refresh_directory_buffer_for_path(&root_path)
        .expect("refresh should keep directory metadata");

    assert_eq!(
        state.marked_directory_entries(),
        vec![alpha.clone()],
        "marked entry should remain selected after refresh"
    );

    std::fs::rename(&alpha_path, &renamed_path).expect("rename file");
    state.record_directory_entry_rename(&alpha_path, &renamed_path);
    state
        .refresh_directory_buffer_for_path(&root_path)
        .expect("refresh after rename");

    let marked = state.marked_directory_entries();
    assert_eq!(marked.len(), 1);
    assert_eq!(marked[0].name, "renamed.md");
    assert_eq!(marked[0].path, renamed_path);

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[test]
fn directory_buffer_mark_state_clears_when_moving_to_another_directory() {
    let root_path = std::env::temp_dir().join(format!(
        "saya-editor-session-directory-mark-move-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos()
    ));
    let first_path = root_path.join("first");
    let second_path = root_path.join("second");
    let first_file_path = first_path.join("alpha.md");
    let second_file_path = second_path.join("beta.md");
    std::fs::create_dir_all(&first_path).expect("first directory");
    std::fs::create_dir_all(&second_path).expect("second directory");
    std::fs::write(&first_file_path, "alpha\n").expect("first file");
    std::fs::write(&second_file_path, "beta\n").expect("second file");
    let mut state = EditorSessionState::new(Some(first_path.clone()));

    let entry = state
        .current_directory_entry(0)
        .expect("entry should exist")
        .clone();
    state.mark_directory_entry(&entry);
    assert_eq!(state.marked_directory_entries().len(), 1);

    state.replace_target_path(second_path);

    assert!(
        state.marked_directory_entries().is_empty(),
        "marks from the previous directory must not leak into the next directory"
    );

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[test]
fn directory_buffer_unmark_and_clear_all_update_mark_state() {
    let root_path = std::env::temp_dir().join(format!(
        "saya-editor-session-directory-mark-clear-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos()
    ));
    let alpha_path = root_path.join("alpha.md");
    let beta_path = root_path.join("beta.md");
    std::fs::create_dir_all(&root_path).expect("test directory");
    std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
    std::fs::write(&beta_path, "beta\n").expect("beta file");
    let mut state = EditorSessionState::new(Some(root_path.clone()));
    let alpha = state
        .current_directory_entry(0)
        .expect("alpha should exist")
        .clone();
    let beta = state
        .current_directory_entry(1)
        .expect("beta should exist")
        .clone();

    state.mark_directory_entry(&alpha);
    state.mark_directory_entry(&beta);
    assert_eq!(state.marked_directory_entries().len(), 2);

    state.unmark_directory_entry(&alpha);
    assert_eq!(state.marked_directory_entries(), vec![beta]);

    state.clear_directory_marks();
    assert!(state.marked_directory_entries().is_empty());

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[test]
fn writable_directory_buffer_deleted_line_builds_delete_plan_without_touching_filesystem() {
    let root_path = unique_test_dir("writable-delete-plan");
    let alpha_path = root_path.join("alpha.md");
    let beta_path = root_path.join("beta.md");
    std::fs::create_dir_all(&root_path).expect("test directory");
    std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
    std::fs::write(&beta_path, "beta\n").expect("beta file");
    let state = EditorSessionState::new(Some(root_path.clone()));

    let plan = state
        .build_directory_buffer_operation_plan("beta.md\n")
        .expect("deleted listing line should produce a plan");

    assert_eq!(
        plan.operations,
        vec![DirectoryBufferPlannedOperation::Delete {
            path: alpha_path.clone(),
            name: "alpha.md".to_string(),
            kind: DirectoryBufferEntryKind::File,
        }]
    );
    assert!(
        alpha_path.exists(),
        "phase 7 only plans deletion and must not mutate the filesystem"
    );
    assert!(beta_path.exists());

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[test]
fn writable_directory_buffer_empty_edited_text_deletes_every_entry_in_plan_only() {
    let root_path = unique_test_dir("writable-delete-all-plan");
    let alpha_path = root_path.join("alpha.md");
    std::fs::create_dir_all(&root_path).expect("test directory");
    std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
    let state = EditorSessionState::new(Some(root_path.clone()));

    let plan = state
        .build_directory_buffer_operation_plan("")
        .expect("empty edited listing should mean every entry was deleted");

    assert_eq!(
        plan.operations,
        vec![DirectoryBufferPlannedOperation::Delete {
            path: alpha_path.clone(),
            name: "alpha.md".to_string(),
            kind: DirectoryBufferEntryKind::File,
        }]
    );
    assert!(
        alpha_path.exists(),
        "empty edited listing must still only prepare a delete plan"
    );

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[test]
fn writable_directory_buffer_rename_builds_rename_plan_and_reorder_is_noop() {
    let root_path = unique_test_dir("writable-rename-plan");
    let alpha_path = root_path.join("alpha.md");
    let beta_path = root_path.join("beta.md");
    let renamed_path = root_path.join("renamed.md");
    std::fs::create_dir_all(&root_path).expect("test directory");
    std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
    std::fs::write(&beta_path, "beta\n").expect("beta file");
    let state = EditorSessionState::new(Some(root_path.clone()));

    let rename_plan = state
        .build_directory_buffer_operation_plan("renamed.md\nbeta.md\n")
        .expect("changed existing line should produce a rename plan");

    assert_eq!(
        rename_plan.operations,
        vec![DirectoryBufferPlannedOperation::Rename {
            from: alpha_path.clone(),
            to: renamed_path,
            from_name: "alpha.md".to_string(),
            to_name: "renamed.md".to_string(),
            kind: DirectoryBufferEntryKind::File,
        }]
    );

    let reorder_plan = state
        .build_directory_buffer_operation_plan("beta.md\nalpha.md\n")
        .expect("pure reorder should still be a valid plan");

    assert!(
        reorder_plan.operations.is_empty(),
        "line reorder alone must not produce filesystem operations"
    );
    assert!(alpha_path.exists());
    assert!(beta_path.exists());

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[test]
fn writable_directory_buffer_added_lines_build_file_and_directory_create_plan() {
    let root_path = unique_test_dir("writable-create-plan");
    let alpha_path = root_path.join("alpha.md");
    std::fs::create_dir_all(&root_path).expect("test directory");
    std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
    let state = EditorSessionState::new(Some(root_path.clone()));

    let plan = state
        .build_directory_buffer_operation_plan("alpha.md\nnotes.md\nsrc/\n")
        .expect("new listing lines should produce create operations");

    assert_eq!(
        plan.operations,
        vec![
            DirectoryBufferPlannedOperation::CreateFile {
                path: root_path.join("notes.md"),
                name: "notes.md".to_string(),
            },
            DirectoryBufferPlannedOperation::CreateDirectory {
                path: root_path.join("src"),
                name: "src".to_string(),
            },
        ]
    );
    assert!(
        !root_path.join("notes.md").exists() && !root_path.join("src").exists(),
        "phase 7 create operations must remain plans only"
    );

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[test]
fn writable_directory_buffer_invalid_diff_returns_validation_errors_without_filesystem_changes() {
    let root_path = unique_test_dir("writable-validation");
    let alpha_path = root_path.join("alpha.md");
    std::fs::create_dir_all(&root_path).expect("test directory");
    std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
    let state = EditorSessionState::new(Some(root_path.clone()));

    let errors = state
        .build_directory_buffer_operation_plan(
            "alpha.md\n\n../escape.md\nalpha.md\nbad/name\nlink@\n",
        )
        .expect_err("invalid edited listing should fail validation");

    assert!(errors.iter().any(|error| matches!(
        error,
        DirectoryBufferPlanValidationError::EmptyLine { line_number: 2 }
    )));
    assert!(errors.iter().any(|error| matches!(
        error,
        DirectoryBufferPlanValidationError::ParentDirectoryEscape { line_number: 3, .. }
    )));
    assert!(errors.iter().any(|error| matches!(
        error,
        DirectoryBufferPlanValidationError::DuplicateName { name, .. } if name == "alpha.md"
    )));
    assert!(errors.iter().any(|error| matches!(
        error,
        DirectoryBufferPlanValidationError::PathSeparator { line_number: 5, .. }
    )));
    assert!(errors.iter().any(|error| matches!(
        error,
        DirectoryBufferPlanValidationError::UnsupportedDecoration { line_number: 6, .. }
    )));
    assert!(
        alpha_path.exists(),
        "validation must happen before any filesystem mutation"
    );

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}
