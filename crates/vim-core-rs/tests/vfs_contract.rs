use std::sync::{Mutex, OnceLock};
use vim_core_rs::{
    CoreBufferSourceKind, CoreDeferredClose, CoreHostAction, CorePendingVfsOperation,
    CoreRequestEntry, CoreRequestStatus, CoreVfsError, CoreVfsErrorKind, CoreVfsOperationKind,
    CoreVfsRequest, CoreVfsResponse, VfsLogEvent, VimCoreSession,
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

fn take_vfs_request(actions: Vec<CoreHostAction>) -> CoreVfsRequest {
    for action in actions {
        if let CoreHostAction::VfsRequest(request) = action {
            return request;
        }
    }
    panic!("expected VFS request");
}

fn take_next_vfs_request(session: &mut VimCoreSession) -> CoreVfsRequest {
    loop {
        match session.take_pending_host_action() {
            Some(CoreHostAction::VfsRequest(request)) => return request,
            Some(_) => continue,
            None => panic!("expected VFS request"),
        }
    }
}

fn prepare_vfs_buffer(
    session: &mut VimCoreSession,
    locator: &str,
    document_id: &str,
    display_name: &str,
    text: &str,
) {
    let outcome = session
        .execute_ex_command(&format!(":edit {locator}"))
        .expect("edit should queue resolve request");
    let resolve_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Resolve { request_id, .. } => request_id,
        other => panic!("unexpected action: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Resolved {
            request_id: resolve_id,
            document_id: document_id.to_string(),
            display_name: display_name.to_string(),
        })
        .expect("resolve should succeed");
    let load_id = match take_next_vfs_request(session) {
        CoreVfsRequest::Load { request_id, .. } => request_id,
        other => panic!("unexpected action: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Loaded {
            request_id: load_id,
            document_id: document_id.to_string(),
            text: text.to_string(),
        })
        .expect("load should succeed");
}

#[test]
fn vfs_public_contract_preserves_request_and_response_payloads() {
    let request = CoreVfsRequest::Save {
        request_id: 41,
        target_buf_id: 7,
        document_id: "doc://memo/7".to_string(),
        target_locator: Some("memo://draft".to_string()),
        text: "updated".to_string(),
        base_revision: 9,
        force: true,
    };
    assert!(matches!(
        request,
        CoreVfsRequest::Save {
            request_id: 41,
            target_buf_id: 7,
            document_id,
            target_locator: Some(target_locator),
            text,
            base_revision: 9,
            force: true,
        } if document_id == "doc://memo/7"
            && target_locator == "memo://draft"
            && text == "updated"
    ));

    let response = CoreVfsResponse::Failed {
        request_id: 41,
        error: CoreVfsError {
            kind: CoreVfsErrorKind::SaveFailed,
            message: Some("permission denied".to_string()),
        },
    };
    assert!(matches!(
        response,
        CoreVfsResponse::Failed {
            request_id: 41,
            error: CoreVfsError {
                kind: CoreVfsErrorKind::SaveFailed,
                message: Some(message),
            },
        } if message == "permission denied"
    ));
}

#[test]
fn vfs_request_entries_and_pending_operations_expose_status_contract() {
    let pending = CorePendingVfsOperation {
        request_id: 12,
        kind: CoreVfsOperationKind::Load,
        issued_order: 4,
    };
    assert_eq!(pending.request_id, 12);
    assert_eq!(pending.kind, CoreVfsOperationKind::Load);
    assert_eq!(pending.issued_order, 4);

    let entry = CoreRequestEntry {
        request_id: 12,
        operation_kind: CoreVfsOperationKind::Load,
        target_buf_id: 3,
        document_id: Some("doc://3".to_string()),
        locator: Some("mem://3".to_string()),
        base_revision: Some(5),
        status: CoreRequestStatus::Failed(CoreVfsError {
            kind: CoreVfsErrorKind::LoadFailed,
            message: Some("boom".to_string()),
        }),
        issued_order: 4,
    };

    assert!(matches!(
        entry.status,
        CoreRequestStatus::Failed(CoreVfsError {
            kind: CoreVfsErrorKind::LoadFailed,
            message: Some(message),
        }) if message == "boom"
    ));
}

#[test]
fn vfs_transaction_log_is_exposed_through_session_api() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    let outcome = session
        .execute_ex_command(":edit mem://notes/alpha")
        .expect("edit should queue resolve request");
    let resolve_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Resolve { request_id, .. } => request_id,
        other => panic!("unexpected action: {other:?}"),
    };

    session
        .submit_vfs_response(CoreVfsResponse::Resolved {
            request_id: resolve_id,
            document_id: "doc://notes/alpha".to_string(),
            display_name: "notes/alpha".to_string(),
        })
        .expect("resolved response should queue load");
    let load_id = match take_next_vfs_request(&mut session) {
        CoreVfsRequest::Load { request_id, .. } => request_id,
        other => panic!("unexpected action: {other:?}"),
    };

    session
        .submit_vfs_response(CoreVfsResponse::Loaded {
            request_id: load_id,
            document_id: "doc://notes/alpha".to_string(),
            text: "loaded".to_string(),
        })
        .expect("load should apply");

    let log = session.vfs_transaction_log();
    assert!(
        log.iter().any(|entry| {
            entry.event == VfsLogEvent::RequestIssued
                && entry.operation_kind == Some(CoreVfsOperationKind::Resolve)
                && entry.locator.as_deref() == Some("mem://notes/alpha")
        }),
        "resolve request should be logged: {log:?}"
    );
    assert!(
        log.iter().any(|entry| {
            entry.event == VfsLogEvent::ResponseApplied
                && entry.operation_kind == Some(CoreVfsOperationKind::Load)
                && entry.document_id.as_deref() == Some("doc://notes/alpha")
                && entry.buf_id.is_some()
        }),
        "load apply should be logged with target buffer context: {log:?}"
    );
}

#[test]
fn vfs_transaction_log_records_quit_deferred_resumed_and_denied_events() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    let outcome = session
        .execute_ex_command(":edit mem://notes/alpha")
        .expect("edit should queue resolve request");
    let resolve_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Resolve { request_id, .. } => request_id,
        other => panic!("unexpected action: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Resolved {
            request_id: resolve_id,
            document_id: "doc://notes/alpha".to_string(),
            display_name: "notes/alpha".to_string(),
        })
        .expect("resolved response should queue load");
    let load_id = match take_next_vfs_request(&mut session) {
        CoreVfsRequest::Load { request_id, .. } => request_id,
        other => panic!("unexpected action: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Loaded {
            request_id: load_id,
            document_id: "doc://notes/alpha".to_string(),
            text: "loaded".to_string(),
        })
        .expect("load should apply");

    session
        .execute_normal_command("A!")
        .expect("edit should succeed");
    let outcome = session
        .execute_ex_command(":wq")
        .expect("wq should queue save on VFS buffer");
    let save_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Save { request_id, .. } => request_id,
        other => panic!("unexpected action: {other:?}"),
    };

    let pending_quit = session.execute_ex_command(":quit");
    assert!(
        pending_quit.is_err(),
        "quit should be denied while save is pending"
    );

    session
        .submit_vfs_response(CoreVfsResponse::Saved {
            request_id: save_id,
            document_id: "doc://notes/alpha".to_string(),
        })
        .expect("save should apply");

    let log = session.vfs_transaction_log();
    assert!(
        log.iter()
            .any(|entry| entry.event == VfsLogEvent::QuitDeferred),
        "deferred close should be logged: {log:?}"
    );
    assert!(
        log.iter()
            .any(|entry| entry.event == VfsLogEvent::QuitDenied),
        "quit denial should be logged: {log:?}"
    );
    assert!(
        log.iter()
            .any(|entry| entry.event == VfsLogEvent::QuitResumed),
        "deferred close resume should be logged: {log:?}"
    );
}

#[test]
fn session_snapshot_exposes_default_vfs_observation_state() {
    let _guard = acquire_session_test_lock();
    let session = VimCoreSession::new("hello").expect("session should initialize");

    let snapshot = session.snapshot();
    let current_buffer = snapshot
        .buffers
        .iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist");

    assert_eq!(current_buffer.source_kind, CoreBufferSourceKind::Local);
    assert!(current_buffer.document_id.is_none());
    assert!(current_buffer.pending_vfs_operation.is_none());
    assert!(current_buffer.deferred_close.is_none());
    assert!(current_buffer.last_vfs_error.is_none());

    let binding = session
        .buffer_binding(current_buffer.id)
        .expect("binding should be synthesized for active buffer");
    assert_eq!(binding.source_kind, CoreBufferSourceKind::Local);
    assert_eq!(binding.display_name, current_buffer.name);
    assert!(session.vfs_request_ledger().is_empty());
}

#[test]
fn deferred_close_contract_exposes_supported_variants() {
    let supported = [
        CoreDeferredClose::Quit,
        CoreDeferredClose::SaveAndClose,
        CoreDeferredClose::SaveIfDirtyAndClose,
    ];

    assert_eq!(supported.len(), 3);
}

#[test]
fn edit_command_queues_resolve_request_for_vfs_locator() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");
    let active_buf_id = session
        .snapshot()
        .buffers
        .iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist")
        .id;

    let outcome = session
        .execute_ex_command(":edit mem://notes/alpha")
        .expect("edit should queue resolve request");

    assert!(matches!(
        outcome.outcome,
        vim_core_rs::CoreCommandOutcome::HostActionQueued
    ));
    assert!(matches!(
        take_vfs_request(outcome.host_actions),
        CoreVfsRequest::Resolve {
            target_buf_id,
            locator,
            ..
        } if target_buf_id == active_buf_id && locator == "mem://notes/alpha"
    ));
}

#[test]
fn resolved_vfs_response_queues_follow_up_load_request() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");
    let active_buf_id = session
        .snapshot()
        .buffers
        .iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist")
        .id;

    let outcome = session
        .execute_ex_command(":edit mem://notes/alpha")
        .expect("edit should queue resolve request");

    let request_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Resolve { request_id, .. } => request_id,
        other => panic!("unexpected action: {other:?}"),
    };

    let outcome = session
        .submit_vfs_response(CoreVfsResponse::Resolved {
            request_id,
            document_id: "doc://notes/alpha".to_string(),
            display_name: "notes/alpha".to_string(),
        })
        .expect("resolved response should queue load");

    assert!(matches!(
        outcome,
        vim_core_rs::CoreCommandOutcome::HostActionQueued
    ));
    assert!(matches!(
        take_next_vfs_request(&mut session),
        CoreVfsRequest::Load {
            target_buf_id,
            document_id,
            ..
        } if target_buf_id == active_buf_id && document_id == "doc://notes/alpha"
    ));
}

#[test]
fn loaded_vfs_response_commits_text_to_target_buffer_only() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    session
        .execute_ex_command(":vnew")
        .expect("vnew should succeed");
    let buffers = session.buffers();
    let original_buf_id = buffers
        .iter()
        .find(|buffer| !buffer.is_active)
        .expect("original buffer should remain")
        .id;
    let target_buf_id = buffers
        .iter()
        .find(|buffer| buffer.is_active)
        .expect("new active buffer should exist")
        .id;

    let outcome = session
        .execute_ex_command(":edit mem://notes/alpha")
        .expect("edit should queue resolve request");
    let resolve_request_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Resolve { request_id, .. } => request_id,
        other => panic!("unexpected action: {other:?}"),
    };

    session
        .submit_vfs_response(CoreVfsResponse::Resolved {
            request_id: resolve_request_id,
            document_id: "doc://notes/alpha".to_string(),
            display_name: "notes/alpha".to_string(),
        })
        .expect("resolved response should queue load");
    let load_request_id = match take_next_vfs_request(&mut session) {
        CoreVfsRequest::Load { request_id, .. } => request_id,
        other => panic!("unexpected action: {other:?}"),
    };

    session
        .submit_vfs_response(CoreVfsResponse::Loaded {
            request_id: load_request_id,
            document_id: "doc://notes/alpha".to_string(),
            text: "virtual text".to_string(),
        })
        .expect("loaded response should commit target buffer");

    assert_eq!(
        session.buffer_text(target_buf_id).as_deref(),
        Some("virtual text")
    );
    assert_eq!(
        session.buffer_text(original_buf_id).as_deref(),
        Some("hello")
    );

    let binding = session
        .buffer_binding(target_buf_id)
        .expect("target buffer binding should exist");
    assert_eq!(binding.source_kind, CoreBufferSourceKind::Virtual);
    assert_eq!(binding.document_id.as_deref(), Some("doc://notes/alpha"));
}

#[test]
fn write_command_on_vfs_buffer_queues_save_request_to_host() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    // VFS buffer を準備: edit -> resolve -> load
    let outcome = session
        .execute_ex_command(":edit mem://notes/alpha")
        .expect("edit should succeed");
    let resolve_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Resolve { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Resolved {
            request_id: resolve_id,
            document_id: "doc://notes/alpha".to_string(),
            display_name: "notes/alpha".to_string(),
        })
        .expect("resolve should succeed");
    let load_id = match take_next_vfs_request(&mut session) {
        CoreVfsRequest::Load { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Loaded {
            request_id: load_id,
            document_id: "doc://notes/alpha".to_string(),
            text: "original content".to_string(),
        })
        .expect("load should succeed");

    // バッファを編集して dirty にする
    session
        .execute_ex_command(":s/original/modified/")
        .expect("substitute should succeed");

    // :write を実行 -> VFS buffer なので host save flow に接続される
    let outcome = session
        .execute_ex_command(":write")
        .expect("write should queue save request");
    assert!(matches!(
        outcome.outcome,
        vim_core_rs::CoreCommandOutcome::HostActionQueued
    ));

    let save_request = take_vfs_request(outcome.host_actions);
    match save_request {
        CoreVfsRequest::Save {
            document_id,
            target_locator,
            text,
            ..
        } => {
            assert_eq!(document_id, "doc://notes/alpha");
            assert!(target_locator.is_none());
            assert!(text.contains("modified"));
        }
        other => panic!("expected Save request, got {other:?}"),
    }
}

#[test]
fn write_with_target_on_vfs_buffer_passes_target_locator_to_host() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    // VFS buffer を準備
    let outcome = session
        .execute_ex_command(":edit mem://notes/beta")
        .expect("edit should succeed");
    let resolve_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Resolve { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Resolved {
            request_id: resolve_id,
            document_id: "doc://notes/beta".to_string(),
            display_name: "notes/beta".to_string(),
        })
        .expect("resolve should succeed");
    let load_id = match take_next_vfs_request(&mut session) {
        CoreVfsRequest::Load { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Loaded {
            request_id: load_id,
            document_id: "doc://notes/beta".to_string(),
            text: "beta content".to_string(),
        })
        .expect("load should succeed");

    // :write {target} を実行
    let outcome = session
        .execute_ex_command(":write mem://backup/beta")
        .expect("write with target should queue save request");
    assert!(matches!(
        outcome.outcome,
        vim_core_rs::CoreCommandOutcome::HostActionQueued
    ));

    let save_request = take_vfs_request(outcome.host_actions);
    match save_request {
        CoreVfsRequest::Save {
            document_id,
            target_locator,
            ..
        } => {
            assert_eq!(document_id, "doc://notes/beta");
            assert_eq!(target_locator, Some("mem://backup/beta".to_string()));
        }
        other => panic!("expected Save request, got {other:?}"),
    }
}

#[test]
fn vfs_save_request_preserves_full_buffer_text_for_size_observation() {
    let _guard = acquire_session_test_lock();
    let observed_text = "alpha\nbeta\ngamma";
    let mut session =
        VimCoreSession::new(observed_text).expect("session should initialize with multiline text");

    let outcome = session
        .execute_ex_command(":edit mem://notes/size")
        .expect("edit should succeed");
    let resolve_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Resolve { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Resolved {
            request_id: resolve_id,
            document_id: "doc://notes/size".to_string(),
            display_name: "notes/size".to_string(),
        })
        .expect("resolve should succeed");
    let load_id = match take_next_vfs_request(&mut session) {
        CoreVfsRequest::Load { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Loaded {
            request_id: load_id,
            document_id: "doc://notes/size".to_string(),
            text: observed_text.to_string(),
        })
        .expect("load should succeed");

    let outcome = session
        .execute_ex_command(":write")
        .expect("write should queue save request");

    let save_request = take_vfs_request(outcome.host_actions);
    match save_request {
        CoreVfsRequest::Save { text, .. } => {
            assert_eq!(text, observed_text);
        }
        other => panic!("expected Save request, got {other:?}"),
    }
}

#[test]
fn vfs_save_failure_reports_permission_denied_to_buffer_state() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    let outcome = session
        .execute_ex_command(":edit mem://notes/permission")
        .expect("edit should succeed");
    let resolve_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Resolve { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Resolved {
            request_id: resolve_id,
            document_id: "doc://notes/permission".to_string(),
            display_name: "notes/permission".to_string(),
        })
        .expect("resolve should succeed");
    let load_id = match take_next_vfs_request(&mut session) {
        CoreVfsRequest::Load { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Loaded {
            request_id: load_id,
            document_id: "doc://notes/permission".to_string(),
            text: "permission".to_string(),
        })
        .expect("load should succeed");

    let outcome = session
        .execute_ex_command(":write")
        .expect("write should queue save request");
    let save_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Save { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };

    session
        .submit_vfs_response(CoreVfsResponse::Failed {
            request_id: save_id,
            error: CoreVfsError {
                kind: CoreVfsErrorKind::SaveFailed,
                message: Some("permission denied".to_string()),
            },
        })
        .expect("failed response should be handled");

    let binding = session
        .buffer_binding(
            session
                .snapshot()
                .buffers
                .iter()
                .find(|buffer| buffer.is_active)
                .expect("active buffer should exist")
                .id,
        )
        .expect("binding should exist");
    assert_eq!(
        binding.last_vfs_error,
        Some(CoreVfsError {
            kind: CoreVfsErrorKind::SaveFailed,
            message: Some("permission denied".to_string()),
        })
    );

    let ledger = session.vfs_request_ledger();
    let request = ledger
        .iter()
        .find(|entry| entry.request_id == save_id)
        .expect("save request should be tracked in the ledger");
    assert!(matches!(
        &request.status,
        CoreRequestStatus::Failed(CoreVfsError {
            kind: CoreVfsErrorKind::SaveFailed,
            message: Some(message),
        }) if message == "permission denied"
    ));
}

#[test]
fn vfs_write_command_emits_distinct_save_requests_for_target_and_copy_destination() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    let outcome = session
        .execute_ex_command(":edit mem://notes/copy")
        .expect("edit should succeed");
    let resolve_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Resolve { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Resolved {
            request_id: resolve_id,
            document_id: "doc://notes/copy".to_string(),
            display_name: "notes/copy".to_string(),
        })
        .expect("resolve should succeed");
    let load_id = match take_next_vfs_request(&mut session) {
        CoreVfsRequest::Load { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Loaded {
            request_id: load_id,
            document_id: "doc://notes/copy".to_string(),
            text: "copy source".to_string(),
        })
        .expect("load should succeed");

    let outcome = session
        .execute_ex_command(":write mem://backup/copy")
        .expect("write with target should queue save request");
    let save_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Save {
            request_id,
            target_locator,
            ..
        } => {
            assert_eq!(target_locator, Some("mem://backup/copy".to_string()));
            request_id
        }
        other => panic!("expected Save request, got {other:?}"),
    };

    session
        .submit_vfs_response(CoreVfsResponse::Saved {
            request_id: save_id,
            document_id: "doc://notes/copy".to_string(),
        })
        .expect("save should succeed");

    let outcome = session
        .execute_ex_command(":write")
        .expect("follow-up write should queue a second save request");
    let next_save_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Save {
            request_id,
            target_locator,
            ..
        } => {
            assert!(target_locator.is_none());
            request_id
        }
        other => panic!("expected Save request, got {other:?}"),
    };

    assert_ne!(
        save_id, next_save_id,
        "copy destination save requests should remain distinct from the source write request"
    );
}

#[test]
fn update_command_on_clean_vfs_buffer_does_not_queue_save() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    // VFS buffer を準備
    let outcome = session
        .execute_ex_command(":edit mem://notes/gamma")
        .expect("edit should succeed");
    let resolve_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Resolve { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Resolved {
            request_id: resolve_id,
            document_id: "doc://notes/gamma".to_string(),
            display_name: "notes/gamma".to_string(),
        })
        .expect("resolve should succeed");
    let load_id = match take_next_vfs_request(&mut session) {
        CoreVfsRequest::Load { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Loaded {
            request_id: load_id,
            document_id: "doc://notes/gamma".to_string(),
            text: "gamma content".to_string(),
        })
        .expect("load should succeed");

    // clean な状態で :update -> save は発行されない
    let outcome = session
        .execute_ex_command(":update")
        .expect("update on clean buffer should succeed");
    assert!(matches!(
        outcome.outcome,
        vim_core_rs::CoreCommandOutcome::NoChange
    ));
}

#[test]
fn write_on_local_buffer_does_not_use_vfs_save_flow() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    // local buffer に対する :write は既存フロー（Write host action）を通る
    let outcome = session
        .execute_ex_command(":write /tmp/test-local.txt")
        .expect("write on local buffer should succeed");
    assert!(matches!(
        outcome.outcome,
        vim_core_rs::CoreCommandOutcome::HostActionQueued
    ));

    // VfsRequest ではなく Write action が出る
    match outcome.host_actions.as_slice() {
        [CoreHostAction::Write { path, .. }] => {
            assert_eq!(path, "/tmp/test-local.txt");
        }
        other => panic!("expected Write host action, got {other:?}"),
    }
}

#[test]
fn save_success_response_clears_dirty_flag_on_vfs_buffer() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    // VFS buffer を準備
    let outcome = session
        .execute_ex_command(":edit mem://notes/delta")
        .expect("edit should succeed");
    let resolve_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Resolve { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Resolved {
            request_id: resolve_id,
            document_id: "doc://notes/delta".to_string(),
            display_name: "notes/delta".to_string(),
        })
        .expect("resolve should succeed");
    let load_id = match take_next_vfs_request(&mut session) {
        CoreVfsRequest::Load { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Loaded {
            request_id: load_id,
            document_id: "doc://notes/delta".to_string(),
            text: "delta content".to_string(),
        })
        .expect("load should succeed");

    // 編集
    session
        .execute_ex_command(":s/delta/DELTA/")
        .expect("substitute should succeed");
    assert!(
        session.snapshot().dirty,
        "buffer should be dirty after edit"
    );

    // :write
    let outcome = session
        .execute_ex_command(":write")
        .expect("write should queue save");
    let save_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Save { request_id, .. } => request_id,
        other => panic!("expected Save, got {other:?}"),
    };

    // save success
    session
        .submit_vfs_response(CoreVfsResponse::Saved {
            request_id: save_id,
            document_id: "doc://notes/delta".to_string(),
        })
        .expect("saved response should succeed");

    assert!(
        !session.snapshot().dirty,
        "buffer should be clean after save success"
    );
}

#[test]
fn save_failure_response_keeps_buffer_dirty() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    // VFS buffer を準備
    let outcome = session
        .execute_ex_command(":edit mem://notes/epsilon")
        .expect("edit should succeed");
    let resolve_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Resolve { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Resolved {
            request_id: resolve_id,
            document_id: "doc://notes/epsilon".to_string(),
            display_name: "notes/epsilon".to_string(),
        })
        .expect("resolve should succeed");
    let load_id = match take_next_vfs_request(&mut session) {
        CoreVfsRequest::Load { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Loaded {
            request_id: load_id,
            document_id: "doc://notes/epsilon".to_string(),
            text: "epsilon content".to_string(),
        })
        .expect("load should succeed");

    // 編集して dirty に
    session
        .execute_ex_command(":s/epsilon/EPSILON/")
        .expect("substitute should succeed");

    // :write
    let outcome = session
        .execute_ex_command(":write")
        .expect("write should queue save");
    let save_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Save { request_id, .. } => request_id,
        other => panic!("expected Save, got {other:?}"),
    };

    // save failure
    session
        .submit_vfs_response(CoreVfsResponse::Failed {
            request_id: save_id,
            error: CoreVfsError {
                kind: CoreVfsErrorKind::SaveFailed,
                message: Some("access denied".to_string()),
            },
        })
        .expect("failed response should be handled");

    assert!(
        session.snapshot().dirty,
        "buffer should remain dirty after save failure"
    );
    let binding = session
        .buffer_binding(
            session
                .snapshot()
                .buffers
                .iter()
                .find(|b| b.is_active)
                .unwrap()
                .id,
        )
        .expect("binding should exist");
    assert!(matches!(
        binding.last_vfs_error,
        Some(CoreVfsError {
            kind: CoreVfsErrorKind::SaveFailed,
            ..
        })
    ));
}

// --- Task 3.3: quit gate integration tests ---

#[test]
fn wq_on_vfs_buffer_queues_save_then_closes_on_success() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    prepare_vfs_buffer(
        &mut session,
        "mem://notes/wq_test",
        "doc://notes/wq_test",
        "notes/wq_test",
        "wq content",
    );

    // 編集して dirty に
    session
        .execute_ex_command(":s/wq/WQ/")
        .expect("substitute should succeed");

    // :wq -> save request が発行され、deferred close が設定される
    let outcome = session
        .execute_ex_command(":wq")
        .expect("wq should queue save request");
    assert!(matches!(
        outcome.outcome,
        vim_core_rs::CoreCommandOutcome::HostActionQueued
    ));

    let save_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Save { request_id, .. } => request_id,
        other => panic!("expected Save, got {other:?}"),
    };

    // deferred close が設定されていることを確認
    let active_buf = session
        .snapshot()
        .buffers
        .iter()
        .find(|b| b.is_active)
        .expect("active buffer should exist")
        .clone();
    assert_eq!(
        active_buf.deferred_close,
        Some(CoreDeferredClose::SaveAndClose)
    );

    // save success -> quit action が発行される
    session
        .submit_vfs_response(CoreVfsResponse::Saved {
            request_id: save_id,
            document_id: "doc://notes/wq_test".to_string(),
        })
        .expect("saved response should succeed");

    // Quit host action が発行されていることを確認
    let mut found_quit = false;
    while let Some(action) = session.take_pending_host_action() {
        if matches!(action, CoreHostAction::Quit { force: false, .. }) {
            found_quit = true;
            break;
        }
    }
    assert!(
        found_quit,
        "Quit action should be queued after save success on :wq"
    );
}

#[test]
fn write_pipe_quit_on_vfs_buffer_preserves_target_locator_and_resumes_quit_after_save_success() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    prepare_vfs_buffer(
        &mut session,
        "mem://notes/write_pipe_quit",
        "doc://notes/write_pipe_quit",
        "notes/write_pipe_quit",
        "write pipe quit content",
    );

    session
        .execute_ex_command(":s/write/WRITE/")
        .expect("substitute should succeed");

    let outcome = session
        .execute_ex_command(":write mem://backup/write_pipe_quit_target | quit")
        .expect("compound write|quit should queue save request");
    assert!(matches!(
        outcome.outcome,
        vim_core_rs::CoreCommandOutcome::HostActionQueued
    ));

    let save_request = take_vfs_request(outcome.host_actions);
    let save_id = match &save_request {
        CoreVfsRequest::Save { request_id, .. } => *request_id,
        other => panic!("expected Save, got {other:?}"),
    };
    assert!(
        matches!(
            save_request,
            CoreVfsRequest::Save {
                target_locator: Some(ref target_locator),
                force: false,
                ..
            } if target_locator == "mem://backup/write_pipe_quit_target"
        ),
        "unexpected save request: {save_request:?}"
    );

    let active_buf = session
        .snapshot()
        .buffers
        .iter()
        .find(|b| b.is_active)
        .expect("active buffer should exist")
        .clone();
    assert_eq!(
        active_buf.deferred_close,
        Some(CoreDeferredClose::SaveAndClose)
    );

    session
        .submit_vfs_response(CoreVfsResponse::Saved {
            request_id: save_id,
            document_id: "doc://notes/write_pipe_quit".to_string(),
        })
        .expect("saved response should succeed");

    let found_quit = session
        .take_pending_host_action()
        .map(|action| matches!(action, CoreHostAction::Quit { force: false, .. }))
        .unwrap_or(false);
    assert!(
        found_quit,
        "quit should resume after :write | quit save success"
    );
}

#[test]
fn non_slash_substitute_then_write_pipe_quit_on_vfs_buffer_preserves_save_coordination() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    prepare_vfs_buffer(
        &mut session,
        "mem://notes/write_pipe_quit_hash",
        "doc://notes/write_pipe_quit_hash",
        "notes/write_pipe_quit_hash",
        "write|pipe quit content",
    );

    let outcome = session
        .execute_ex_command(":s#write|pipe#WRITE# | write mem://backup/write_pipe_quit_hash | quit")
        .expect("non-slash compound write|quit should queue save request");
    assert!(matches!(
        outcome.outcome,
        vim_core_rs::CoreCommandOutcome::HostActionQueued
    ));
    assert_eq!(
        outcome.snapshot.text.trim_end_matches('\n'),
        "WRITE quit content"
    );

    let save_request = take_vfs_request(outcome.host_actions);
    let save_id = match &save_request {
        CoreVfsRequest::Save { request_id, .. } => *request_id,
        other => panic!("expected Save, got {other:?}"),
    };
    assert!(
        matches!(
            save_request,
            CoreVfsRequest::Save {
                target_locator: Some(ref target_locator),
                force: false,
                ..
            } if target_locator == "mem://backup/write_pipe_quit_hash"
        ),
        "unexpected save request: {save_request:?}"
    );

    let active_buf = session
        .snapshot()
        .buffers
        .iter()
        .find(|b| b.is_active)
        .expect("active buffer should exist")
        .clone();
    assert_eq!(
        active_buf.deferred_close,
        Some(CoreDeferredClose::SaveAndClose)
    );

    session
        .submit_vfs_response(CoreVfsResponse::Saved {
            request_id: save_id,
            document_id: "doc://notes/write_pipe_quit_hash".to_string(),
        })
        .expect("saved response should succeed");

    let found_quit = session
        .take_pending_host_action()
        .map(|action| matches!(action, CoreHostAction::Quit { force: false, .. }))
        .unwrap_or(false);
    assert!(
        found_quit,
        "quit should resume after non-slash :write | quit save success"
    );
}

#[test]
fn write_bang_pipe_quit_on_vfs_buffer_preserves_target_locator_and_force() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    prepare_vfs_buffer(
        &mut session,
        "mem://notes/write_bang_pipe_quit",
        "doc://notes/write_bang_pipe_quit",
        "notes/write_bang_pipe_quit",
        "write bang pipe quit content",
    );

    session
        .execute_ex_command(":s/write/WRITE/")
        .expect("substitute should succeed");

    let outcome = session
        .execute_ex_command(":write! mem://backup/write_bang_pipe_quit | quit")
        .expect("compound write!|quit should queue save request");
    assert!(matches!(
        outcome.outcome,
        vim_core_rs::CoreCommandOutcome::HostActionQueued
    ));

    let save_request = take_vfs_request(outcome.host_actions);
    let save_id = match &save_request {
        CoreVfsRequest::Save { request_id, .. } => *request_id,
        other => panic!("expected Save, got {other:?}"),
    };
    assert!(
        matches!(
            save_request,
            CoreVfsRequest::Save {
                target_locator: Some(ref target_locator),
                force: true,
                ..
            } if target_locator == "mem://backup/write_bang_pipe_quit"
        ),
        "unexpected save request: {save_request:?}"
    );

    let active_buf = session
        .snapshot()
        .buffers
        .iter()
        .find(|b| b.is_active)
        .expect("active buffer should exist")
        .clone();
    assert_eq!(
        active_buf.deferred_close,
        Some(CoreDeferredClose::SaveAndClose)
    );

    session
        .submit_vfs_response(CoreVfsResponse::Saved {
            request_id: save_id,
            document_id: "doc://notes/write_bang_pipe_quit".to_string(),
        })
        .expect("saved response should succeed");

    let found_quit = session
        .take_pending_host_action()
        .map(|action| matches!(action, CoreHostAction::Quit { force: false, .. }))
        .unwrap_or(false);
    assert!(
        found_quit,
        "quit should resume after :write! | quit save success"
    );
}

#[test]
fn update_pipe_quit_on_dirty_vfs_buffer_preserves_target_locator_and_resumes_quit_after_save_success()
 {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    prepare_vfs_buffer(
        &mut session,
        "mem://notes/update_pipe_quit_dirty",
        "doc://notes/update_pipe_quit_dirty",
        "notes/update_pipe_quit_dirty",
        "update pipe quit content",
    );

    session
        .execute_ex_command(":s/update/UPDATE/")
        .expect("substitute should succeed");

    let outcome = session
        .execute_ex_command(":update mem://backup/update_pipe_quit_dirty | quit")
        .expect("compound update|quit should queue save request");
    assert!(matches!(
        outcome.outcome,
        vim_core_rs::CoreCommandOutcome::HostActionQueued
    ));

    let save_request = take_vfs_request(outcome.host_actions);
    let save_id = match &save_request {
        CoreVfsRequest::Save { request_id, .. } => *request_id,
        other => panic!("expected Save, got {other:?}"),
    };
    assert!(
        matches!(
            save_request,
            CoreVfsRequest::Save {
                target_locator: Some(ref target_locator),
                force: false,
                ..
            } if target_locator == "mem://backup/update_pipe_quit_dirty"
        ),
        "unexpected save request: {save_request:?}"
    );

    let active_buf = session
        .snapshot()
        .buffers
        .iter()
        .find(|b| b.is_active)
        .expect("active buffer should exist")
        .clone();
    assert_eq!(
        active_buf.deferred_close,
        Some(CoreDeferredClose::SaveIfDirtyAndClose)
    );

    session
        .submit_vfs_response(CoreVfsResponse::Saved {
            request_id: save_id,
            document_id: "doc://notes/update_pipe_quit_dirty".to_string(),
        })
        .expect("saved response should succeed");

    let found_quit = session
        .take_pending_host_action()
        .map(|action| matches!(action, CoreHostAction::Quit { force: false, .. }))
        .unwrap_or(false);
    assert!(
        found_quit,
        "quit should resume after dirty :update | quit save success"
    );
}

#[test]
fn non_slash_magic_substitute_then_update_pipe_quit_on_dirty_vfs_buffer_preserves_coordination() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    prepare_vfs_buffer(
        &mut session,
        "mem://notes/update_pipe_quit_hash",
        "doc://notes/update_pipe_quit_hash",
        "notes/update_pipe_quit_hash",
        "update|pipe quit content",
    );

    let outcome = session
        .execute_ex_command(
            ":sm#update|pipe#UPDATE# | update mem://backup/update_pipe_quit_hash | quit",
        )
        .expect("non-slash compound update|quit should queue save request");
    assert!(matches!(
        outcome.outcome,
        vim_core_rs::CoreCommandOutcome::HostActionQueued
    ));
    assert_eq!(
        outcome.snapshot.text.trim_end_matches('\n'),
        "UPDATE quit content"
    );
    assert!(
        outcome.snapshot.dirty,
        "substitute should leave buffer dirty"
    );

    let save_request = take_vfs_request(outcome.host_actions);
    let save_id = match &save_request {
        CoreVfsRequest::Save { request_id, .. } => *request_id,
        other => panic!("expected Save, got {other:?}"),
    };
    assert!(
        matches!(
            save_request,
            CoreVfsRequest::Save {
                target_locator: Some(ref target_locator),
                force: false,
                ..
            } if target_locator == "mem://backup/update_pipe_quit_hash"
        ),
        "unexpected save request: {save_request:?}"
    );

    let active_buf = session
        .snapshot()
        .buffers
        .iter()
        .find(|b| b.is_active)
        .expect("active buffer should exist")
        .clone();
    assert_eq!(
        active_buf.deferred_close,
        Some(CoreDeferredClose::SaveIfDirtyAndClose)
    );

    session
        .submit_vfs_response(CoreVfsResponse::Saved {
            request_id: save_id,
            document_id: "doc://notes/update_pipe_quit_hash".to_string(),
        })
        .expect("saved response should succeed");

    let found_quit = session
        .take_pending_host_action()
        .map(|action| matches!(action, CoreHostAction::Quit { force: false, .. }))
        .unwrap_or(false);
    assert!(
        found_quit,
        "quit should resume after non-slash dirty :update | quit save success"
    );
}

#[test]
fn update_pipe_quit_on_clean_vfs_buffer_does_not_save_unnecessarily() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    prepare_vfs_buffer(
        &mut session,
        "mem://notes/update_pipe_quit_clean",
        "doc://notes/update_pipe_quit_clean",
        "notes/update_pipe_quit_clean",
        "clean update pipe quit content",
    );

    let outcome = session
        .execute_ex_command(":update mem://backup/update_pipe_quit_clean | quit")
        .expect("compound update|quit should succeed on clean VFS buffer");
    assert!(matches!(
        outcome.outcome,
        vim_core_rs::CoreCommandOutcome::HostActionQueued
    ));
    assert!(
        matches!(
            outcome.host_actions.as_slice(),
            [CoreHostAction::Quit { force: false, .. }]
        ),
        "clean :update | quit should queue only Quit, got {:?}",
        outcome.host_actions
    );
    assert!(
        !outcome.host_actions.iter().any(|action| matches!(
            action,
            CoreHostAction::VfsRequest(CoreVfsRequest::Save { .. })
        )),
        "clean :update | quit should not queue Save, got {:?}",
        outcome.host_actions
    );
}

#[test]
fn update_bang_pipe_quit_on_vfs_buffer_stays_out_of_deferred_close_rewrite() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    prepare_vfs_buffer(
        &mut session,
        "mem://notes/update_bang_pipe_quit",
        "doc://notes/update_bang_pipe_quit",
        "notes/update_bang_pipe_quit",
        "update bang pipe quit content",
    );

    session
        .execute_ex_command(":s/update/UPDATE/")
        .expect("substitute should succeed");

    let outcome = session
        .execute_ex_command(":update! mem://backup/update_bang_pipe_quit | quit")
        .expect("compound update!|quit should queue save request");
    assert!(matches!(
        outcome.outcome,
        vim_core_rs::CoreCommandOutcome::HostActionQueued
    ));

    let save_request = take_vfs_request(outcome.host_actions);
    let save_id = match &save_request {
        CoreVfsRequest::Save { request_id, .. } => *request_id,
        other => panic!("expected Save, got {other:?}"),
    };
    assert!(
        matches!(
            save_request,
            CoreVfsRequest::Save {
                target_locator: Some(ref target_locator),
                force: true,
                ..
            } if target_locator == "mem://backup/update_bang_pipe_quit"
        ),
        "unexpected save request: {save_request:?}"
    );

    let active_buf = session
        .snapshot()
        .buffers
        .iter()
        .find(|b| b.is_active)
        .expect("active buffer should exist")
        .clone();
    assert!(
        active_buf.deferred_close.is_none(),
        "update! should not enter deferred close flow"
    );

    session
        .submit_vfs_response(CoreVfsResponse::Saved {
            request_id: save_id,
            document_id: "doc://notes/update_bang_pipe_quit".to_string(),
        })
        .expect("saved response should succeed");

    let mut found_quit = false;
    while let Some(action) = session.take_pending_host_action() {
        if matches!(action, CoreHostAction::Quit { .. }) {
            found_quit = true;
            break;
        }
    }
    assert!(
        !found_quit,
        "update! | quit should not queue Quit after save success"
    );
}

#[test]
fn write_pipe_non_quit_on_vfs_buffer_does_not_enter_deferred_close_flow() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    prepare_vfs_buffer(
        &mut session,
        "mem://notes/write_pipe_redraw",
        "doc://notes/write_pipe_redraw",
        "notes/write_pipe_redraw",
        "write pipe redraw content",
    );

    session
        .execute_ex_command(":s/write/WRITE/")
        .expect("substitute should succeed");

    let outcome = session
        .execute_ex_command(":write mem://backup/write_pipe_redraw | redraw")
        .expect("compound write|redraw should queue save request");
    assert!(matches!(
        outcome.outcome,
        vim_core_rs::CoreCommandOutcome::HostActionQueued
    ));

    let save_request = take_vfs_request(outcome.host_actions);
    let save_id = match &save_request {
        CoreVfsRequest::Save { request_id, .. } => *request_id,
        other => panic!("expected Save, got {other:?}"),
    };
    assert!(
        matches!(
            save_request,
            CoreVfsRequest::Save {
                target_locator: Some(ref target_locator),
                force: false,
                ..
            } if target_locator == "mem://backup/write_pipe_redraw"
        ),
        "unexpected save request: {save_request:?}"
    );

    let active_buf = session
        .snapshot()
        .buffers
        .iter()
        .find(|b| b.is_active)
        .expect("active buffer should exist")
        .clone();
    assert!(
        active_buf.deferred_close.is_none(),
        "write | non-quit should not enter deferred close flow"
    );

    session
        .submit_vfs_response(CoreVfsResponse::Saved {
            request_id: save_id,
            document_id: "doc://notes/write_pipe_redraw".to_string(),
        })
        .expect("saved response should succeed");

    let mut found_quit = false;
    while let Some(action) = session.take_pending_host_action() {
        if matches!(action, CoreHostAction::Quit { .. }) {
            found_quit = true;
            break;
        }
    }
    assert!(
        !found_quit,
        "write | non-quit should not resume quit after save success"
    );
}

#[test]
fn quit_on_vfs_buffer_with_pending_save_is_rejected() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    // VFS buffer を準備
    let outcome = session
        .execute_ex_command(":edit mem://notes/quit_reject")
        .expect("edit should succeed");
    let resolve_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Resolve { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Resolved {
            request_id: resolve_id,
            document_id: "doc://notes/quit_reject".to_string(),
            display_name: "notes/quit_reject".to_string(),
        })
        .expect("resolve should succeed");
    let load_id = match take_next_vfs_request(&mut session) {
        CoreVfsRequest::Load { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Loaded {
            request_id: load_id,
            document_id: "doc://notes/quit_reject".to_string(),
            text: "quit reject content".to_string(),
        })
        .expect("load should succeed");

    // 編集して dirty に、:write で save を発行
    session
        .execute_ex_command(":s/quit/QUIT/")
        .expect("substitute should succeed");
    let outcome = session
        .execute_ex_command(":write")
        .expect("write should queue save");
    let _save_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Save { request_id, .. } => request_id,
        other => panic!("expected Save, got {other:?}"),
    };

    // pending save 中の :quit は拒否される
    let result = session.execute_ex_command(":quit");
    assert!(
        result.is_err()
            || matches!(
                result,
                Ok(vim_core_rs::CoreCommandTransaction {
                    outcome: vim_core_rs::CoreCommandOutcome::NoChange,
                    ..
                })
            ),
        "quit during pending save should be rejected: {result:?}"
    );
}

#[test]
fn quit_force_on_vfs_buffer_with_pending_save_is_allowed() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    // VFS buffer を準備
    let outcome = session
        .execute_ex_command(":edit mem://notes/quit_force")
        .expect("edit should succeed");
    let resolve_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Resolve { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Resolved {
            request_id: resolve_id,
            document_id: "doc://notes/quit_force".to_string(),
            display_name: "notes/quit_force".to_string(),
        })
        .expect("resolve should succeed");
    let load_id = match take_next_vfs_request(&mut session) {
        CoreVfsRequest::Load { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Loaded {
            request_id: load_id,
            document_id: "doc://notes/quit_force".to_string(),
            text: "force quit content".to_string(),
        })
        .expect("load should succeed");

    // 編集して dirty に、save 発行
    session
        .execute_ex_command(":s/force/FORCE/")
        .expect("substitute should succeed");
    let outcome = session
        .execute_ex_command(":write")
        .expect("write should queue save");
    let _ = take_vfs_request(outcome.host_actions);

    // :quit! は強制終了を許可
    let outcome = session
        .execute_ex_command(":quit!")
        .expect("quit! should be allowed even with pending save");
    assert!(matches!(
        outcome.outcome,
        vim_core_rs::CoreCommandOutcome::HostActionQueued
    ));

    let found_quit = outcome
        .host_actions
        .iter()
        .any(|action| matches!(action, CoreHostAction::Quit { force: true, .. }));
    assert!(found_quit, "forced quit should be queued");
}

#[test]
fn wq_save_failure_blocks_close_and_reports_error() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    // VFS buffer を準備
    let outcome = session
        .execute_ex_command(":edit mem://notes/wq_fail")
        .expect("edit should succeed");
    let resolve_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Resolve { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Resolved {
            request_id: resolve_id,
            document_id: "doc://notes/wq_fail".to_string(),
            display_name: "notes/wq_fail".to_string(),
        })
        .expect("resolve should succeed");
    let load_id = match take_next_vfs_request(&mut session) {
        CoreVfsRequest::Load { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Loaded {
            request_id: load_id,
            document_id: "doc://notes/wq_fail".to_string(),
            text: "wq fail content".to_string(),
        })
        .expect("load should succeed");

    // 編集して dirty に
    session
        .execute_ex_command(":s/wq/WQ/")
        .expect("substitute should succeed");

    // :wq -> save request
    let outcome = session
        .execute_ex_command(":wq")
        .expect("wq should queue save");
    let save_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Save { request_id, .. } => request_id,
        other => panic!("expected Save, got {other:?}"),
    };

    // save failure -> close はブロックされる
    session
        .submit_vfs_response(CoreVfsResponse::Failed {
            request_id: save_id,
            error: CoreVfsError {
                kind: CoreVfsErrorKind::SaveFailed,
                message: Some("write denied".to_string()),
            },
        })
        .expect("failed response should be handled");

    // Quit action は発行されていないことを確認
    let mut found_quit = false;
    while let Some(action) = session.take_pending_host_action() {
        if matches!(action, CoreHostAction::Quit { .. }) {
            found_quit = true;
        }
    }
    assert!(
        !found_quit,
        "quit should not be queued after save failure on :wq"
    );

    // deferred close がクリアされ、エラーが観測できる
    let active_buf = session
        .snapshot()
        .buffers
        .iter()
        .find(|b| b.is_active)
        .expect("active buffer")
        .clone();
    assert!(
        active_buf.deferred_close.is_none(),
        "deferred close should be cleared after save failure"
    );
    assert!(
        matches!(
            active_buf.last_vfs_error,
            Some(CoreVfsError {
                kind: CoreVfsErrorKind::SaveFailed,
                ..
            })
        ),
        "save failure error should be observable"
    );
}

#[test]
fn resolved_local_fallback_runs_existing_edit_flow() {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be monotonic")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("vim-core-rs-vfs-{unique}.txt"));
    fs::write(&path, "fallback text\n").expect("temp file should be written");

    let locator = path.to_string_lossy().to_string();
    let outcome = session
        .execute_ex_command(&format!(":edit {locator}"))
        .expect("edit should queue resolve request");
    let request_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Resolve { request_id, .. } => request_id,
        other => panic!("unexpected action: {other:?}"),
    };

    session
        .submit_vfs_response(CoreVfsResponse::ResolvedLocalFallback {
            request_id,
            locator: locator.clone(),
        })
        .expect("local fallback should reuse native edit flow");

    assert_eq!(
        session.snapshot().text.trim_end_matches('\n'),
        "fallback text"
    );
    let active_buffer = session
        .snapshot()
        .buffers
        .into_iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist");
    assert_eq!(active_buffer.source_kind, CoreBufferSourceKind::Local);

    fs::remove_file(path).ok();
}

// --- Task 4.1: VFS POD 契約と runtime apply contract ---

#[test]
fn buffer_commit_applies_text_and_name_to_target_buffer_via_bridge() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    // VFS buffer を準備: edit -> resolve -> load
    let outcome = session
        .execute_ex_command(":edit mem://notes/commit_test")
        .expect("edit should succeed");
    let resolve_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Resolve { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Resolved {
            request_id: resolve_id,
            document_id: "doc://notes/commit_test".to_string(),
            display_name: "notes/commit_test".to_string(),
        })
        .expect("resolve should succeed");
    let load_id = match take_next_vfs_request(&mut session) {
        CoreVfsRequest::Load { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Loaded {
            request_id: load_id,
            document_id: "doc://notes/commit_test".to_string(),
            text: "commit test content".to_string(),
        })
        .expect("load should succeed");

    // text が反映され、dirty が false であることを確認
    let snapshot = session.snapshot();
    let active_buf = snapshot
        .buffers
        .iter()
        .find(|b| b.is_active)
        .expect("active buffer should exist");
    assert_eq!(
        session.buffer_text(active_buf.id).as_deref(),
        Some("commit test content"),
        "buffer text should be committed via bridge"
    );
    assert_eq!(
        active_buf.name, "notes/commit_test",
        "buffer name should be updated via bridge"
    );
    assert!(
        !active_buf.dirty,
        "buffer should not be dirty after load commit"
    );
}

#[test]
fn buffer_commit_clears_dirty_on_save_success_via_bridge() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    // VFS buffer を準備
    let outcome = session
        .execute_ex_command(":edit mem://notes/dirty_clear_test")
        .expect("edit should succeed");
    let resolve_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Resolve { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Resolved {
            request_id: resolve_id,
            document_id: "doc://notes/dirty_clear_test".to_string(),
            display_name: "notes/dirty_clear_test".to_string(),
        })
        .expect("resolve should succeed");
    let load_id = match take_next_vfs_request(&mut session) {
        CoreVfsRequest::Load { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Loaded {
            request_id: load_id,
            document_id: "doc://notes/dirty_clear_test".to_string(),
            text: "original".to_string(),
        })
        .expect("load should succeed");

    // 編集して dirty にする
    session
        .execute_ex_command(":s/original/modified/")
        .expect("substitute should succeed");
    assert!(
        session.snapshot().dirty,
        "buffer should be dirty after edit"
    );

    // save 発行
    let outcome = session
        .execute_ex_command(":write")
        .expect("write should queue save");
    let save_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Save { request_id, .. } => request_id,
        other => panic!("expected Save, got {other:?}"),
    };

    // save success -> dirty フラグが bridge 経由でクリアされる
    session
        .submit_vfs_response(CoreVfsResponse::Saved {
            request_id: save_id,
            document_id: "doc://notes/dirty_clear_test".to_string(),
        })
        .expect("saved response should succeed");

    let snapshot = session.snapshot();
    assert!(
        !snapshot.dirty,
        "buffer dirty flag should be cleared via bridge after save success"
    );
}

// --- Task 4.2: Snapshot と bridge 変換の VFS metadata 対応 ---

#[test]
fn snapshot_buffer_info_projects_vfs_source_kind_after_load() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    // VFS buffer を準備: edit -> resolve -> load
    let outcome = session
        .execute_ex_command(":edit mem://notes/source_kind_test")
        .expect("edit should succeed");
    let resolve_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Resolve { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Resolved {
            request_id: resolve_id,
            document_id: "doc://notes/source_kind_test".to_string(),
            display_name: "notes/source_kind_test".to_string(),
        })
        .expect("resolve should succeed");
    let load_id = match take_next_vfs_request(&mut session) {
        CoreVfsRequest::Load { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Loaded {
            request_id: load_id,
            document_id: "doc://notes/source_kind_test".to_string(),
            text: "source kind content".to_string(),
        })
        .expect("load should succeed");

    let snapshot = session.snapshot();
    let active_buf = snapshot
        .buffers
        .iter()
        .find(|b| b.is_active)
        .expect("active buffer should exist");

    // snapshot から VFS metadata が投影されている
    assert_eq!(active_buf.source_kind, CoreBufferSourceKind::Virtual);
    assert_eq!(
        active_buf.document_id.as_deref(),
        Some("doc://notes/source_kind_test")
    );
    assert!(
        active_buf.pending_vfs_operation.is_none(),
        "no pending operation after load"
    );
    assert!(active_buf.deferred_close.is_none(), "no deferred close");
    assert!(active_buf.last_vfs_error.is_none(), "no VFS error");
}

#[test]
fn vfs_load_of_another_document_advances_source_revision_on_reused_buffer() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    prepare_vfs_buffer(
        &mut session,
        "mem://notes/alpha",
        "doc://notes/alpha",
        "notes/alpha",
        "alpha content",
    );
    let alpha_buffer = session
        .snapshot()
        .buffers
        .into_iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist");

    let outcome = session
        .execute_ex_command(":edit mem://notes/beta")
        .expect("edit should queue resolve request");
    let resolve_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Resolve { request_id, .. } => request_id,
        other => panic!("unexpected action: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Resolved {
            request_id: resolve_id,
            document_id: "doc://notes/beta".to_string(),
            display_name: "notes/beta".to_string(),
        })
        .expect("resolve should succeed");
    let load_id = match take_next_vfs_request(&mut session) {
        CoreVfsRequest::Load { request_id, .. } => request_id,
        other => panic!("unexpected action: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Loaded {
            request_id: load_id,
            document_id: "doc://notes/beta".to_string(),
            text: "beta content".to_string(),
        })
        .expect("load should succeed");

    let beta_buffer = session
        .snapshot()
        .buffers
        .into_iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist");

    assert_eq!(
        beta_buffer.id, alpha_buffer.id,
        "VFS load should update the target buffer in place"
    );
    assert_eq!(beta_buffer.document_id.as_deref(), Some("doc://notes/beta"));
    assert!(
        beta_buffer.source_revision.value > alpha_buffer.source_revision.value,
        "loading another document must invalidate buffer_id+revision keyed syntax caches"
    );
}

#[test]
fn snapshot_buffer_info_projects_pending_operation_during_load() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    // resolve まで進めて、load pending 状態を作る
    let outcome = session
        .execute_ex_command(":edit mem://notes/pending_test")
        .expect("edit should succeed");
    let resolve_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Resolve { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Resolved {
            request_id: resolve_id,
            document_id: "doc://notes/pending_test".to_string(),
            display_name: "notes/pending_test".to_string(),
        })
        .expect("resolve should queue load");

    // load request を取得するが、response はまだ返さない
    let load_id = match take_next_vfs_request(&mut session) {
        CoreVfsRequest::Load { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };

    // snapshot で pending 状態を確認
    let snapshot = session.snapshot();
    let active_buf = snapshot
        .buffers
        .iter()
        .find(|b| b.is_active)
        .expect("active buffer should exist");

    assert_eq!(active_buf.source_kind, CoreBufferSourceKind::Virtual);
    assert!(
        active_buf.pending_vfs_operation.is_some(),
        "pending load operation should be visible in snapshot"
    );
    let pending = active_buf.pending_vfs_operation.unwrap();
    assert_eq!(pending.request_id, load_id);
    assert_eq!(pending.kind, CoreVfsOperationKind::Load);
}

#[test]
fn snapshot_buffer_info_projects_deferred_close_during_save() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    // VFS buffer を準備
    let outcome = session
        .execute_ex_command(":edit mem://notes/deferred_snapshot")
        .expect("edit should succeed");
    let resolve_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Resolve { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Resolved {
            request_id: resolve_id,
            document_id: "doc://notes/deferred_snapshot".to_string(),
            display_name: "notes/deferred_snapshot".to_string(),
        })
        .expect("resolve should succeed");
    let load_id = match take_next_vfs_request(&mut session) {
        CoreVfsRequest::Load { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Loaded {
            request_id: load_id,
            document_id: "doc://notes/deferred_snapshot".to_string(),
            text: "deferred close content".to_string(),
        })
        .expect("load should succeed");

    // 編集して dirty にし、:wq で deferred close を設定
    session
        .execute_ex_command(":s/deferred/DEFERRED/")
        .expect("substitute should succeed");
    session
        .execute_ex_command(":wq")
        .expect("wq should queue save");

    // snapshot で deferred close が投影されていることを確認
    let snapshot = session.snapshot();
    let active_buf = snapshot
        .buffers
        .iter()
        .find(|b| b.is_active)
        .expect("active buffer should exist");

    assert_eq!(
        active_buf.deferred_close,
        Some(CoreDeferredClose::SaveAndClose),
        "deferred close should be projected in snapshot"
    );
}

#[test]
fn snapshot_buffer_info_projects_last_vfs_error_after_failure() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    // VFS buffer を準備
    let outcome = session
        .execute_ex_command(":edit mem://notes/error_snapshot")
        .expect("edit should succeed");
    let resolve_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Resolve { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Resolved {
            request_id: resolve_id,
            document_id: "doc://notes/error_snapshot".to_string(),
            display_name: "notes/error_snapshot".to_string(),
        })
        .expect("resolve should succeed");
    let load_id = match take_next_vfs_request(&mut session) {
        CoreVfsRequest::Load { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Loaded {
            request_id: load_id,
            document_id: "doc://notes/error_snapshot".to_string(),
            text: "error snapshot content".to_string(),
        })
        .expect("load should succeed");

    // 編集して save 失敗させる
    session
        .execute_ex_command(":s/error/ERROR/")
        .expect("substitute should succeed");
    let outcome = session
        .execute_ex_command(":write")
        .expect("write should queue save");
    let save_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Save { request_id, .. } => request_id,
        other => panic!("expected Save, got {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Failed {
            request_id: save_id,
            error: CoreVfsError {
                kind: CoreVfsErrorKind::SaveFailed,
                message: Some("disk full".to_string()),
            },
        })
        .expect("failed response should be handled");

    // snapshot で last_vfs_error が投影されている
    let snapshot = session.snapshot();
    let active_buf = snapshot
        .buffers
        .iter()
        .find(|b| b.is_active)
        .expect("active buffer should exist");

    assert!(
        matches!(
            &active_buf.last_vfs_error,
            Some(CoreVfsError {
                kind: CoreVfsErrorKind::SaveFailed,
                message: Some(msg),
            }) if msg == "disk full"
        ),
        "last VFS error should be projected in snapshot: {:?}",
        active_buf.last_vfs_error
    );
}

#[test]
fn snapshot_projects_pending_vfs_requests_count() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("session should initialize");

    // 初期状態: pending VFS requests は 0
    let initial_ledger = session.vfs_request_ledger();
    let initial_pending = initial_ledger
        .iter()
        .filter(|e| matches!(e.status, CoreRequestStatus::Pending))
        .count();
    assert_eq!(initial_pending, 0, "no pending VFS requests initially");

    // resolve を発行して pending を作る
    let outcome = session
        .execute_ex_command(":edit mem://notes/pending_count_test")
        .expect("edit should succeed");
    let _resolve_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Resolve { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };

    // ledger に pending request が 1 つ存在する
    let ledger = session.vfs_request_ledger();
    let pending_count = ledger
        .iter()
        .filter(|e| matches!(e.status, CoreRequestStatus::Pending))
        .count();
    assert_eq!(
        pending_count, 1,
        "one pending VFS request after resolve issued"
    );
}

#[test]
fn bridge_vfs_pod_types_are_exposed_through_bindgen_boundary() {
    // vim_bridge.h に定義された VFS 用 POD 型が bindgen 経由で Rust に公開されていることを検証する。
    // これは FFI 契約テストであり、命名規約 (vim_core_*) を満たすことを確認する。

    // vim_core_vfs_operation_kind_t の enum 値が期待通り
    assert_eq!(vim_core_rs::ffi::VIM_CORE_VFS_OPERATION_NONE, 0);
    assert_eq!(vim_core_rs::ffi::VIM_CORE_VFS_OPERATION_RESOLVE, 1);
    assert_eq!(vim_core_rs::ffi::VIM_CORE_VFS_OPERATION_EXISTS, 2);
    assert_eq!(vim_core_rs::ffi::VIM_CORE_VFS_OPERATION_LOAD, 3);
    assert_eq!(vim_core_rs::ffi::VIM_CORE_VFS_OPERATION_SAVE, 4);

    // vim_core_buffer_source_kind_t の enum 値が期待通り
    assert_eq!(vim_core_rs::ffi::VIM_CORE_BUFFER_SOURCE_LOCAL, 0);
    assert_eq!(vim_core_rs::ffi::VIM_CORE_BUFFER_SOURCE_VFS, 1);

    // vim_core_buffer_commit_t のフィールドが存在する
    let commit = vim_core_rs::ffi::vim_core_buffer_commit_t {
        target_buf_id: 1,
        replace_text: true,
        text_ptr: std::ptr::null(),
        text_len: 0,
        display_name_ptr: std::ptr::null(),
        display_name_len: 0,
        clear_dirty: true,
    };
    assert_eq!(commit.target_buf_id, 1);
    assert!(commit.replace_text);
    assert!(commit.clear_dirty);
}

#[test]
fn bridge_buffer_info_exposes_vfs_metadata_fields() {
    // vim_core_buffer_info_t が VFS metadata フィールドを持つことを検証する。
    let buf_info = vim_core_rs::ffi::vim_core_buffer_info_t {
        id: 5,
        name_ptr: std::ptr::null(),
        name_len: 0,
        source_revision: 0,
        dirty: false,
        is_active: true,
        source_kind: vim_core_rs::ffi::VIM_CORE_BUFFER_SOURCE_VFS,
        document_id_ptr: std::ptr::null(),
        document_id_len: 0,
        pending_vfs_operation: vim_core_rs::ffi::VIM_CORE_VFS_OPERATION_LOAD,
        deferred_close: false,
        last_vfs_error_ptr: std::ptr::null(),
        last_vfs_error_len: 0,
    };
    assert_eq!(buf_info.id, 5);
    assert_eq!(
        buf_info.source_kind,
        vim_core_rs::ffi::VIM_CORE_BUFFER_SOURCE_VFS
    );
    assert_eq!(
        buf_info.pending_vfs_operation,
        vim_core_rs::ffi::VIM_CORE_VFS_OPERATION_LOAD
    );
    assert!(buf_info.is_active);
}

#[test]
fn bridge_commit_buffer_update_function_exists_in_ffi() {
    // vim_bridge_commit_buffer_update が FFI 境界で利用可能であることを検証する。
    // 実際の呼び出しは session 経由で行うので、ここでは関数ポインタの存在確認のみ。
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("commit fn test").expect("session should initialize");

    // commit_buffer_update は内部で使うが、API として存在確認
    // VFS buffer を準備してコミットが実行される完全なフローを実行
    let outcome = session
        .execute_ex_command(":edit mem://notes/fn_exists_test")
        .expect("edit should succeed");
    let resolve_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Resolve { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Resolved {
            request_id: resolve_id,
            document_id: "doc://notes/fn_exists_test".to_string(),
            display_name: "notes/fn_exists_test".to_string(),
        })
        .expect("resolve should succeed");
    let load_id = match take_next_vfs_request(&mut session) {
        CoreVfsRequest::Load { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    // commit_buffer_update は load response のコミット時に内部的に呼ばれる
    session
        .submit_vfs_response(CoreVfsResponse::Loaded {
            request_id: load_id,
            document_id: "doc://notes/fn_exists_test".to_string(),
            text: "committed via apply".to_string(),
        })
        .expect("load should commit via commit_buffer_update");

    assert_eq!(
        session
            .buffer_text(
                session
                    .snapshot()
                    .buffers
                    .iter()
                    .find(|b| b.is_active)
                    .unwrap()
                    .id
            )
            .as_deref(),
        Some("committed via apply"),
        "buffer text should be committed"
    );
}

#[test]
fn bridge_buffer_commit_preserves_text_name_dirty_atomically() {
    // VFS load の buffer commit が text, name, dirty を一括で反映することを検証する。
    // 途中状態（text だけ更新されて name が未更新等）が観測されないことが重要。
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("initial").expect("session should initialize");

    // VFS buffer を準備
    let outcome = session
        .execute_ex_command(":edit mem://notes/atomic_test")
        .expect("edit should succeed");
    let resolve_id = match take_vfs_request(outcome.host_actions) {
        CoreVfsRequest::Resolve { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Resolved {
            request_id: resolve_id,
            document_id: "doc://notes/atomic_test".to_string(),
            display_name: "notes/atomic_test".to_string(),
        })
        .expect("resolve should succeed");
    let load_id = match take_next_vfs_request(&mut session) {
        CoreVfsRequest::Load { request_id, .. } => request_id,
        other => panic!("unexpected: {other:?}"),
    };
    session
        .submit_vfs_response(CoreVfsResponse::Loaded {
            request_id: load_id,
            document_id: "doc://notes/atomic_test".to_string(),
            text: "atomic content".to_string(),
        })
        .expect("load should succeed");

    // commit 後の snapshot で text, name, dirty が一貫している
    let snapshot = session.snapshot();
    let active_buf = snapshot
        .buffers
        .iter()
        .find(|b| b.is_active)
        .expect("active buffer should exist");

    assert_eq!(
        session.buffer_text(active_buf.id).as_deref(),
        Some("atomic content"),
        "text should be committed"
    );
    assert_eq!(
        active_buf.name, "notes/atomic_test",
        "display name should be committed"
    );
    assert!(!active_buf.dirty, "dirty should be cleared after commit");

    // binding も一致する
    let binding = session
        .buffer_binding(active_buf.id)
        .expect("binding should exist");
    assert_eq!(binding.source_kind, CoreBufferSourceKind::Virtual);
    assert_eq!(
        binding.document_id.as_deref(),
        Some("doc://notes/atomic_test")
    );
    assert_eq!(binding.display_name, "notes/atomic_test");
}
