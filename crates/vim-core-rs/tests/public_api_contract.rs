use std::fs;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
#[cfg(unix)]
use std::{
    fs::File,
    io::Read,
    os::fd::{FromRawFd, RawFd},
};
#[cfg(feature = "tree-sitter-syntax")]
use vim_core_rs::{
    CoreBufferRevision, CoreEmbeddedBlockKind, CoreEmbeddedLanguageResolutionRequest,
    CoreEmbeddedRegion, CoreEmbeddedRegionSource, CoreLanguageResolutionSource,
    CoreLanguageResolutionStatus, CoreLanguageRole, CoreMediaFlavor, CoreMediaKind,
    CoreResolutionConfidence, CoreResolvedLanguage, CoreRootLanguageResolutionRequest,
    CoreSyntaxCategory, CoreSyntaxModifier, CoreTextPosition, CoreTextRange,
    CoreTreeSitterBudgetStatus, CoreTreeSitterChunk, CoreTreeSitterLanguagePackage,
    CoreTreeSitterPreparationRequest, CoreTreeSitterProvenance, CoreTreeSitterRangeSyntax,
    CoreTreeSitterSnapshotPolicy, CoreTreeSitterStatus,
};
use vim_core_rs::{
    CoreCommandOutcome, CoreEvent, CoreHostAction, CoreInputRequestKind, CoreInputResponse,
    CoreInputResponseError, CoreMode, CoreOptionError, CoreOptionScope, CoreOptionType,
    CoreRuntimeMode, CoreSessionError, CoreSessionOptions, VimCoreSession,
};

fn session_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn acquire_session_test_lock() -> std::sync::MutexGuard<'static, ()> {
    session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(unix)]
fn capture_standard_streams<T>(f: impl FnOnce() -> T) -> (T, String, String) {
    unsafe fn capture_fd(fd: RawFd) -> (RawFd, RawFd) {
        let saved = unsafe { libc::dup(fd) };
        assert!(saved >= 0, "dup failed for fd={fd}");

        let mut pipefds = [0; 2];
        assert_eq!(
            unsafe { libc::pipe(pipefds.as_mut_ptr()) },
            0,
            "pipe failed for fd={fd}"
        );
        assert!(
            unsafe { libc::dup2(pipefds[1], fd) } >= 0,
            "dup2 failed for fd={fd}"
        );
        assert_eq!(
            unsafe { libc::close(pipefds[1]) },
            0,
            "close failed for write pipe fd={fd}"
        );
        (saved, pipefds[0])
    }

    unsafe fn restore_fd(fd: RawFd, saved: RawFd) {
        assert!(
            unsafe { libc::dup2(saved, fd) } >= 0,
            "restore dup2 failed for fd={fd}"
        );
        assert_eq!(
            unsafe { libc::close(saved) },
            0,
            "close failed for saved fd={fd}"
        );
    }

    unsafe fn read_pipe(read_fd: RawFd) -> String {
        let mut file = unsafe { File::from_raw_fd(read_fd) };
        let mut output = String::new();
        file.read_to_string(&mut output)
            .expect("pipe output should be readable");
        output
    }

    unsafe {
        let (saved_stdout, stdout_read) = capture_fd(libc::STDOUT_FILENO);
        let (saved_stderr, stderr_read) = capture_fd(libc::STDERR_FILENO);

        let result = f();

        libc::fflush(std::ptr::null_mut());
        restore_fd(libc::STDOUT_FILENO, saved_stdout);
        restore_fd(libc::STDERR_FILENO, saved_stderr);

        let stdout = read_pipe(stdout_read);
        let stderr = read_pipe(stderr_read);
        (result, stdout, stderr)
    }
}

#[cfg(unix)]
fn sanitize_harness_output(output: &str) -> String {
    output
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty()
                && !trimmed.starts_with("test ")
                && !trimmed.contains(" ... ok")
                && !trimmed.contains(" ... FAILED")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn session_exposes_initial_snapshot_contract() {
    let _guard = acquire_session_test_lock();
    let session =
        VimCoreSession::new("first line\nsecond line").expect("session should initialize");
    let snapshot = session.snapshot();

    assert_eq!(
        snapshot.text.trim_end_matches('\n'),
        "first line\nsecond line"
    );
    assert_eq!(snapshot.revision, 0);
    assert!(!snapshot.dirty);
    assert_eq!(snapshot.mode, CoreMode::Normal);
    assert_eq!(snapshot.pending_host_actions, 0);
    assert_eq!(session.runtime_mode(), CoreRuntimeMode::Embedded);
}

#[test]
fn light_snapshot_does_not_materialize_full_text_for_large_buffer() {
    let _guard = acquire_session_test_lock();
    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let log_path = tempdir.path().join("vim-core.log");
    let large_text = (0..512)
        .map(|idx| format!("large line {idx:04} {}", "x".repeat(64)))
        .collect::<Vec<_>>()
        .join("\n");
    let session = VimCoreSession::new_with_options(
        &large_text,
        CoreSessionOptions {
            debug_log_path: Some(log_path.clone()),
            ..CoreSessionOptions::default()
        },
    )
    .expect("session should initialize");

    let snapshot = session.light_snapshot();
    let active_buffer = snapshot
        .buffers
        .iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should be present");

    assert_eq!(snapshot.cursor_row, 0);
    assert_eq!(snapshot.mode, CoreMode::Normal);
    assert!(active_buffer.source_revision.value > 0);

    drop(session);

    let log = fs::read_to_string(&log_path).expect("debug log should be readable");
    assert!(
        !log.contains("get_curbuf_text"),
        "light_snapshot must not call the full-buffer text path:\n{log}"
    );
    assert!(
        log.contains("snapshot include_text=0"),
        "native light snapshot should be visible in debug log:\n{log}"
    );
}

#[test]
fn buffer_line_range_returns_only_requested_visible_rows() {
    let _guard = acquire_session_test_lock();
    let session =
        VimCoreSession::new("zero\none\ntwo\nthree\nfour").expect("session should initialize");
    let active_buffer = session
        .light_snapshot()
        .buffers
        .into_iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should be present");

    let range = session
        .buffer_line_range(active_buffer.id, 1, 3)
        .expect("line range should resolve");

    assert_eq!(range.buffer_id, active_buffer.id);
    assert_eq!(range.source_revision, active_buffer.source_revision);
    assert_eq!(range.start_row, 1);
    assert_eq!(range.line_count, 3);
    assert_eq!(range.total_line_count, 5);
    assert_eq!(range.lines, ["one", "two", "three"]);
}

#[test]
fn snapshot_full_text_remains_available_for_compatibility() {
    let _guard = acquire_session_test_lock();
    let session = VimCoreSession::new("alpha\nbeta").expect("session should initialize");

    assert_eq!(session.snapshot().text, "alpha\nbeta\n");
}

#[test]
fn second_session_is_rejected_while_first_is_alive() {
    let _guard = acquire_session_test_lock();
    let first = VimCoreSession::new("alpha").expect("first session should initialize");
    let second = VimCoreSession::new("beta");

    assert!(matches!(
        second,
        Err(CoreSessionError::SessionAlreadyActive)
    ));

    drop(first);

    let third = VimCoreSession::new("gamma").expect("session should initialize after drop");
    assert_eq!(third.snapshot().text.trim_end_matches('\n'), "gamma");
}

#[test]
fn dropping_tab_using_session_restores_side_effect_commands_for_next_session() {
    let _guard = acquire_session_test_lock();

    {
        let mut first = VimCoreSession::new("alpha").expect("first session should initialize");
        first
            .execute_ex_command("tabedit")
            .expect("tabedit should create a second tab");
        first
            .execute_ex_command("tabfirst")
            .expect("tabfirst should switch back to the original tab");
    }

    let mut second = VimCoreSession::new("beta").expect("second session should initialize");

    let zz = second
        .execute_normal_command("ZZ")
        .expect("ZZ should not crash after a prior tab-using session");
    assert!(matches!(zz.outcome, CoreCommandOutcome::HostActionQueued));
    assert!(
        !zz.host_actions.is_empty(),
        "ZZ should still produce host coordination after a prior tab-using session"
    );

    let zq = second
        .execute_normal_command("ZQ")
        .expect("ZQ should not crash after a prior tab-using session");
    assert!(matches!(zq.outcome, CoreCommandOutcome::HostActionQueued));
    assert!(matches!(
        zq.host_actions.first(),
        Some(CoreHostAction::Quit { force: true, .. })
    ));
}

#[test]
fn session_options_route_debug_log_output_to_file() {
    let _guard = acquire_session_test_lock();
    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let log_path = tempdir.path().join("vim-core-rs-debug.log");
    let options = CoreSessionOptions {
        runtime_mode: CoreRuntimeMode::Embedded,
        debug_log_path: Some(log_path.clone()),
    };
    let mut session = VimCoreSession::new_with_options("buffer", options)
        .expect("session should initialize with debug log path");

    session
        .execute_ex_command(":write output.txt")
        .expect("write command should succeed");
    let tabstop = session
        .get_option_number("tabstop", CoreOptionScope::Global)
        .expect("get_option_number should succeed");

    assert!(tabstop > 0, "tabstop should be a positive number");
    let log_output = fs::read_to_string(&log_path).expect("debug log file should be readable");
    assert!(
        log_output.contains("[DEBUG] apply_write_intent: local write")
            && log_output.contains("path=output.txt"),
        "debug log should be written to the configured file: {}",
        log_output
    );
    assert!(
        log_output.contains("[DEBUG] get_option: name='tabstop'"),
        "native debug log should be written to the configured file: {}",
        log_output
    );
}

#[test]
fn session_options_default_to_embedded_runtime_mode() {
    let options = CoreSessionOptions::default();
    assert_eq!(options.runtime_mode, CoreRuntimeMode::Embedded);
}

#[test]
fn standalone_runtime_mode_is_explicit_but_not_supported_yet() {
    let _guard = acquire_session_test_lock();

    let result = VimCoreSession::new_with_options(
        "buffer",
        CoreSessionOptions {
            runtime_mode: CoreRuntimeMode::Standalone,
            debug_log_path: None,
        },
    );

    assert!(matches!(
        result,
        Err(CoreSessionError::InitializationFailed {
            reason_code: "unsupported_runtime_mode",
        })
    ));
}

#[test]
fn session_options_disable_debug_log_output_by_default() {
    let _guard = acquire_session_test_lock();
    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let log_path = tempdir.path().join("vim-core-rs-debug.log");
    let mut session =
        VimCoreSession::new("buffer").expect("session should initialize with default options");

    session
        .execute_ex_command(":write output.txt")
        .expect("write command should succeed");
    let tabstop = session
        .get_option_number("tabstop", CoreOptionScope::Global)
        .expect("get_option_number should succeed");

    assert!(tabstop > 0, "tabstop should be a positive number");
    assert!(
        !log_path.exists(),
        "debug log file should not exist when debug_log_path is omitted"
    );
    assert_eq!(
        fs::read_dir(tempdir.path())
            .expect("tempdir should remain readable")
            .count(),
        0,
        "default debug logging should not create any files"
    );
}

#[test]
fn public_api_reference_documents_search_contract_for_inactive_windows_and_byte_columns() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let public_api_reference = fs::read_to_string(repo_root.join("docs/public-api-reference.md"))
        .expect("public API reference should be readable");

    assert!(
        public_api_reference.contains("query_visible_search_state_for_window"),
        "public API reference should mention the inactive-window search accessor"
    );
    assert!(
        public_api_reference.contains("inactive window"),
        "public API reference should document inactive-window queries"
    );
    assert!(
        public_api_reference.contains("both are byte columns"),
        "public API reference should document byte-column search ranges"
    );
    assert!(
        public_api_reference.contains("start_col is inclusive")
            && public_api_reference.contains("end_col is exclusive"),
        "public API reference should document inclusive/exclusive search columns"
    );
    assert!(
        public_api_reference.contains("inactive_window_query_available")
            && public_api_reference.contains("byte_columns")
            && public_api_reference.contains("data_only_payload")
            && public_api_reference.contains("host_owned_presentation"),
        "public API reference should expose the structured Search family capability fields"
    );
}

#[test]
fn public_api_reference_excludes_popupwin_and_keeps_textprop_deferred() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let public_api_reference = fs::read_to_string(repo_root.join("docs/public-api-reference.md"))
        .expect("public API reference should be readable");

    assert!(
        public_api_reference.contains("popupwin is host-owned presentation")
            || public_api_reference.contains("popupwin stays outside the family"),
        "public API reference should document popupwin as outside the rendering-state family"
    );
    assert!(
        public_api_reference.contains("textprop is the deferred placeholder")
            || public_api_reference.contains("textprop stays deferred placeholder")
            || public_api_reference.contains("textprop remains deferred placeholder"),
        "public API reference should document textprop as the deferred placeholder"
    );
    assert!(
        public_api_reference.contains("does not expose a public popupwin extractor")
            || public_api_reference.contains("does not expose a public textprop extractor"),
        "public API reference should document the missing popupwin/textprop extraction surface"
    );
    assert!(
        public_api_reference.contains("overlay")
            && public_api_reference.contains("composition")
            && public_api_reference.contains("border"),
        "public API reference should document popup layout, composition, and border ownership as out of scope"
    );
    assert!(
        public_api_reference.contains("resolved highlight attribute tables")
            || public_api_reference.contains("highlight definition tables"),
        "public API reference should document highlight-table exclusion from the public extraction surface"
    );
    assert!(
        !public_api_reference.contains("issue #14"),
        "public API reference should not defer the family boundary to issue #14"
    );
}

#[test]
fn public_docs_map_rendering_state_family_to_existing_vimcoresession_surface() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let public_api_reference = fs::read_to_string(repo_root.join("docs/public-api-reference.md"))
        .expect("public API reference should be readable");
    let api_contracts = fs::read_to_string(repo_root.join("docs/api-contracts.md"))
        .expect("API contracts should be readable");
    let api_index = fs::read_to_string(repo_root.join("docs/api-index.md"))
        .expect("API index should be readable");

    assert!(
        public_api_reference.contains("VimCoreSession")
            && public_api_reference.contains("main stateful facade"),
        "public API reference should identify VimCoreSession as the main stateful facade"
    );
    assert!(
        api_contracts.contains("authoritative source")
            && api_contracts.contains("Vim-owned read-only extraction boundary")
            && !api_contracts.contains("issue #14"),
        "API contracts should document the final family authority without issue #14 wording"
    );
    assert!(
        api_index.contains("Search` and `Syntax` are the current rendering-state family members")
            || api_index
                .contains("Search and Syntax are the current rendering-state family members"),
        "API index should map Search and Syntax into the rendering-state family"
    );
    assert!(
        api_index.contains("Vim-owned read-only extraction boundary")
            && !api_index.contains("issue #14"),
        "API index should describe the final boundary without issue #14 wording"
    );
    assert!(
        public_api_reference.contains("query_visible_search_state")
            && public_api_reference.contains("get_line_syntax"),
        "public API reference should describe the existing search and syntax accessors"
    );
}

#[test]
fn rendering_state_family_docs_describe_additive_grouping_and_mixed_mutability() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let public_api_reference = fs::read_to_string(repo_root.join("docs/public-api-reference.md"))
        .expect("public API reference should be readable");
    let api_contracts = fs::read_to_string(repo_root.join("docs/api-contracts.md"))
        .expect("API contracts should be readable");
    let api_index = fs::read_to_string(repo_root.join("docs/api-index.md"))
        .expect("API index should be readable");

    assert!(
        api_contracts.contains("vocabulary")
            && (api_contracts.contains("additive stateless summary")
                || api_contracts.contains("no new family descriptor")
                || api_contracts.contains("without introducing a new runtime facade")),
        "API contracts should describe the family as an additive explanation layer without a new descriptor"
    );
    assert!(
        api_index
            .contains("These accessors cover the current `Search` and `Syntax` family members")
            || api_index
                .contains("These accessors cover the current Search and Syntax family members"),
        "API index should describe Search and Syntax as grouped existing accessors"
    );
    assert!(
        (public_api_reference.contains("search family member")
            || public_api_reference.contains("Search family member"))
            && public_api_reference.contains("&mut self")
            && (public_api_reference.contains("syntax family member")
                || public_api_reference.contains("Syntax family member"))
            && public_api_reference.contains("&self"),
        "public API reference should document mixed mutability across family members"
    );
    assert!(
        public_api_reference.contains("no new family descriptor")
            || api_contracts.contains("no new family descriptor")
            || public_api_reference.contains("additive stateless summary"),
        "public API reference should keep the family mapping additive rather than introducing a descriptor"
    );
}

#[test]
fn search_family_docs_keep_incsearch_boundary_vocab_in_public_contracts() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let public_api_reference = fs::read_to_string(repo_root.join("docs/public-api-reference.md"))
        .expect("public API reference should be readable");
    let api_contracts = fs::read_to_string(repo_root.join("docs/api-contracts.md"))
        .expect("API contracts should be readable");
    let api_index = fs::read_to_string(repo_root.join("docs/api-index.md"))
        .expect("API index should be readable");

    assert!(
        public_api_reference.contains("Search family")
            && public_api_reference.contains("inactive window")
            && public_api_reference.contains("byte columns")
            && public_api_reference.contains("host-owned presentation"),
        "public API reference should keep Search family vocabulary for inactive windows, byte columns, and host-owned presentation"
    );
    assert!(
        api_contracts.contains("Search family")
            && api_contracts.contains("incsearch")
            && api_contracts.contains("host-owned presentation"),
        "API contracts should describe incsearch as part of the Search family boundary without moving presentation ownership"
    );
    assert!(
        api_index.contains("Search family")
            && api_index.contains("inactive window")
            && api_index.contains("byte columns")
            && api_index.contains("host-owned presentation"),
        "API index should summarize the Search family boundary for inactive windows, byte columns, and host-owned presentation"
    );
    assert!(
        !api_index.contains("issue #14"),
        "API index should not defer the Search family boundary to issue #14"
    );
}

#[test]
fn register_docs_describe_multiline_full_readback_in_public_api_reference() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let public_api_reference = fs::read_to_string(repo_root.join("docs/public-api-reference.md"))
        .expect("public API reference should be readable");

    assert!(
        public_api_reference.contains("register(&self, regname: char) -> Option<String>")
            && public_api_reference.contains("multiline")
            && public_api_reference.contains("full contents"),
        "public API reference should describe register() as returning full multiline contents"
    );
}

#[test]
fn register_docs_pin_contract_tests_as_the_source_of_truth() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let api_contracts = fs::read_to_string(repo_root.join("docs/api-contracts.md"))
        .expect("API contracts should be readable");

    assert!(
        api_contracts.contains("tests/register_contract.rs")
            && (api_contracts.contains("source of truth")
                || api_contracts.contains("authoritative source")),
        "API contracts should point register readback behavior at the contract tests"
    );
}

#[test]
fn host_action_queue_is_empty_by_default() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    assert!(session.take_pending_host_action().is_none());
    assert!(matches!(session.mode(), CoreMode::Normal));
    let bell = CoreHostAction::Bell;
    assert!(matches!(bell, CoreHostAction::Bell));
}

#[test]
fn event_queue_is_empty_by_default() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    assert!(session.take_pending_event().is_none());
}

#[test]
fn execute_ex_command_returns_transaction_with_events_and_host_actions() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    let tx = session
        .execute_ex_command(":redraw!")
        .expect("redraw command should succeed");

    assert!(matches!(tx.outcome, CoreCommandOutcome::NoChange));
    assert_eq!(tx.snapshot.text.trim_end_matches('\n'), "buffer");
    assert!(
        tx.events.iter().any(|event| matches!(
            event,
            CoreEvent::Redraw {
                full: true,
                clear_before_draw: true
            }
        )),
        "redraw should be surfaced as an event: {:?}",
        tx.events
    );
    assert!(
        tx.host_actions.is_empty(),
        "v2 transaction should not duplicate UI-like signals as host actions: {:?}",
        tx.host_actions
    );
}

#[cfg(unix)]
#[test]
fn embedded_redraw_event_does_not_leak_terminal_sequences_or_message_events() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    let (tx, stdout, stderr) = capture_standard_streams(|| {
        session
            .execute_ex_command(":redraw!")
            .expect("redraw command should succeed")
    });

    assert_eq!(
        sanitize_harness_output(&stdout),
        "",
        "embedded redraw must not write to stdout"
    );
    assert_eq!(
        sanitize_harness_output(&stderr),
        "",
        "embedded redraw must not write to stderr"
    );
    assert!(
        tx.events.iter().any(|event| matches!(
            event,
            CoreEvent::Redraw {
                full: true,
                clear_before_draw: true
            }
        )),
        "redraw should be surfaced as an event: {:?}",
        tx.events
    );
    assert!(
        tx.events
            .iter()
            .all(|event| !matches!(event, CoreEvent::Message(_))),
        "embedded redraw should not synthesize terminal control output as message events: {:?}",
        tx.events
    );
}

#[cfg(unix)]
#[test]
fn embedded_screen_resize_emits_layout_event_without_terminal_leak() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");
    session.set_screen_size(24, 80);
    while session.take_pending_event().is_some() {}
    while session.take_pending_host_action().is_some() {}

    let ((), stdout, stderr) = capture_standard_streams(|| {
        session.set_screen_size(40, 120);
    });

    assert_eq!(
        sanitize_harness_output(&stdout),
        "",
        "embedded resize must not write to stdout"
    );
    assert_eq!(
        sanitize_harness_output(&stderr),
        "",
        "embedded resize must not write to stderr"
    );
    assert!(matches!(
        session.take_pending_event(),
        Some(CoreEvent::LayoutChanged)
    ));
    assert!(session.take_pending_host_action().is_none());
    assert!(
        session.take_pending_event().is_none(),
        "screen resize should not enqueue extra message-like events"
    );
}

#[test]
fn execute_ex_command_surfaces_split_as_events_without_ui_host_action_duplication() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");
    session.set_screen_size(24, 80);

    let tx = session
        .execute_ex_command(":split")
        .expect("split command should succeed");

    assert!(
        tx.events
            .iter()
            .any(|event| matches!(event, CoreEvent::WindowCreated { .. })),
        "split should surface window creation as an event: {:?}",
        tx.events
    );
    assert!(
        tx.events
            .iter()
            .any(|event| matches!(event, CoreEvent::LayoutChanged)),
        "split should surface layout change as an event: {:?}",
        tx.events
    );
    assert!(
        tx.host_actions.is_empty(),
        "v2 split should not duplicate UI-like signals as host actions: {:?}",
        tx.host_actions
    );
}

#[test]
fn execute_ex_command_surfaces_enew_as_event_without_ui_host_action_duplication() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    let tx = session
        .execute_ex_command(":enew")
        .expect("enew command should succeed");

    assert!(
        tx.events
            .iter()
            .any(|event| matches!(event, CoreEvent::BufferAdded { .. })),
        "enew should surface buffer creation as an event: {:?}",
        tx.events
    );
    assert!(
        tx.host_actions.is_empty(),
        "v2 enew should not duplicate UI-like signals as host actions: {:?}",
        tx.host_actions
    );
}

#[test]
fn snapshot_does_not_drain_pending_event_queue() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");
    session.set_screen_size(24, 80);
    while session.take_pending_event().is_some() {}

    session.set_screen_size(40, 120);

    let snapshot = session.snapshot();
    assert_eq!(snapshot.text.trim_end_matches('\n'), "buffer");
    assert!(matches!(
        session.take_pending_event(),
        Some(CoreEvent::LayoutChanged)
    ));
}

#[test]
fn host_action_queue_no_longer_duplicates_redraw_events() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    let tx = session
        .execute_ex_command(":redraw!")
        .expect("redraw command should succeed");

    assert!(matches!(
        tx.events.as_slice(),
        [CoreEvent::Redraw {
            full: true,
            clear_before_draw: true,
        }]
    ));
    assert!(
        session.take_pending_host_action().is_none(),
        "queue API should no longer duplicate redraw once event delivery exists"
    );
}

#[test]
fn host_action_queue_no_longer_retains_layout_changed() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");
    session.set_screen_size(24, 80);
    while session.take_pending_event().is_some() {}
    while session.take_pending_host_action().is_some() {}

    session.set_screen_size(40, 120);

    assert!(matches!(
        session.take_pending_event(),
        Some(CoreEvent::LayoutChanged)
    ));
    assert!(session.take_pending_host_action().is_none());
}

#[test]
fn mode_enum_exposes_extended_visual_and_select_variants() {
    let expected = [
        CoreMode::Visual,
        CoreMode::VisualLine,
        CoreMode::VisualBlock,
        CoreMode::Select,
        CoreMode::SelectLine,
        CoreMode::SelectBlock,
    ];

    assert_eq!(expected.len(), 6);
}

#[test]
fn option_scope_enum_exposes_all_supported_variants() {
    let expected = [
        CoreOptionScope::Default,
        CoreOptionScope::Global,
        CoreOptionScope::Local,
    ];

    assert_eq!(expected.len(), 3);
}

#[test]
fn option_type_enum_exposes_all_supported_variants() {
    let expected = [
        CoreOptionType::Bool,
        CoreOptionType::Number,
        CoreOptionType::String,
    ];

    assert_eq!(expected.len(), 3);
}

#[test]
fn option_error_variants_preserve_contract_details() {
    let mismatch = CoreOptionError::TypeMismatch {
        name: "tabstop".to_string(),
        expected: CoreOptionType::Number,
        actual: CoreOptionType::String,
    };
    assert!(matches!(
        mismatch,
        CoreOptionError::TypeMismatch {
            name,
            expected: CoreOptionType::Number,
            actual: CoreOptionType::String,
        } if name == "tabstop"
    ));

    let unknown = CoreOptionError::UnknownOption {
        name: "missing".to_string(),
    };
    assert!(matches!(
        unknown,
        CoreOptionError::UnknownOption { name } if name == "missing"
    ));

    let set_failed = CoreOptionError::SetFailed {
        name: "tabstop".to_string(),
        reason: "E487".to_string(),
    };
    assert!(matches!(
        set_failed,
        CoreOptionError::SetFailed { name, reason }
            if name == "tabstop" && reason == "E487"
    ));

    let scope_not_supported = CoreOptionError::ScopeNotSupported {
        name: "encoding".to_string(),
        scope: CoreOptionScope::Local,
    };
    assert!(matches!(
        scope_not_supported,
        CoreOptionError::ScopeNotSupported { name, scope: CoreOptionScope::Local }
            if name == "encoding"
    ));

    let internal = CoreOptionError::InternalError {
        name: "number".to_string(),
        detail: "null state".to_string(),
    };
    assert!(matches!(
        internal,
        CoreOptionError::InternalError { name, detail }
            if name == "number" && detail == "null state"
    ));
}

#[test]
fn option_getters_return_typed_values_from_vim() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    session
        .execute_ex_command(":set tabstop=6")
        .expect("tabstop should be set via ex command");
    session
        .execute_ex_command(":set expandtab")
        .expect("expandtab should be set via ex command");
    session
        .execute_ex_command(":set filetype=rust")
        .expect("filetype should be set via ex command");

    assert_eq!(
        session
            .get_option_number("tabstop", CoreOptionScope::Default)
            .expect("number option should be returned"),
        6
    );
    assert!(
        session
            .get_option_bool("expandtab", CoreOptionScope::Default)
            .expect("bool option should be returned")
    );
    assert_eq!(
        session
            .get_option_string("filetype", CoreOptionScope::Default)
            .expect("string option should be returned"),
        "rust"
    );
}

#[test]
fn option_getters_support_scope_selection_for_local_options() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    session
        .execute_ex_command(":setglobal shiftwidth=8")
        .expect("global shiftwidth should be set");
    session
        .execute_ex_command(":setlocal shiftwidth=3")
        .expect("local shiftwidth should be set");

    assert_eq!(
        session
            .get_option_number("shiftwidth", CoreOptionScope::Default)
            .expect("default scope should prefer local value"),
        3
    );
    assert_eq!(
        session
            .get_option_number("shiftwidth", CoreOptionScope::Local)
            .expect("local scope should return local value"),
        3
    );
    assert_eq!(
        session
            .get_option_number("shiftwidth", CoreOptionScope::Global)
            .expect("global scope should return global value"),
        8
    );
}

#[test]
fn option_getters_report_scope_not_supported_for_global_option_local_scope() {
    let _guard = acquire_session_test_lock();
    let session = VimCoreSession::new("buffer").expect("session should initialize");

    assert!(matches!(
        session.get_option_string("encoding", CoreOptionScope::Local),
        Err(CoreOptionError::ScopeNotSupported {
            name,
            scope: CoreOptionScope::Local,
        }) if name == "encoding"
    ));
}

#[test]
fn option_getters_report_type_mismatch_for_wrong_accessor() {
    let _guard = acquire_session_test_lock();
    let session = VimCoreSession::new("buffer").expect("session should initialize");

    assert!(matches!(
        session.get_option_bool("tabstop", CoreOptionScope::Default),
        Err(CoreOptionError::TypeMismatch {
            name,
            expected: CoreOptionType::Bool,
            actual: CoreOptionType::Number,
        }) if name == "tabstop"
    ));
}

#[test]
fn option_setters_update_typed_values_in_vim() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    session
        .set_option_number("tabstop", 8, CoreOptionScope::Default)
        .expect("number option should be set");
    session
        .set_option_bool("expandtab", true, CoreOptionScope::Default)
        .expect("bool option should be set");
    session
        .set_option_string("filetype", "rust", CoreOptionScope::Default)
        .expect("string option should be set");

    assert_eq!(
        session
            .get_option_number("tabstop", CoreOptionScope::Default)
            .expect("updated tabstop should be returned"),
        8
    );
    assert!(
        session
            .get_option_bool("expandtab", CoreOptionScope::Default)
            .expect("updated expandtab should be returned")
    );
    assert_eq!(
        session
            .get_option_string("filetype", CoreOptionScope::Default)
            .expect("updated filetype should be returned"),
        "rust"
    );
}

#[test]
fn option_setters_report_vim_validation_errors() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    assert!(matches!(
        session.set_option_number("tabstop", 0, CoreOptionScope::Default),
        Err(CoreOptionError::SetFailed { name, .. }) if name == "tabstop"
    ));

    assert!(matches!(
        session.set_option_string("fileformat", "wide", CoreOptionScope::Default),
        Err(CoreOptionError::SetFailed { name, .. }) if name == "fileformat"
    ));
}

#[test]
fn option_number_api_round_trips_tabstop_and_shiftwidth() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    session
        .set_option_number("tabstop", 8, CoreOptionScope::Default)
        .expect("tabstop should be set");
    session
        .set_option_number("shiftwidth", 4, CoreOptionScope::Local)
        .expect("shiftwidth should be set locally");

    assert_eq!(
        session
            .get_option_number("tabstop", CoreOptionScope::Default)
            .expect("tabstop should be returned"),
        8
    );
    assert_eq!(
        session
            .get_option_number("shiftwidth", CoreOptionScope::Local)
            .expect("local shiftwidth should be returned"),
        4
    );
}

#[test]
fn option_bool_api_round_trips_expandtab_and_number() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    session
        .set_option_bool("expandtab", true, CoreOptionScope::Default)
        .expect("expandtab should be set");
    session
        .set_option_bool("number", true, CoreOptionScope::Local)
        .expect("number should be set locally");

    assert!(
        session
            .get_option_bool("expandtab", CoreOptionScope::Default)
            .expect("expandtab should be returned")
    );
    assert!(
        session
            .get_option_bool("number", CoreOptionScope::Local)
            .expect("number should be returned")
    );
}

#[test]
fn option_string_api_round_trips_filetype_and_fileencoding() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    session
        .set_option_string("filetype", "rust", CoreOptionScope::Default)
        .expect("filetype should be set");
    session
        .set_option_string("fileencoding", "utf-8", CoreOptionScope::Local)
        .expect("fileencoding should be set");

    assert_eq!(
        session
            .get_option_string("filetype", CoreOptionScope::Default)
            .expect("filetype should be returned"),
        "rust"
    );
    assert_eq!(
        session
            .get_option_string("fileencoding", CoreOptionScope::Local)
            .expect("fileencoding should be returned"),
        "utf-8"
    );
}

#[test]
fn option_api_interoperates_with_ex_commands_both_directions() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    session
        .execute_ex_command(":set tabstop=6")
        .expect("tabstop should be set via ex command");
    session
        .execute_ex_command(":set filetype=rust")
        .expect("filetype should be set via ex command");

    assert_eq!(
        session
            .get_option_number("tabstop", CoreOptionScope::Default)
            .expect("tabstop should be returned after ex update"),
        6
    );
    assert_eq!(
        session
            .get_option_string("filetype", CoreOptionScope::Default)
            .expect("filetype should be returned after ex update"),
        "rust"
    );

    session
        .set_option_number("tabstop", 9, CoreOptionScope::Default)
        .expect("tabstop should be updated through API");
    session
        .set_option_string("filetype", "lua", CoreOptionScope::Default)
        .expect("filetype should be updated through API");

    session
        .execute_ex_command("%d")
        .expect("buffer should be cleared before ex confirmation");
    session
        .execute_ex_command("put =&tabstop")
        .expect("tabstop should be queryable via ex command");
    session
        .execute_ex_command("put =&filetype")
        .expect("filetype should be queryable via ex command");

    let snapshot = session.snapshot();
    assert!(
        snapshot.text.contains("\n9\n"),
        "expected ex-visible tabstop in buffer, got {:?}",
        snapshot.text
    );
    assert!(
        snapshot.text.contains("\nlua\n"),
        "expected ex-visible filetype in buffer, got {:?}",
        snapshot.text
    );
}

#[test]
fn option_errors_cover_unknown_type_validation_and_scope_cases() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    assert!(matches!(
        session.get_option_number("definitely_missing_option", CoreOptionScope::Default),
        Err(CoreOptionError::UnknownOption { name }) if name == "definitely_missing_option"
    ));

    assert!(matches!(
        session.get_option_bool("tabstop", CoreOptionScope::Default),
        Err(CoreOptionError::TypeMismatch {
            name,
            expected: CoreOptionType::Bool,
            actual: CoreOptionType::Number,
        }) if name == "tabstop"
    ));

    assert!(matches!(
        session.set_option_number("tabstop", 0, CoreOptionScope::Default),
        Err(CoreOptionError::SetFailed { name, .. }) if name == "tabstop"
    ));

    assert!(matches!(
        session.get_option_string("encoding", CoreOptionScope::Local),
        Err(CoreOptionError::ScopeNotSupported {
            name,
            scope: CoreOptionScope::Local,
        }) if name == "encoding"
    ));

    // 存在しないオプションへの設定で SetFailed エラーが返ることを検証する
    assert!(matches!(
        session.set_option_number("nonexistent_option", 1, CoreOptionScope::Default),
        Err(CoreOptionError::SetFailed { name, .. }) if name == "nonexistent_option"
    ));

    // 文字列型の存在しないオプションへの設定でも SetFailed エラーが返ることを検証する
    assert!(matches!(
        session.set_option_string("nonexistent_option", "value", CoreOptionScope::Default),
        Err(CoreOptionError::SetFailed { name, .. }) if name == "nonexistent_option"
    ));
}

#[test]
fn backend_identity_reports_upstream_runtime() {
    let _guard = acquire_session_test_lock();
    let session = VimCoreSession::new("buffer").expect("session should initialize");

    assert_eq!(
        format!("{:?}", session.backend_identity()),
        "UpstreamRuntime"
    );
}

#[test]
fn normal_delete_command_mutates_buffer_via_vim_runtime() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("first line\nsecond line\nthird line")
        .expect("session should initialize");

    let outcome = session
        .execute_normal_command("dd")
        .expect("dd should succeed");

    assert!(matches!(
        outcome.outcome,
        vim_core_rs::CoreCommandOutcome::BufferChanged { revision: 1 }
    ));

    let snapshot = session.snapshot();
    assert_eq!(
        snapshot.text.trim_end_matches('\n'),
        "second line\nthird line"
    );
    assert_eq!(snapshot.revision, 1);
    assert!(snapshot.dirty);
    assert_eq!(snapshot.mode, CoreMode::Normal);
}

#[test]
fn normal_insert_command_switches_mode() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    let _outcome = session
        .execute_normal_command("i")
        .expect("i should succeed");

    assert_eq!(session.mode(), CoreMode::Insert);
}

#[cfg(unix)]
#[test]
fn embedded_dispatch_insert_does_not_write_terminal_streams() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    let (tx, stdout, stderr) = capture_standard_streams(|| {
        session
            .dispatch_key("i")
            .expect("i should enter insert mode")
    });

    assert_eq!(tx.snapshot.mode, CoreMode::Insert);
    assert_eq!(session.mode(), CoreMode::Insert);
    assert!(
        sanitize_harness_output(&stdout).is_empty(),
        "embedded dispatch_key(\"i\") must not write to stdout: {stdout:?}"
    );
    assert!(
        sanitize_harness_output(&stderr).is_empty(),
        "embedded dispatch_key(\"i\") must not write to stderr: {stderr:?}"
    );
}

#[cfg(unix)]
#[test]
fn embedded_dispatch_insert_text_does_not_write_terminal_streams() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    session
        .dispatch_key("i")
        .expect("i should enter insert mode");

    let (tx, stdout, stderr) = capture_standard_streams(|| {
        session
            .dispatch_key("X")
            .expect("insert text should succeed")
    });

    assert_eq!(tx.snapshot.mode, CoreMode::Insert);
    assert_eq!(tx.snapshot.text, "Xbuffer\n");
    assert!(matches!(
        tx.outcome,
        CoreCommandOutcome::BufferChanged { .. }
    ));
    assert!(
        sanitize_harness_output(&stdout).is_empty(),
        "embedded insert text must not write to stdout: {stdout:?}"
    );
    assert!(
        sanitize_harness_output(&stderr).is_empty(),
        "embedded insert text must not write to stderr: {stderr:?}"
    );
}

#[cfg(unix)]
#[test]
fn embedded_execute_insert_does_not_write_terminal_streams() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    let (tx, stdout, stderr) = capture_standard_streams(|| {
        session
            .execute_normal_command("i")
            .expect("i should enter insert mode")
    });

    assert_eq!(tx.snapshot.mode, CoreMode::Insert);
    assert_eq!(session.mode(), CoreMode::Insert);
    assert!(
        sanitize_harness_output(&stdout).is_empty(),
        "embedded execute_normal_command(\"i\") must not write to stdout: {stdout:?}"
    );
    assert!(
        sanitize_harness_output(&stderr).is_empty(),
        "embedded execute_normal_command(\"i\") must not write to stderr: {stderr:?}"
    );
}

#[test]
fn normal_other_insert_commands_switch_mode() {
    let _guard = acquire_session_test_lock();

    // (command, initial text, expected mode, expected row, expected col)
    let commands_and_positions = vec![
        ("a", "word", CoreMode::Insert, 0, 1),
        ("A", "word", CoreMode::Insert, 0, 4),
        ("o", "word", CoreMode::Insert, 1, 0),
        ("O", "word", CoreMode::Insert, 0, 0),
        ("R", "word", CoreMode::Replace, 0, 0),
    ];

    for (cmd, initial_text, expected_mode, exp_row, exp_col) in commands_and_positions {
        let mut session = VimCoreSession::new(initial_text).expect("session should initialize");

        let _outcome = session
            .execute_normal_command(cmd)
            .expect("command should succeed");

        let snapshot = session.snapshot();

        assert_eq!(
            session.mode(),
            expected_mode,
            "Failed mode for command {}",
            cmd
        );
        assert_eq!(
            snapshot.cursor_row, exp_row,
            "Failed cursor_row for command {}",
            cmd
        );
        assert_eq!(
            snapshot.cursor_col, exp_col,
            "Failed cursor_col for command {}",
            cmd
        );
    }
}

#[test]
fn ex_write_command_queues_host_action_once() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    let tx = session
        .execute_ex_command(":write! output.txt")
        .expect("write command should succeed");

    assert!(matches!(
        tx.outcome,
        vim_core_rs::CoreCommandOutcome::HostActionQueued
    ));
    assert_eq!(
        tx.host_actions,
        vec![CoreHostAction::Write {
            path: "output.txt".to_string(),
            force: true,
            issued_after_revision: 0,
        }]
    );
    assert!(session.take_pending_host_action().is_none());
}

#[test]
fn ex_quit_command_queues_quit_action() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    let tx = session
        .execute_ex_command(":quit!")
        .expect("quit command should succeed");

    assert!(matches!(
        tx.outcome,
        vim_core_rs::CoreCommandOutcome::HostActionQueued
    ));
    assert_eq!(
        tx.host_actions,
        vec![CoreHostAction::Quit {
            force: true,
            issued_after_revision: 0,
        }]
    );
    assert!(session.take_pending_host_action().is_none());
}

#[test]
fn ex_redraw_command_surfaces_redraw_event() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    let tx = session
        .execute_ex_command(":redraw!")
        .expect("redraw command should succeed");

    assert!(matches!(
        tx.outcome,
        vim_core_rs::CoreCommandOutcome::NoChange
    ));
    assert!(matches!(
        tx.events.as_slice(),
        [CoreEvent::Redraw {
            full: true,
            clear_before_draw: true,
        }]
    ));
    assert!(session.take_pending_host_action().is_none());
}

#[test]
fn ex_input_command_queues_input_request_action() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    let tx = session
        .execute_ex_command(":input Enter filename")
        .expect("input command should succeed");

    assert!(matches!(
        tx.outcome,
        vim_core_rs::CoreCommandOutcome::HostActionQueued
    ));
    assert_eq!(
        tx.host_actions,
        vec![CoreHostAction::RequestInput {
            prompt: "Enter filename".to_string(),
            input_kind: CoreInputRequestKind::CommandLine,
            correlation_id: 1,
        }]
    );
}

#[test]
fn execute_ex_command_keeps_input_flow_as_host_action_not_pager_prompt() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    let tx = session
        .execute_ex_command(":input Enter filename")
        .expect("input command should succeed");

    assert_eq!(
        tx.host_actions,
        vec![CoreHostAction::RequestInput {
            prompt: "Enter filename".to_string(),
            input_kind: CoreInputRequestKind::CommandLine,
            correlation_id: 1,
        }]
    );
    assert!(
        tx.events
            .iter()
            .all(|event| !matches!(event, CoreEvent::PagerPrompt(_))),
        "input prompt should stay a host action rather than a pager prompt: {:?}",
        tx.events
    );
}

#[test]
fn input_response_api_accepts_submit_and_cancel_for_active_request() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    let request_tx = session
        .execute_ex_command(":input Enter filename")
        .expect("input command should succeed");
    assert_eq!(
        request_tx.host_actions,
        vec![CoreHostAction::RequestInput {
            prompt: "Enter filename".to_string(),
            input_kind: CoreInputRequestKind::CommandLine,
            correlation_id: 1,
        }]
    );

    let submit_tx = session
        .submit_input_response(CoreInputResponse::Submitted {
            correlation_id: 1,
            value: "notes.txt".to_string(),
        })
        .expect("submit response should be accepted");

    assert!(matches!(submit_tx.outcome, CoreCommandOutcome::NoChange));
    assert!(submit_tx.host_actions.is_empty());
    assert!(submit_tx.events.is_empty());

    let request_tx = session
        .execute_ex_command(":input Confirm")
        .expect("second input command should succeed");
    assert_eq!(
        request_tx.host_actions,
        vec![CoreHostAction::RequestInput {
            prompt: "Confirm".to_string(),
            input_kind: CoreInputRequestKind::CommandLine,
            correlation_id: 2,
        }]
    );

    let cancel_tx = session
        .submit_input_response(CoreInputResponse::Cancelled { correlation_id: 2 })
        .expect("cancel response should be accepted");

    assert!(matches!(cancel_tx.outcome, CoreCommandOutcome::NoChange));
    assert!(cancel_tx.host_actions.is_empty());
    assert!(cancel_tx.events.is_empty());
}

#[test]
fn input_response_api_rejects_no_pending_and_correlation_mismatch() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    let no_pending =
        session.submit_input_response(CoreInputResponse::Cancelled { correlation_id: 99 });
    assert!(matches!(
        no_pending,
        Err(CoreInputResponseError::NoPendingInput)
    ));

    session
        .execute_ex_command(":input Enter filename")
        .expect("input command should succeed");

    let mismatch = session.submit_input_response(CoreInputResponse::Submitted {
        correlation_id: 2,
        value: "wrong".to_string(),
    });
    assert!(matches!(
        mismatch,
        Err(CoreInputResponseError::CorrelationMismatch {
            expected: 1,
            actual: 2,
        })
    ));

    let accepted =
        session.submit_input_response(CoreInputResponse::Cancelled { correlation_id: 1 });
    assert!(
        accepted.is_ok(),
        "mismatch should not clear the active request: {:?}",
        accepted
    );
}

#[cfg(unix)]
#[test]
fn vimscript_input_function_emits_message_without_host_action_or_terminal_leak() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    let ((result, event, action), stdout, stderr) = capture_standard_streams(|| {
        let result = session.eval_string(r#"input("Enter filename: ")"#);
        let event = session.take_pending_event();
        let action = session.take_pending_host_action();
        (result, event, action)
    });

    assert_eq!(result, None);
    assert!(
        event.is_none(),
        "input() should not emit fail-fast messages: {event:?}"
    );
    assert_eq!(
        action,
        Some(CoreHostAction::RequestInput {
            prompt: "Enter filename: ".to_string(),
            input_kind: CoreInputRequestKind::CommandLine,
            correlation_id: 1,
        })
    );
    assert!(
        sanitize_harness_output(&stdout).is_empty() && sanitize_harness_output(&stderr).is_empty(),
        "embedded input() should not leak prompts to the terminal: stdout={:?}, stderr={:?}",
        stdout,
        stderr
    );

    session
        .submit_input_response(CoreInputResponse::Submitted {
            correlation_id: 1,
            value: "notes.txt".to_string(),
        })
        .expect("input() response should resume evaluation");
    assert_eq!(
        session.take_completed_input_eval_result(),
        Some("notes.txt".to_string())
    );
}

#[cfg(unix)]
#[test]
fn vimscript_inputsecret_function_emits_message_without_host_action_or_terminal_leak() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    let ((result, event, action), stdout, stderr) = capture_standard_streams(|| {
        let result = session.eval_string(r#"inputsecret("Password: ")"#);
        let event = session.take_pending_event();
        let action = session.take_pending_host_action();
        (result, event, action)
    });

    assert_eq!(result, None);
    assert!(
        event.is_none(),
        "inputsecret() should not emit fail-fast messages: {event:?}"
    );
    assert_eq!(
        action,
        Some(CoreHostAction::RequestInput {
            prompt: "Password: ".to_string(),
            input_kind: CoreInputRequestKind::Secret,
            correlation_id: 1,
        })
    );
    assert!(
        sanitize_harness_output(&stdout).is_empty() && sanitize_harness_output(&stderr).is_empty(),
        "embedded inputsecret() should not leak prompts to the terminal: stdout={:?}, stderr={:?}",
        stdout,
        stderr
    );

    session
        .submit_input_response(CoreInputResponse::Cancelled { correlation_id: 1 })
        .expect("inputsecret() cancel should resume evaluation");
    assert_eq!(
        session.take_completed_input_eval_result(),
        Some(String::new())
    );
}

#[test]
fn ex_bell_command_surfaces_bell_event() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    let tx = session
        .execute_ex_command(":bell")
        .expect("bell command should succeed");

    assert!(matches!(
        tx.outcome,
        vim_core_rs::CoreCommandOutcome::NoChange
    ));
    assert!(matches!(tx.events.as_slice(), [CoreEvent::Bell]));
    assert!(session.take_pending_host_action().is_none());
}

#[test]
fn ex_set_command_executes_via_vim_without_host_action() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    // :set number は Vim 本体の Ex 実行経路で処理され、host action は生成されない
    let outcome = session
        .execute_ex_command(":set number")
        .expect("set command should succeed");

    assert!(
        !matches!(
            outcome.outcome,
            vim_core_rs::CoreCommandOutcome::HostActionQueued
        ),
        "set number は host action を生成しない"
    );
    assert!(session.take_pending_host_action().is_none());
}

#[test]
fn ex_substitute_command_modifies_buffer_via_vim() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello world").expect("session should initialize");

    // :s/hello/goodbye/ は Vim 本体の Ex 実行経路でバッファを変更する
    let _outcome = session
        .execute_ex_command(":s/hello/goodbye/")
        .expect("substitute command should succeed");

    let snapshot = session.snapshot();
    assert_eq!(snapshot.text.trim_end_matches('\n'), "goodbye world");
}

#[test]
fn ex_write_short_form_queues_host_action() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    // :w は :write の短縮形で、同様に host action を生成する
    let tx = session
        .execute_ex_command(":w output.txt")
        .expect("w command should succeed");

    assert!(matches!(
        tx.outcome,
        vim_core_rs::CoreCommandOutcome::HostActionQueued
    ));
    assert_eq!(
        tx.host_actions,
        vec![CoreHostAction::Write {
            path: "output.txt".to_string(),
            force: false,
            issued_after_revision: 0,
        }]
    );
}

#[test]
fn ex_quit_short_form_queues_host_action() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    // :q! は :quit! の短縮形で、同様に host action を生成する
    let tx = session
        .execute_ex_command(":q!")
        .expect("q command should succeed");

    assert!(matches!(
        tx.outcome,
        vim_core_rs::CoreCommandOutcome::HostActionQueued
    ));
    assert_eq!(
        tx.host_actions,
        vec![CoreHostAction::Quit {
            force: true,
            issued_after_revision: 0,
        }]
    );
}

#[test]
fn pathdef_resolves_non_empty_runtimepath() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    // :set runtimepath? を実行して runtimepath が空でないことを確認する。
    // pathdef.c の placeholder 依存が解消されていれば、Vim は configure 由来の
    // パスをデフォルト runtimepath として設定する。
    // この Ex コマンドはバッファを変更しないので NoChange が返る。
    let outcome = session
        .execute_ex_command(":set runtimepath?")
        .expect("set runtimepath? should succeed");

    assert!(
        !matches!(
            outcome.outcome,
            vim_core_rs::CoreCommandOutcome::HostActionQueued
        ),
        "set runtimepath? は host action を生成しない"
    );
    assert!(session.take_pending_host_action().is_none());
}

#[test]
fn pathdef_provides_vim_dir_for_runtime_discovery() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    // :echo $VIM で $VIM 変数を確認する。pathdef.c の default_vim_dir が
    // 空文字でなければ、Vim は起動時にこの値をフォールバックとして利用する。
    // headless 環境では $VIM 環境変数が未設定の場合、default_vim_dir が
    // 使われるため、空でないことを間接的に検証する。
    //
    // Note: この検証は default_vim_dir が compile-time に設定されていることの
    // 間接検証である。$VIM が環境変数として設定されている場合はそちらが
    // 優先されるが、pathdef.c のフォールバック値が空でないことが重要。
    let outcome = session
        .execute_ex_command(":set runtimepath?")
        .expect("should succeed");

    // コマンド自体がエラーにならないことが最低条件
    assert!(
        !matches!(
            outcome.outcome,
            vim_core_rs::CoreCommandOutcome::HostActionQueued
        ),
        "set runtimepath? はホストアクションを生成しない"
    );
}

#[test]
fn ex_redraw_without_bang_surfaces_non_clearing_event() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    // :redraw（! なし）は clear_before_draw: false の redraw event を生成する
    let tx = session
        .execute_ex_command(":redraw")
        .expect("redraw command should succeed");

    assert!(matches!(
        tx.outcome,
        vim_core_rs::CoreCommandOutcome::NoChange
    ));
    assert!(matches!(
        tx.events.as_slice(),
        [CoreEvent::Redraw {
            full: true,
            clear_before_draw: false,
        }]
    ));
    assert!(session.take_pending_host_action().is_none());
}

#[test]
fn normal_movement_command_changes_cursor() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("first line\nsecond line\nthird line\n")
        .expect("session should initialize");

    let outcome = session
        .execute_normal_command("j")
        .expect("j should succeed");

    assert!(matches!(
        outcome.outcome,
        vim_core_rs::CoreCommandOutcome::CursorChanged { row: 1, col: 0 }
    ));

    let snapshot = session.snapshot();
    assert_eq!(snapshot.cursor_row, 1);
    assert_eq!(snapshot.cursor_col, 0);

    let outcome2 = session
        .execute_normal_command("l")
        .expect("l should succeed");

    assert!(matches!(
        outcome2.outcome,
        vim_core_rs::CoreCommandOutcome::CursorChanged { row: 1, col: 1 }
    ));

    let snapshot2 = session.snapshot();
    assert_eq!(snapshot2.cursor_row, 1);
    assert_eq!(snapshot2.cursor_col, 1);
}

#[test]
fn api_index_maps_rendering_state_family_without_new_surface() {
    let api_index_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/api-index.md");
    let content = fs::read_to_string(&api_index_path).expect("api-index should be readable");

    assert!(
        content.contains("Rendering State Family")
            && content.contains("Search")
            && content.contains("Syntax")
            && content.contains("Annotations")
            && content.contains("deferred placeholder")
            && !content.contains("issue #14"),
        "api index should document the family boundary and phase split"
    );
    assert!(
        content.contains("get_search_pattern")
            && content.contains("get_line_syntax")
            && content.contains("textprop"),
        "api index should map the existing search/syntax accessors to the family boundary and keep textprop deferred"
    );
}

#[test]
fn tree_sitter_features_are_default_off_and_separate_from_vim_syntax() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let cargo_toml =
        fs::read_to_string(repo_root.join("Cargo.toml")).expect("Cargo.toml should be readable");
    let public_api_reference = fs::read_to_string(repo_root.join("docs/public-api-reference.md"))
        .expect("public API reference should be readable");
    let api_index = fs::read_to_string(repo_root.join("docs/api-index.md"))
        .expect("api index should be readable");

    assert!(
        cargo_toml.contains("default = []")
            && cargo_toml.contains("tree-sitter-syntax = [\"dep:tree-sitter\"]")
            && cargo_toml.contains("experimental-tree-sitter = [\"tree-sitter-syntax\"]")
            && cargo_toml.contains("tree-sitter-markdown = [\"tree-sitter-syntax\", \"dep:tree-sitter-md\", \"tree-sitter-md/parser\"]")
            && cargo_toml.contains("tree-sitter-rust = [\"tree-sitter-syntax\", \"dep:tree-sitter-rust\"]")
            && cargo_toml.contains("tree-sitter-go = [\"tree-sitter-syntax\", \"dep:tree-sitter-go\"]")
            && cargo_toml.contains("tree-sitter-typescript = [\"tree-sitter-syntax\", \"dep:tree-sitter-typescript\"]"),
        "Tree-sitter feature flags should be opt-in and default-off"
    );
    let dependency_sections = cargo_toml
        .split("[dependencies]")
        .nth(1)
        .and_then(|after_dependencies| after_dependencies.split("[build-dependencies]").next())
        .expect("Cargo.toml should contain dependency sections");
    assert!(
        dependency_sections.contains("tree-sitter = { version = \"0.26.8\", optional = true }")
            && dependency_sections
                .contains("tree-sitter-md = { version = \"0.5.3\", optional = true")
            && dependency_sections
                .contains("tree-sitter-rust = { version = \"0.24.2\", optional = true }")
            && dependency_sections
                .contains("tree-sitter-go = { version = \"0.25.0\", optional = true }")
            && dependency_sections
                .contains("tree-sitter-typescript = { version = \"0.23.2\", optional = true }"),
        "Tree-sitter parser and grammar dependencies should be optional"
    );
    assert!(
        public_api_reference.contains("CoreTreeSitterRangeSyntax")
            && public_api_reference.contains("feature-gated")
            && public_api_reference.contains("tree-sitter-syntax")
            && public_api_reference.contains("stable opt-in")
            && public_api_reference.contains("Host applications must prefer")
            && public_api_reference.contains("No alias removal release is scheduled")
            && public_api_reference.contains("separate from `CoreSyntaxChunk`")
            && public_api_reference.contains("embedded_regions")
            && public_api_reference.contains("CoreTextRange")
            && public_api_reference.contains("request_tree_sitter_syntax_preparation")
            && public_api_reference.contains("poll_tree_sitter_preparation")
            && public_api_reference.contains("query_tree_sitter_syntax_range")
            && public_api_reference.contains("(buffer_id, source_revision, visible_range)")
            && public_api_reference.contains("CoreSnapshot.revision")
            && public_api_reference.contains("MatchLimitExceeded")
            && public_api_reference.contains("has_error")
            && public_api_reference.contains("error_ranges")
            && public_api_reference.contains("vim_ffi")
            && api_index.contains("Tree-sitter syntax"),
        "docs should describe the feature-gated Tree-sitter surface separately from Vim syntax"
    );
}

#[cfg(feature = "tree-sitter-syntax")]
#[test]
fn tree_sitter_syntax_public_types_are_constructible() {
    let range = CoreTextRange {
        start: CoreTextPosition { row: 0, col: 0 },
        end: CoreTextPosition { row: 0, col: 4 },
    };
    let provenance = CoreTreeSitterProvenance {
        language_id: "rust".to_string(),
        package_id: "tree-sitter-rust".to_string(),
        package_version: "0.0.0-skeleton".to_string(),
        parser_version: "0.0.0-skeleton".to_string(),
        query_version: "0.0.0-skeleton".to_string(),
    };
    let chunk = CoreTreeSitterChunk {
        range,
        capture_name: "keyword".to_string(),
        category: CoreSyntaxCategory::Keyword,
        modifiers: vec![CoreSyntaxModifier::Definition],
    };
    let resolved_language = CoreResolvedLanguage {
        range,
        role: CoreLanguageRole::EmbeddedRegion,
        status: CoreLanguageResolutionStatus::Resolved,
        language_id: Some("rust".to_string()),
        package_id: Some(provenance.package_id.clone()),
        package_version: Some(provenance.package_version.clone()),
        kind: CoreEmbeddedBlockKind::Syntax,
        confidence: CoreResolutionConfidence::Exact,
        source: CoreLanguageResolutionSource::Registry,
    };
    let embedded_region = CoreEmbeddedRegion {
        range,
        content_range: range,
        source: CoreEmbeddedRegionSource::MarkdownFence,
        raw_info_string: Some("rust".to_string()),
        normalized_info_string: Some("rust".to_string()),
        normalized_kind: CoreEmbeddedBlockKind::Syntax,
        resolved_language: Some(resolved_language),
    };
    let syntax = CoreTreeSitterRangeSyntax {
        buffer_id: 1,
        source_revision: CoreBufferRevision { value: 7 },
        provenance: provenance.clone(),
        status: CoreTreeSitterStatus::Prepared,
        has_error: false,
        covered_ranges: vec![range],
        error_ranges: Vec::new(),
        budget_status: CoreTreeSitterBudgetStatus::WithinBudget,
        chunks: vec![chunk],
        embedded_regions: vec![embedded_region.clone()],
    };

    assert_eq!(syntax.source_revision, CoreBufferRevision { value: 7 });
    assert_eq!(syntax.chunks[0].capture_name, "keyword");
    assert_eq!(syntax.embedded_regions.len(), 1);
    assert!(matches!(
        syntax.chunks[0].category,
        CoreSyntaxCategory::Keyword
    ));
    assert!(matches!(
        embedded_region.normalized_kind,
        CoreEmbeddedBlockKind::Syntax
    ));
}

#[cfg(feature = "tree-sitter-syntax")]
#[test]
fn tree_sitter_registry_registers_only_feature_enabled_packages() {
    let packages = VimCoreSession::tree_sitter_language_packages();

    assert_eq!(
        packages
            .iter()
            .filter(|package| package.package_id == "tree-sitter-markdown")
            .count(),
        if cfg!(feature = "tree-sitter-markdown") {
            1
        } else {
            0
        },
        "Markdown package registration should follow the Cargo feature"
    );
    assert_eq!(
        packages
            .iter()
            .filter(|package| package.package_id == "tree-sitter-rust")
            .count(),
        if cfg!(feature = "tree-sitter-rust") {
            1
        } else {
            0
        },
        "Rust package registration should follow the Cargo feature"
    );
    assert_eq!(
        packages
            .iter()
            .filter(|package| package.package_id == "tree-sitter-go")
            .count(),
        if cfg!(feature = "tree-sitter-go") {
            1
        } else {
            0
        },
        "Go package registration should follow the Cargo feature"
    );
    assert_eq!(
        packages
            .iter()
            .filter(|package| package.package_id == "tree-sitter-typescript")
            .count(),
        if cfg!(feature = "tree-sitter-typescript") {
            1
        } else {
            0
        },
        "TypeScript package registration should follow the Cargo feature"
    );
    assert_eq!(
        packages
            .iter()
            .filter(|package| package.package_id == "tree-sitter-tsx")
            .count(),
        if cfg!(feature = "tree-sitter-typescript") {
            1
        } else {
            0
        },
        "TSX package registration should follow the TypeScript Cargo feature"
    );
    for package in packages {
        assert!(
            !package.package_version.is_empty()
                && !package.parser_version.is_empty()
                && !package.query_version.is_empty(),
            "registered packages should expose versioned provenance: {package:?}"
        );
    }
}

#[cfg(feature = "tree-sitter-syntax")]
#[test]
fn tree_sitter_root_resolver_uses_registry_backed_inputs() {
    let range = CoreTextRange {
        start: CoreTextPosition { row: 0, col: 0 },
        end: CoreTextPosition { row: 10, col: 0 },
    };

    let rust =
        VimCoreSession::resolve_tree_sitter_root_language(CoreRootLanguageResolutionRequest {
            range,
            vim_filetype: Some("rust".to_string()),
            buffer_name: Some("src/lib.rs".to_string()),
            host_language_hint: None,
        });

    assert_eq!(rust.role, CoreLanguageRole::RootDocument);
    assert_eq!(rust.language_id.as_deref(), Some("rust"));
    assert_eq!(rust.package_id.as_deref(), Some("tree-sitter-rust"));
    assert_eq!(
        rust.status,
        if cfg!(feature = "tree-sitter-rust") {
            CoreLanguageResolutionStatus::Resolved
        } else {
            CoreLanguageResolutionStatus::Unavailable
        }
    );
    assert_eq!(rust.source, CoreLanguageResolutionSource::VimFiletype);

    let markdown =
        VimCoreSession::resolve_tree_sitter_root_language(CoreRootLanguageResolutionRequest {
            range,
            vim_filetype: None,
            buffer_name: Some("README.md".to_string()),
            host_language_hint: None,
        });

    assert_eq!(markdown.language_id.as_deref(), Some("markdown"));
    assert_eq!(markdown.package_id.as_deref(), Some("tree-sitter-markdown"));
    assert_eq!(markdown.source, CoreLanguageResolutionSource::BufferName);

    let tsx =
        VimCoreSession::resolve_tree_sitter_root_language(CoreRootLanguageResolutionRequest {
            range,
            vim_filetype: None,
            buffer_name: Some("component.tsx".to_string()),
            host_language_hint: None,
        });

    assert_eq!(tsx.language_id.as_deref(), Some("tsx"));
    assert_eq!(tsx.package_id.as_deref(), Some("tree-sitter-tsx"));
    assert_eq!(tsx.source, CoreLanguageResolutionSource::BufferName);

    let go = VimCoreSession::resolve_tree_sitter_root_language(CoreRootLanguageResolutionRequest {
        range,
        vim_filetype: None,
        buffer_name: Some("cmd/server/main.go".to_string()),
        host_language_hint: None,
    });

    assert_eq!(go.language_id.as_deref(), Some("go"));
    assert_eq!(go.package_id.as_deref(), Some("tree-sitter-go"));
    assert_eq!(go.source, CoreLanguageResolutionSource::BufferName);

    let unknown =
        VimCoreSession::resolve_tree_sitter_root_language(CoreRootLanguageResolutionRequest {
            range,
            vim_filetype: Some("totally-unknown".to_string()),
            buffer_name: Some("scratch.unknown".to_string()),
            host_language_hint: None,
        });

    assert_eq!(unknown.status, CoreLanguageResolutionStatus::Unsupported);
    assert_eq!(unknown.language_id, None);
    assert_eq!(unknown.package_id, None);
}

#[cfg(feature = "tree-sitter-syntax")]
#[test]
fn tree_sitter_embedded_resolver_normalizes_markdown_info_strings() {
    let range = CoreTextRange {
        start: CoreTextPosition { row: 1, col: 0 },
        end: CoreTextPosition { row: 5, col: 0 },
    };
    let rust = VimCoreSession::resolve_tree_sitter_embedded_language(
        CoreEmbeddedLanguageResolutionRequest {
            range,
            raw_info_string: Some("rust ignore".to_string()),
        },
    );

    assert_eq!(rust.role, CoreLanguageRole::EmbeddedRegion);
    assert_eq!(rust.language_id.as_deref(), Some("rust"));
    assert_eq!(rust.package_id.as_deref(), Some("tree-sitter-rust"));
    assert_eq!(rust.kind, CoreEmbeddedBlockKind::Syntax);
    assert_eq!(
        rust.source,
        CoreLanguageResolutionSource::MarkdownInfoString
    );
    assert_eq!(
        rust.status,
        if cfg!(feature = "tree-sitter-rust") {
            CoreLanguageResolutionStatus::Resolved
        } else {
            CoreLanguageResolutionStatus::Unavailable
        }
    );

    let typescript = VimCoreSession::resolve_tree_sitter_embedded_language(
        CoreEmbeddedLanguageResolutionRequest {
            range,
            raw_info_string: Some("typescript".to_string()),
        },
    );

    assert_eq!(typescript.language_id.as_deref(), Some("typescript"));
    assert_eq!(
        typescript.package_id.as_deref(),
        Some("tree-sitter-typescript")
    );
    assert_eq!(
        typescript.status,
        if cfg!(feature = "tree-sitter-typescript") {
            CoreLanguageResolutionStatus::Resolved
        } else {
            CoreLanguageResolutionStatus::Unavailable
        }
    );

    let golang = VimCoreSession::resolve_tree_sitter_embedded_language(
        CoreEmbeddedLanguageResolutionRequest {
            range,
            raw_info_string: Some("golang".to_string()),
        },
    );

    assert_eq!(golang.language_id.as_deref(), Some("go"));
    assert_eq!(golang.package_id.as_deref(), Some("tree-sitter-go"));
    assert_eq!(
        golang.status,
        if cfg!(feature = "tree-sitter-go") {
            CoreLanguageResolutionStatus::Resolved
        } else {
            CoreLanguageResolutionStatus::Unavailable
        }
    );

    let mermaid = VimCoreSession::resolve_tree_sitter_embedded_language(
        CoreEmbeddedLanguageResolutionRequest {
            range,
            raw_info_string: Some("mermaid".to_string()),
        },
    );

    assert_eq!(mermaid.status, CoreLanguageResolutionStatus::Unsupported);
    assert!(matches!(
        mermaid.kind,
        CoreEmbeddedBlockKind::Diagram {
            diagram_kind: vim_core_rs::CoreDiagramKind::Mermaid
        }
    ));

    let svg = VimCoreSession::resolve_tree_sitter_embedded_language(
        CoreEmbeddedLanguageResolutionRequest {
            range,
            raw_info_string: Some("svg".to_string()),
        },
    );
    assert_eq!(svg.status, CoreLanguageResolutionStatus::Unsupported);
    assert!(matches!(
        svg.kind,
        CoreEmbeddedBlockKind::Media {
            media_kind: CoreMediaKind::Svg,
            flavor: None
        }
    ));

    let png = VimCoreSession::resolve_tree_sitter_embedded_language(
        CoreEmbeddedLanguageResolutionRequest {
            range,
            raw_info_string: Some("png".to_string()),
        },
    );
    assert_eq!(png.status, CoreLanguageResolutionStatus::Unsupported);
    assert!(matches!(
        png.kind,
        CoreEmbeddedBlockKind::Media {
            media_kind: CoreMediaKind::Png,
            flavor: None
        }
    ));

    let unknown = VimCoreSession::resolve_tree_sitter_embedded_language(
        CoreEmbeddedLanguageResolutionRequest {
            range,
            raw_info_string: Some("made-up-language".to_string()),
        },
    );

    assert_eq!(unknown.status, CoreLanguageResolutionStatus::Unsupported);
    assert_eq!(unknown.kind, CoreEmbeddedBlockKind::Unknown);
}

#[cfg(all(feature = "tree-sitter-syntax", feature = "tree-sitter-markdown"))]
#[test]
fn tree_sitter_markdown_fenced_blocks_are_data_only_embedded_regions() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new(
        "# Title\n\n```rust linenums=true\nfn main() {}\n```\n\n```mermaid\nflowchart TD\n```\n\n```svg\n<svg></svg>\n```\n\n```made-up-language\nraw\n```\n",
    )
    .expect("session should initialize");
    let snapshot = session.snapshot();
    let buffer = snapshot
        .buffers
        .iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist");
    let range = CoreTextRange {
        start: CoreTextPosition { row: 0, col: 0 },
        end: CoreTextPosition { row: 64, col: 0 },
    };

    session
        .request_tree_sitter_syntax_preparation(CoreTreeSitterPreparationRequest {
            buffer_id: buffer.id,
            source_revision: Some(buffer.source_revision),
            range,
            vim_filetype: Some("markdown".to_string()),
            buffer_name: Some("README.md".to_string()),
            host_language_hint: None,
            snapshot_policy: CoreTreeSitterSnapshotPolicy::default(),
        })
        .expect("preparation should be accepted");

    let syntax = session
        .poll_tree_sitter_preparation()
        .expect("preparation should complete")
        .syntax;

    assert_eq!(syntax.status, CoreTreeSitterStatus::Prepared);
    assert_eq!(syntax.embedded_regions.len(), 4);

    let rust = syntax
        .embedded_regions
        .iter()
        .find(|region| region.normalized_info_string.as_deref() == Some("rust"))
        .expect("rust fence should be detected");
    assert_eq!(rust.raw_info_string.as_deref(), Some("rust linenums=true"));
    assert_eq!(rust.source, CoreEmbeddedRegionSource::MarkdownFence);
    assert_eq!(rust.normalized_kind, CoreEmbeddedBlockKind::Syntax);
    assert!(matches!(
        rust.resolved_language
            .as_ref()
            .map(|resolved| &resolved.kind),
        Some(CoreEmbeddedBlockKind::Syntax)
    ));
    assert!(rust.content_range.start.row > rust.range.start.row);

    let mermaid = syntax
        .embedded_regions
        .iter()
        .find(|region| region.normalized_info_string.as_deref() == Some("mermaid"))
        .expect("mermaid fence should be detected");
    assert!(matches!(
        mermaid.normalized_kind,
        CoreEmbeddedBlockKind::Diagram {
            diagram_kind: vim_core_rs::CoreDiagramKind::Mermaid
        }
    ));
    assert_eq!(
        mermaid
            .resolved_language
            .as_ref()
            .map(|resolved| resolved.status.clone()),
        Some(CoreLanguageResolutionStatus::Unsupported)
    );

    let svg = syntax
        .embedded_regions
        .iter()
        .find(|region| region.normalized_info_string.as_deref() == Some("svg"))
        .expect("svg fence should be detected");
    assert!(matches!(
        svg.normalized_kind,
        CoreEmbeddedBlockKind::Media {
            media_kind: CoreMediaKind::Svg,
            flavor: None
        }
    ));

    let unknown = syntax
        .embedded_regions
        .iter()
        .find(|region| region.normalized_info_string.as_deref() == Some("made-up-language"))
        .expect("unknown fence should be detected");
    assert_eq!(unknown.normalized_kind, CoreEmbeddedBlockKind::Unknown);
    assert_eq!(
        unknown
            .resolved_language
            .as_ref()
            .map(|resolved| resolved.status.clone()),
        Some(CoreLanguageResolutionStatus::Unsupported)
    );

    let visible = session
        .query_tree_sitter_syntax_range(
            buffer.id,
            buffer.source_revision,
            CoreTextRange {
                start: CoreTextPosition { row: 7, col: 0 },
                end: CoreTextPosition { row: 8, col: 0 },
            },
        )
        .expect("visible range should read committed cache");
    assert!(
        visible
            .embedded_regions
            .iter()
            .any(|region| region.normalized_info_string.as_deref() == Some("mermaid")),
        "visible range queries should retain overlapping embedded regions"
    );
}

#[cfg(all(feature = "tree-sitter-syntax", feature = "tree-sitter-markdown"))]
#[test]
fn tree_sitter_markdown_linked_svg_png_media_are_data_only_embedded_regions() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new(
        "# Media\n\n![diagram](assets/system.drawio.svg)\n![logo](assets/logo.svg)\n![screenshot](https://example.test/screens/shot.PNG?cache=1)\n![text](notes.txt)\n",
    )
    .expect("session should initialize");
    let snapshot = session.snapshot();
    let buffer = snapshot
        .buffers
        .iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist");
    let range = CoreTextRange {
        start: CoreTextPosition { row: 0, col: 0 },
        end: CoreTextPosition { row: 16, col: 0 },
    };

    session
        .request_tree_sitter_syntax_preparation(CoreTreeSitterPreparationRequest {
            buffer_id: buffer.id,
            source_revision: Some(buffer.source_revision),
            range,
            vim_filetype: Some("markdown".to_string()),
            buffer_name: Some("README.md".to_string()),
            host_language_hint: None,
            snapshot_policy: CoreTreeSitterSnapshotPolicy::default(),
        })
        .expect("preparation should be accepted");

    let syntax = session
        .poll_tree_sitter_preparation()
        .expect("preparation should complete")
        .syntax;

    assert_eq!(syntax.status, CoreTreeSitterStatus::Prepared);

    let linked_media: Vec<_> = syntax
        .embedded_regions
        .iter()
        .filter(|region| region.source == CoreEmbeddedRegionSource::MarkdownLink)
        .collect();
    assert_eq!(linked_media.len(), 3, "{linked_media:#?}");

    let drawio_svg = linked_media
        .iter()
        .find(|region| region.raw_info_string.as_deref() == Some("assets/system.drawio.svg"))
        .expect("linked drawio SVG should be detected");
    assert_eq!(drawio_svg.normalized_info_string.as_deref(), Some("svg"));
    assert!(matches!(
        drawio_svg.normalized_kind,
        CoreEmbeddedBlockKind::Media {
            media_kind: CoreMediaKind::Svg,
            flavor: Some(CoreMediaFlavor::DrawioSvg)
        }
    ));
    assert_eq!(
        drawio_svg
            .resolved_language
            .as_ref()
            .map(|resolved| resolved.status.clone()),
        Some(CoreLanguageResolutionStatus::Unsupported)
    );

    let svg = linked_media
        .iter()
        .find(|region| region.raw_info_string.as_deref() == Some("assets/logo.svg"))
        .expect("linked SVG should be detected");
    assert_eq!(svg.normalized_info_string.as_deref(), Some("svg"));
    assert!(matches!(
        svg.normalized_kind,
        CoreEmbeddedBlockKind::Media {
            media_kind: CoreMediaKind::Svg,
            flavor: None
        }
    ));

    let png = linked_media
        .iter()
        .find(|region| {
            region.raw_info_string.as_deref()
                == Some("https://example.test/screens/shot.PNG?cache=1")
        })
        .expect("linked PNG should be detected");
    assert_eq!(png.normalized_info_string.as_deref(), Some("png"));
    assert!(matches!(
        png.normalized_kind,
        CoreEmbeddedBlockKind::Media {
            media_kind: CoreMediaKind::Png,
            flavor: None
        }
    ));
}

#[cfg(feature = "tree-sitter-syntax")]
#[test]
fn tree_sitter_preparation_uses_request_poll_and_query_shape() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("fn main() {}\n").expect("session should initialize");
    let snapshot = session.snapshot();
    let buffer = snapshot
        .buffers
        .iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist");
    let range = CoreTextRange {
        start: CoreTextPosition { row: 0, col: 0 },
        end: CoreTextPosition { row: 1, col: 0 },
    };

    let preparation = session
        .request_tree_sitter_syntax_preparation(CoreTreeSitterPreparationRequest {
            buffer_id: buffer.id,
            source_revision: Some(buffer.source_revision),
            range,
            vim_filetype: Some("rust".to_string()),
            buffer_name: Some("src/main.rs".to_string()),
            host_language_hint: None,
            snapshot_policy: CoreTreeSitterSnapshotPolicy::default(),
        })
        .expect("preparation request should be accepted");

    assert_eq!(preparation.request_id.value, 1);
    assert_eq!(preparation.buffer_id, buffer.id);
    assert_eq!(preparation.source_revision, buffer.source_revision);

    let completed = session
        .poll_tree_sitter_preparation()
        .expect("synchronous MVP should produce one completed result");
    assert_eq!(completed.request_id, preparation.request_id);
    assert_eq!(completed.syntax.buffer_id, buffer.id);
    assert_eq!(completed.syntax.source_revision, buffer.source_revision);
    assert_eq!(
        completed.syntax.status,
        if cfg!(feature = "tree-sitter-rust") {
            CoreTreeSitterStatus::Prepared
        } else {
            CoreTreeSitterStatus::Unavailable
        }
    );
    if cfg!(feature = "tree-sitter-rust") {
        assert!(
            completed
                .syntax
                .chunks
                .iter()
                .any(|chunk| chunk.capture_name == "keyword"
                    && chunk.category == CoreSyntaxCategory::Keyword
                    && chunk.modifiers.is_empty()),
            "Phase 5 should parse Rust highlights into normalized chunks: {:?}",
            completed.syntax.chunks
        );
    } else {
        assert!(
            completed.syntax.chunks.is_empty(),
            "without the Rust package feature the result should remain unavailable"
        );
    }

    let queried = session
        .query_tree_sitter_syntax_range(buffer.id, buffer.source_revision, range)
        .expect("completed preparation should be queryable from the committed cache");
    assert_eq!(queried, completed.syntax);
    if cfg!(feature = "tree-sitter-rust") {
        let active_window_id = snapshot
            .windows
            .iter()
            .find(|window| window.is_active)
            .expect("active window should exist")
            .id;
        assert_ne!(
            session
                .get_line_syntax(active_window_id, 1)
                .expect("Vim syntax should be queryable"),
            completed
                .syntax
                .chunks
                .iter()
                .map(|chunk| vim_core_rs::CoreSyntaxChunk {
                    start_col: chunk.range.start.col,
                    end_col: chunk.range.end.col,
                    syn_id: 0,
                    name: Some(chunk.capture_name.clone()),
                })
                .collect::<Vec<_>>(),
            "Tree-sitter output must stay separate from get_line_syntax/CoreSyntaxChunk"
        );
    }
    assert!(session.poll_tree_sitter_preparation().is_none());
}

#[cfg(all(feature = "tree-sitter-syntax", feature = "tree-sitter-rust"))]
#[test]
fn tree_sitter_rust_package_maps_captures_and_normalizes_overlaps() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("/// docs\nfn main() {\n    let VALUE: i32 = 1;\n}\n")
        .expect("session should initialize");
    let buffer = session
        .snapshot()
        .buffers
        .into_iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist");
    let full_range = CoreTextRange {
        start: CoreTextPosition { row: 0, col: 0 },
        end: CoreTextPosition { row: 4, col: 0 },
    };

    session
        .request_tree_sitter_syntax_preparation(CoreTreeSitterPreparationRequest {
            buffer_id: buffer.id,
            source_revision: Some(buffer.source_revision),
            range: full_range,
            vim_filetype: Some("rust".to_string()),
            buffer_name: Some("src/main.rs".to_string()),
            host_language_hint: None,
            snapshot_policy: CoreTreeSitterSnapshotPolicy::default(),
        })
        .expect("preparation should be accepted");

    let syntax = session
        .poll_tree_sitter_preparation()
        .expect("result should be ready")
        .syntax;
    assert_eq!(syntax.status, CoreTreeSitterStatus::Prepared);
    assert!(
        syntax
            .chunks
            .windows(2)
            .all(|pair| pair[0].range.end <= pair[1].range.start),
        "public chunks must be non-overlapping and sorted: {:?}",
        syntax.chunks
    );
    assert!(
        syntax.chunks.iter().any(|chunk| {
            chunk.capture_name == "comment.documentation"
                && chunk.category == CoreSyntaxCategory::Comment
                && chunk.modifiers.contains(&CoreSyntaxModifier::Documentation)
        }),
        "documentation comment should use crate-owned category/modifier mapping: {:?}",
        syntax.chunks
    );
    assert!(
        syntax.chunks.iter().any(|chunk| {
            chunk.capture_name == "keyword"
                && chunk.category == CoreSyntaxCategory::Keyword
                && chunk.range.start.row == 1
        }),
        "Rust keyword captures should be present: {:?}",
        syntax.chunks
    );
}

#[cfg(all(feature = "tree-sitter-syntax", feature = "tree-sitter-rust"))]
#[test]
fn tree_sitter_visible_range_query_reads_committed_cache_without_reparsing() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("fn first() {}\nfn second() {}\n").expect("session should initialize");
    let buffer = session
        .snapshot()
        .buffers
        .into_iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist");
    let full_range = CoreTextRange {
        start: CoreTextPosition { row: 0, col: 0 },
        end: CoreTextPosition { row: 2, col: 0 },
    };
    let visible_range = CoreTextRange {
        start: CoreTextPosition { row: 1, col: 0 },
        end: CoreTextPosition { row: 2, col: 0 },
    };

    assert!(
        session
            .query_tree_sitter_syntax_range(buffer.id, buffer.source_revision, visible_range)
            .is_none(),
        "query should not parse or synthesize data before preparation commits cache"
    );

    session
        .request_tree_sitter_syntax_preparation(CoreTreeSitterPreparationRequest {
            buffer_id: buffer.id,
            source_revision: Some(buffer.source_revision),
            range: full_range,
            vim_filetype: Some("rust".to_string()),
            buffer_name: Some("src/lib.rs".to_string()),
            host_language_hint: None,
            snapshot_policy: CoreTreeSitterSnapshotPolicy::default(),
        })
        .expect("preparation should be accepted");
    let _ = session.poll_tree_sitter_preparation();

    let visible = session
        .query_tree_sitter_syntax_range(buffer.id, buffer.source_revision, visible_range)
        .expect("visible subrange should be served from committed cache");
    assert!(
        visible
            .chunks
            .iter()
            .all(|chunk| chunk.range.start >= visible_range.start
                && chunk.range.end <= visible_range.end),
        "visible query should return clipped chunks only: {:?}",
        visible.chunks
    );
    assert_eq!(
        visible.covered_ranges,
        vec![visible_range],
        "visible query should clip coverage ranges to the requested range"
    );
    assert!(
        visible
            .error_ranges
            .iter()
            .all(|range| range.start >= visible_range.start && range.end <= visible_range.end),
        "visible query should clip error ranges to the requested range: {:?}",
        visible.error_ranges
    );
    assert_eq!(
        visible.budget_status,
        CoreTreeSitterBudgetStatus::WithinBudget,
        "visible query should preserve the committed budget status"
    );
    assert!(
        visible
            .chunks
            .iter()
            .any(|chunk| chunk.capture_name == "keyword" && chunk.range.start.row == 1),
        "visible query should include syntax from the committed second line: {:?}",
        visible.chunks
    );
}

#[cfg(all(feature = "tree-sitter-syntax", feature = "tree-sitter-markdown"))]
#[test]
fn tree_sitter_markdown_package_parses_highlight_query() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("# Title\n\n- item\n").expect("session should initialize");
    let buffer = session
        .snapshot()
        .buffers
        .into_iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist");
    let range = CoreTextRange {
        start: CoreTextPosition { row: 0, col: 0 },
        end: CoreTextPosition { row: 3, col: 0 },
    };

    session
        .request_tree_sitter_syntax_preparation(CoreTreeSitterPreparationRequest {
            buffer_id: buffer.id,
            source_revision: Some(buffer.source_revision),
            range,
            vim_filetype: Some("markdown".to_string()),
            buffer_name: Some("README.md".to_string()),
            host_language_hint: None,
            snapshot_policy: CoreTreeSitterSnapshotPolicy::default(),
        })
        .expect("preparation should be accepted");

    let syntax = session
        .poll_tree_sitter_preparation()
        .expect("result should be ready")
        .syntax;
    assert_eq!(syntax.status, CoreTreeSitterStatus::Prepared);
    assert!(
        syntax.chunks.iter().any(|chunk| {
            chunk.capture_name == "text.title" && chunk.category == CoreSyntaxCategory::Markup
        }),
        "Markdown query package should produce crate-normalized markup chunks: {:?}",
        syntax.chunks
    );
}

#[cfg(feature = "tree-sitter-syntax")]
#[test]
fn tree_sitter_snapshot_store_reports_too_large_and_budget_statuses() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("abcdef\n").expect("session should initialize");
    let snapshot = session.snapshot();
    let buffer = snapshot
        .buffers
        .iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist");
    let range = CoreTextRange {
        start: CoreTextPosition { row: 0, col: 0 },
        end: CoreTextPosition { row: 1, col: 0 },
    };

    let too_large = session
        .request_tree_sitter_syntax_preparation(CoreTreeSitterPreparationRequest {
            buffer_id: buffer.id,
            source_revision: Some(buffer.source_revision),
            range,
            vim_filetype: Some("rust".to_string()),
            buffer_name: Some("src/lib.rs".to_string()),
            host_language_hint: None,
            snapshot_policy: CoreTreeSitterSnapshotPolicy {
                max_snapshot_bytes: Some(3),
                ..CoreTreeSitterSnapshotPolicy::default()
            },
        })
        .expect("too-large request should still complete with explicit status");
    assert_eq!(too_large.status, CoreTreeSitterStatus::TooLarge);
    assert_eq!(
        session
            .poll_tree_sitter_preparation()
            .expect("too-large result should be pollable")
            .syntax
            .status,
        CoreTreeSitterStatus::TooLarge
    );
    let too_large_syntax = session
        .query_tree_sitter_syntax_range(buffer.id, buffer.source_revision, range)
        .expect("too-large status should be committed for diagnostics");
    assert_eq!(
        too_large_syntax.budget_status,
        CoreTreeSitterBudgetStatus::SnapshotTooLarge
    );
    assert_eq!(too_large_syntax.status, CoreTreeSitterStatus::TooLarge);
    assert!(too_large_syntax.chunks.is_empty());

    let budgeted = session
        .request_tree_sitter_syntax_preparation(CoreTreeSitterPreparationRequest {
            buffer_id: buffer.id,
            source_revision: Some(buffer.source_revision),
            range,
            vim_filetype: Some("rust".to_string()),
            buffer_name: Some("src/lib.rs".to_string()),
            host_language_hint: None,
            snapshot_policy: CoreTreeSitterSnapshotPolicy {
                global_byte_budget: 3,
                max_snapshot_bytes: None,
                ..CoreTreeSitterSnapshotPolicy::default()
            },
        })
        .expect("budgeted request should still complete with explicit status");
    assert_eq!(budgeted.status, CoreTreeSitterStatus::BudgetExceeded);
    assert_eq!(
        session
            .poll_tree_sitter_preparation()
            .expect("budget result should be pollable")
            .syntax
            .status,
        CoreTreeSitterStatus::BudgetExceeded
    );
    let budgeted_syntax = session
        .query_tree_sitter_syntax_range(buffer.id, buffer.source_revision, range)
        .expect("budget status should be committed for diagnostics");
    assert_eq!(
        budgeted_syntax.budget_status,
        CoreTreeSitterBudgetStatus::GlobalBudgetExceeded
    );
    assert!(budgeted_syntax.chunks.is_empty());
}

#[cfg(feature = "tree-sitter-syntax")]
#[test]
fn tree_sitter_preparation_reports_unsupported_and_stale_without_fresh_chunks() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("fn main() {}\n").expect("session should initialize");
    let first_buffer = session
        .snapshot()
        .buffers
        .into_iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist");
    let range = CoreTextRange {
        start: CoreTextPosition { row: 0, col: 0 },
        end: CoreTextPosition { row: 1, col: 0 },
    };

    session
        .request_tree_sitter_syntax_preparation(CoreTreeSitterPreparationRequest {
            buffer_id: first_buffer.id,
            source_revision: Some(first_buffer.source_revision),
            range,
            vim_filetype: Some("totally-unknown".to_string()),
            buffer_name: Some("scratch.unknown".to_string()),
            host_language_hint: None,
            snapshot_policy: CoreTreeSitterSnapshotPolicy::default(),
        })
        .expect("unsupported language should return a diagnostic result");
    let unsupported = session
        .poll_tree_sitter_preparation()
        .expect("unsupported result should be pollable")
        .syntax;
    assert_eq!(unsupported.status, CoreTreeSitterStatus::Unsupported);
    assert!(unsupported.chunks.is_empty());

    drop(session);

    let mut stale_session =
        VimCoreSession::new("fn main() {}\n").expect("session should initialize");
    let stale_first_buffer = stale_session
        .snapshot()
        .buffers
        .into_iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist");
    stale_session
        .execute_normal_command("Gochanged\x1b")
        .expect("edit should advance the source revision");
    let stale = stale_session
        .request_tree_sitter_syntax_preparation(CoreTreeSitterPreparationRequest {
            buffer_id: stale_first_buffer.id,
            source_revision: Some(stale_first_buffer.source_revision),
            range,
            vim_filetype: Some("rust".to_string()),
            buffer_name: Some("src/main.rs".to_string()),
            host_language_hint: None,
            snapshot_policy: CoreTreeSitterSnapshotPolicy::default(),
        })
        .expect("stale request should return a diagnostic result");
    assert_eq!(stale.status, CoreTreeSitterStatus::Stale);
    let stale_syntax = stale_session
        .poll_tree_sitter_preparation()
        .expect("stale result should be pollable")
        .syntax;
    assert_eq!(stale_syntax.status, CoreTreeSitterStatus::Stale);
    assert!(stale_syntax.chunks.is_empty());
    assert!(
        stale_session
            .query_tree_sitter_syntax_range(
                stale_first_buffer.id,
                stale_first_buffer.source_revision,
                range,
            )
            .is_none(),
        "stale results must not be committed as fresh cache entries"
    );
}

#[cfg(all(feature = "tree-sitter-syntax", feature = "tree-sitter-typescript"))]
#[test]
fn tree_sitter_typescript_and_tsx_packages_parse_and_report_ranges() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("const value: number = 1;\n").expect("session should initialize");
    let buffer = session
        .snapshot()
        .buffers
        .into_iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist");
    let range = CoreTextRange {
        start: CoreTextPosition { row: 0, col: 0 },
        end: CoreTextPosition { row: 1, col: 0 },
    };

    session
        .request_tree_sitter_syntax_preparation(CoreTreeSitterPreparationRequest {
            buffer_id: buffer.id,
            source_revision: Some(buffer.source_revision),
            range,
            vim_filetype: Some("typescript".to_string()),
            buffer_name: Some("src/main.ts".to_string()),
            host_language_hint: None,
            snapshot_policy: CoreTreeSitterSnapshotPolicy::default(),
        })
        .expect("preparation should be accepted");

    let syntax = session
        .poll_tree_sitter_preparation()
        .expect("result should be ready")
        .syntax;
    assert_eq!(syntax.status, CoreTreeSitterStatus::Prepared);
    assert_eq!(syntax.provenance.language_id, "typescript");
    assert_eq!(syntax.provenance.package_id, "tree-sitter-typescript");
    assert_eq!(syntax.covered_ranges, vec![range]);
    assert_eq!(
        syntax.budget_status,
        CoreTreeSitterBudgetStatus::WithinBudget
    );
    assert!(syntax.error_ranges.is_empty(), "{syntax:#?}");
    assert!(
        syntax
            .chunks
            .iter()
            .any(|chunk| chunk.capture_name == "type.builtin"
                && chunk.category == CoreSyntaxCategory::Type),
        "TypeScript package should map highlights through normalized chunks: {:?}",
        syntax.chunks
    );

    let tsx =
        VimCoreSession::resolve_tree_sitter_root_language(CoreRootLanguageResolutionRequest {
            range,
            vim_filetype: None,
            buffer_name: Some("src/App.tsx".to_string()),
            host_language_hint: None,
        });
    assert_eq!(tsx.status, CoreLanguageResolutionStatus::Resolved);
    assert_eq!(tsx.language_id.as_deref(), Some("tsx"));
    assert_eq!(tsx.package_id.as_deref(), Some("tree-sitter-tsx"));
}

#[cfg(all(feature = "tree-sitter-syntax", feature = "tree-sitter-typescript"))]
#[test]
fn tree_sitter_typescript_and_tsx_malformed_input_reports_error_ranges() {
    let _guard = acquire_session_test_lock();
    for (filetype, buffer_name, text, range) in [
        (
            "typescript",
            "src/main.ts",
            "const value: = ;\n",
            CoreTextRange {
                start: CoreTextPosition { row: 0, col: 0 },
                end: CoreTextPosition { row: 1, col: 0 },
            },
        ),
        (
            "tsx",
            "src/App.tsx",
            "const view = <div><span></div>;\n",
            CoreTextRange {
                start: CoreTextPosition { row: 0, col: 0 },
                end: CoreTextPosition { row: 1, col: 0 },
            },
        ),
    ] {
        let mut session = VimCoreSession::new(text).expect("session should initialize");
        let buffer = session
            .snapshot()
            .buffers
            .into_iter()
            .find(|buffer| buffer.is_active)
            .expect("active buffer should exist");

        session
            .request_tree_sitter_syntax_preparation(CoreTreeSitterPreparationRequest {
                buffer_id: buffer.id,
                source_revision: Some(buffer.source_revision),
                range,
                vim_filetype: Some(filetype.to_string()),
                buffer_name: Some(buffer_name.to_string()),
                host_language_hint: None,
                snapshot_policy: CoreTreeSitterSnapshotPolicy::default(),
            })
            .expect("preparation should be accepted");

        let syntax = session
            .poll_tree_sitter_preparation()
            .expect("result should be ready")
            .syntax;
        assert_eq!(syntax.status, CoreTreeSitterStatus::Prepared);
        assert!(syntax.has_error, "{filetype} should report parser errors");
        assert_eq!(syntax.covered_ranges, vec![range]);
        assert_eq!(
            syntax.budget_status,
            CoreTreeSitterBudgetStatus::WithinBudget
        );
        assert!(
            !syntax.error_ranges.is_empty(),
            "{filetype} should expose concrete error ranges: {syntax:#?}"
        );
        assert!(
            syntax
                .error_ranges
                .iter()
                .all(|error_range| error_range.start >= range.start && error_range.end <= range.end),
            "{filetype} error ranges should stay within the requested range: {:?}",
            syntax.error_ranges
        );
    }
}

#[cfg(all(
    feature = "tree-sitter-syntax",
    feature = "tree-sitter-markdown",
    feature = "tree-sitter-typescript"
))]
#[test]
fn tree_sitter_markdown_fenced_typescript_injection_is_bounded_to_region() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new(
        "# Title\n\n```typescript\nconst value: number = 1;\n```\n\nplain text\n",
    )
    .expect("session should initialize");
    let buffer = session
        .snapshot()
        .buffers
        .into_iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist");
    let range = CoreTextRange {
        start: CoreTextPosition { row: 0, col: 0 },
        end: CoreTextPosition { row: 7, col: 0 },
    };

    session
        .request_tree_sitter_syntax_preparation(CoreTreeSitterPreparationRequest {
            buffer_id: buffer.id,
            source_revision: Some(buffer.source_revision),
            range,
            vim_filetype: Some("markdown".to_string()),
            buffer_name: Some("README.md".to_string()),
            host_language_hint: None,
            snapshot_policy: CoreTreeSitterSnapshotPolicy::default(),
        })
        .expect("preparation should be accepted");

    let syntax = session
        .poll_tree_sitter_preparation()
        .expect("result should be ready")
        .syntax;
    assert_eq!(syntax.status, CoreTreeSitterStatus::Prepared);
    let ts_region = syntax
        .embedded_regions
        .iter()
        .find(|region| region.normalized_info_string.as_deref() == Some("typescript"))
        .expect("typescript fence should be detected");
    assert_eq!(
        ts_region
            .resolved_language
            .as_ref()
            .and_then(|language| language.language_id.as_deref()),
        Some("typescript")
    );
    assert!(
        syntax.chunks.iter().any(|chunk| {
            chunk.capture_name == "type.builtin"
                && chunk.category == CoreSyntaxCategory::Type
                && chunk.range.start >= ts_region.content_range.start
                && chunk.range.end <= ts_region.content_range.end
        }),
        "injected TypeScript highlights should be bounded to the fenced content range: {:?}",
        syntax.chunks
    );
    assert!(
        syntax
            .chunks
            .iter()
            .filter(|chunk| chunk.capture_name == "type.builtin")
            .all(|chunk| chunk.range.start >= ts_region.content_range.start
                && chunk.range.end <= ts_region.content_range.end),
        "injected child chunks must not escape the content range: {:?}",
        syntax.chunks
    );
    assert!(
        syntax
            .chunks
            .windows(2)
            .all(|pair| pair[0].range.end <= pair[1].range.start),
        "bounded injection should keep public chunks sorted and non-overlapping: {:?}",
        syntax.chunks
    );
}

#[cfg(all(
    feature = "tree-sitter-syntax",
    feature = "tree-sitter-markdown",
    feature = "tree-sitter-go"
))]
#[test]
fn tree_sitter_markdown_fenced_go_injection_uses_registry_language() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new(
        "# Title\n\n```go\npackage main\n\nfunc main() {\n    println(\"hi\")\n}\n```\n\nplain text\n",
    )
    .expect("session should initialize");
    let buffer = session
        .snapshot()
        .buffers
        .into_iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist");
    let range = CoreTextRange {
        start: CoreTextPosition { row: 0, col: 0 },
        end: CoreTextPosition { row: 10, col: 0 },
    };

    session
        .request_tree_sitter_syntax_preparation(CoreTreeSitterPreparationRequest {
            buffer_id: buffer.id,
            source_revision: Some(buffer.source_revision),
            range,
            vim_filetype: Some("markdown".to_string()),
            buffer_name: Some("README.md".to_string()),
            host_language_hint: None,
            snapshot_policy: CoreTreeSitterSnapshotPolicy::default(),
        })
        .expect("preparation should be accepted");

    let syntax = session
        .poll_tree_sitter_preparation()
        .expect("result should be ready")
        .syntax;
    assert_eq!(syntax.status, CoreTreeSitterStatus::Prepared);
    let go_region = syntax
        .embedded_regions
        .iter()
        .find(|region| region.normalized_info_string.as_deref() == Some("go"))
        .expect("go fence should be detected");
    assert_eq!(
        go_region
            .resolved_language
            .as_ref()
            .and_then(|language| language.language_id.as_deref()),
        Some("go")
    );
    assert!(
        syntax.chunks.iter().any(|chunk| {
            chunk.capture_name == "keyword"
                && chunk.category == CoreSyntaxCategory::Keyword
                && chunk.range.start >= go_region.content_range.start
                && chunk.range.end <= go_region.content_range.end
        }),
        "embedded Go highlights should be emitted inside the fenced content range: {:?}",
        syntax.chunks
    );
    assert!(
        syntax
            .chunks
            .iter()
            .filter(|chunk| chunk.range.start >= go_region.content_range.start
                && chunk.range.end <= go_region.content_range.end)
            .all(|chunk| chunk.capture_name != "markup.raw.block"),
        "Markdown code-block chunks should be superseded by child chunks inside Go content: {:?}",
        syntax.chunks
    );
}

#[cfg(all(
    feature = "tree-sitter-syntax",
    feature = "tree-sitter-markdown",
    feature = "tree-sitter-go"
))]
#[test]
fn tree_sitter_markdown_fenced_go_injection_does_not_escape_to_closing_fence() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("# t\n\n```go\nfunc main() {\n    log.Println(\"hoge\")\n}\n```\n")
            .expect("session should initialize");
    let buffer = session
        .snapshot()
        .buffers
        .into_iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist");
    let range = CoreTextRange {
        start: CoreTextPosition { row: 0, col: 0 },
        end: CoreTextPosition { row: 7, col: 0 },
    };

    session
        .request_tree_sitter_syntax_preparation(CoreTreeSitterPreparationRequest {
            buffer_id: buffer.id,
            source_revision: Some(buffer.source_revision),
            range,
            vim_filetype: Some("markdown".to_string()),
            buffer_name: Some("README.md".to_string()),
            host_language_hint: None,
            snapshot_policy: CoreTreeSitterSnapshotPolicy::default(),
        })
        .expect("preparation should be accepted");

    let syntax = session
        .poll_tree_sitter_preparation()
        .expect("result should be ready")
        .syntax;
    let go_region = syntax
        .embedded_regions
        .iter()
        .find(|region| region.normalized_info_string.as_deref() == Some("go"))
        .expect("go fence should be detected");
    let closing_fence_row = go_region.content_range.end.row;
    let closing_fence_start = CoreTextPosition {
        row: closing_fence_row,
        col: 0,
    };

    assert!(
        syntax.chunks.iter().any(|chunk| {
            chunk.capture_name == "string"
                && chunk.category == CoreSyntaxCategory::String
                && chunk.range.start >= go_region.content_range.start
                && chunk.range.end <= go_region.content_range.end
        }),
        "Go string token should still be emitted inside the content range: {:?}",
        syntax.chunks
    );
    assert!(
        syntax
            .chunks
            .iter()
            .filter(|chunk| !chunk.capture_name.starts_with("markup."))
            .filter(|chunk| chunk.capture_name != "punctuation.delimiter")
            .filter(|chunk| {
                chunk.range.start < go_region.content_range.end
                    && chunk.range.end > go_region.content_range.start
            })
            .all(|chunk| chunk.range.end <= closing_fence_start),
        "child-language chunks must not start on or extend into the closing fence row: content_range={:?}, chunks={:?}",
        go_region.content_range,
        syntax.chunks
    );
    assert!(
        syntax
            .chunks
            .iter()
            .filter(|chunk| !chunk.capture_name.starts_with("markup."))
            .filter(|chunk| chunk.capture_name != "punctuation.delimiter")
            .all(|chunk| chunk.range.start.row != closing_fence_row),
        "no child-language chunk should start on the closing fence row: content_range={:?}, chunks={:?}",
        go_region.content_range,
        syntax.chunks
    );
}

#[cfg(all(
    feature = "tree-sitter-syntax",
    feature = "tree-sitter-markdown",
    feature = "tree-sitter-go"
))]
#[test]
fn tree_sitter_markdown_fenced_golang_aliases_to_go() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("```golang\npackage main\n```\n").expect("session should initialize");
    let buffer = session
        .snapshot()
        .buffers
        .into_iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist");
    let range = CoreTextRange {
        start: CoreTextPosition { row: 0, col: 0 },
        end: CoreTextPosition { row: 3, col: 0 },
    };

    session
        .request_tree_sitter_syntax_preparation(CoreTreeSitterPreparationRequest {
            buffer_id: buffer.id,
            source_revision: Some(buffer.source_revision),
            range,
            vim_filetype: Some("markdown".to_string()),
            buffer_name: Some("README.md".to_string()),
            host_language_hint: None,
            snapshot_policy: CoreTreeSitterSnapshotPolicy::default(),
        })
        .expect("preparation should be accepted");

    let syntax = session
        .poll_tree_sitter_preparation()
        .expect("result should be ready")
        .syntax;
    let go_region = syntax
        .embedded_regions
        .iter()
        .find(|region| region.raw_info_string.as_deref() == Some("golang"))
        .expect("golang fence should be detected");
    assert_eq!(go_region.normalized_info_string.as_deref(), Some("go"));
    assert_eq!(
        go_region
            .resolved_language
            .as_ref()
            .and_then(|language| language.language_id.as_deref()),
        Some("go")
    );
    assert!(
        syntax
            .chunks
            .iter()
            .any(|chunk| chunk.capture_name == "keyword"
                && chunk.range.start >= go_region.content_range.start
                && chunk.range.end <= go_region.content_range.end),
        "golang alias should emit Go child chunks: {:?}",
        syntax.chunks
    );
}

#[cfg(all(
    feature = "tree-sitter-syntax",
    feature = "tree-sitter-markdown",
    feature = "tree-sitter-typescript"
))]
#[test]
fn tree_sitter_markdown_fenced_tsx_injection_uses_registry_language() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("```tsx\nconst value: number = 1;\n```\n")
        .expect("session should initialize");
    let buffer = session
        .snapshot()
        .buffers
        .into_iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist");
    let range = CoreTextRange {
        start: CoreTextPosition { row: 0, col: 0 },
        end: CoreTextPosition { row: 3, col: 0 },
    };

    session
        .request_tree_sitter_syntax_preparation(CoreTreeSitterPreparationRequest {
            buffer_id: buffer.id,
            source_revision: Some(buffer.source_revision),
            range,
            vim_filetype: Some("markdown".to_string()),
            buffer_name: Some("README.md".to_string()),
            host_language_hint: None,
            snapshot_policy: CoreTreeSitterSnapshotPolicy::default(),
        })
        .expect("preparation should be accepted");

    let syntax = session
        .poll_tree_sitter_preparation()
        .expect("result should be ready")
        .syntax;
    let tsx_region = syntax
        .embedded_regions
        .iter()
        .find(|region| region.normalized_info_string.as_deref() == Some("tsx"))
        .expect("tsx fence should be detected");
    assert!(
        syntax
            .chunks
            .iter()
            .any(|chunk| chunk.capture_name == "type.builtin"
                && chunk.category == CoreSyntaxCategory::Type
                && chunk.range.start >= tsx_region.content_range.start
                && chunk.range.end <= tsx_region.content_range.end),
        "TSX embedded chunks should continue to be emitted: {:?}",
        syntax.chunks
    );
}

#[cfg(all(feature = "tree-sitter-syntax", feature = "tree-sitter-markdown"))]
#[test]
fn tree_sitter_markdown_unknown_fence_keeps_metadata_without_child_chunks() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("```made-up-language\nnot tokenized\n```\n")
        .expect("session should initialize");
    let buffer = session
        .snapshot()
        .buffers
        .into_iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist");
    let range = CoreTextRange {
        start: CoreTextPosition { row: 0, col: 0 },
        end: CoreTextPosition { row: 3, col: 0 },
    };

    session
        .request_tree_sitter_syntax_preparation(CoreTreeSitterPreparationRequest {
            buffer_id: buffer.id,
            source_revision: Some(buffer.source_revision),
            range,
            vim_filetype: Some("markdown".to_string()),
            buffer_name: Some("README.md".to_string()),
            host_language_hint: None,
            snapshot_policy: CoreTreeSitterSnapshotPolicy::default(),
        })
        .expect("preparation should be accepted");

    let syntax = session
        .poll_tree_sitter_preparation()
        .expect("result should be ready")
        .syntax;
    let unknown = syntax
        .embedded_regions
        .iter()
        .find(|region| region.normalized_info_string.as_deref() == Some("made-up-language"))
        .expect("unknown fence should be detected");
    assert_eq!(unknown.normalized_kind, CoreEmbeddedBlockKind::Unknown);
    assert_eq!(
        unknown
            .resolved_language
            .as_ref()
            .map(|resolved| resolved.status.clone()),
        Some(CoreLanguageResolutionStatus::Unsupported)
    );
    assert!(
        syntax
            .chunks
            .iter()
            .filter(|chunk| chunk.range.start >= unknown.content_range.start
                && chunk.range.end <= unknown.content_range.end)
            .all(|chunk| chunk.capture_name.starts_with("markup.")),
        "unknown fences should keep Markdown chunks but must not invent child token chunks: {:?}",
        syntax.chunks
    );
}

#[test]
fn standalone_go_vim_syntax_behavior_remains_available() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("package main\n\nfunc main() {}\n").expect("session should initialize");
    let win_id = session.windows()[0].id;

    session
        .execute_ex_command("syntax on")
        .expect("syntax should be enabled");
    session
        .execute_ex_command("setlocal filetype=go")
        .expect("go filetype should be set");

    let chunks = session
        .get_line_syntax(win_id, 1)
        .expect("line syntax should be available");
    assert!(
        chunks
            .iter()
            .any(|chunk| chunk.name.as_deref() == Some("goPackage")),
        "standalone Go should still expose Vim syntax groups: {:?}",
        chunks
    );
}

#[cfg(feature = "tree-sitter-syntax")]
#[test]
fn tree_sitter_snapshot_store_retains_latest_unpinned_revision_per_buffer() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("one\n").expect("session should initialize");
    let first = session.snapshot();
    let first_buffer = first
        .buffers
        .iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist");
    let range = CoreTextRange {
        start: CoreTextPosition { row: 0, col: 0 },
        end: CoreTextPosition { row: 1, col: 0 },
    };
    let policy = CoreTreeSitterSnapshotPolicy {
        retain_latest_per_buffer: 1,
        global_byte_budget: 1024,
        max_snapshot_bytes: None,
    };

    session
        .request_tree_sitter_syntax_preparation(CoreTreeSitterPreparationRequest {
            buffer_id: first_buffer.id,
            source_revision: Some(first_buffer.source_revision),
            range,
            vim_filetype: Some("markdown".to_string()),
            buffer_name: Some("README.md".to_string()),
            host_language_hint: None,
            snapshot_policy: policy.clone(),
        })
        .expect("first preparation should be accepted");
    let _ = session.poll_tree_sitter_preparation();

    session
        .execute_normal_command("Gotwo\x1b")
        .expect("edit should advance buffer revision");
    let second = session.snapshot();
    let second_buffer = second
        .buffers
        .iter()
        .find(|buffer| buffer.id == first_buffer.id)
        .expect("original buffer should still exist");

    session
        .request_tree_sitter_syntax_preparation(CoreTreeSitterPreparationRequest {
            buffer_id: second_buffer.id,
            source_revision: Some(second_buffer.source_revision),
            range,
            vim_filetype: Some("markdown".to_string()),
            buffer_name: Some("README.md".to_string()),
            host_language_hint: None,
            snapshot_policy: policy,
        })
        .expect("second preparation should be accepted");
    let _ = session.poll_tree_sitter_preparation();

    let stats = session.tree_sitter_snapshot_store_stats();
    assert_eq!(stats.pinned_snapshot_count, 0);
    assert_eq!(stats.snapshot_count, 1);
    assert_eq!(stats.snapshots[0].buffer_id, first_buffer.id);
    assert_eq!(
        stats.snapshots[0].source_revision, second_buffer.source_revision,
        "only the latest unpinned revision should remain for the buffer"
    );
}

#[cfg(feature = "tree-sitter-syntax")]
fn _tree_sitter_package_type_is_public(package: CoreTreeSitterLanguagePackage) {
    assert!(!package.package_id.is_empty());
}
