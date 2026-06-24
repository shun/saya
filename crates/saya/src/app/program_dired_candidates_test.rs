use super::program_test_support::*;
use super::*;

fn dired_phase1_config_source() -> &'static str {
    r#"
        const trimTrailingSlash = (path) => path.length > 1 && path.endsWith("/") ? path.slice(0, -1) : path;
        const dirname = (path) => {
            const normalized = trimTrailingSlash(path || ".");
            const index = normalized.lastIndexOf("/");
            if (index < 0) return ".";
            return index === 0 ? "/" : normalized.slice(0, index);
        };
        saya.commands.register("dired.enter", async () => {
            const entry = await saya.filer.currentEntry();
            if (entry) {
                await saya.commands.execute(`edit ${entry.path}`);
            }
        });
        saya.commands.register("dired.up", async () => {
            const trimTrailingSlash = (path) => path.length > 1 && path.endsWith("/") ? path.slice(0, -1) : path;
            const dirname = (path) => {
                const normalized = trimTrailingSlash(path || ".");
                const index = normalized.lastIndexOf("/");
                if (index < 0) return ".";
                return index === 0 ? "/" : normalized.slice(0, index);
            };
            const buffer = await saya.buffer.current();
            await saya.commands.execute(`edit ${dirname(buffer.path || ".")}`);
        });
        saya.commands.register("dired.refresh", async () => {
            const buffer = await saya.buffer.current();
            await saya.commands.execute(`edit ${buffer.path || "."}`);
        });
        saya.keymap.set("normal", "-", saya.commands.execute("dired.up"));
        saya.keymap.set("normal", "<Enter>", saya.commands.execute("dired.enter"));
        saya.keymap.set("normal", "gr", saya.commands.execute("dired.refresh"));
    "#
}

fn dired_phase3_config_source() -> &'static str {
    r#"
        saya.commands.register("dired.createFile", async () => {
            const buffer = await saya.buffer.current();
            await saya.filer.createFile(`${buffer.path}/created.txt`);
        });
        saya.commands.register("dired.createDirectory", async () => {
            const buffer = await saya.buffer.current();
            await saya.filer.createDirectory(`${buffer.path}/created-dir`);
        });
        saya.commands.register("dired.rename", async () => {
            const entry = await saya.filer.currentEntry();
            await saya.filer.rename(entry.path, `${entry.rootPath}/renamed.txt`);
        });
        saya.commands.register("dired.deleteConfirmed", async () => {
            const entry = await saya.filer.currentEntry();
            await saya.filer.delete(entry.path, { confirm: true });
        });
        saya.commands.register("dired.deleteWithoutConfirm", async () => {
            const entry = await saya.filer.currentEntry();
            await saya.filer.delete(entry.path);
        });
        saya.commands.register("dired.createFileCollision", async () => {
            const buffer = await saya.buffer.current();
            await saya.filer.createFile(`${buffer.path}/existing.txt`);
        });
        saya.commands.register("dired.renameMissing", async () => {
            const buffer = await saya.buffer.current();
            await saya.filer.rename(`${buffer.path}/missing.txt`, `${buffer.path}/never.txt`);
        });
    "#
}

fn dired_phase4_config_source() -> &'static str {
    r#"
        saya.commands.register("dired.mark", async () => {
            const entry = await saya.filer.currentEntry();
            if (entry) {
                await saya.filer.mark(entry.path);
            }
        });
        saya.commands.register("dired.unmark", async () => {
            const entry = await saya.filer.currentEntry();
            if (entry) {
                await saya.filer.unmark(entry.path);
            }
        });
        saya.commands.register("dired.clearMarks", async () => {
            await saya.filer.clearMarks();
        });
        saya.commands.register("dired.bulkDeletePreview", async () => {
            await saya.filer.bulkDeletePreview();
        });
        saya.commands.register("dired.bulkDeleteWithoutPreview", async () => {
            await saya.filer.bulkDelete({ confirm: true, previewId: "stale" });
        });
    "#
}

fn dired_phase12_config_source() -> &'static str {
    r#"
        saya.commands.register("dired.filterRust", async () => {
            const buffer = await saya.buffer.current();
            await saya.filer.list(buffer.path || ".", {
                showHidden: false,
                sortBy: "name",
                filter: "rs",
            });
        });
        saya.commands.register("dired.createFilteredRust", async () => {
            const buffer = await saya.buffer.current();
            await saya.filer.createFile(`${buffer.path}/beta.rs`);
        });
    "#
}

fn prepare_dired_runtime_fixture(
    config_name: &str,
    config_source: &str,
) -> (
    PathBuf,
    crate::app::bootstrap::BootstrapOutcome,
    crate::app::session::EditorSessionState,
    RuntimeSessionOwner,
) {
    let config_path = unique_path(config_name).with_extension("ts");
    std::fs::write(&config_path, config_source).expect("config file");
    let outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::File(config_path.clone()),
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let session_state = outcome.editor_session_state();
    let runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
        .expect("runtime session should initialize");
    (config_path, outcome, session_state, runtime_session)
}

fn open_dired_listing_for_test(
    root_path: &std::path::Path,
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    session_state: &mut crate::app::session::EditorSessionState,
) {
    execute_runtime_host_command(
        &format!("edit {}", root_path.display()),
        outcome,
        session_state,
    )
    .expect("open directory listing");
}

fn directory_buffer_display_texts_for_test(
    session_state: &crate::app::session::EditorSessionState,
) -> Vec<String> {
    session_state
        .directory_buffer()
        .expect("directory metadata should remain active")
        .entries
        .iter()
        .map(|entry| entry.display_text.clone())
        .collect()
}

fn assert_directory_listing_state(
    outcome: &crate::app::bootstrap::BootstrapOutcome,
    session_state: &crate::app::session::EditorSessionState,
    root_path: &std::path::Path,
    expected_text: &str,
) {
    assert_eq!(outcome.target_path.as_deref(), Some(root_path));
    assert_eq!(
        session_state.target_path().map(PathBuf::as_path),
        Some(root_path)
    );
    assert_eq!(outcome.core_bridge.snapshot().text, expected_text);
    let expected_entries = expected_text
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    assert_eq!(
        directory_buffer_display_texts_for_test(session_state),
        expected_entries
    );
}

async fn execute_runtime_command_for_test(
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    session_state: &mut crate::app::session::EditorSessionState,
    runtime_session: &mut RuntimeSessionOwner,
    command_name: &str,
) {
    let mut transient_msg = None;
    let mut need_redraw = false;
    let mut runtime_presentation_intents = Vec::new();
    let mut floating_window_manager = FloatingWindowManager::default();
    let mut completion_float_manager = CompletionFloatManager::default();
    let mut lsp_diagnostic_store = LspDiagnosticStore::default();
    let mut terminal_float_manager = TerminalFloatManager::default();
    let mut panel_manager = PanelManager::default();
    let shutdown = execute_startup_keymap_registered_command(
        Some(runtime_session),
        command_name,
        outcome,
        session_state,
        &mut floating_window_manager,
        &mut completion_float_manager,
        &mut lsp_diagnostic_store,
        &mut terminal_float_manager,
        &mut panel_manager,
        None,
        &mut transient_msg,
        &mut need_redraw,
        &mut runtime_presentation_intents,
        None,
    )
    .await;

    assert_eq!(shutdown, None);
    assert_eq!(transient_msg, None);
}

async fn execute_runtime_command_outcome_for_test(
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    session_state: &mut crate::app::session::EditorSessionState,
    runtime_session: &mut RuntimeSessionOwner,
    command_name: &str,
) -> (Option<String>, bool) {
    let mut transient_msg = None;
    let mut need_redraw = false;
    let mut runtime_presentation_intents = Vec::new();
    let mut floating_window_manager = FloatingWindowManager::default();
    let mut completion_float_manager = CompletionFloatManager::default();
    let mut lsp_diagnostic_store = LspDiagnosticStore::default();
    let mut terminal_float_manager = TerminalFloatManager::default();
    let mut panel_manager = PanelManager::default();
    let shutdown = execute_startup_keymap_registered_command(
        Some(runtime_session),
        command_name,
        outcome,
        session_state,
        &mut floating_window_manager,
        &mut completion_float_manager,
        &mut lsp_diagnostic_store,
        &mut terminal_float_manager,
        &mut panel_manager,
        None,
        &mut transient_msg,
        &mut need_redraw,
        &mut runtime_presentation_intents,
        None,
    )
    .await;

    assert_eq!(shutdown, None);
    assert!(runtime_presentation_intents.is_empty());
    (transient_msg, need_redraw)
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_buffer_changed_event_dispatches_after_text_revision_changes() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("runtime-buffer-changed-target");
    std::fs::write(&target_path, "initial\n").expect("target file");
    let seed =
        crate::runtime::callback_registry_seed::CallbackRegistrySeed::from_startup_entries(vec![
            crate::runtime::config::StartupRegistryEntry::Event {
                name: "bufferChanged".to_string(),
                callback_source: r#"
                        async () => {
                            await saya.commands.execute("write");
                        }
                    "#
                .to_string(),
            },
        ]);
    let mut runtime_session =
        RuntimeSessionOwner::spawn(seed).expect("runtime owner should initialize");
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::File(target_path.clone()),
        config_source: crate::app::cli::ConfigSource::Default,
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    let mut transient_msg = None;
    let mut need_redraw = false;
    let mut runtime_presentation_intents = Vec::new();
    let mut floating_window_manager = FloatingWindowManager::default();
    let mut completion_float_manager = CompletionFloatManager::default();
    let mut lsp_diagnostic_store = LspDiagnosticStore::default();
    let mut terminal_float_manager = TerminalFloatManager::default();
    let mut panel_manager = PanelManager::default();

    let before = outcome.core_bridge.light_snapshot();
    outcome.core_bridge.dispatch_key("i").expect("enter insert");
    outcome.core_bridge.dispatch_key("x").expect("insert text");
    let after = outcome.core_bridge.light_snapshot();
    assert_ne!(before.revision, after.revision);
    assert!(
        after.dirty,
        "bufferChanged precondition should leave the edited buffer dirty before dispatch"
    );
    assert_eq!(
        std::fs::read_to_string(&target_path).expect("target file before event"),
        "initial\n",
        "the event side effect must be what changes the file on disk"
    );

    let shutdown = dispatch_buffer_changed_with_runtime(
        Some(&mut runtime_session),
        &mut outcome,
        &mut session_state,
        &mut transient_msg,
        &mut need_redraw,
        &mut runtime_presentation_intents,
        &mut floating_window_manager,
        &mut completion_float_manager,
        &mut lsp_diagnostic_store,
        &mut terminal_float_manager,
        &mut panel_manager,
        None,
    )
    .await;

    assert_eq!(shutdown, None);
    assert_eq!(transient_msg, Some("Saved successfully".to_string()));
    assert!(need_redraw, "runtime event dispatch should request redraw");
    assert_eq!(
        std::fs::read_to_string(&target_path).expect("target file after event"),
        outcome.core_bridge.snapshot().text,
        "bufferChanged callback should save the edited buffer contents"
    );
    assert!(
        !session_state.is_dirty(),
        "successful event-triggered write should clear session dirty state"
    );

    std::fs::remove_file(target_path).expect("cleanup target file");
}

#[tokio::test(flavor = "current_thread")]
async fn dired_enter_opens_directory_entry_from_current_line() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("dired-enter-directory-root");
    let nested_path = root_path.join("src");
    let nested_file = nested_path.join("mod.rs");
    let readme_path = root_path.join("README.md");
    let config_path = unique_path("dired-enter-directory-init").with_extension("ts");
    std::fs::create_dir_all(&nested_path).expect("nested directory");
    std::fs::write(&nested_file, "mod\n").expect("nested file");
    std::fs::write(&readme_path, "hello\n").expect("readme file");
    std::fs::write(&config_path, dired_phase1_config_source()).expect("config file");
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::File(config_path.clone()),
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
        .expect("runtime session should initialize");

    execute_runtime_host_command(
        &format!("edit {}", root_path.display()),
        &mut outcome,
        &mut session_state,
    )
    .expect("open root listing");
    execute_runtime_command_for_test(
        &mut outcome,
        &mut session_state,
        &mut runtime_session,
        "dired.enter",
    )
    .await;

    assert_eq!(outcome.target_path, Some(nested_path.clone()));
    assert_eq!(outcome.core_bridge.snapshot().text, "mod.rs\n");

    std::fs::remove_file(config_path).expect("cleanup config");
    std::fs::remove_dir_all(root_path).expect("cleanup root directory");
}

async fn assert_dired_open_in_split_keeps_inactive_shared_buffer_unchanged(
    split_command: &str,
    fixture_name: &str,
) {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path(&format!("dired-open-{fixture_name}-root"));
    let nested_path = root_path.join("src");
    let readme_path = root_path.join("README.md");
    let target_path = root_path.join("notes.txt");
    let config_path = unique_path(&format!("dired-open-{fixture_name}-init")).with_extension("ts");
    std::fs::create_dir_all(&nested_path).expect("nested directory");
    std::fs::write(&readme_path, "hello\n").expect("readme file");
    std::fs::write(&target_path, "notes\n").expect("target file");
    std::fs::write(
        &config_path,
        r#"
            saya.commands.register("dired.open", async () => {
                const buffer = await saya.buffer.current();
                const currentPath = buffer.path || ".";
                const directory = currentPath.endsWith("/")
                    ? (currentPath.slice(0, -1) || "/")
                    : (currentPath.lastIndexOf("/") >= 0 ? currentPath.slice(0, currentPath.lastIndexOf("/")) || "/" : ".");
                await saya.commands.execute(`edit ${directory}`);
            });
        "#,
    )
    .expect("config file");

    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::File(target_path.clone()),
        config_source: crate::app::cli::ConfigSource::File(config_path.clone()),
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
        .expect("runtime session should initialize");

    outcome
        .core_bridge
        .apply_ex_command(split_command)
        .unwrap_or_else(|_| panic!("{split_command} should succeed"));
    let split_snapshot = outcome.core_bridge.snapshot();
    assert_eq!(split_snapshot.windows.len(), 2);
    let inactive_window = split_snapshot
        .windows
        .iter()
        .find(|window| !window.is_active)
        .expect("split should leave an inactive window")
        .clone();
    assert_eq!(
        split_snapshot
            .active_window()
            .expect("split should keep an active window")
            .buf_id,
        inactive_window.buf_id,
        "vsplit starts with both windows displaying the same buffer"
    );

    execute_runtime_command_for_test(
        &mut outcome,
        &mut session_state,
        &mut runtime_session,
        "dired.open",
    )
    .await;

    let after = outcome.core_bridge.snapshot();
    let active_window = after
        .active_window()
        .expect("dired.open should leave an active window");
    let inactive_after = after
        .window(inactive_window.id)
        .expect("inactive split window should stay open");
    assert_ne!(
        active_window.buf_id, inactive_after.buf_id,
        "dired.open must detach the active split before loading the directory"
    );
    let active_text = outcome.core_bridge.snapshot().text;
    assert!(
        active_text.contains("src/\n")
            && active_text.contains("README.md\n")
            && active_text.contains("notes.txt\n"),
        "active pane should show the directory listing: {active_text:?}"
    );
    let inactive_text = outcome
        .core_bridge
        .buffer_line_range(inactive_after.buf_id, 0, 16)
        .expect("inactive buffer text should remain readable")
        .lines
        .join("\n");
    assert_eq!(
        inactive_text, "notes",
        "inactive split pane should keep the original file buffer"
    );

    std::fs::remove_file(config_path).expect("cleanup config");
    std::fs::remove_dir_all(root_path).expect("cleanup root directory");
}

#[tokio::test(flavor = "current_thread")]
async fn dired_open_in_vertical_split_keeps_inactive_shared_buffer_unchanged() {
    assert_dired_open_in_split_keeps_inactive_shared_buffer_unchanged(":vsplit", "vsplit").await;
}

#[tokio::test(flavor = "current_thread")]
async fn dired_open_in_horizontal_split_keeps_inactive_shared_buffer_unchanged() {
    assert_dired_open_in_split_keeps_inactive_shared_buffer_unchanged(":split", "split").await;
}

#[tokio::test(flavor = "current_thread")]
async fn dired_open_in_split_keeps_inactive_markdown_highlight_metadata() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("dired-open-markdown-highlight-root");
    let target_path = root_path.join("AGENTS.md");
    let config_path = unique_path("dired-open-markdown-highlight-init").with_extension("ts");
    std::fs::create_dir_all(&root_path).expect("root directory");
    std::fs::write(
        &target_path,
        "# AGENTS.md\n\n## Project\n\n- keep markdown metadata visible\n",
    )
    .expect("markdown file");
    std::fs::write(
        &config_path,
        r#"
            saya.commands.register("dired.open", async () => {
                const buffer = await saya.buffer.current();
                const currentPath = buffer.path || ".";
                const directory = currentPath.endsWith("/")
                    ? (currentPath.slice(0, -1) || "/")
                    : (currentPath.lastIndexOf("/") >= 0 ? currentPath.slice(0, currentPath.lastIndexOf("/")) || "/" : ".");
                await saya.commands.execute(`edit ${directory}`);
            });
        "#,
    )
    .expect("config file");

    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::File(target_path.clone()),
        config_source: crate::app::cli::ConfigSource::File(config_path.clone()),
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
        .expect("runtime session should initialize");

    outcome
        .core_bridge
        .apply_ex_command(":vsplit")
        .expect("vsplit should succeed");
    let inactive_window = outcome
        .core_bridge
        .snapshot()
        .windows
        .iter()
        .find(|window| !window.is_active)
        .expect("split should leave an inactive window")
        .clone();
    let mut markdown_metadata_cache = MarkdownMetadataCache::default();
    let before_dired = outcome.core_bridge.snapshot();
    let before_maps = collect_workspace_markdown_document_maps(
        &mut markdown_metadata_cache,
        &session_state,
        &outcome.core_bridge,
        &before_dired,
    );
    assert!(
        before_maps.contains_key(&inactive_window.id),
        "initial markdown render should populate metadata for the split buffer"
    );

    execute_runtime_command_for_test(
        &mut outcome,
        &mut session_state,
        &mut runtime_session,
        "dired.open",
    )
    .await;

    let after = outcome.core_bridge.snapshot();
    let active_window = after
        .active_window()
        .expect("dired.open should leave an active window");
    assert_ne!(
        active_window.buf_id, inactive_window.buf_id,
        "dired.open should detach the active pane from the shared markdown buffer"
    );

    let maps = collect_workspace_markdown_document_maps(
        &mut markdown_metadata_cache,
        &session_state,
        &outcome.core_bridge,
        &after,
    );

    assert!(
        maps.contains_key(&inactive_window.id),
        "inactive markdown pane should keep markdown metadata after active pane opens dired"
    );
    assert!(
        !maps.contains_key(&active_window.id),
        "active dired pane should not receive markdown metadata"
    );

    std::fs::remove_file(config_path).expect("cleanup config");
    std::fs::remove_dir_all(root_path).expect("cleanup root directory");
}

#[tokio::test(flavor = "current_thread")]
async fn dired_enter_from_split_listing_keeps_inactive_file_buffer_unchanged() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("dired-enter-split-root");
    let nested_path = root_path.join("src");
    let readme_path = root_path.join("README.md");
    let target_path = root_path.join("notes.txt");
    let config_path = unique_path("dired-enter-split-init").with_extension("ts");
    std::fs::create_dir_all(&nested_path).expect("nested directory");
    std::fs::write(&readme_path, "hello\n").expect("readme file");
    std::fs::write(&target_path, "notes\n").expect("target file");
    std::fs::write(
        &config_path,
        r#"
            saya.commands.register("dired.open", async () => {
                const dirname = (path) => {
                    const normalized = path.length > 1 && path.endsWith("/") ? path.slice(0, -1) : path;
                    const index = normalized.lastIndexOf("/");
                    if (index < 0) return ".";
                    return index === 0 ? "/" : normalized.slice(0, index);
                };
                const buffer = await saya.buffer.current();
                await saya.commands.execute(`edit ${dirname(buffer.path || ".")}`);
            });
            saya.commands.register("dired.enter", async () => {
                const entry = await saya.filer.currentEntry();
                if (entry) {
                    await saya.commands.execute(`edit ${entry.path}`);
                }
            });
        "#,
    )
    .expect("config file");

    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::File(target_path.clone()),
        config_source: crate::app::cli::ConfigSource::File(config_path.clone()),
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
        .expect("runtime session should initialize");

    outcome
        .core_bridge
        .apply_ex_command(":vsplit")
        .expect("vsplit should succeed");
    let inactive_window = outcome
        .core_bridge
        .snapshot()
        .windows
        .iter()
        .find(|window| !window.is_active)
        .expect("split should leave an inactive window")
        .clone();

    execute_runtime_command_for_test(
        &mut outcome,
        &mut session_state,
        &mut runtime_session,
        "dired.open",
    )
    .await;
    let readme_row = outcome
        .core_bridge
        .snapshot()
        .text
        .lines()
        .position(|line| line == "README.md")
        .expect("README.md should appear in the dired listing");
    for _ in 0..readme_row {
        outcome
            .core_bridge
            .dispatch_key("j")
            .expect("move in dired");
    }
    execute_runtime_command_for_test(
        &mut outcome,
        &mut session_state,
        &mut runtime_session,
        "dired.enter",
    )
    .await;

    assert_eq!(
        outcome.core_bridge.snapshot().text,
        "hello\n",
        "active dired pane should open the selected file"
    );
    let inactive_after = outcome
        .core_bridge
        .snapshot()
        .window(inactive_window.id)
        .expect("inactive split window should stay open")
        .clone();
    let inactive_text = outcome
        .core_bridge
        .buffer_line_range(inactive_after.buf_id, 0, 16)
        .expect("inactive buffer text should remain readable")
        .lines
        .join("\n");
    assert_eq!(
        inactive_text, "notes",
        "inactive split pane should keep the original file buffer after dired.enter"
    );

    std::fs::remove_file(config_path).expect("cleanup config");
    std::fs::remove_dir_all(root_path).expect("cleanup root directory");
}

#[tokio::test(flavor = "current_thread")]
async fn dired_enter_opens_file_entry_from_current_line() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("dired-enter-file-root");
    let readme_path = root_path.join("README.md");
    let config_path = unique_path("dired-enter-file-init").with_extension("ts");
    std::fs::create_dir_all(&root_path).expect("root directory");
    std::fs::write(&readme_path, "hello\n").expect("readme file");
    std::fs::write(&config_path, dired_phase1_config_source()).expect("config file");
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::File(config_path.clone()),
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
        .expect("runtime session should initialize");

    execute_runtime_host_command(
        &format!("edit {}", root_path.display()),
        &mut outcome,
        &mut session_state,
    )
    .expect("open root listing");
    execute_runtime_command_for_test(
        &mut outcome,
        &mut session_state,
        &mut runtime_session,
        "dired.enter",
    )
    .await;

    assert_eq!(outcome.target_path, Some(readme_path.clone()));
    assert_eq!(outcome.core_bridge.snapshot().text, "hello\n");
    let mut markdown_metadata_cache = MarkdownMetadataCache::default();
    let snapshot = outcome.core_bridge.snapshot();
    let active_window = snapshot
        .active_window()
        .expect("dired.enter should leave an active markdown window");
    let active_buffer = snapshot
        .buffers
        .iter()
        .find(|buffer| buffer.id == active_window.buf_id)
        .expect("active buffer metadata should exist");
    assert_eq!(
        active_buffer.name,
        root_path.display().to_string(),
        "regression guard: dired-entered file keeps the stale directory buffer name"
    );
    assert!(
        active_buffer
            .document_id
            .as_deref()
            .is_some_and(|document_id| document_id.starts_with("file://")
                && document_id.ends_with("README.md")),
        "dired VFS load should expose README.md through document_id"
    );
    let maps = collect_workspace_markdown_document_maps(
        &mut markdown_metadata_cache,
        &session_state,
        &outcome.core_bridge,
        &snapshot,
    );
    assert!(
        maps.contains_key(&active_window.id),
        "markdown file opened from dired should collect markdown metadata for highlighting; active_window={:?}, buffers={:?}, target_path={:?}, session_target={:?}",
        active_window,
        snapshot.buffers,
        outcome.target_path,
        session_state.target_path()
    );

    std::fs::remove_file(config_path).expect("cleanup config");
    std::fs::remove_dir_all(root_path).expect("cleanup root directory");
}

#[cfg(feature = "tree-sitter-syntax")]
#[tokio::test(flavor = "current_thread")]
async fn dired_enter_collects_tree_sitter_highlight_for_supported_languages() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for (file_name, source, expected_language) in [
        ("main.rs", "fn main() { let value = 1; }\n", "rust"),
        (
            "main.ts",
            "export function main(value: number): number { return value + 1; }\n",
            "typescript",
        ),
        ("main.go", "package main\n\nfunc main() {}\n", "go"),
        (
            "App.tsx",
            "export const App = () => <main>{1}</main>;\n",
            "tsx",
        ),
    ] {
        let root_path = unique_path(&format!("dired-enter-syntax-{expected_language}-root"));
        let source_path = root_path.join(file_name);
        let config_path = unique_path(&format!("dired-enter-syntax-{expected_language}-init"))
            .with_extension("ts");
        std::fs::create_dir_all(&root_path).expect("root directory");
        std::fs::write(&source_path, source).expect("source file");
        std::fs::write(&config_path, dired_phase1_config_source()).expect("config file");
        let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
            input_source: crate::app::cli::InputSource::Empty,
            config_source: crate::app::cli::ConfigSource::File(config_path.clone()),
            ..crate::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        outcome.core_bridge.set_screen_size(24, 80);
        outcome
            .core_bridge
            .apply_ex_command("syntax on")
            .expect("syntax on should enable Tree-sitter highlight collection");
        let mut session_state = outcome.editor_session_state();
        let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
            .expect("runtime session should initialize");

        execute_runtime_host_command(
            &format!("edit {}", root_path.display()),
            &mut outcome,
            &mut session_state,
        )
        .expect("open root listing");
        execute_runtime_command_for_test(
            &mut outcome,
            &mut session_state,
            &mut runtime_session,
            "dired.enter",
        )
        .await;

        assert_eq!(outcome.target_path, Some(source_path.clone()));
        let snapshot = outcome.core_bridge.snapshot();
        let active_window = snapshot
            .active_window()
            .expect("dired.enter should leave an active source window");
        let active_buffer = snapshot
            .buffers
            .iter()
            .find(|buffer| buffer.id == active_window.buf_id)
            .expect("active buffer metadata should exist");
        assert_eq!(
            active_buffer.name,
            root_path.display().to_string(),
            "regression guard: dired-entered {file_name} keeps the stale directory buffer name"
        );
        assert!(
            active_buffer
                .document_id
                .as_deref()
                .is_some_and(|document_id| document_id.starts_with("file://")
                    && document_id.ends_with(file_name)),
            "dired VFS load should expose the opened file through document_id"
        );
        let mut viewport_store = WindowViewportStore::new();
        let line_ranges =
            collect_workspace_line_ranges(&outcome.core_bridge, &snapshot, &viewport_store);
        let mut languages = BTreeSet::new();
        for _ in 0..20 {
            let syntax_by_window = collect_workspace_tree_sitter_syntax(
                &mut outcome.core_bridge,
                &snapshot,
                &viewport_store,
                &line_ranges,
            );
            languages = syntax_by_window
                .get(&active_window.id)
                .into_iter()
                .map(|syntax| syntax.provenance.language_id.as_str())
                .map(str::to_string)
                .collect();
            if languages.contains(expected_language) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            languages.contains(expected_language),
            "dired-opened {file_name} should collect Tree-sitter syntax for {expected_language}; languages={languages:?}, active_buffer={active_buffer:?}"
        );
        let mut search_refresh_store = WindowSearchRefreshStore::default();
        let mut markdown_metadata_cache = MarkdownMetadataCache::default();
        let mut rendered_languages = BTreeSet::new();
        for _ in 0..20 {
            let workspace = build_workspace_render_output(
                &mut outcome,
                &mut session_state,
                &mut viewport_store,
                ViewportSyncMode::Core,
                &mut search_refresh_store,
                &mut markdown_metadata_cache,
                None,
                "",
                0,
                None,
                None,
                None,
                None,
                None,
                80,
                24,
                None,
                None,
                None,
                None,
            )
            .expect("dired-opened source workspace should render");
            rendered_languages = workspace
                .panes
                .iter()
                .flat_map(|pane| pane.syntax_chunks.iter())
                .filter_map(|chunk| chunk.language.as_deref())
                .map(str::to_string)
                .collect();
            if rendered_languages.contains(expected_language) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            rendered_languages.contains(expected_language),
            "dired-opened {file_name} should render Tree-sitter syntax for {expected_language}; rendered_languages={rendered_languages:?}"
        );

        std::fs::remove_file(config_path).expect("cleanup config");
        std::fs::remove_dir_all(root_path).expect("cleanup root directory");
    }
}

/// dired 経由で TypeScript ファイルを開いた場合でも、直接開きと同様に
/// Vim の行ベース syntax（`get_line_syntax`）が有効になり、実ハイライト
/// チャンク（syn_id != 0）が得られることを保証する回帰テスト。
///
/// Tree-sitter 言語推定（document_id 由来）だけを見る既存テストでは、
/// filetype 未設定による Vim syntax 無効化（ハイライト不発）を検出できない。
#[tokio::test(flavor = "current_thread")]
async fn dired_enter_typescript_file_enables_vim_line_syntax() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let source = "export function main(value: number): number { return value + 1; }\n";
    let root_path = unique_path("dired-enter-ts-vim-syntax-root");
    let source_path = root_path.join("main.ts");
    let config_path = unique_path("dired-enter-ts-vim-syntax-init").with_extension("ts");
    std::fs::create_dir_all(&root_path).expect("root directory");
    std::fs::write(&source_path, source).expect("source file");
    std::fs::write(&config_path, dired_phase1_config_source()).expect("config file");

    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::File(config_path.clone()),
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    outcome.core_bridge.set_screen_size(24, 80);
    outcome
        .core_bridge
        .apply_ex_command("syntax on")
        .expect("syntax on should enable Vim syntax highlighting");
    let mut session_state = outcome.editor_session_state();
    let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
        .expect("runtime session should initialize");

    execute_runtime_host_command(
        &format!("edit {}", root_path.display()),
        &mut outcome,
        &mut session_state,
    )
    .expect("open root listing");
    execute_runtime_command_for_test(
        &mut outcome,
        &mut session_state,
        &mut runtime_session,
        "dired.enter",
    )
    .await;

    assert_eq!(outcome.target_path, Some(source_path.clone()));
    let snapshot = outcome.core_bridge.snapshot();
    let active_window = snapshot
        .active_window()
        .expect("dired.enter should leave an active source window");
    let active_buffer = snapshot
        .buffers
        .iter()
        .find(|buffer| buffer.id == active_window.buf_id)
        .expect("active buffer metadata should exist");

    // dired 経由では buffer 名はディレクトリのまま残るが（既存仕様）、
    // filetype 検出が走るため Vim syntax は有効になる。
    // 1行目（`export function ...`）にキーワード等が含まれるため、Vim syntax が
    // 有効なら syn_id != 0 のハイライトチャンクが必ず得られる。
    let mut highlighted = false;
    for _ in 0..20 {
        let chunks = outcome
            .core_bridge
            .get_line_syntax(active_window.id, 1)
            .expect("get_line_syntax should succeed for visible line");
        if chunks.iter().any(|chunk| chunk.syn_id != 0) {
            highlighted = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    assert!(
        highlighted,
        "dired-opened TypeScript file should enable Vim line syntax (syn_id != 0 chunk on line 1); \
         active_buffer.name={:?}, document_id={:?}",
        active_buffer.name, active_buffer.document_id
    );

    std::fs::remove_file(config_path).expect("cleanup config");
    std::fs::remove_dir_all(root_path).expect("cleanup root directory");
}

#[cfg(feature = "tree-sitter-syntax")]
#[tokio::test(flavor = "current_thread")]
async fn dired_open_then_enter_renders_tree_sitter_highlight_for_another_file() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for (file_name, source, expected_language) in [
        ("main.rs", "fn main() { let value = 1; }\n", "rust"),
        (
            "main.ts",
            "export function main(value: number): number { return value + 1; }\n",
            "typescript",
        ),
    ] {
        let root_path = unique_path(&format!("dired-open-enter-{expected_language}-root"));
        let initial_path = root_path.join("aaa.txt");
        let source_path = root_path.join(file_name);
        let config_path =
            unique_path(&format!("dired-open-enter-{expected_language}-init")).with_extension("ts");
        std::fs::create_dir_all(&root_path).expect("root directory");
        std::fs::write(&initial_path, "initial\n").expect("initial file");
        std::fs::write(&source_path, source).expect("source file");
        std::fs::write(
            &config_path,
            r#"
                saya.commands.register("dired.open", async () => {
                    const trimTrailingSlash = (path) => path.length > 1 && path.endsWith("/") ? path.slice(0, -1) : path;
                    const dirname = (path) => {
                        const normalized = trimTrailingSlash(path || ".");
                        const index = normalized.lastIndexOf("/");
                        if (index < 0) return ".";
                        return index === 0 ? "/" : normalized.slice(0, index);
                    };
                    const currentPath = await saya.buffer.currentPath() || ".";
                    await saya.commands.execute(`edit ${dirname(currentPath)}`);
                });
                saya.commands.register("dired.enter", async () => {
                    const entry = await saya.filer.currentEntry();
                    if (entry) {
                        await saya.commands.execute(`edit ${entry.path}`);
                    }
                });
                saya.keymap.set("normal", "-", saya.commands.execute("dired.open"));
                saya.keymap.set("normal", "<Enter>", saya.commands.execute("dired.enter"));
            "#,
        )
        .expect("config file");

        let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
            input_source: crate::app::cli::InputSource::File(initial_path.clone()),
            config_source: crate::app::cli::ConfigSource::File(config_path.clone()),
            ..crate::app::cli::LaunchRequest::default()
        })
        .expect("launch should succeed");
        outcome.core_bridge.set_screen_size(24, 80);
        outcome
            .core_bridge
            .apply_ex_command("syntax on")
            .expect("syntax on should enable Tree-sitter highlight collection");
        let mut session_state = outcome.editor_session_state();
        let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
            .expect("runtime session should initialize");

        execute_runtime_command_for_test(
            &mut outcome,
            &mut session_state,
            &mut runtime_session,
            "dired.open",
        )
        .await;
        let target_row = outcome
            .core_bridge
            .snapshot()
            .text
            .lines()
            .position(|line| line == file_name)
            .unwrap_or_else(|| panic!("{file_name} should appear in dired listing"));
        for _ in 0..target_row {
            outcome
                .core_bridge
                .dispatch_key("j")
                .expect("move in dired");
        }
        execute_runtime_command_for_test(
            &mut outcome,
            &mut session_state,
            &mut runtime_session,
            "dired.enter",
        )
        .await;

        assert_eq!(outcome.target_path, Some(source_path.clone()));
        let snapshot = outcome.core_bridge.snapshot();
        let active_window = snapshot
            .active_window()
            .expect("dired.enter should leave an active source window");
        let active_buffer = snapshot
            .buffers
            .iter()
            .find(|buffer| buffer.id == active_window.buf_id)
            .expect("active buffer metadata should exist");
        assert_eq!(
            active_buffer.name,
            root_path.display().to_string(),
            "regression guard: dired-open then dired-enter keeps stale directory buffer name"
        );
        assert!(
            active_buffer
                .document_id
                .as_deref()
                .is_some_and(|document_id| document_id.starts_with("file://")
                    && document_id.ends_with(file_name)),
            "dired-open then dired-enter should expose the opened file through document_id"
        );

        let mut viewport_store = WindowViewportStore::new();
        let mut search_refresh_store = WindowSearchRefreshStore::default();
        let mut markdown_metadata_cache = MarkdownMetadataCache::default();
        let mut rendered_languages = BTreeSet::new();
        for _ in 0..20 {
            let workspace = build_workspace_render_output(
                &mut outcome,
                &mut session_state,
                &mut viewport_store,
                ViewportSyncMode::Core,
                &mut search_refresh_store,
                &mut markdown_metadata_cache,
                None,
                "",
                0,
                None,
                None,
                None,
                None,
                None,
                80,
                24,
                None,
                None,
                None,
                None,
            )
            .expect("dired-opened source workspace should render");
            rendered_languages = workspace
                .panes
                .iter()
                .flat_map(|pane| pane.syntax_chunks.iter())
                .filter_map(|chunk| chunk.language.as_deref())
                .map(str::to_string)
                .collect();
            if rendered_languages.contains(expected_language) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            rendered_languages.contains(expected_language),
            "dired-open then dired-enter should render Tree-sitter syntax for {expected_language}; rendered_languages={rendered_languages:?}, active_buffer={active_buffer:?}"
        );

        std::fs::remove_file(config_path).expect("cleanup config");
        std::fs::remove_dir_all(root_path).expect("cleanup root directory");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn dired_enter_keeps_directory_state_when_file_load_fails() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("dired-enter-load-failure-root");
    let binary_path = root_path.join("bad.bin");
    let config_path = unique_path("dired-enter-load-failure-init").with_extension("ts");
    std::fs::create_dir_all(&root_path).expect("root directory");
    std::fs::write(&binary_path, [0xff, 0xfe, 0xfd]).expect("binary file");
    std::fs::write(&config_path, dired_phase1_config_source()).expect("config file");
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::File(config_path.clone()),
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
        .expect("runtime session should initialize");

    execute_runtime_host_command(
        &format!("edit {}", root_path.display()),
        &mut outcome,
        &mut session_state,
    )
    .expect("open root listing");
    execute_runtime_command_for_test(
        &mut outcome,
        &mut session_state,
        &mut runtime_session,
        "dired.enter",
    )
    .await;

    assert_eq!(
        outcome.target_path,
        Some(root_path.clone()),
        "failed file loads must not retarget the host session away from the directory buffer"
    );
    assert_eq!(
        session_state.target_path().map(PathBuf::as_path),
        Some(root_path.as_path())
    );
    assert!(
        session_state.directory_buffer().is_some(),
        "dired metadata should remain active so filer styling and currentEntry keep working"
    );
    assert_eq!(outcome.core_bridge.snapshot().text, "bad.bin\n");

    std::fs::remove_file(config_path).expect("cleanup config");
    std::fs::remove_dir_all(root_path).expect("cleanup root directory");
}

#[tokio::test(flavor = "current_thread")]
async fn dired_up_opens_parent_directory() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("dired-up-root");
    let child_path = root_path.join("child");
    let config_path = unique_path("dired-up-init").with_extension("ts");
    std::fs::create_dir_all(&child_path).expect("child directory");
    std::fs::write(&config_path, dired_phase1_config_source()).expect("config file");
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::File(config_path.clone()),
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
        .expect("runtime session should initialize");

    execute_runtime_host_command(
        &format!("edit {}", child_path.display()),
        &mut outcome,
        &mut session_state,
    )
    .expect("open child listing");
    execute_runtime_command_for_test(
        &mut outcome,
        &mut session_state,
        &mut runtime_session,
        "dired.up",
    )
    .await;

    assert_eq!(outcome.target_path, Some(root_path.clone()));
    assert!(outcome.core_bridge.snapshot().text.contains("child/\n"));

    std::fs::remove_file(config_path).expect("cleanup config");
    std::fs::remove_dir_all(root_path).expect("cleanup root directory");
}

#[tokio::test(flavor = "current_thread")]
async fn dired_refresh_reloads_current_directory_listing() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("dired-refresh-root");
    let readme_path = root_path.join("README.md");
    let later_path = root_path.join("later.txt");
    let config_path = unique_path("dired-refresh-init").with_extension("ts");
    std::fs::create_dir_all(&root_path).expect("root directory");
    std::fs::write(&readme_path, "hello\n").expect("readme file");
    std::fs::write(&config_path, dired_phase1_config_source()).expect("config file");
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::File(config_path.clone()),
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
        .expect("runtime session should initialize");

    execute_runtime_host_command(
        &format!("edit {}", root_path.display()),
        &mut outcome,
        &mut session_state,
    )
    .expect("open root listing");
    assert!(!outcome.core_bridge.snapshot().text.contains("later.txt\n"));
    std::fs::write(&later_path, "later\n").expect("later file");
    execute_runtime_command_for_test(
        &mut outcome,
        &mut session_state,
        &mut runtime_session,
        "dired.refresh",
    )
    .await;

    assert!(outcome.core_bridge.snapshot().text.contains("later.txt\n"));

    std::fs::remove_file(config_path).expect("cleanup config");
    std::fs::remove_dir_all(root_path).expect("cleanup root directory");
}

#[tokio::test(flavor = "current_thread")]
async fn dired_filter_projects_listing_and_current_entry_metadata() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("dired-filter-root");
    let alpha_path = root_path.join("alpha.rs");
    let notes_path = root_path.join("notes.txt");
    let hidden_path = root_path.join(".hidden.rs");
    let config_path = unique_path("dired-filter-init").with_extension("ts");
    std::fs::create_dir_all(&root_path).expect("root directory");
    std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
    std::fs::write(&notes_path, "notes\n").expect("notes file");
    std::fs::write(&hidden_path, "hidden\n").expect("hidden file");
    std::fs::write(&config_path, dired_phase12_config_source()).expect("config file");
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::File(config_path.clone()),
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
        .expect("runtime session should initialize");

    execute_runtime_host_command(
        &format!("edit {}", root_path.display()),
        &mut outcome,
        &mut session_state,
    )
    .expect("open root listing");
    execute_runtime_command_for_test(
        &mut outcome,
        &mut session_state,
        &mut runtime_session,
        "dired.filterRust",
    )
    .await;

    assert_eq!(outcome.core_bridge.snapshot().text, "alpha.rs\n");
    let entry = MainRuntimeHostSession::new(&mut outcome, &mut session_state)
        .current_filer_entry()
        .expect("current filer entry should resolve")
        .expect("filtered listing should keep a current entry");
    assert_eq!(entry.name, "alpha.rs");
    assert_eq!(entry.path, alpha_path.to_string_lossy());

    std::fs::remove_file(config_path).expect("cleanup config");
    std::fs::remove_dir_all(root_path).expect("cleanup root directory");
}

#[tokio::test(flavor = "current_thread")]
async fn dired_filter_sort_and_hidden_state_survive_operation_refresh() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("dired-filter-refresh-root");
    let alpha_path = root_path.join("alpha.rs");
    let notes_path = root_path.join("notes.txt");
    let hidden_path = root_path.join(".hidden.rs");
    let beta_path = root_path.join("beta.rs");
    let config_path = unique_path("dired-filter-refresh-init").with_extension("ts");
    std::fs::create_dir_all(&root_path).expect("root directory");
    std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
    std::fs::write(&notes_path, "notes\n").expect("notes file");
    std::fs::write(&hidden_path, "hidden\n").expect("hidden file");
    std::fs::write(&config_path, dired_phase12_config_source()).expect("config file");
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::File(config_path.clone()),
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
        .expect("runtime session should initialize");

    execute_runtime_host_command(
        &format!("edit {}", root_path.display()),
        &mut outcome,
        &mut session_state,
    )
    .expect("open root listing");
    execute_runtime_command_for_test(
        &mut outcome,
        &mut session_state,
        &mut runtime_session,
        "dired.filterRust",
    )
    .await;
    execute_runtime_command_for_test(
        &mut outcome,
        &mut session_state,
        &mut runtime_session,
        "dired.createFilteredRust",
    )
    .await;

    assert!(beta_path.is_file());
    assert_eq!(outcome.core_bridge.snapshot().text, "alpha.rs\nbeta.rs\n");
    let entries = session_state
        .directory_buffer()
        .expect("directory metadata should remain active")
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(entries, vec!["alpha.rs", "beta.rs"]);

    std::fs::remove_file(config_path).expect("cleanup config");
    std::fs::remove_dir_all(root_path).expect("cleanup root directory");
}

#[tokio::test(flavor = "current_thread")]
async fn dired_create_file_refreshes_directory_listing() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("dired-create-file-root");
    let created_path = root_path.join("created.txt");
    std::fs::create_dir_all(&root_path).expect("root directory");
    let (config_path, mut outcome, mut session_state, mut runtime_session) =
        prepare_dired_runtime_fixture("dired-create-file-init", dired_phase3_config_source());

    open_dired_listing_for_test(&root_path, &mut outcome, &mut session_state);
    assert_directory_listing_state(&outcome, &session_state, &root_path, "\n");
    execute_runtime_command_for_test(
        &mut outcome,
        &mut session_state,
        &mut runtime_session,
        "dired.createFile",
    )
    .await;

    assert!(created_path.is_file());
    assert_eq!(
        std::fs::read_to_string(&created_path).expect("created file contents"),
        ""
    );
    assert_directory_listing_state(&outcome, &session_state, &root_path, "created.txt\n");

    std::fs::remove_file(config_path).expect("cleanup config");
    std::fs::remove_dir_all(root_path).expect("cleanup root directory");
}

#[tokio::test(flavor = "current_thread")]
async fn dired_create_directory_refreshes_directory_listing() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("dired-create-directory-root");
    let created_path = root_path.join("created-dir");
    std::fs::create_dir_all(&root_path).expect("root directory");
    let (config_path, mut outcome, mut session_state, mut runtime_session) =
        prepare_dired_runtime_fixture("dired-create-directory-init", dired_phase3_config_source());

    open_dired_listing_for_test(&root_path, &mut outcome, &mut session_state);
    assert_directory_listing_state(&outcome, &session_state, &root_path, "\n");
    execute_runtime_command_for_test(
        &mut outcome,
        &mut session_state,
        &mut runtime_session,
        "dired.createDirectory",
    )
    .await;

    assert!(created_path.is_dir());
    assert_directory_listing_state(&outcome, &session_state, &root_path, "created-dir/\n");

    std::fs::remove_file(config_path).expect("cleanup config");
    std::fs::remove_dir_all(root_path).expect("cleanup root directory");
}

#[tokio::test(flavor = "current_thread")]
async fn dired_rename_refreshes_directory_listing_and_preserves_cursor_target() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("dired-rename-root");
    let source_path = root_path.join("source.txt");
    let renamed_path = root_path.join("renamed.txt");
    std::fs::create_dir_all(&root_path).expect("root directory");
    std::fs::write(&source_path, "hello\n").expect("source file");
    let (config_path, mut outcome, mut session_state, mut runtime_session) =
        prepare_dired_runtime_fixture("dired-rename-init", dired_phase3_config_source());

    open_dired_listing_for_test(&root_path, &mut outcome, &mut session_state);
    assert_directory_listing_state(&outcome, &session_state, &root_path, "source.txt\n");
    execute_runtime_command_for_test(
        &mut outcome,
        &mut session_state,
        &mut runtime_session,
        "dired.rename",
    )
    .await;

    assert!(!source_path.exists());
    assert!(renamed_path.is_file());
    assert_eq!(
        std::fs::read_to_string(&renamed_path).expect("renamed file contents"),
        "hello\n"
    );
    let snapshot = outcome.core_bridge.snapshot();
    assert_eq!(snapshot.text, "renamed.txt\n");
    assert_eq!(snapshot.cursor_row, 0);
    assert_eq!(
        directory_buffer_display_texts_for_test(&session_state),
        vec!["renamed.txt".to_string()]
    );

    std::fs::remove_file(config_path).expect("cleanup config");
    std::fs::remove_dir_all(root_path).expect("cleanup root directory");
}

#[tokio::test(flavor = "current_thread")]
async fn dired_copy_and_move_are_host_mediated_and_refresh_directory_listing() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("dired-copy-move-root");
    let source_path = root_path.join("source.txt");
    let copied_path = root_path.join("copied.txt");
    let moved_path = root_path.join("moved.txt");
    let directory_source_path = root_path.join("source-dir");
    let directory_moved_path = root_path.join("moved-dir");
    std::fs::create_dir_all(&root_path).expect("root directory");
    std::fs::create_dir_all(&directory_source_path).expect("source directory");
    std::fs::write(&source_path, "hello\n").expect("source file");
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
    .expect("open root listing");
    let copy_report = execute_runtime_filer_operation(
        RuntimeFilerOperation::Copy {
            from: source_path.clone(),
            to: copied_path.clone(),
        },
        &mut outcome,
        &mut session_state,
    )
    .expect("copy should succeed through host-mediated filer operation");
    let move_report = execute_runtime_filer_operation(
        RuntimeFilerOperation::Move {
            from: copied_path.clone(),
            to: moved_path.clone(),
        },
        &mut outcome,
        &mut session_state,
    )
    .expect("move should succeed through host-mediated filer operation");
    let move_directory_report = execute_runtime_filer_operation(
        RuntimeFilerOperation::Move {
            from: directory_source_path.clone(),
            to: directory_moved_path.clone(),
        },
        &mut outcome,
        &mut session_state,
    )
    .expect("directory move should use the host rename path");

    assert_eq!(copy_report.operation, RuntimeFilerOperationKind::Copy);
    assert_eq!(move_report.operation, RuntimeFilerOperationKind::Move);
    assert_eq!(
        move_directory_report.operation,
        RuntimeFilerOperationKind::Move
    );
    assert_eq!(
        std::fs::read_to_string(&source_path).expect("source file remains after copy"),
        "hello\n"
    );
    assert!(
        !copied_path.exists(),
        "move should remove the intermediate path"
    );
    assert_eq!(
        std::fs::read_to_string(&moved_path).expect("moved file should exist"),
        "hello\n"
    );
    assert!(!directory_source_path.exists());
    assert!(directory_moved_path.is_dir());
    let snapshot = outcome.core_bridge.snapshot();
    assert!(snapshot.text.contains("source.txt\n"));
    assert!(snapshot.text.contains("moved.txt\n"));
    assert!(snapshot.text.contains("moved-dir/\n"));
    assert!(!snapshot.text.contains("copied.txt\n"));

    std::fs::remove_dir_all(root_path).expect("cleanup root directory");
}

#[tokio::test(flavor = "current_thread")]
async fn dired_recursive_delete_and_trash_policy_fail_without_mutation() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("dired-recursive-delete-policy");
    let non_empty_dir = root_path.join("non-empty");
    let child_path = non_empty_dir.join("child.txt");
    let copy_dir = root_path.join("copy-dir");
    let copied_dir = root_path.join("copied-dir");
    let trash_target = root_path.join("trash-me.txt");
    std::fs::create_dir_all(&non_empty_dir).expect("nested directory");
    std::fs::create_dir_all(&copy_dir).expect("copy directory");
    std::fs::write(&child_path, "child\n").expect("child file");
    std::fs::write(&trash_target, "trash\n").expect("trash target");
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
    .expect("open root listing");
    let recursive_without_opt_in = execute_runtime_filer_operation(
        RuntimeFilerOperation::Delete {
            path: non_empty_dir.clone(),
            confirm: true,
            recursive: false,
            trash: false,
        },
        &mut outcome,
        &mut session_state,
    );
    assert!(
        recursive_without_opt_in.is_err(),
        "non-empty directory delete must not silently become recursive"
    );
    assert!(child_path.is_file());

    let directory_copy = execute_runtime_filer_operation(
        RuntimeFilerOperation::Copy {
            from: copy_dir.clone(),
            to: copied_dir.clone(),
        },
        &mut outcome,
        &mut session_state,
    );
    assert!(matches!(
        directory_copy,
        Err(RuntimeFilerError::OperationFailed {
            kind: RuntimeFilerErrorKind::Unsupported,
            ..
        })
    ));
    assert!(copy_dir.is_dir());
    assert!(!copied_dir.exists());

    let trash_request = execute_runtime_filer_operation(
        RuntimeFilerOperation::Delete {
            path: trash_target.clone(),
            confirm: true,
            recursive: false,
            trash: true,
        },
        &mut outcome,
        &mut session_state,
    );
    assert!(matches!(
        trash_request,
        Err(RuntimeFilerError::OperationFailed {
            kind: RuntimeFilerErrorKind::Unsupported,
            ..
        })
    ));
    assert!(
        trash_target.is_file(),
        "unsupported trash backend must fail without deleting permanently"
    );

    std::fs::remove_dir_all(root_path).expect("cleanup root directory");
}

#[tokio::test(flavor = "current_thread")]
async fn dired_delete_confirmed_single_file_refreshes_directory_listing() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("dired-delete-file-root");
    let delete_path = root_path.join("delete-me.txt");
    let keep_path = root_path.join("keep.txt");
    std::fs::create_dir_all(&root_path).expect("root directory");
    std::fs::write(&delete_path, "delete\n").expect("delete file");
    std::fs::write(&keep_path, "keep\n").expect("keep file");
    let (config_path, mut outcome, mut session_state, mut runtime_session) =
        prepare_dired_runtime_fixture("dired-delete-file-init", dired_phase3_config_source());

    open_dired_listing_for_test(&root_path, &mut outcome, &mut session_state);
    assert_directory_listing_state(
        &outcome,
        &session_state,
        &root_path,
        "delete-me.txt\nkeep.txt\n",
    );
    execute_runtime_command_for_test(
        &mut outcome,
        &mut session_state,
        &mut runtime_session,
        "dired.deleteConfirmed",
    )
    .await;

    assert!(!delete_path.exists());
    assert!(keep_path.is_file());
    assert_directory_listing_state(&outcome, &session_state, &root_path, "keep.txt\n");

    std::fs::remove_file(config_path).expect("cleanup config");
    std::fs::remove_dir_all(root_path).expect("cleanup root directory");
}

#[tokio::test(flavor = "current_thread")]
async fn dired_delete_confirmed_single_empty_directory_refreshes_directory_listing() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("dired-delete-directory-root");
    let delete_path = root_path.join("delete-dir");
    std::fs::create_dir_all(&delete_path).expect("delete directory");
    let (config_path, mut outcome, mut session_state, mut runtime_session) =
        prepare_dired_runtime_fixture("dired-delete-directory-init", dired_phase3_config_source());

    open_dired_listing_for_test(&root_path, &mut outcome, &mut session_state);
    assert_directory_listing_state(&outcome, &session_state, &root_path, "delete-dir/\n");
    execute_runtime_command_for_test(
        &mut outcome,
        &mut session_state,
        &mut runtime_session,
        "dired.deleteConfirmed",
    )
    .await;

    assert!(!delete_path.exists());
    assert_directory_listing_state(&outcome, &session_state, &root_path, "\n");

    std::fs::remove_file(config_path).expect("cleanup config");
    std::fs::remove_dir_all(root_path).expect("cleanup root directory");
}

#[tokio::test(flavor = "current_thread")]
async fn dired_delete_requires_explicit_confirmation() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("dired-delete-confirm-root");
    let delete_path = root_path.join("delete-me.txt");
    std::fs::create_dir_all(&root_path).expect("root directory");
    std::fs::write(&delete_path, "delete\n").expect("delete file");
    let (config_path, mut outcome, mut session_state, mut runtime_session) =
        prepare_dired_runtime_fixture("dired-delete-confirm-init", dired_phase3_config_source());

    open_dired_listing_for_test(&root_path, &mut outcome, &mut session_state);
    assert_directory_listing_state(&outcome, &session_state, &root_path, "delete-me.txt\n");
    let (transient_msg, need_redraw) = execute_runtime_command_outcome_for_test(
        &mut outcome,
        &mut session_state,
        &mut runtime_session,
        "dired.deleteWithoutConfirm",
    )
    .await;

    assert!(delete_path.is_file());
    let message = transient_msg.expect("delete without confirmation should surface an error");
    assert!(
        message.contains("confirmationRequired") && message.contains("delete-me.txt"),
        "message should include structured error kind and path, got: {message}"
    );
    assert!(need_redraw);
    assert_directory_listing_state(&outcome, &session_state, &root_path, "delete-me.txt\n");

    std::fs::remove_file(config_path).expect("cleanup config");
    std::fs::remove_dir_all(root_path).expect("cleanup root directory");
}

#[tokio::test(flavor = "current_thread")]
async fn dired_mark_unmark_and_clear_are_available_from_typescript_commands() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("dired-mark-runtime-root");
    let alpha_path = root_path.join("alpha.txt");
    let beta_path = root_path.join("beta.txt");
    let config_path = unique_path("dired-mark-runtime-init").with_extension("ts");
    std::fs::create_dir_all(&root_path).expect("root directory");
    std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
    std::fs::write(&beta_path, "beta\n").expect("beta file");
    std::fs::write(&config_path, dired_phase4_config_source()).expect("config file");
    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::Empty,
        config_source: crate::app::cli::ConfigSource::File(config_path.clone()),
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
        .expect("runtime session should initialize");

    execute_runtime_host_command(
        &format!("edit {}", root_path.display()),
        &mut outcome,
        &mut session_state,
    )
    .expect("open root listing");
    execute_runtime_command_for_test(
        &mut outcome,
        &mut session_state,
        &mut runtime_session,
        "dired.mark",
    )
    .await;
    assert_eq!(session_state.marked_directory_entries().len(), 1);

    execute_runtime_command_for_test(
        &mut outcome,
        &mut session_state,
        &mut runtime_session,
        "dired.unmark",
    )
    .await;
    assert!(session_state.marked_directory_entries().is_empty());

    execute_runtime_command_for_test(
        &mut outcome,
        &mut session_state,
        &mut runtime_session,
        "dired.mark",
    )
    .await;
    outcome.core_bridge.dispatch_key("j").expect("move to beta");
    execute_runtime_command_for_test(
        &mut outcome,
        &mut session_state,
        &mut runtime_session,
        "dired.mark",
    )
    .await;
    assert_eq!(session_state.marked_directory_entries().len(), 2);

    execute_runtime_command_for_test(
        &mut outcome,
        &mut session_state,
        &mut runtime_session,
        "dired.clearMarks",
    )
    .await;
    assert!(session_state.marked_directory_entries().is_empty());

    std::fs::remove_file(config_path).expect("cleanup config");
    std::fs::remove_dir_all(root_path).expect("cleanup root directory");
}

#[tokio::test(flavor = "current_thread")]
async fn dired_bulk_delete_requires_preview_id_and_confirmation_before_deleting_marks() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("dired-bulk-delete-root");
    let alpha_path = root_path.join("alpha.txt");
    let beta_path = root_path.join("beta.txt");
    let keep_path = root_path.join("keep.txt");
    std::fs::create_dir_all(&root_path).expect("root directory");
    std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
    std::fs::write(&beta_path, "beta\n").expect("beta file");
    std::fs::write(&keep_path, "keep\n").expect("keep file");
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
    .expect("open root listing");
    assert_directory_listing_state(
        &outcome,
        &session_state,
        &root_path,
        "alpha.txt\nbeta.txt\nkeep.txt\n",
    );
    execute_runtime_filer_operation(
        RuntimeFilerOperation::Mark {
            path: alpha_path.clone(),
        },
        &mut outcome,
        &mut session_state,
    )
    .expect("mark alpha");
    execute_runtime_filer_operation(
        RuntimeFilerOperation::Mark {
            path: beta_path.clone(),
        },
        &mut outcome,
        &mut session_state,
    )
    .expect("mark beta");

    let without_preview = execute_runtime_filer_operation(
        RuntimeFilerOperation::BulkDelete {
            preview_id: String::new(),
            confirm: true,
        },
        &mut outcome,
        &mut session_state,
    );
    assert!(matches!(
        without_preview,
        Err(RuntimeFilerError::OperationFailed {
            kind: RuntimeFilerErrorKind::ConfirmationRequired,
            ..
        })
    ));
    assert!(alpha_path.is_file());
    assert!(beta_path.is_file());
    assert_eq!(session_state.marked_directory_entries().len(), 2);

    let preview = execute_runtime_filer_operation(
        RuntimeFilerOperation::BulkDeletePreview,
        &mut outcome,
        &mut session_state,
    )
    .expect("preview should be available");
    assert_eq!(
        preview.operation,
        RuntimeFilerOperationKind::BulkDeletePreview
    );
    assert_eq!(preview.entries.len(), 2);
    assert_eq!(
        preview
            .entries
            .iter()
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>(),
        vec![
            alpha_path.to_string_lossy().to_string(),
            beta_path.to_string_lossy().to_string(),
        ]
    );
    let preview_id = preview.preview_id.expect("preview id");

    let without_confirm = execute_runtime_filer_operation(
        RuntimeFilerOperation::BulkDelete {
            preview_id: preview_id.clone(),
            confirm: false,
        },
        &mut outcome,
        &mut session_state,
    );
    assert!(matches!(
        without_confirm,
        Err(RuntimeFilerError::OperationFailed {
            kind: RuntimeFilerErrorKind::ConfirmationRequired,
            ..
        })
    ));
    assert!(alpha_path.is_file());
    assert!(beta_path.is_file());
    assert_eq!(session_state.marked_directory_entries().len(), 2);
    assert_directory_listing_state(
        &outcome,
        &session_state,
        &root_path,
        "alpha.txt\nbeta.txt\nkeep.txt\n",
    );

    let report = execute_runtime_filer_operation(
        RuntimeFilerOperation::BulkDelete {
            preview_id,
            confirm: true,
        },
        &mut outcome,
        &mut session_state,
    )
    .expect("confirmed bulk delete should succeed");

    assert_eq!(report.operation, RuntimeFilerOperationKind::BulkDelete);
    assert_eq!(report.entries.len(), 2);
    assert_eq!(
        report
            .entries
            .iter()
            .map(|entry| entry.display_text.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha.txt", "beta.txt"]
    );
    assert!(!alpha_path.exists());
    assert!(!beta_path.exists());
    assert!(keep_path.is_file());
    assert!(session_state.marked_directory_entries().is_empty());
    assert_directory_listing_state(&outcome, &session_state, &root_path, "keep.txt\n");

    std::fs::remove_dir_all(root_path).expect("cleanup root directory");
}

#[tokio::test(flavor = "current_thread")]
async fn dired_create_file_collision_surfaces_structured_error() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("dired-create-collision-root");
    let existing_path = root_path.join("existing.txt");
    std::fs::create_dir_all(&root_path).expect("root directory");
    std::fs::write(&existing_path, "existing\n").expect("existing file");
    let (config_path, mut outcome, mut session_state, mut runtime_session) =
        prepare_dired_runtime_fixture("dired-create-collision-init", dired_phase3_config_source());

    open_dired_listing_for_test(&root_path, &mut outcome, &mut session_state);
    assert_directory_listing_state(&outcome, &session_state, &root_path, "existing.txt\n");
    let (transient_msg, need_redraw) = execute_runtime_command_outcome_for_test(
        &mut outcome,
        &mut session_state,
        &mut runtime_session,
        "dired.createFileCollision",
    )
    .await;

    let message = transient_msg.expect("collision should surface an error");
    assert!(
        message.contains("alreadyExists") && message.contains("existing.txt"),
        "message should include structured error kind and path, got: {message}"
    );
    assert!(need_redraw);
    assert_eq!(
        std::fs::read_to_string(&existing_path).expect("existing file"),
        "existing\n"
    );
    assert_directory_listing_state(&outcome, &session_state, &root_path, "existing.txt\n");

    std::fs::remove_file(config_path).expect("cleanup config");
    std::fs::remove_dir_all(root_path).expect("cleanup root directory");
}

#[tokio::test(flavor = "current_thread")]
async fn dired_rename_missing_path_surfaces_structured_error() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("dired-rename-missing-root");
    std::fs::create_dir_all(&root_path).expect("root directory");
    let (config_path, mut outcome, mut session_state, mut runtime_session) =
        prepare_dired_runtime_fixture("dired-rename-missing-init", dired_phase3_config_source());

    open_dired_listing_for_test(&root_path, &mut outcome, &mut session_state);
    assert_directory_listing_state(&outcome, &session_state, &root_path, "\n");
    let (transient_msg, need_redraw) = execute_runtime_command_outcome_for_test(
        &mut outcome,
        &mut session_state,
        &mut runtime_session,
        "dired.renameMissing",
    )
    .await;

    let message = transient_msg.expect("missing path should surface an error");
    assert!(
        message.contains("notFound") && message.contains("missing.txt"),
        "message should include structured error kind and path, got: {message}"
    );
    assert!(need_redraw);
    assert!(!root_path.join("never.txt").exists());
    assert_directory_listing_state(&outcome, &session_state, &root_path, "\n");

    std::fs::remove_file(config_path).expect("cleanup config");
    std::fs::remove_dir_all(root_path).expect("cleanup root directory");
}

#[tokio::test(flavor = "current_thread")]
async fn startup_registered_dired_keymap_opens_directory_listing() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root_path = unique_path("startup-dired-root");
    let nested_path = root_path.join("src");
    let readme_path = root_path.join("README.md");
    let target_path = root_path.join("notes.txt");
    let config_path = unique_path("startup-dired-init").with_extension("ts");
    std::fs::create_dir_all(&nested_path).expect("test directory");
    std::fs::write(&readme_path, "hello\n").expect("readme file");
    std::fs::write(&target_path, "notes\n").expect("target file");
    std::fs::write(
        &config_path,
        r#"
            saya.commands.register("dired.open", async () => {
                const buffer = await saya.buffer.current();
                const currentPath = buffer.path || ".";
                const directory = currentPath.endsWith("/")
                    ? (currentPath.slice(0, -1) || "/")
                    : (currentPath.lastIndexOf("/") >= 0 ? currentPath.slice(0, currentPath.lastIndexOf("/")) || "/" : ".");
                await saya.commands.execute(`edit ${directory}`);
            });
            saya.keymap.set("normal", "-", saya.commands.execute("dired.open"));
        "#,
    )
    .expect("config file");

    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::File(target_path.clone()),
        config_source: crate::app::cli::ConfigSource::File(config_path.clone()),
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
        .expect("runtime session should initialize");
    let mut transient_msg = None;
    let mut need_redraw = false;
    let mut runtime_presentation_intents = Vec::new();

    let mode = outcome.core_bridge.mode();
    let action = startup_keymap_action_for_input(
        &outcome.startup_registry.keymaps,
        mode,
        &KeyInput::Char('-'),
    )
    .unwrap_or_else(|| {
        panic!(
            "dired keymap should resolve; mode={mode:?}, keymaps={:?}, warnings={:?}",
            outcome.startup_registry.keymaps, outcome.warnings
        )
    });
    let StartupKeymapAction::RegisteredCommand(command_name) = action else {
        panic!("dired keymap should point at a registered command");
    };
    let mut floating_window_manager = FloatingWindowManager::default();
    let mut completion_float_manager = CompletionFloatManager::default();
    let mut lsp_diagnostic_store = LspDiagnosticStore::default();
    let mut terminal_float_manager = TerminalFloatManager::default();
    let mut panel_manager = PanelManager::default();
    let shutdown = execute_startup_keymap_registered_command(
        Some(&mut runtime_session),
        &command_name,
        &mut outcome,
        &mut session_state,
        &mut floating_window_manager,
        &mut completion_float_manager,
        &mut lsp_diagnostic_store,
        &mut terminal_float_manager,
        &mut panel_manager,
        None,
        &mut transient_msg,
        &mut need_redraw,
        &mut runtime_presentation_intents,
        None,
    )
    .await;

    assert_eq!(shutdown, None);
    assert_eq!(
        transient_msg, None,
        "dired startup command should not surface swap or pager messages"
    );
    assert!(runtime_presentation_intents.is_empty());
    assert!(floating_window_manager.is_empty());
    assert_eq!(completion_float_manager.active_documentation_id(), None);
    assert!(panel_manager.snapshots().is_empty());
    let mut expected_entries = vec!["src/", "README.md", "notes.txt"];
    if root_path.join(".notes.txt.swp").exists() {
        expected_entries.insert(1, ".notes.txt.swp");
    }
    let expected_listing = expected_entries.join("\n") + "\n";
    assert_directory_listing_state(&outcome, &session_state, &root_path, &expected_listing);

    std::fs::remove_file(config_path).expect("cleanup config");
    std::fs::remove_dir_all(root_path).expect("cleanup root directory");
}

#[tokio::test(flavor = "current_thread")]
async fn startup_registered_dired_keymap_opens_current_directory_for_relative_file() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let config_path = unique_path("startup-dired-relative-init").with_extension("ts");
    std::fs::write(
        &config_path,
        r#"
            saya.commands.register("dired.open", async () => {
                const buffer = await saya.buffer.current();
                const currentPath = buffer.path || ".";
                const directory = currentPath.endsWith("/")
                    ? (currentPath.slice(0, -1) || "/")
                    : (currentPath.lastIndexOf("/") >= 0 ? currentPath.slice(0, currentPath.lastIndexOf("/")) || "/" : ".");
                await saya.commands.execute(`edit ${directory}`);
            });
            saya.keymap.set("normal", "-", saya.commands.execute("dired.open"));
        "#,
    )
    .expect("config file");

    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::File(PathBuf::from("AGENTS.md")),
        config_source: crate::app::cli::ConfigSource::File(config_path.clone()),
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
        .expect("runtime session should initialize");
    let mut transient_msg = None;
    let mut need_redraw = false;
    let mut runtime_presentation_intents = Vec::new();

    let action = startup_keymap_action_for_input(
        &outcome.startup_registry.keymaps,
        outcome.core_bridge.mode(),
        &KeyInput::Char('-'),
    )
    .expect("dired keymap should resolve");
    let StartupKeymapAction::RegisteredCommand(command_name) = action else {
        panic!("dired keymap should point at a registered command");
    };
    let mut floating_window_manager = FloatingWindowManager::default();
    let mut completion_float_manager = CompletionFloatManager::default();
    let mut lsp_diagnostic_store = LspDiagnosticStore::default();
    let mut terminal_float_manager = TerminalFloatManager::default();
    let mut panel_manager = PanelManager::default();
    let shutdown = execute_startup_keymap_registered_command(
        Some(&mut runtime_session),
        &command_name,
        &mut outcome,
        &mut session_state,
        &mut floating_window_manager,
        &mut completion_float_manager,
        &mut lsp_diagnostic_store,
        &mut terminal_float_manager,
        &mut panel_manager,
        None,
        &mut transient_msg,
        &mut need_redraw,
        &mut runtime_presentation_intents,
        None,
    )
    .await;

    assert_eq!(shutdown, None);
    assert!(
        matches!(
            transient_msg.as_deref(),
            None | Some("E301: Oops, lost the swap file!!!")
        ),
        "dired startup command should not surface runtime command errors: {transient_msg:?}"
    );
    assert!(runtime_presentation_intents.is_empty());
    assert!(floating_window_manager.is_empty());
    assert_eq!(completion_float_manager.active_documentation_id(), None);
    assert!(panel_manager.snapshots().is_empty());
    assert_eq!(outcome.target_path, Some(PathBuf::from(".")));
    assert_eq!(
        session_state.target_path().map(PathBuf::as_path),
        Some(std::path::Path::new("."))
    );
    let directory_buffer = session_state
        .directory_buffer()
        .expect("relative dired command should leave directory metadata active");
    assert_eq!(
        outcome.core_bridge.snapshot().text,
        directory_buffer.display_text,
        "relative dired command should replace the previous file buffer with the directory listing"
    );
    let display_texts = directory_buffer_display_texts_for_test(&session_state);
    assert!(display_texts.contains(&"Cargo.toml".to_string()));
    assert_ne!(
        outcome.core_bridge.snapshot().text,
        std::fs::read_to_string("AGENTS.md").expect("source file should remain readable"),
        "relative dired listing should replace the previous file contents in the active buffer"
    );

    std::fs::remove_file(config_path).expect("cleanup config");
}

#[tokio::test(flavor = "current_thread")]
async fn repository_dired_keymap_moves_above_current_directory_from_relative_file() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let config_path = unique_path("repository-dired-up-relative-init").with_extension("ts");
    let root_path = unique_repo_relative_path("repository-dired-up-relative");
    let child_path = root_path.join("child");
    let target_path = child_path.join("file.txt");
    std::fs::create_dir_all(&child_path).expect("child directory");
    std::fs::write(&target_path, "relative\n").expect("relative test file");
    let plugin_path = crate::support::paths::dev_ts_plugins_dir().join("saya-dired.ts");
    std::fs::write(
        &config_path,
        format!(
            r#"
                import {{ setupSayaDired }} from "{}";
                setupSayaDired({{ keymap: {{}} }});
            "#,
            plugin_path.display()
        ),
    )
    .expect("config file");

    let mut outcome = crate::app::bootstrap::prepare_launch(crate::app::cli::LaunchRequest {
        input_source: crate::app::cli::InputSource::File(target_path.clone()),
        config_source: crate::app::cli::ConfigSource::File(config_path.clone()),
        ..crate::app::cli::LaunchRequest::default()
    })
    .expect("launch should succeed");
    let mut session_state = outcome.editor_session_state();
    let mut runtime_session = RuntimeSessionOwner::spawn(outcome.callback_registry.clone())
        .expect("runtime session should initialize");

    for _ in 0..2 {
        let action = startup_keymap_action_for_input(
            &outcome.startup_registry.keymaps,
            outcome.core_bridge.mode(),
            &KeyInput::Char('-'),
        )
        .expect("dired keymap should resolve");
        let StartupKeymapAction::RegisteredCommand(command_name) = action else {
            panic!("dired keymap should point at a registered command");
        };
        let mut transient_msg = None;
        let mut need_redraw = false;
        let mut runtime_presentation_intents = Vec::new();
        let mut floating_window_manager = FloatingWindowManager::default();
        let mut completion_float_manager = CompletionFloatManager::default();
        let mut lsp_diagnostic_store = LspDiagnosticStore::default();
        let mut terminal_float_manager = TerminalFloatManager::default();
        let mut panel_manager = PanelManager::default();
        execute_startup_keymap_registered_command(
            Some(&mut runtime_session),
            &command_name,
            &mut outcome,
            &mut session_state,
            &mut floating_window_manager,
            &mut completion_float_manager,
            &mut lsp_diagnostic_store,
            &mut terminal_float_manager,
            &mut panel_manager,
            None,
            &mut transient_msg,
            &mut need_redraw,
            &mut runtime_presentation_intents,
            None,
        )
        .await;
        assert_eq!(
            transient_msg, None,
            "dired up should not surface swap or pager messages"
        );
    }

    assert_eq!(outcome.target_path, Some(root_path.clone()));
    assert_eq!(session_state.target_path(), Some(&root_path));

    std::fs::remove_file(config_path).expect("cleanup config");
    std::fs::remove_dir_all(root_path).expect("cleanup relative root directory");
}
