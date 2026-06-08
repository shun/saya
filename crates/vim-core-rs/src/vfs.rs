#![cfg_attr(not(test), allow(dead_code))]

use std::collections::{BTreeMap, BTreeSet};

/// VFS transaction のログイベント種別。
/// request 発行、response 適用、拒否、deferred close の各操作を区別する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VfsLogEvent {
    /// request が発行された
    RequestIssued,
    /// response が正常に適用された
    ResponseApplied,
    /// stale response として拒否された（revision mismatch）
    StaleRejected,
    /// protocol mismatch として拒否された（operation kind 不一致）
    ProtocolMismatchRejected,
    /// revision mismatch により dirty 維持された
    RevisionMismatchKeepDirty,
    /// local fallback が選択された
    LocalFallbackSelected,
    /// deferred close が設定された（quit deferred）
    QuitDeferred,
    /// deferred close が解放された（quit resumed）
    QuitResumed,
    /// quit が拒否された（pending save 中など）
    QuitDenied,
    /// unknown request として拒否された
    UnknownRequestRejected,
}

/// VFS transaction の詳細ログエントリ。
/// 各ログには request_id、buf_id、document_id、locator、base_revision、
/// current_revision、operation_kind を含め、原因追跡を可能にする。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VfsLogEntry {
    pub event: VfsLogEvent,
    pub operation_kind: Option<CoreVfsOperationKind>,
    pub request_id: Option<u64>,
    pub buf_id: Option<i32>,
    pub document_id: Option<String>,
    pub locator: Option<String>,
    pub base_revision: Option<u64>,
    pub current_revision: Option<u64>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreBufferSourceKind {
    Local,
    Virtual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreVfsOperationKind {
    Resolve,
    Exists,
    Load,
    Save,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreDeferredClose {
    Quit,
    SaveAndClose,
    SaveIfDirtyAndClose,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreVfsErrorKind {
    ResolveFailed,
    ExistsFailed,
    LoadFailed,
    SaveFailed,
    NotFound,
    InvalidResponse,
    HostUnavailable,
    Cancelled,
    TimedOut,
    RevisionMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreVfsError {
    pub kind: CoreVfsErrorKind,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorePendingVfsOperation {
    pub request_id: u64,
    pub kind: CoreVfsOperationKind,
    pub issued_order: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreBufferBinding {
    pub buf_id: i32,
    pub source_kind: CoreBufferSourceKind,
    pub locator: Option<String>,
    pub document_id: Option<String>,
    pub display_name: String,
    pub committed_revision: u64,
    pub pending_operation: Option<CorePendingVfsOperation>,
    pub deferred_close: Option<CoreDeferredClose>,
    pub last_saved_revision: Option<u64>,
    pub last_vfs_error: Option<CoreVfsError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreRequestStatus {
    Pending,
    Succeeded,
    Failed(CoreVfsError),
    Cancelled,
    TimedOut,
    Stale {
        reason: String,
    },
    ProtocolMismatch {
        expected: CoreVfsOperationKind,
        actual: CoreVfsOperationKind,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreRequestEntry {
    pub request_id: u64,
    pub operation_kind: CoreVfsOperationKind,
    pub target_buf_id: i32,
    pub document_id: Option<String>,
    pub locator: Option<String>,
    pub base_revision: Option<u64>,
    pub status: CoreRequestStatus,
    pub issued_order: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreVfsRequest {
    Resolve {
        request_id: u64,
        target_buf_id: i32,
        locator: String,
    },
    Exists {
        request_id: u64,
        locator: String,
    },
    Load {
        request_id: u64,
        target_buf_id: i32,
        document_id: String,
    },
    Save {
        request_id: u64,
        target_buf_id: i32,
        document_id: String,
        target_locator: Option<String>,
        text: String,
        base_revision: u64,
        force: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreVfsResponse {
    Resolved {
        request_id: u64,
        document_id: String,
        display_name: String,
    },
    ResolvedLocalFallback {
        request_id: u64,
        locator: String,
    },
    ResolvedMissing {
        request_id: u64,
        locator: String,
    },
    ExistsResult {
        request_id: u64,
        exists: bool,
    },
    Loaded {
        request_id: u64,
        document_id: String,
        text: String,
    },
    Saved {
        request_id: u64,
        document_id: String,
    },
    Failed {
        request_id: u64,
        error: CoreVfsError,
    },
    Cancelled {
        request_id: u64,
    },
    TimedOut {
        request_id: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoreResponseApplyOutcome {
    Applied,
    StaleRejected,
    ProtocolMismatchRejected,
    UnknownRequest,
}

#[derive(Debug, Clone)]
struct BufferState {
    binding: CoreBufferBinding,
    current_revision: u64,
}

impl BufferState {
    fn local(buf_id: i32, display_name: String) -> Self {
        Self {
            binding: CoreBufferBinding {
                buf_id,
                source_kind: CoreBufferSourceKind::Local,
                locator: None,
                document_id: None,
                display_name,
                committed_revision: 0,
                pending_operation: None,
                deferred_close: None,
                last_saved_revision: None,
                last_vfs_error: None,
            },
            current_revision: 0,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct DocumentCoordinator {
    next_request_id: u64,
    next_issued_order: u64,
    bindings: BTreeMap<i32, BufferState>,
    requests: BTreeMap<u64, CoreRequestEntry>,
    transaction_log: Vec<VfsLogEntry>,
}

impl DocumentCoordinator {
    pub(crate) fn new() -> Self {
        Self {
            next_request_id: 1,
            next_issued_order: 1,
            bindings: BTreeMap::new(),
            requests: BTreeMap::new(),
            transaction_log: Vec::new(),
        }
    }

    /// transaction log のエントリ一覧を返す。
    /// host が pending 状態や失敗理由を UI に反映する際の診断情報として使用できる。
    pub(crate) fn transaction_log(&self) -> &[VfsLogEntry] {
        &self.transaction_log
    }

    fn emit_log(&mut self, entry: VfsLogEntry) {
        debug_log!(
            "[VFS] event={:?} op={:?} req_id={:?} buf={:?} doc_id={:?} loc={:?} base_rev={:?} cur_rev={:?} detail={:?}",
            entry.event,
            entry.operation_kind,
            entry.request_id,
            entry.buf_id,
            entry.document_id,
            entry.locator,
            entry.base_revision,
            entry.current_revision,
            entry.detail,
        );
        self.transaction_log.push(entry);
    }

    fn log_binding_event(&mut self, buf_id: i32, event: VfsLogEvent, detail: Option<String>) {
        let (operation_kind, request_id, document_id, locator, base_revision, current_revision) =
            if let Some(state) = self.bindings.get(&buf_id) {
                (
                    state.binding.pending_operation.map(|pending| pending.kind),
                    state
                        .binding
                        .pending_operation
                        .map(|pending| pending.request_id),
                    state.binding.document_id.clone(),
                    state.binding.locator.clone(),
                    Some(state.binding.committed_revision),
                    Some(state.current_revision),
                )
            } else {
                (None, None, None, None, None, None)
            };

        self.emit_log(VfsLogEntry {
            event,
            operation_kind,
            request_id,
            buf_id: Some(buf_id),
            document_id,
            locator,
            base_revision,
            current_revision,
            detail,
        });
    }

    pub(crate) fn sync_buffers(&mut self, buffers: &[(i32, String)]) {
        let active_ids: BTreeSet<i32> = buffers.iter().map(|(buf_id, _)| *buf_id).collect();
        self.bindings
            .retain(|buf_id, _| active_ids.contains(buf_id));

        for (buf_id, display_name) in buffers {
            let state = self
                .bindings
                .entry(*buf_id)
                .or_insert_with(|| BufferState::local(*buf_id, display_name.clone()));
            state.binding.display_name = display_name.clone();
        }
    }

    pub(crate) fn binding(&self, buf_id: i32) -> Option<&CoreBufferBinding> {
        self.bindings.get(&buf_id).map(|state| &state.binding)
    }

    pub(crate) fn request_entry(&self, request_id: u64) -> Option<CoreRequestEntry> {
        self.requests.get(&request_id).cloned()
    }

    pub(crate) fn ledger_entries(&self) -> Vec<CoreRequestEntry> {
        self.requests.values().cloned().collect()
    }

    pub(crate) fn bind_virtual_document(
        &mut self,
        buf_id: i32,
        locator: Option<String>,
        document_id: String,
        display_name: String,
        committed_revision: u64,
    ) {
        let state = self
            .bindings
            .entry(buf_id)
            .or_insert_with(|| BufferState::local(buf_id, display_name.clone()));
        state.binding.source_kind = CoreBufferSourceKind::Virtual;
        state.binding.locator = locator;
        state.binding.document_id = Some(document_id);
        state.binding.display_name = display_name;
        state.binding.committed_revision = committed_revision;
        state.binding.last_saved_revision = None;
        state.binding.last_vfs_error = None;
        state.current_revision = committed_revision;
    }

    pub(crate) fn bind_local_buffer(
        &mut self,
        buf_id: i32,
        locator: Option<String>,
        display_name: String,
        committed_revision: u64,
    ) {
        let state = self
            .bindings
            .entry(buf_id)
            .or_insert_with(|| BufferState::local(buf_id, display_name.clone()));
        state.binding.source_kind = CoreBufferSourceKind::Local;
        state.binding.locator = locator;
        state.binding.document_id = None;
        state.binding.display_name = display_name;
        state.binding.committed_revision = committed_revision;
        state.binding.pending_operation = None;
        state.binding.deferred_close = None;
        state.binding.last_saved_revision = None;
        state.binding.last_vfs_error = None;
        state.current_revision = committed_revision;
    }

    pub(crate) fn commit_loaded_revision(&mut self, buf_id: i32, revision: u64) {
        if let Some(state) = self.bindings.get_mut(&buf_id) {
            state.binding.committed_revision = revision;
            state.binding.last_saved_revision = Some(revision);
            state.binding.last_vfs_error = None;
            state.current_revision = revision;
        }
    }

    pub(crate) fn note_buffer_revision(&mut self, buf_id: i32, revision: u64) {
        let state = self
            .bindings
            .entry(buf_id)
            .or_insert_with(|| BufferState::local(buf_id, String::new()));
        state.current_revision = revision;
    }

    pub(crate) fn issue_resolve(&mut self, target_buf_id: i32, locator: String) -> CoreVfsRequest {
        let (request_id, issued_order) = self.allocate_request_identity();
        let state = self
            .bindings
            .entry(target_buf_id)
            .or_insert_with(|| BufferState::local(target_buf_id, locator.clone()));
        state.binding.locator = Some(locator.clone());
        state.binding.pending_operation = Some(CorePendingVfsOperation {
            request_id,
            kind: CoreVfsOperationKind::Resolve,
            issued_order,
        });
        let current_revision = state.current_revision;

        self.requests.insert(
            request_id,
            CoreRequestEntry {
                request_id,
                operation_kind: CoreVfsOperationKind::Resolve,
                target_buf_id,
                document_id: None,
                locator: Some(locator.clone()),
                base_revision: None,
                status: CoreRequestStatus::Pending,
                issued_order,
            },
        );

        self.emit_log(VfsLogEntry {
            event: VfsLogEvent::RequestIssued,
            operation_kind: Some(CoreVfsOperationKind::Resolve),
            request_id: Some(request_id),
            buf_id: Some(target_buf_id),
            document_id: None,
            locator: Some(locator.clone()),
            base_revision: None,
            current_revision: Some(current_revision),
            detail: None,
        });

        CoreVfsRequest::Resolve {
            request_id,
            target_buf_id,
            locator,
        }
    }

    pub(crate) fn issue_exists(&mut self, locator: String) -> CoreVfsRequest {
        let (request_id, issued_order) = self.allocate_request_identity();
        self.requests.insert(
            request_id,
            CoreRequestEntry {
                request_id,
                operation_kind: CoreVfsOperationKind::Exists,
                target_buf_id: 0,
                document_id: None,
                locator: Some(locator.clone()),
                base_revision: None,
                status: CoreRequestStatus::Pending,
                issued_order,
            },
        );

        self.emit_log(VfsLogEntry {
            event: VfsLogEvent::RequestIssued,
            operation_kind: Some(CoreVfsOperationKind::Exists),
            request_id: Some(request_id),
            buf_id: None,
            document_id: None,
            locator: Some(locator.clone()),
            base_revision: None,
            current_revision: None,
            detail: None,
        });

        CoreVfsRequest::Exists {
            request_id,
            locator,
        }
    }

    pub(crate) fn issue_load(&mut self, target_buf_id: i32, document_id: String) -> CoreVfsRequest {
        let (request_id, issued_order) = self.allocate_request_identity();
        let state = self
            .bindings
            .entry(target_buf_id)
            .or_insert_with(|| BufferState::local(target_buf_id, document_id.clone()));
        state.binding.pending_operation = Some(CorePendingVfsOperation {
            request_id,
            kind: CoreVfsOperationKind::Load,
            issued_order,
        });
        state.binding.document_id = Some(document_id.clone());
        state.binding.source_kind = CoreBufferSourceKind::Virtual;
        let current_revision = state.current_revision;
        let locator = state.binding.locator.clone();

        self.requests.insert(
            request_id,
            CoreRequestEntry {
                request_id,
                operation_kind: CoreVfsOperationKind::Load,
                target_buf_id,
                document_id: Some(document_id.clone()),
                locator: locator.clone(),
                base_revision: None,
                status: CoreRequestStatus::Pending,
                issued_order,
            },
        );

        self.emit_log(VfsLogEntry {
            event: VfsLogEvent::RequestIssued,
            operation_kind: Some(CoreVfsOperationKind::Load),
            request_id: Some(request_id),
            buf_id: Some(target_buf_id),
            document_id: Some(document_id.clone()),
            locator,
            base_revision: None,
            current_revision: Some(current_revision),
            detail: None,
        });

        CoreVfsRequest::Load {
            request_id,
            target_buf_id,
            document_id,
        }
    }

    pub(crate) fn issue_save(
        &mut self,
        target_buf_id: i32,
        document_id: String,
        target_locator: Option<String>,
        text: String,
        force: bool,
    ) -> CoreVfsRequest {
        let (request_id, issued_order) = self.allocate_request_identity();
        let state = self
            .bindings
            .entry(target_buf_id)
            .or_insert_with(|| BufferState::local(target_buf_id, document_id.clone()));
        state.binding.source_kind = CoreBufferSourceKind::Virtual;
        state.binding.document_id = Some(document_id.clone());
        state.binding.pending_operation = Some(CorePendingVfsOperation {
            request_id,
            kind: CoreVfsOperationKind::Save,
            issued_order,
        });
        if let Some(target_locator) = &target_locator {
            state.binding.locator = Some(target_locator.clone());
        }
        let base_revision = state.current_revision;

        let save_locator = target_locator
            .clone()
            .or_else(|| state.binding.locator.clone());

        self.requests.insert(
            request_id,
            CoreRequestEntry {
                request_id,
                operation_kind: CoreVfsOperationKind::Save,
                target_buf_id,
                document_id: Some(document_id.clone()),
                locator: save_locator.clone(),
                base_revision: Some(base_revision),
                status: CoreRequestStatus::Pending,
                issued_order,
            },
        );

        self.emit_log(VfsLogEntry {
            event: VfsLogEvent::RequestIssued,
            operation_kind: Some(CoreVfsOperationKind::Save),
            request_id: Some(request_id),
            buf_id: Some(target_buf_id),
            document_id: Some(document_id.clone()),
            locator: save_locator,
            base_revision: Some(base_revision),
            current_revision: Some(base_revision),
            detail: if force {
                Some("force=true".to_string())
            } else {
                None
            },
        });

        CoreVfsRequest::Save {
            request_id,
            target_buf_id,
            document_id,
            target_locator,
            text,
            base_revision,
            force,
        }
    }

    pub(crate) fn apply_response(&mut self, response: CoreVfsResponse) -> CoreResponseApplyOutcome {
        let request_id = response.request_id();
        let Some(entry) = self.requests.get(&request_id).cloned() else {
            self.emit_log(VfsLogEntry {
                event: VfsLogEvent::UnknownRequestRejected,
                operation_kind: None,
                request_id: Some(request_id),
                buf_id: None,
                document_id: None,
                locator: None,
                base_revision: None,
                current_revision: None,
                detail: Some("unknown request_id in response".to_string()),
            });
            return CoreResponseApplyOutcome::UnknownRequest;
        };
        let target_buf_id = entry.target_buf_id;
        let current_revision = self
            .bindings
            .get(&target_buf_id)
            .map(|s| s.current_revision);

        let actual_kind = response.operation_kind();
        if entry.operation_kind != actual_kind {
            self.requests
                .get_mut(&request_id)
                .expect("request should exist")
                .status = CoreRequestStatus::ProtocolMismatch {
                expected: entry.operation_kind,
                actual: actual_kind,
            };
            self.clear_pending_if_matches(target_buf_id, request_id);
            self.record_buffer_error(
                target_buf_id,
                CoreVfsError {
                    kind: CoreVfsErrorKind::InvalidResponse,
                    message: Some(format!(
                        "expected {:?} response but received {:?}",
                        entry.operation_kind, actual_kind
                    )),
                },
            );
            self.emit_log(VfsLogEntry {
                event: VfsLogEvent::ProtocolMismatchRejected,
                operation_kind: Some(entry.operation_kind),
                request_id: Some(request_id),
                buf_id: Some(target_buf_id),
                document_id: entry.document_id.clone(),
                locator: entry.locator.clone(),
                base_revision: entry.base_revision,
                current_revision,
                detail: Some(format!(
                    "expected {:?} but received {:?}",
                    entry.operation_kind, actual_kind
                )),
            });
            return CoreResponseApplyOutcome::ProtocolMismatchRejected;
        }

        match response {
            CoreVfsResponse::Resolved {
                document_id,
                display_name,
                ..
            } => {
                let locator = entry.locator;
                self.requests
                    .get_mut(&request_id)
                    .expect("request should exist")
                    .status = CoreRequestStatus::Succeeded;
                self.clear_pending_if_matches(target_buf_id, request_id);
                self.bind_virtual_document(
                    target_buf_id,
                    locator.clone(),
                    document_id.clone(),
                    display_name,
                    0,
                );
                self.emit_log(VfsLogEntry {
                    event: VfsLogEvent::ResponseApplied,
                    operation_kind: Some(CoreVfsOperationKind::Resolve),
                    request_id: Some(request_id),
                    buf_id: Some(target_buf_id),
                    document_id: Some(document_id),
                    locator,
                    base_revision: None,
                    current_revision,
                    detail: None,
                });
                CoreResponseApplyOutcome::Applied
            }
            CoreVfsResponse::ResolvedLocalFallback { locator, .. } => {
                self.requests
                    .get_mut(&request_id)
                    .expect("request should exist")
                    .status = CoreRequestStatus::Succeeded;
                self.clear_pending_if_matches(target_buf_id, request_id);
                self.bind_local_buffer(target_buf_id, Some(locator.clone()), locator.clone(), 0);
                self.emit_log(VfsLogEntry {
                    event: VfsLogEvent::LocalFallbackSelected,
                    operation_kind: Some(CoreVfsOperationKind::Resolve),
                    request_id: Some(request_id),
                    buf_id: Some(target_buf_id),
                    document_id: None,
                    locator: Some(locator),
                    base_revision: None,
                    current_revision,
                    detail: None,
                });
                CoreResponseApplyOutcome::Applied
            }
            CoreVfsResponse::ResolvedMissing { locator, .. } => {
                let error = CoreVfsError {
                    kind: CoreVfsErrorKind::NotFound,
                    message: Some(format!("missing locator: {}", locator)),
                };
                self.requests
                    .get_mut(&request_id)
                    .expect("request should exist")
                    .status = CoreRequestStatus::Failed(error.clone());
                self.clear_pending_if_matches(target_buf_id, request_id);
                self.record_buffer_error(target_buf_id, error);
                self.emit_log(VfsLogEntry {
                    event: VfsLogEvent::ResponseApplied,
                    operation_kind: Some(CoreVfsOperationKind::Resolve),
                    request_id: Some(request_id),
                    buf_id: Some(target_buf_id),
                    document_id: None,
                    locator: Some(locator),
                    base_revision: None,
                    current_revision,
                    detail: Some("resolved as missing".to_string()),
                });
                CoreResponseApplyOutcome::Applied
            }
            CoreVfsResponse::ExistsResult { exists, .. } => {
                self.requests
                    .get_mut(&request_id)
                    .expect("request should exist")
                    .status = if exists {
                    CoreRequestStatus::Succeeded
                } else {
                    CoreRequestStatus::Failed(CoreVfsError {
                        kind: CoreVfsErrorKind::NotFound,
                        message: Some("resolved path does not exist".to_string()),
                    })
                };
                self.emit_log(VfsLogEntry {
                    event: VfsLogEvent::ResponseApplied,
                    operation_kind: Some(CoreVfsOperationKind::Exists),
                    request_id: Some(request_id),
                    buf_id: Some(target_buf_id),
                    document_id: entry.document_id.clone(),
                    locator: entry.locator.clone(),
                    base_revision: None,
                    current_revision,
                    detail: Some(format!("exists={}", exists)),
                });
                CoreResponseApplyOutcome::Applied
            }
            CoreVfsResponse::Loaded { document_id, .. } => {
                let locator = entry.locator;
                let display_name = self
                    .bindings
                    .get(&target_buf_id)
                    .map(|state| state.binding.display_name.clone())
                    .unwrap_or_else(|| document_id.clone());
                self.requests
                    .get_mut(&request_id)
                    .expect("request should exist")
                    .status = CoreRequestStatus::Succeeded;
                self.clear_pending_if_matches(target_buf_id, request_id);
                self.bind_virtual_document(
                    target_buf_id,
                    locator.clone(),
                    document_id.clone(),
                    display_name,
                    0,
                );
                self.emit_log(VfsLogEntry {
                    event: VfsLogEvent::ResponseApplied,
                    operation_kind: Some(CoreVfsOperationKind::Load),
                    request_id: Some(request_id),
                    buf_id: Some(target_buf_id),
                    document_id: Some(document_id),
                    locator,
                    base_revision: None,
                    current_revision,
                    detail: None,
                });
                CoreResponseApplyOutcome::Applied
            }
            CoreVfsResponse::Saved { document_id, .. } => {
                let Some(base_revision) = entry.base_revision else {
                    self.requests
                        .get_mut(&request_id)
                        .expect("request should exist")
                        .status = CoreRequestStatus::ProtocolMismatch {
                        expected: CoreVfsOperationKind::Save,
                        actual: CoreVfsOperationKind::Save,
                    };
                    self.clear_pending_if_matches(target_buf_id, request_id);
                    self.record_buffer_error(
                        target_buf_id,
                        CoreVfsError {
                            kind: CoreVfsErrorKind::InvalidResponse,
                            message: Some("save response missing base revision".to_string()),
                        },
                    );
                    self.emit_log(VfsLogEntry {
                        event: VfsLogEvent::ProtocolMismatchRejected,
                        operation_kind: Some(CoreVfsOperationKind::Save),
                        request_id: Some(request_id),
                        buf_id: Some(target_buf_id),
                        document_id: Some(document_id),
                        locator: entry.locator.clone(),
                        base_revision: None,
                        current_revision,
                        detail: Some("save response missing base revision".to_string()),
                    });
                    return CoreResponseApplyOutcome::ProtocolMismatchRejected;
                };

                let Some(state) = self.bindings.get_mut(&target_buf_id) else {
                    self.requests
                        .get_mut(&request_id)
                        .expect("request should exist")
                        .status = CoreRequestStatus::Failed(CoreVfsError {
                        kind: CoreVfsErrorKind::HostUnavailable,
                        message: Some(
                            "target buffer disappeared before save completed".to_string(),
                        ),
                    });
                    self.emit_log(VfsLogEntry {
                        event: VfsLogEvent::ResponseApplied,
                        operation_kind: Some(CoreVfsOperationKind::Save),
                        request_id: Some(request_id),
                        buf_id: Some(target_buf_id),
                        document_id: Some(document_id),
                        locator: entry.locator.clone(),
                        base_revision: Some(base_revision),
                        current_revision: None,
                        detail: Some("target buffer disappeared".to_string()),
                    });
                    return CoreResponseApplyOutcome::Applied;
                };

                if state.binding.document_id.as_deref() != Some(document_id.as_str()) {
                    let cur_rev = state.current_revision;
                    self.requests
                        .get_mut(&request_id)
                        .expect("request should exist")
                        .status = CoreRequestStatus::Failed(CoreVfsError {
                        kind: CoreVfsErrorKind::InvalidResponse,
                        message: Some("save response document_id mismatch".to_string()),
                    });
                    self.clear_pending_if_matches(target_buf_id, request_id);
                    self.record_buffer_error(
                        target_buf_id,
                        CoreVfsError {
                            kind: CoreVfsErrorKind::InvalidResponse,
                            message: Some("save response document_id mismatch".to_string()),
                        },
                    );
                    self.emit_log(VfsLogEntry {
                        event: VfsLogEvent::ProtocolMismatchRejected,
                        operation_kind: Some(CoreVfsOperationKind::Save),
                        request_id: Some(request_id),
                        buf_id: Some(target_buf_id),
                        document_id: Some(document_id),
                        locator: entry.locator.clone(),
                        base_revision: Some(base_revision),
                        current_revision: Some(cur_rev),
                        detail: Some("document_id mismatch".to_string()),
                    });
                    return CoreResponseApplyOutcome::ProtocolMismatchRejected;
                }

                if state.current_revision != base_revision {
                    let cur_rev = state.current_revision;
                    let reason = format!(
                        "save response for revision {} arrived after revision {}",
                        base_revision, cur_rev
                    );
                    self.requests
                        .get_mut(&request_id)
                        .expect("request should exist")
                        .status = CoreRequestStatus::Stale {
                        reason: reason.clone(),
                    };
                    self.clear_pending_if_matches(target_buf_id, request_id);
                    self.record_buffer_error(
                        target_buf_id,
                        CoreVfsError {
                            kind: CoreVfsErrorKind::RevisionMismatch,
                            message: Some(reason),
                        },
                    );
                    self.emit_log(VfsLogEntry {
                        event: VfsLogEvent::StaleRejected,
                        operation_kind: Some(CoreVfsOperationKind::Save),
                        request_id: Some(request_id),
                        buf_id: Some(target_buf_id),
                        document_id: Some(document_id),
                        locator: entry.locator.clone(),
                        base_revision: Some(base_revision),
                        current_revision: Some(cur_rev),
                        detail: Some(format!(
                            "revision mismatch: base={} current={}",
                            base_revision, cur_rev
                        )),
                    });
                    return CoreResponseApplyOutcome::StaleRejected;
                }

                self.requests
                    .get_mut(&request_id)
                    .expect("request should exist")
                    .status = CoreRequestStatus::Succeeded;
                state.binding.committed_revision = base_revision;
                state.binding.last_saved_revision = Some(base_revision);
                state.binding.last_vfs_error = None;
                self.clear_pending_if_matches(target_buf_id, request_id);
                self.emit_log(VfsLogEntry {
                    event: VfsLogEvent::ResponseApplied,
                    operation_kind: Some(CoreVfsOperationKind::Save),
                    request_id: Some(request_id),
                    buf_id: Some(target_buf_id),
                    document_id: Some(document_id),
                    locator: entry.locator.clone(),
                    base_revision: Some(base_revision),
                    current_revision: Some(base_revision),
                    detail: None,
                });
                CoreResponseApplyOutcome::Applied
            }
            CoreVfsResponse::Failed { error, .. } => {
                self.requests
                    .get_mut(&request_id)
                    .expect("request should exist")
                    .status = CoreRequestStatus::Failed(error.clone());
                self.clear_pending_if_matches(target_buf_id, request_id);
                self.record_buffer_error(target_buf_id, error.clone());
                self.emit_log(VfsLogEntry {
                    event: VfsLogEvent::ResponseApplied,
                    operation_kind: Some(entry.operation_kind),
                    request_id: Some(request_id),
                    buf_id: Some(target_buf_id),
                    document_id: entry.document_id.clone(),
                    locator: entry.locator.clone(),
                    base_revision: entry.base_revision,
                    current_revision,
                    detail: Some(format!("failed: {:?} - {:?}", error.kind, error.message)),
                });
                CoreResponseApplyOutcome::Applied
            }
            CoreVfsResponse::Cancelled { .. } => {
                self.requests
                    .get_mut(&request_id)
                    .expect("request should exist")
                    .status = CoreRequestStatus::Cancelled;
                self.clear_pending_if_matches(target_buf_id, request_id);
                self.record_buffer_error(
                    target_buf_id,
                    CoreVfsError {
                        kind: CoreVfsErrorKind::Cancelled,
                        message: Some("request cancelled".to_string()),
                    },
                );
                self.emit_log(VfsLogEntry {
                    event: VfsLogEvent::ResponseApplied,
                    operation_kind: Some(entry.operation_kind),
                    request_id: Some(request_id),
                    buf_id: Some(target_buf_id),
                    document_id: entry.document_id.clone(),
                    locator: entry.locator.clone(),
                    base_revision: entry.base_revision,
                    current_revision,
                    detail: Some("cancelled".to_string()),
                });
                CoreResponseApplyOutcome::Applied
            }
            CoreVfsResponse::TimedOut { .. } => {
                self.requests
                    .get_mut(&request_id)
                    .expect("request should exist")
                    .status = CoreRequestStatus::TimedOut;
                self.clear_pending_if_matches(target_buf_id, request_id);
                self.record_buffer_error(
                    target_buf_id,
                    CoreVfsError {
                        kind: CoreVfsErrorKind::TimedOut,
                        message: Some("request timed out".to_string()),
                    },
                );
                self.emit_log(VfsLogEntry {
                    event: VfsLogEvent::ResponseApplied,
                    operation_kind: Some(entry.operation_kind),
                    request_id: Some(request_id),
                    buf_id: Some(target_buf_id),
                    document_id: entry.document_id.clone(),
                    locator: entry.locator.clone(),
                    base_revision: entry.base_revision,
                    current_revision,
                    detail: Some("timed out".to_string()),
                });
                CoreResponseApplyOutcome::Applied
            }
        }
    }

    fn allocate_request_identity(&mut self) -> (u64, u64) {
        let request_id = self.next_request_id;
        let issued_order = self.next_issued_order;
        self.next_request_id += 1;
        self.next_issued_order += 1;
        (request_id, issued_order)
    }

    pub(crate) fn is_vfs_buffer(&self, buf_id: i32) -> bool {
        self.bindings
            .get(&buf_id)
            .map(|state| state.binding.source_kind == CoreBufferSourceKind::Virtual)
            .unwrap_or(false)
    }

    pub(crate) fn has_pending_save(&self, buf_id: i32) -> bool {
        self.bindings
            .get(&buf_id)
            .and_then(|state| state.binding.pending_operation.as_ref())
            .map(|pending| pending.kind == CoreVfsOperationKind::Save)
            .unwrap_or(false)
    }

    pub(crate) fn deferred_close(&self, buf_id: i32) -> Option<CoreDeferredClose> {
        self.bindings
            .get(&buf_id)
            .and_then(|state| state.binding.deferred_close)
    }

    pub(crate) fn set_deferred_close(&mut self, buf_id: i32, close: CoreDeferredClose) {
        if let Some(state) = self.bindings.get_mut(&buf_id) {
            debug_log!(
                "[DEBUG] DocumentCoordinator::set_deferred_close: buf_id={} close={:?}",
                buf_id,
                close
            );
            state.binding.deferred_close = Some(close);
        }
        self.log_binding_event(
            buf_id,
            VfsLogEvent::QuitDeferred,
            Some(format!("deferred_close={close:?}")),
        );
    }

    pub(crate) fn clear_deferred_close(&mut self, buf_id: i32, reason: &str) {
        if let Some(state) = self.bindings.get_mut(&buf_id) {
            debug_log!(
                "[DEBUG] DocumentCoordinator::clear_deferred_close: buf_id={}",
                buf_id
            );
            state.binding.deferred_close = None;
        }
        self.log_binding_event(buf_id, VfsLogEvent::QuitResumed, Some(reason.to_string()));
    }

    pub(crate) fn log_quit_denied(&mut self, buf_id: i32, reason: &str) {
        self.log_binding_event(buf_id, VfsLogEvent::QuitDenied, Some(reason.to_string()));
    }

    pub(crate) fn buffer_text_snapshot(&self, buf_id: i32) -> Option<(String, u64)> {
        // coordinator は text を保持しない。呼び出し元が text を渡す必要がある。
        // これは document_id と current_revision を返すためのヘルパー。
        self.bindings.get(&buf_id).and_then(|state| {
            state
                .binding
                .document_id
                .as_ref()
                .map(|doc_id| (doc_id.clone(), state.current_revision))
        })
    }

    fn clear_pending_if_matches(&mut self, buf_id: i32, request_id: u64) {
        if let Some(state) = self.bindings.get_mut(&buf_id)
            && state
                .binding
                .pending_operation
                .as_ref()
                .map(|pending| pending.request_id == request_id)
                .unwrap_or(false)
        {
            state.binding.pending_operation = None;
        }
    }

    fn record_buffer_error(&mut self, buf_id: i32, error: CoreVfsError) {
        if let Some(state) = self.bindings.get_mut(&buf_id) {
            state.binding.last_vfs_error = Some(error);
        }
    }
}

impl CoreVfsResponse {
    pub(crate) fn request_id(&self) -> u64 {
        match self {
            CoreVfsResponse::Resolved { request_id, .. }
            | CoreVfsResponse::ResolvedLocalFallback { request_id, .. }
            | CoreVfsResponse::ResolvedMissing { request_id, .. }
            | CoreVfsResponse::ExistsResult { request_id, .. }
            | CoreVfsResponse::Loaded { request_id, .. }
            | CoreVfsResponse::Saved { request_id, .. }
            | CoreVfsResponse::Failed { request_id, .. }
            | CoreVfsResponse::Cancelled { request_id }
            | CoreVfsResponse::TimedOut { request_id } => *request_id,
        }
    }

    pub(crate) fn operation_kind(&self) -> CoreVfsOperationKind {
        match self {
            CoreVfsResponse::Resolved { .. }
            | CoreVfsResponse::ResolvedLocalFallback { .. }
            | CoreVfsResponse::ResolvedMissing { .. } => CoreVfsOperationKind::Resolve,
            CoreVfsResponse::ExistsResult { .. } => CoreVfsOperationKind::Exists,
            CoreVfsResponse::Loaded { .. } => CoreVfsOperationKind::Load,
            CoreVfsResponse::Saved { .. } => CoreVfsOperationKind::Save,
            CoreVfsResponse::Failed { error, .. } => match error.kind {
                CoreVfsErrorKind::ResolveFailed | CoreVfsErrorKind::NotFound => {
                    CoreVfsOperationKind::Resolve
                }
                CoreVfsErrorKind::ExistsFailed => CoreVfsOperationKind::Exists,
                CoreVfsErrorKind::LoadFailed => CoreVfsOperationKind::Load,
                CoreVfsErrorKind::SaveFailed | CoreVfsErrorKind::RevisionMismatch => {
                    CoreVfsOperationKind::Save
                }
                CoreVfsErrorKind::InvalidResponse
                | CoreVfsErrorKind::HostUnavailable
                | CoreVfsErrorKind::Cancelled
                | CoreVfsErrorKind::TimedOut => CoreVfsOperationKind::Save,
            },
            CoreVfsResponse::Cancelled { .. } | CoreVfsResponse::TimedOut { .. } => {
                CoreVfsOperationKind::Save
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_ids_are_unique_and_monotonic() {
        let mut coordinator = DocumentCoordinator::new();
        coordinator.sync_buffers(&[(3, "alpha".to_string())]);

        let first = coordinator.issue_resolve(3, "mem://alpha".to_string());
        let second = coordinator.issue_exists("mem://alpha".to_string());

        assert!(matches!(
            first,
            CoreVfsRequest::Resolve {
                request_id: 1,
                target_buf_id: 3,
                ..
            }
        ));
        assert!(matches!(
            second,
            CoreVfsRequest::Exists { request_id: 2, .. }
        ));
        assert_eq!(coordinator.ledger_entries().len(), 2);
    }

    #[test]
    fn protocol_mismatch_marks_request_and_preserves_binding() {
        let mut coordinator = DocumentCoordinator::new();
        coordinator.sync_buffers(&[(5, "draft".to_string())]);

        let request = coordinator.issue_load(5, "doc://draft".to_string());
        let request_id = match request {
            CoreVfsRequest::Load { request_id, .. } => request_id,
            other => panic!("unexpected request: {:?}", other),
        };

        let outcome = coordinator.apply_response(CoreVfsResponse::ExistsResult {
            request_id,
            exists: true,
        });

        assert_eq!(outcome, CoreResponseApplyOutcome::ProtocolMismatchRejected);
        assert!(matches!(
            coordinator.ledger_entries()[0].status,
            CoreRequestStatus::ProtocolMismatch {
                expected: CoreVfsOperationKind::Load,
                actual: CoreVfsOperationKind::Exists,
            }
        ));
        let binding = coordinator.binding(5).expect("binding should remain");
        assert!(binding.pending_operation.is_none());
        assert!(matches!(
            binding.last_vfs_error,
            Some(CoreVfsError {
                kind: CoreVfsErrorKind::InvalidResponse,
                ..
            })
        ));
    }

    #[test]
    fn issue_save_captures_current_revision_as_base() {
        let mut coordinator = DocumentCoordinator::new();
        coordinator.sync_buffers(&[(10, "notes".to_string())]);
        coordinator.bind_virtual_document(
            10,
            Some("mem://notes".to_string()),
            "doc://notes".to_string(),
            "notes".to_string(),
            3,
        );
        coordinator.note_buffer_revision(10, 3);

        let request = coordinator.issue_save(
            10,
            "doc://notes".to_string(),
            None,
            "content v3".to_string(),
            false,
        );

        match request {
            CoreVfsRequest::Save {
                target_buf_id,
                document_id,
                target_locator,
                text,
                base_revision,
                force,
                ..
            } => {
                assert_eq!(target_buf_id, 10);
                assert_eq!(document_id, "doc://notes");
                assert!(target_locator.is_none());
                assert_eq!(text, "content v3");
                assert_eq!(base_revision, 3);
                assert!(!force);
            }
            other => panic!("expected Save request, got {:?}", other),
        }

        let binding = coordinator.binding(10).expect("binding should exist");
        assert!(matches!(
            binding.pending_operation,
            Some(CorePendingVfsOperation {
                kind: CoreVfsOperationKind::Save,
                ..
            })
        ));
    }

    #[test]
    fn issue_save_with_target_locator_passes_save_as_target() {
        let mut coordinator = DocumentCoordinator::new();
        coordinator.sync_buffers(&[(11, "draft".to_string())]);
        coordinator.bind_virtual_document(
            11,
            Some("mem://draft".to_string()),
            "doc://draft".to_string(),
            "draft".to_string(),
            1,
        );
        coordinator.note_buffer_revision(11, 1);

        let request = coordinator.issue_save(
            11,
            "doc://draft".to_string(),
            Some("mem://backup/draft".to_string()),
            "draft text".to_string(),
            false,
        );

        match request {
            CoreVfsRequest::Save {
                target_locator,
                document_id,
                ..
            } => {
                assert_eq!(target_locator, Some("mem://backup/draft".to_string()));
                assert_eq!(document_id, "doc://draft");
            }
            other => panic!("expected Save request, got {:?}", other),
        }
    }

    #[test]
    fn save_success_with_matching_revision_clears_dirty() {
        let mut coordinator = DocumentCoordinator::new();
        coordinator.sync_buffers(&[(12, "clean".to_string())]);
        coordinator.bind_virtual_document(
            12,
            Some("mem://clean".to_string()),
            "doc://clean".to_string(),
            "clean".to_string(),
            5,
        );
        coordinator.note_buffer_revision(12, 5);

        let request = coordinator.issue_save(
            12,
            "doc://clean".to_string(),
            None,
            "clean text".to_string(),
            false,
        );
        let request_id = match request {
            CoreVfsRequest::Save { request_id, .. } => request_id,
            other => panic!("expected Save, got {:?}", other),
        };

        let outcome = coordinator.apply_response(CoreVfsResponse::Saved {
            request_id,
            document_id: "doc://clean".to_string(),
        });

        assert_eq!(outcome, CoreResponseApplyOutcome::Applied);
        let binding = coordinator.binding(12).expect("binding should exist");
        assert_eq!(binding.committed_revision, 5);
        assert_eq!(binding.last_saved_revision, Some(5));
        assert!(binding.last_vfs_error.is_none());
        assert!(binding.pending_operation.is_none());
    }

    #[test]
    fn save_failure_keeps_dirty_and_records_error() {
        let mut coordinator = DocumentCoordinator::new();
        coordinator.sync_buffers(&[(13, "failing".to_string())]);
        coordinator.bind_virtual_document(
            13,
            Some("mem://failing".to_string()),
            "doc://failing".to_string(),
            "failing".to_string(),
            2,
        );
        coordinator.note_buffer_revision(13, 2);

        let request = coordinator.issue_save(
            13,
            "doc://failing".to_string(),
            None,
            "failing text".to_string(),
            false,
        );
        let request_id = match request {
            CoreVfsRequest::Save { request_id, .. } => request_id,
            other => panic!("expected Save, got {:?}", other),
        };

        let outcome = coordinator.apply_response(CoreVfsResponse::Failed {
            request_id,
            error: CoreVfsError {
                kind: CoreVfsErrorKind::SaveFailed,
                message: Some("permission denied".to_string()),
            },
        });

        assert_eq!(outcome, CoreResponseApplyOutcome::Applied);
        let binding = coordinator.binding(13).expect("binding should exist");
        assert_eq!(binding.committed_revision, 2);
        assert!(binding.last_saved_revision.is_none());
        assert!(matches!(
            binding.last_vfs_error,
            Some(CoreVfsError {
                kind: CoreVfsErrorKind::SaveFailed,
                ..
            })
        ));
        assert!(binding.pending_operation.is_none());
    }

    #[test]
    fn save_cancel_keeps_dirty_and_records_cancellation() {
        let mut coordinator = DocumentCoordinator::new();
        coordinator.sync_buffers(&[(14, "cancel".to_string())]);
        coordinator.bind_virtual_document(
            14,
            Some("mem://cancel".to_string()),
            "doc://cancel".to_string(),
            "cancel".to_string(),
            1,
        );
        coordinator.note_buffer_revision(14, 1);

        let request = coordinator.issue_save(
            14,
            "doc://cancel".to_string(),
            None,
            "cancel text".to_string(),
            false,
        );
        let request_id = match request {
            CoreVfsRequest::Save { request_id, .. } => request_id,
            other => panic!("expected Save, got {:?}", other),
        };

        let outcome = coordinator.apply_response(CoreVfsResponse::Cancelled { request_id });
        assert_eq!(outcome, CoreResponseApplyOutcome::Applied);
        let binding = coordinator.binding(14).expect("binding should exist");
        assert!(binding.last_saved_revision.is_none());
        assert!(matches!(
            binding.last_vfs_error,
            Some(CoreVfsError {
                kind: CoreVfsErrorKind::Cancelled,
                ..
            })
        ));
    }

    #[test]
    fn save_timeout_keeps_dirty_and_records_timeout() {
        let mut coordinator = DocumentCoordinator::new();
        coordinator.sync_buffers(&[(15, "timeout".to_string())]);
        coordinator.bind_virtual_document(
            15,
            Some("mem://timeout".to_string()),
            "doc://timeout".to_string(),
            "timeout".to_string(),
            1,
        );
        coordinator.note_buffer_revision(15, 1);

        let request = coordinator.issue_save(
            15,
            "doc://timeout".to_string(),
            None,
            "timeout text".to_string(),
            false,
        );
        let request_id = match request {
            CoreVfsRequest::Save { request_id, .. } => request_id,
            other => panic!("expected Save, got {:?}", other),
        };

        let outcome = coordinator.apply_response(CoreVfsResponse::TimedOut { request_id });
        assert_eq!(outcome, CoreResponseApplyOutcome::Applied);
        let binding = coordinator.binding(15).expect("binding should exist");
        assert!(binding.last_saved_revision.is_none());
        assert!(matches!(
            binding.last_vfs_error,
            Some(CoreVfsError {
                kind: CoreVfsErrorKind::TimedOut,
                ..
            })
        ));
    }

    #[test]
    fn multiple_concurrent_saves_do_not_let_stale_success_clear_dirty() {
        let mut coordinator = DocumentCoordinator::new();
        coordinator.sync_buffers(&[(16, "racing".to_string())]);
        coordinator.bind_virtual_document(
            16,
            Some("mem://racing".to_string()),
            "doc://racing".to_string(),
            "racing".to_string(),
            1,
        );
        coordinator.note_buffer_revision(16, 1);

        // 最初の save request (base_revision=1)
        let first_request = coordinator.issue_save(
            16,
            "doc://racing".to_string(),
            None,
            "version 1".to_string(),
            false,
        );
        let first_id = match first_request {
            CoreVfsRequest::Save { request_id, .. } => request_id,
            other => panic!("expected Save, got {:?}", other),
        };

        // ユーザーが編集して revision が進む
        coordinator.note_buffer_revision(16, 2);

        // 2つ目の save request (base_revision=2)
        let second_request = coordinator.issue_save(
            16,
            "doc://racing".to_string(),
            None,
            "version 2".to_string(),
            false,
        );
        let second_id = match second_request {
            CoreVfsRequest::Save { request_id, .. } => request_id,
            other => panic!("expected Save, got {:?}", other),
        };

        // 最初の save が成功を返す -> revision mismatch で stale
        let outcome = coordinator.apply_response(CoreVfsResponse::Saved {
            request_id: first_id,
            document_id: "doc://racing".to_string(),
        });
        assert_eq!(outcome, CoreResponseApplyOutcome::StaleRejected);

        // 2つ目の save が成功を返す -> revision 一致で適用
        let outcome = coordinator.apply_response(CoreVfsResponse::Saved {
            request_id: second_id,
            document_id: "doc://racing".to_string(),
        });
        assert_eq!(outcome, CoreResponseApplyOutcome::Applied);
        let binding = coordinator.binding(16).expect("binding should exist");
        assert_eq!(binding.committed_revision, 2);
        assert_eq!(binding.last_saved_revision, Some(2));
    }

    // --- Task 3.3: quit gate tests ---

    #[test]
    fn has_pending_save_reports_true_when_save_pending() {
        let mut coordinator = DocumentCoordinator::new();
        coordinator.sync_buffers(&[(20, "pending".to_string())]);
        coordinator.bind_virtual_document(
            20,
            Some("mem://pending".to_string()),
            "doc://pending".to_string(),
            "pending".to_string(),
            1,
        );
        coordinator.note_buffer_revision(20, 1);

        assert!(!coordinator.has_pending_save(20));

        coordinator.issue_save(
            20,
            "doc://pending".to_string(),
            None,
            "pending text".to_string(),
            false,
        );

        assert!(coordinator.has_pending_save(20));
    }

    #[test]
    fn set_deferred_close_and_check_deferred_close() {
        let mut coordinator = DocumentCoordinator::new();
        coordinator.sync_buffers(&[(21, "closing".to_string())]);
        coordinator.bind_virtual_document(
            21,
            Some("mem://closing".to_string()),
            "doc://closing".to_string(),
            "closing".to_string(),
            1,
        );

        assert!(coordinator.deferred_close(21).is_none());

        coordinator.set_deferred_close(21, CoreDeferredClose::SaveAndClose);
        assert_eq!(
            coordinator.deferred_close(21),
            Some(CoreDeferredClose::SaveAndClose)
        );
    }

    #[test]
    fn clear_deferred_close_removes_deferred_state() {
        let mut coordinator = DocumentCoordinator::new();
        coordinator.sync_buffers(&[(22, "clear".to_string())]);
        coordinator.bind_virtual_document(
            22,
            Some("mem://clear".to_string()),
            "doc://clear".to_string(),
            "clear".to_string(),
            1,
        );

        coordinator.set_deferred_close(22, CoreDeferredClose::Quit);
        coordinator.clear_deferred_close(22, "test clear");
        assert!(coordinator.deferred_close(22).is_none());
    }

    #[test]
    fn is_vfs_buffer_returns_true_for_virtual_false_for_local() {
        let mut coordinator = DocumentCoordinator::new();
        coordinator.sync_buffers(&[(30, "local".to_string()), (31, "virtual".to_string())]);
        coordinator.bind_virtual_document(
            31,
            Some("mem://virtual".to_string()),
            "doc://virtual".to_string(),
            "virtual".to_string(),
            1,
        );

        assert!(!coordinator.is_vfs_buffer(30));
        assert!(coordinator.is_vfs_buffer(31));
        assert!(!coordinator.is_vfs_buffer(999));
    }

    #[test]
    fn stale_save_response_keeps_committed_revision_dirty_side() {
        let mut coordinator = DocumentCoordinator::new();
        coordinator.sync_buffers(&[(7, "memo".to_string())]);
        coordinator.bind_virtual_document(
            7,
            Some("mem://memo".to_string()),
            "doc://memo".to_string(),
            "memo".to_string(),
            4,
        );
        coordinator.note_buffer_revision(7, 4);

        let request = coordinator.issue_save(
            7,
            "doc://memo".to_string(),
            None,
            "updated".to_string(),
            false,
        );
        let request_id = match request {
            CoreVfsRequest::Save {
                request_id,
                base_revision,
                ..
            } => {
                assert_eq!(base_revision, 4);
                request_id
            }
            other => panic!("unexpected request: {:?}", other),
        };

        coordinator.note_buffer_revision(7, 5);
        let outcome = coordinator.apply_response(CoreVfsResponse::Saved {
            request_id,
            document_id: "doc://memo".to_string(),
        });

        assert_eq!(outcome, CoreResponseApplyOutcome::StaleRejected);
        let binding = coordinator.binding(7).expect("binding should exist");
        assert_eq!(binding.committed_revision, 4);
        assert_eq!(binding.last_saved_revision, None);
        assert!(binding.pending_operation.is_none());
        assert!(matches!(
            binding.last_vfs_error,
            Some(CoreVfsError {
                kind: CoreVfsErrorKind::RevisionMismatch,
                ..
            })
        ));
        assert!(matches!(
            coordinator.ledger_entries()[0].status,
            CoreRequestStatus::Stale { .. }
        ));
    }
}
