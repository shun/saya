use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use tokio::sync::{mpsc, oneshot};

use crate::features::lsp::runtime_bridge::{LspRuntimeBridgeRequest, LspRuntimeBridgeResponse};
use crate::features::selector::host_adapter::SelectorHostViewAdapter;
use crate::features::selector::runtime::{
    RuntimeSelectorControlRequest, RuntimeSelectorControllerCommand, SelectorViewBackend,
};
use crate::features::selector::tui_state::SelectorTuiProjectionSink;
use crate::presentation::overlay::effect::RuntimePresentationIntent;
use crate::runtime::callback_registry_seed::CallbackRegistrySeed;
use crate::runtime::live::{
    HostCapabilityBridge, ReadonlyBufferSnapshot, ReadonlyEditorSnapshot, ReadonlyWindowSnapshot,
    RuntimeCommandError, RuntimeDispatchError, RuntimeDispatchReport, RuntimeEventPayload,
    RuntimeFilerCurrentEntry, RuntimeFilerEntry, RuntimeFilerError, RuntimeFilerErrorKind,
    RuntimeFilerListOptions, RuntimeFilerOperation, RuntimeFilerOperationKind,
    RuntimeFilerOperationReport, RuntimeFloatOpenRequest, RuntimeFloatSnapshot, RuntimeInitError,
    RuntimeInputPromptRequest, RuntimeInputPromptResponse, RuntimeMode, SayaLiveRuntime,
};
use crate::runtime::message::runtime_callback_failure_message;
use crate::runtime::refresh::runtime_dispatch_requests_redraw;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuntimeDispatchOutcome {
    pub transient_message: Option<String>,
    pub requires_redraw: bool,
    pub shutdown_intent: Option<RuntimeShutdownIntent>,
    pub presentation_intents: Vec<RuntimePresentationIntent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuntimeCommandEffect {
    pub transient_message: Option<String>,
    pub follow_up_events: Vec<RuntimeEventPayload>,
    pub shutdown_intent: Option<RuntimeShutdownIntent>,
    pub presentation_intents: Vec<RuntimePresentationIntent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeShutdownIntent {
    UserQuit,
    UserForceQuit,
}

pub trait RuntimeHostSession {
    fn current_buffer_snapshot(&mut self) -> ReadonlyBufferSnapshot;
    fn current_window_snapshot(&mut self) -> ReadonlyWindowSnapshot;
    fn open_float(
        &mut self,
        request: RuntimeFloatOpenRequest,
    ) -> Result<RuntimeFloatSnapshot, RuntimeCommandError> {
        log::debug!(
            "[runtime_integration][window] openFloat unsupported by host session: content={:?}",
            request.content
        );
        Err(RuntimeCommandError::UnknownCommand {
            name: "window.openFloat".to_string(),
        })
    }
    fn close_float(&mut self, id: u64) -> Result<bool, RuntimeCommandError> {
        log::debug!(
            "[runtime_integration][window] close unsupported by host session: id={}",
            id
        );
        Err(RuntimeCommandError::UnknownCommand {
            name: "window.close".to_string(),
        })
    }
    fn focus_float(&mut self, id: u64) -> Result<bool, RuntimeCommandError> {
        log::debug!(
            "[runtime_integration][window] focus unsupported by host session: id={}",
            id
        );
        Err(RuntimeCommandError::UnknownCommand {
            name: "window.focus".to_string(),
        })
    }
    fn list_float_snapshots(&mut self) -> Result<Vec<RuntimeFloatSnapshot>, RuntimeCommandError> {
        log::debug!("[runtime_integration][window] float snapshots unsupported by host session");
        Err(RuntimeCommandError::UnknownCommand {
            name: "window.floats".to_string(),
        })
    }
    fn current_editor_snapshot(&mut self) -> ReadonlyEditorSnapshot;
    fn current_filer_entry(
        &mut self,
    ) -> Result<Option<RuntimeFilerCurrentEntry>, RuntimeFilerError> {
        Ok(None)
    }
    fn list_filer_entries(
        &mut self,
        path: std::path::PathBuf,
        options: RuntimeFilerListOptions,
    ) -> Result<Vec<RuntimeFilerEntry>, RuntimeFilerError> {
        crate::runtime::live::list_local_filer_entries(path, options)
    }
    fn execute_filer_operation(
        &mut self,
        operation: RuntimeFilerOperation,
    ) -> Result<RuntimeFilerOperationReport, RuntimeFilerError> {
        let (kind, path, target_path) = runtime_filer_operation_parts(&operation);
        Err(RuntimeFilerError::OperationFailed {
            operation: kind,
            path,
            target_path,
            kind: RuntimeFilerErrorKind::Unsupported,
            message: "runtime host session does not support filer operations".to_string(),
        })
    }
    fn execute_host_command(
        &mut self,
        name: &str,
    ) -> Result<RuntimeCommandEffect, RuntimeCommandError>;
    fn request_input_prompt(
        &mut self,
        request: RuntimeInputPromptRequest,
    ) -> Result<RuntimeInputPromptHostResponse, RuntimeCommandError> {
        log::debug!(
            "[runtime_integration][input] prompt unsupported by host session: title={}, placeholder_present={}",
            request.title,
            request.placeholder.is_some()
        );
        Ok(RuntimeInputPromptHostResponse::Completed(
            RuntimeInputPromptResponse::Cancelled,
        ))
    }
    fn execute_lsif_request(
        &mut self,
        request: LspRuntimeBridgeRequest,
    ) -> Result<LspRuntimeBridgeResponse, RuntimeCommandError> {
        Err(RuntimeCommandError::CommandFailed {
            name: "lsif.request".to_string(),
            message: format!(
                "LSIF bridge is not configured for method {}",
                request.method
            ),
        })
    }
}

#[derive(Debug)]
pub enum RuntimeInputPromptHostResponse {
    Completed(RuntimeInputPromptResponse),
    Pending,
}

pub struct RuntimeEventMapper;

impl RuntimeEventMapper {
    pub fn buffer_open(buffer: ReadonlyBufferSnapshot) -> RuntimeEventPayload {
        RuntimeEventPayload::BufferOpen(crate::runtime::live::BufferEventPayload { buffer })
    }

    pub fn buffer_write_post(buffer: ReadonlyBufferSnapshot) -> RuntimeEventPayload {
        RuntimeEventPayload::BufferWritePost(crate::runtime::live::BufferEventPayload { buffer })
    }

    pub fn buffer_changed(buffer: ReadonlyBufferSnapshot) -> RuntimeEventPayload {
        RuntimeEventPayload::BufferChanged(crate::runtime::live::BufferEventPayload { buffer })
    }

    pub fn buffer_closed(buffer: ReadonlyBufferSnapshot) -> RuntimeEventPayload {
        RuntimeEventPayload::BufferClosed(crate::runtime::live::BufferEventPayload { buffer })
    }
}

pub struct RuntimeOutcomeProjector;

impl RuntimeOutcomeProjector {
    pub fn project_report(report: &RuntimeDispatchReport) -> RuntimeDispatchOutcome {
        RuntimeDispatchOutcome {
            transient_message: None,
            requires_redraw: runtime_dispatch_requests_redraw(report),
            shutdown_intent: None,
            presentation_intents: Vec::new(),
        }
    }

    pub fn project_error(error: &RuntimeDispatchError) -> RuntimeDispatchOutcome {
        let transient_message = runtime_callback_failure_message(error);
        RuntimeDispatchOutcome {
            requires_redraw: transient_message.is_some(),
            transient_message,
            shutdown_intent: None,
            presentation_intents: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct CachedRuntimeSnapshots {
    buffer: ReadonlyBufferSnapshot,
    window: ReadonlyWindowSnapshot,
    editor: ReadonlyEditorSnapshot,
    current_filer_entry: Option<RuntimeFilerCurrentEntry>,
}

impl Default for CachedRuntimeSnapshots {
    fn default() -> Self {
        Self {
            buffer: ReadonlyBufferSnapshot {
                id: 1,
                path: None,
                line_count: 1,
                cursor_row: 0,
                cursor_col: 0,
                current_line: String::new(),
                text: String::new(),
            },
            window: ReadonlyWindowSnapshot { id: 1 },
            editor: ReadonlyEditorSnapshot {
                mode: RuntimeMode::Normal,
            },
            current_filer_entry: None,
        }
    }
}

struct RuntimeHostCommandRequest {
    name: String,
    reply: oneshot::Sender<Result<(), RuntimeCommandError>>,
}

struct RuntimeInputPromptChannelRequest {
    request: RuntimeInputPromptRequest,
    reply: oneshot::Sender<Result<RuntimeInputPromptResponse, RuntimeCommandError>>,
}

struct RuntimeFilerOperationRequest {
    operation: RuntimeFilerOperation,
    reply: oneshot::Sender<Result<RuntimeFilerOperationReport, RuntimeFilerError>>,
}

struct RuntimeFilerListRequest {
    path: std::path::PathBuf,
    options: RuntimeFilerListOptions,
    reply: oneshot::Sender<Result<Vec<RuntimeFilerEntry>, RuntimeFilerError>>,
}

struct RuntimeFloatOpenChannelRequest {
    request: RuntimeFloatOpenRequest,
    reply: oneshot::Sender<Result<RuntimeFloatSnapshot, RuntimeCommandError>>,
}

struct RuntimeFloatIdChannelRequest {
    id: u64,
    operation: &'static str,
    reply: oneshot::Sender<Result<bool, RuntimeCommandError>>,
}

struct RuntimeFloatSnapshotsChannelRequest {
    reply: oneshot::Sender<Result<Vec<RuntimeFloatSnapshot>, RuntimeCommandError>>,
}

struct RuntimeLsifRequest {
    request: LspRuntimeBridgeRequest,
    reply: oneshot::Sender<Result<LspRuntimeBridgeResponse, RuntimeCommandError>>,
}

fn runtime_filer_operation_parts(
    operation: &RuntimeFilerOperation,
) -> (
    RuntimeFilerOperationKind,
    std::path::PathBuf,
    Option<std::path::PathBuf>,
) {
    match operation {
        RuntimeFilerOperation::CreateFile { path } => {
            (RuntimeFilerOperationKind::CreateFile, path.clone(), None)
        }
        RuntimeFilerOperation::CreateDirectory { path } => (
            RuntimeFilerOperationKind::CreateDirectory,
            path.clone(),
            None,
        ),
        RuntimeFilerOperation::Copy { from, to } => (
            RuntimeFilerOperationKind::Copy,
            from.clone(),
            Some(to.clone()),
        ),
        RuntimeFilerOperation::Move { from, to } => (
            RuntimeFilerOperationKind::Move,
            from.clone(),
            Some(to.clone()),
        ),
        RuntimeFilerOperation::Rename { from, to } => (
            RuntimeFilerOperationKind::Rename,
            from.clone(),
            Some(to.clone()),
        ),
        RuntimeFilerOperation::Delete { path, .. } => {
            (RuntimeFilerOperationKind::Delete, path.clone(), None)
        }
        RuntimeFilerOperation::Mark { path } => {
            (RuntimeFilerOperationKind::Mark, path.clone(), None)
        }
        RuntimeFilerOperation::Unmark { path } => {
            (RuntimeFilerOperationKind::Unmark, path.clone(), None)
        }
        RuntimeFilerOperation::ClearMarks => (
            RuntimeFilerOperationKind::ClearMarks,
            std::path::PathBuf::new(),
            None,
        ),
        RuntimeFilerOperation::BulkDeletePreview => (
            RuntimeFilerOperationKind::BulkDeletePreview,
            std::path::PathBuf::new(),
            None,
        ),
        RuntimeFilerOperation::BulkDelete { preview_id, .. } => (
            RuntimeFilerOperationKind::BulkDelete,
            std::path::PathBuf::from(preview_id),
            None,
        ),
    }
}

struct ChannelBackedHostBridge {
    snapshots: Arc<Mutex<CachedRuntimeSnapshots>>,
    selector_view_backend: Arc<dyn SelectorViewBackend>,
    command_sender: mpsc::UnboundedSender<RuntimeHostCommandRequest>,
    input_prompt_sender: mpsc::UnboundedSender<RuntimeInputPromptChannelRequest>,
    lsif_request_sender: mpsc::UnboundedSender<RuntimeLsifRequest>,
    filer_operation_sender: mpsc::UnboundedSender<RuntimeFilerOperationRequest>,
    filer_list_sender: mpsc::UnboundedSender<RuntimeFilerListRequest>,
    float_open_sender: mpsc::UnboundedSender<RuntimeFloatOpenChannelRequest>,
    float_focus_sender: mpsc::UnboundedSender<RuntimeFloatIdChannelRequest>,
    float_close_sender: mpsc::UnboundedSender<RuntimeFloatIdChannelRequest>,
    float_snapshots_sender: mpsc::UnboundedSender<RuntimeFloatSnapshotsChannelRequest>,
}

impl HostCapabilityBridge for ChannelBackedHostBridge {
    fn selector_view_backend(&self) -> Option<Arc<dyn SelectorViewBackend>> {
        Some(self.selector_view_backend.clone())
    }

    fn execute_host_command(
        &self,
        name: &str,
    ) -> crate::runtime::live::BoxFuture<Result<(), RuntimeCommandError>> {
        let command_sender = self.command_sender.clone();
        let name = name.to_string();
        Box::pin(async move {
            let (reply, receiver) = oneshot::channel();
            command_sender
                .send(RuntimeHostCommandRequest {
                    name: name.clone(),
                    reply,
                })
                .map_err(|_| RuntimeCommandError::CommandFailed {
                    name: name.clone(),
                    message: "host command channel closed".to_string(),
                })?;
            receiver
                .await
                .map_err(|_| RuntimeCommandError::CommandFailed {
                    name,
                    message: "host command reply channel closed".to_string(),
                })?
        })
    }

    fn request_input_prompt(
        &self,
        request: RuntimeInputPromptRequest,
    ) -> crate::runtime::live::BoxFuture<Result<RuntimeInputPromptResponse, RuntimeCommandError>>
    {
        let input_prompt_sender = self.input_prompt_sender.clone();
        Box::pin(async move {
            let (reply, receiver) = oneshot::channel();
            input_prompt_sender
                .send(RuntimeInputPromptChannelRequest { request, reply })
                .map_err(|_| RuntimeCommandError::CommandFailed {
                    name: "input.prompt".to_string(),
                    message: "host input prompt channel closed".to_string(),
                })?;
            receiver
                .await
                .map_err(|_| RuntimeCommandError::CommandFailed {
                    name: "input.prompt".to_string(),
                    message: "host input prompt reply channel closed".to_string(),
                })?
        })
    }

    fn execute_lsif_request(
        &self,
        request: LspRuntimeBridgeRequest,
    ) -> crate::runtime::live::BoxFuture<Result<LspRuntimeBridgeResponse, RuntimeCommandError>>
    {
        let lsif_request_sender = self.lsif_request_sender.clone();
        Box::pin(async move {
            let (reply, receiver) = oneshot::channel();
            lsif_request_sender
                .send(RuntimeLsifRequest { request, reply })
                .map_err(|_| RuntimeCommandError::CommandFailed {
                    name: "lsif.request".to_string(),
                    message: "host LSIF request channel closed".to_string(),
                })?;
            receiver
                .await
                .map_err(|_| RuntimeCommandError::CommandFailed {
                    name: "lsif.request".to_string(),
                    message: "host LSIF request reply channel closed".to_string(),
                })?
        })
    }

    fn current_buffer(&self) -> crate::runtime::live::BoxFuture<ReadonlyBufferSnapshot> {
        let snapshots = self.snapshots.clone();
        Box::pin(async move {
            snapshots
                .lock()
                .expect("runtime snapshots mutex should not poison")
                .buffer
                .clone()
        })
    }

    fn current_window(&self) -> crate::runtime::live::BoxFuture<ReadonlyWindowSnapshot> {
        let snapshots = self.snapshots.clone();
        Box::pin(async move {
            snapshots
                .lock()
                .expect("runtime snapshots mutex should not poison")
                .window
                .clone()
        })
    }

    fn open_float(
        &self,
        request: RuntimeFloatOpenRequest,
    ) -> crate::runtime::live::BoxFuture<Result<RuntimeFloatSnapshot, RuntimeCommandError>> {
        let float_open_sender = self.float_open_sender.clone();
        Box::pin(async move {
            let (reply, receiver) = oneshot::channel();
            float_open_sender
                .send(RuntimeFloatOpenChannelRequest { request, reply })
                .map_err(|_| RuntimeCommandError::CommandFailed {
                    name: "window.openFloat".to_string(),
                    message: "host float open channel closed".to_string(),
                })?;
            receiver
                .await
                .map_err(|_| RuntimeCommandError::CommandFailed {
                    name: "window.openFloat".to_string(),
                    message: "host float open reply channel closed".to_string(),
                })?
        })
    }

    fn close_float(
        &self,
        id: u64,
    ) -> crate::runtime::live::BoxFuture<Result<bool, RuntimeCommandError>> {
        let float_close_sender = self.float_close_sender.clone();
        Box::pin(async move {
            let (reply, receiver) = oneshot::channel();
            float_close_sender
                .send(RuntimeFloatIdChannelRequest {
                    id,
                    operation: "window.close",
                    reply,
                })
                .map_err(|_| RuntimeCommandError::CommandFailed {
                    name: "window.close".to_string(),
                    message: "host float close channel closed".to_string(),
                })?;
            receiver
                .await
                .map_err(|_| RuntimeCommandError::CommandFailed {
                    name: "window.close".to_string(),
                    message: "host float close reply channel closed".to_string(),
                })?
        })
    }

    fn focus_float(
        &self,
        id: u64,
    ) -> crate::runtime::live::BoxFuture<Result<bool, RuntimeCommandError>> {
        let float_focus_sender = self.float_focus_sender.clone();
        Box::pin(async move {
            let (reply, receiver) = oneshot::channel();
            float_focus_sender
                .send(RuntimeFloatIdChannelRequest {
                    id,
                    operation: "window.focus",
                    reply,
                })
                .map_err(|_| RuntimeCommandError::CommandFailed {
                    name: "window.focus".to_string(),
                    message: "host float focus channel closed".to_string(),
                })?;
            receiver
                .await
                .map_err(|_| RuntimeCommandError::CommandFailed {
                    name: "window.focus".to_string(),
                    message: "host float focus reply channel closed".to_string(),
                })?
        })
    }

    fn list_float_snapshots(
        &self,
    ) -> crate::runtime::live::BoxFuture<Result<Vec<RuntimeFloatSnapshot>, RuntimeCommandError>>
    {
        let float_snapshots_sender = self.float_snapshots_sender.clone();
        Box::pin(async move {
            let (reply, receiver) = oneshot::channel();
            float_snapshots_sender
                .send(RuntimeFloatSnapshotsChannelRequest { reply })
                .map_err(|_| RuntimeCommandError::CommandFailed {
                    name: "window.floats".to_string(),
                    message: "host float snapshots channel closed".to_string(),
                })?;
            receiver
                .await
                .map_err(|_| RuntimeCommandError::CommandFailed {
                    name: "window.floats".to_string(),
                    message: "host float snapshots reply channel closed".to_string(),
                })?
        })
    }

    fn current_editor(&self) -> crate::runtime::live::BoxFuture<ReadonlyEditorSnapshot> {
        let snapshots = self.snapshots.clone();
        Box::pin(async move {
            snapshots
                .lock()
                .expect("runtime snapshots mutex should not poison")
                .editor
                .clone()
        })
    }

    fn current_filer_entry(
        &self,
    ) -> crate::runtime::live::BoxFuture<Result<Option<RuntimeFilerCurrentEntry>, RuntimeFilerError>>
    {
        let snapshots = self.snapshots.clone();
        Box::pin(async move {
            Ok(snapshots
                .lock()
                .expect("runtime snapshots mutex should not poison")
                .current_filer_entry
                .clone())
        })
    }

    fn list_filer_entries(
        &self,
        path: std::path::PathBuf,
        options: RuntimeFilerListOptions,
    ) -> crate::runtime::live::BoxFuture<Result<Vec<RuntimeFilerEntry>, RuntimeFilerError>> {
        let filer_list_sender = self.filer_list_sender.clone();
        Box::pin(async move {
            let (reply, receiver) = oneshot::channel();
            filer_list_sender
                .send(RuntimeFilerListRequest {
                    path: path.clone(),
                    options,
                    reply,
                })
                .map_err(|_| RuntimeFilerError::ReadFailed {
                    path: path.clone(),
                    message: "host filer list channel closed".to_string(),
                })?;
            receiver.await.map_err(|_| RuntimeFilerError::ReadFailed {
                path,
                message: "host filer list reply channel closed".to_string(),
            })?
        })
    }

    fn execute_filer_operation(
        &self,
        operation: RuntimeFilerOperation,
    ) -> crate::runtime::live::BoxFuture<Result<RuntimeFilerOperationReport, RuntimeFilerError>>
    {
        let filer_operation_sender = self.filer_operation_sender.clone();
        Box::pin(async move {
            let (kind, path, target_path) = runtime_filer_operation_parts(&operation);
            let (reply, receiver) = oneshot::channel();
            filer_operation_sender
                .send(RuntimeFilerOperationRequest { operation, reply })
                .map_err(|_| RuntimeFilerError::OperationFailed {
                    operation: kind,
                    path: path.clone(),
                    target_path: target_path.clone(),
                    kind: RuntimeFilerErrorKind::Io,
                    message: "host filer operation channel closed".to_string(),
                })?;
            receiver
                .await
                .map_err(|_| RuntimeFilerError::OperationFailed {
                    operation: kind,
                    path,
                    target_path,
                    kind: RuntimeFilerErrorKind::Io,
                    message: "host filer operation reply channel closed".to_string(),
                })?
        })
    }
}

pub struct RuntimeSessionOwner {
    runtime: SayaLiveRuntime,
    snapshots: Arc<Mutex<CachedRuntimeSnapshots>>,
    selector_tui_projection_sink: Arc<SelectorTuiProjectionSink>,
    _command_sender: mpsc::UnboundedSender<RuntimeHostCommandRequest>,
    command_receiver: mpsc::UnboundedReceiver<RuntimeHostCommandRequest>,
    _input_prompt_sender: mpsc::UnboundedSender<RuntimeInputPromptChannelRequest>,
    input_prompt_receiver: mpsc::UnboundedReceiver<RuntimeInputPromptChannelRequest>,
    pending_input_prompt_reply:
        Option<oneshot::Sender<Result<RuntimeInputPromptResponse, RuntimeCommandError>>>,
    pending_runtime_command: Option<PendingRuntimeCommand>,
    _lsif_request_sender: mpsc::UnboundedSender<RuntimeLsifRequest>,
    lsif_request_receiver: mpsc::UnboundedReceiver<RuntimeLsifRequest>,
    _filer_operation_sender: mpsc::UnboundedSender<RuntimeFilerOperationRequest>,
    filer_operation_receiver: mpsc::UnboundedReceiver<RuntimeFilerOperationRequest>,
    _filer_list_sender: mpsc::UnboundedSender<RuntimeFilerListRequest>,
    filer_list_receiver: mpsc::UnboundedReceiver<RuntimeFilerListRequest>,
    _float_open_sender: mpsc::UnboundedSender<RuntimeFloatOpenChannelRequest>,
    float_open_receiver: mpsc::UnboundedReceiver<RuntimeFloatOpenChannelRequest>,
    _float_focus_sender: mpsc::UnboundedSender<RuntimeFloatIdChannelRequest>,
    float_focus_receiver: mpsc::UnboundedReceiver<RuntimeFloatIdChannelRequest>,
    _float_close_sender: mpsc::UnboundedSender<RuntimeFloatIdChannelRequest>,
    float_close_receiver: mpsc::UnboundedReceiver<RuntimeFloatIdChannelRequest>,
    _float_snapshots_sender: mpsc::UnboundedSender<RuntimeFloatSnapshotsChannelRequest>,
    float_snapshots_receiver: mpsc::UnboundedReceiver<RuntimeFloatSnapshotsChannelRequest>,
}

impl RuntimeSessionOwner {
    pub fn spawn(seed: CallbackRegistrySeed) -> Result<Self, RuntimeInitError> {
        log::debug!(
            "[runtime_integration] spawning runtime session owner: commands={}, events={}",
            seed.commands().len(),
            seed.events().len()
        );
        let snapshots = Arc::new(Mutex::new(CachedRuntimeSnapshots::default()));
        let selector_tui_projection_sink = Arc::new(SelectorTuiProjectionSink::new());
        let selector_view_backend = Arc::new(SelectorHostViewAdapter::new(
            selector_tui_projection_sink.clone(),
            10,
        ));
        let (command_sender, command_receiver) = mpsc::unbounded_channel();
        let (input_prompt_sender, input_prompt_receiver) = mpsc::unbounded_channel();
        let (lsif_request_sender, lsif_request_receiver) = mpsc::unbounded_channel();
        let (filer_operation_sender, filer_operation_receiver) = mpsc::unbounded_channel();
        let (filer_list_sender, filer_list_receiver) = mpsc::unbounded_channel();
        let (float_open_sender, float_open_receiver) = mpsc::unbounded_channel();
        let (float_focus_sender, float_focus_receiver) = mpsc::unbounded_channel();
        let (float_close_sender, float_close_receiver) = mpsc::unbounded_channel();
        let (float_snapshots_sender, float_snapshots_receiver) = mpsc::unbounded_channel();
        let bridge = Arc::new(ChannelBackedHostBridge {
            snapshots: snapshots.clone(),
            selector_view_backend,
            command_sender: command_sender.clone(),
            input_prompt_sender: input_prompt_sender.clone(),
            lsif_request_sender: lsif_request_sender.clone(),
            filer_operation_sender: filer_operation_sender.clone(),
            filer_list_sender: filer_list_sender.clone(),
            float_open_sender: float_open_sender.clone(),
            float_focus_sender: float_focus_sender.clone(),
            float_close_sender: float_close_sender.clone(),
            float_snapshots_sender: float_snapshots_sender.clone(),
        });
        let runtime = SayaLiveRuntime::spawn_from_seed(bridge, seed)?;
        Ok(Self {
            runtime,
            snapshots,
            selector_tui_projection_sink,
            _command_sender: command_sender,
            command_receiver,
            _input_prompt_sender: input_prompt_sender,
            input_prompt_receiver,
            pending_input_prompt_reply: None,
            pending_runtime_command: None,
            _lsif_request_sender: lsif_request_sender,
            lsif_request_receiver,
            _filer_operation_sender: filer_operation_sender,
            filer_operation_receiver,
            _filer_list_sender: filer_list_sender,
            filer_list_receiver,
            _float_open_sender: float_open_sender,
            float_open_receiver,
            _float_focus_sender: float_focus_sender,
            float_focus_receiver,
            _float_close_sender: float_close_sender,
            float_close_receiver,
            _float_snapshots_sender: float_snapshots_sender,
            float_snapshots_receiver,
        })
    }

    pub fn selector_tui_projection_sink(&self) -> Arc<SelectorTuiProjectionSink> {
        self.selector_tui_projection_sink.clone()
    }

    pub async fn control_selector<H: RuntimeHostSession>(
        &mut self,
        id: u64,
        command: RuntimeSelectorControllerCommand,
        host_session: &mut H,
    ) -> RuntimeDispatchOutcome {
        self.refresh_cached_snapshots(host_session);
        log::info!(
            "[runtime_integration][selector] host selector control start: id={}, command={:?}",
            id,
            command
        );
        let projection_count_before = self.selector_tui_projection_sink.projection_count();
        let receipt = match self
            .runtime
            .control_selector(id, RuntimeSelectorControlRequest { command })
        {
            Ok(receipt) => receipt,
            Err(error) => {
                log::debug!(
                    "[runtime_integration][selector] selector control failed before queue: id={}, command={:?}, error={:?}",
                    id,
                    command,
                    error
                );
                return RuntimeDispatchOutcome {
                    transient_message: Some(format!("Selector control failed: {:?}", error)),
                    requires_redraw: true,
                    shutdown_intent: None,
                    presentation_intents: Vec::new(),
                };
            }
        };
        match receipt.await_result().await {
            Ok(snapshot) => {
                let projection_count_after = self.selector_tui_projection_sink.projection_count();
                log::info!(
                    "[runtime_integration][selector] selector control succeeded: id={}, command={:?}, cursor={}, offset={}, hidden={}, cancelled={}",
                    snapshot.id,
                    command,
                    snapshot.view.cursor,
                    snapshot.view.offset,
                    snapshot.view.hidden,
                    snapshot.view.cancelled
                );
                RuntimeDispatchOutcome {
                    transient_message: None,
                    requires_redraw: projection_count_after != projection_count_before,
                    shutdown_intent: None,
                    presentation_intents: Vec::new(),
                }
            }
            Err(error) => {
                log::debug!(
                    "[runtime_integration][selector] selector control failed: id={}, command={:?}, error={:?}",
                    id,
                    command,
                    error
                );
                RuntimeDispatchOutcome {
                    transient_message: Some(format!("Selector control failed: {:?}", error)),
                    requires_redraw: true,
                    shutdown_intent: None,
                    presentation_intents: Vec::new(),
                }
            }
        }
    }

    pub async fn dispatch<H: RuntimeHostSession>(
        &mut self,
        event: RuntimeEventPayload,
        host_session: &mut H,
    ) -> RuntimeDispatchOutcome {
        let mut aggregate = RuntimeDispatchOutcome::default();
        let mut pending_events = VecDeque::from([event]);

        while let Some(next_event) = pending_events.pop_front() {
            let (projected, follow_up_events) = self.dispatch_once(next_event, host_session).await;
            merge_dispatch_outcome(&mut aggregate, projected);
            pending_events.extend(follow_up_events);
        }

        aggregate
    }

    pub async fn execute_command<H: RuntimeHostSession>(
        &mut self,
        name: &str,
        host_session: &mut H,
    ) -> RuntimeDispatchOutcome {
        if self.pending_runtime_command.is_some() {
            log::debug!(
                "[runtime_integration][command] refusing to start command while another runtime command is pending prompt: command={}",
                name
            );
            return RuntimeDispatchOutcome {
                transient_message: Some("Runtime command is waiting for input".to_string()),
                requires_redraw: true,
                shutdown_intent: None,
                presentation_intents: Vec::new(),
            };
        }
        self.refresh_cached_snapshots(host_session);
        log::info!(
            "[runtime_integration][command] execute runtime command through session owner: command={}",
            name
        );

        let receipt = match self.runtime.execute_command(name) {
            Ok(receipt) => receipt,
            Err(error) => {
                log::debug!(
                    "[runtime_integration] failed to queue runtime command: command={}, error={:?}",
                    name,
                    error
                );
                return RuntimeDispatchOutcome {
                    transient_message: Some(format!("Runtime command failed: {:?}", error)),
                    requires_redraw: true,
                    shutdown_intent: None,
                    presentation_intents: Vec::new(),
                };
            }
        };

        let selector_projection_count_before = self.selector_tui_projection_sink.projection_count();
        let mut aggregate = self
            .await_runtime_command(name.to_string(), receipt.into_receiver(), host_session)
            .await;
        if let Some(pending_command) = aggregate.pending_runtime_command.take() {
            self.pending_runtime_command = Some(pending_command);
        }
        let selector_projection_count_after = self.selector_tui_projection_sink.projection_count();
        let follow_up_events = std::mem::take(&mut aggregate.follow_up_events);
        let mut dispatch_outcome = aggregate.outcome;
        if selector_projection_count_after != selector_projection_count_before {
            log::debug!(
                "[runtime_integration][selector] runtime command changed selector TUI projections: command={}, before={}, after={}",
                name,
                selector_projection_count_before,
                selector_projection_count_after
            );
            dispatch_outcome.requires_redraw = true;
        }
        for event in follow_up_events {
            merge_dispatch_outcome(
                &mut dispatch_outcome,
                self.dispatch(event, host_session).await,
            );
        }
        dispatch_outcome
    }

    pub async fn respond_to_input_prompt<H: RuntimeHostSession>(
        &mut self,
        response: RuntimeInputPromptResponse,
        host_session: &mut H,
    ) -> RuntimeDispatchOutcome {
        let Some(reply) = self.pending_input_prompt_reply.take() else {
            log::debug!(
                "[runtime_integration][input] prompt response ignored without pending runtime prompt"
            );
            return RuntimeDispatchOutcome {
                transient_message: Some("No runtime input prompt is active".to_string()),
                requires_redraw: true,
                shutdown_intent: None,
                presentation_intents: Vec::new(),
            };
        };
        let _ = reply.send(Ok(response));
        let Some(pending) = self.pending_runtime_command.take() else {
            return RuntimeDispatchOutcome {
                transient_message: None,
                requires_redraw: true,
                shutdown_intent: None,
                presentation_intents: Vec::new(),
            };
        };
        log::info!(
            "[runtime_integration][input] resuming runtime command after input prompt: command={}",
            pending.name
        );
        let mut aggregate = self
            .await_runtime_command(pending.name, pending.receiver, host_session)
            .await;
        if let Some(pending_command) = aggregate.pending_runtime_command.take() {
            self.pending_runtime_command = Some(pending_command);
        }
        let follow_up_events = std::mem::take(&mut aggregate.follow_up_events);
        let mut dispatch_outcome = aggregate.outcome;
        dispatch_outcome.requires_redraw = true;
        for event in follow_up_events {
            merge_dispatch_outcome(
                &mut dispatch_outcome,
                self.dispatch(event, host_session).await,
            );
        }
        dispatch_outcome
    }

    async fn dispatch_once<H: RuntimeHostSession>(
        &mut self,
        event: RuntimeEventPayload,
        host_session: &mut H,
    ) -> (RuntimeDispatchOutcome, Vec<RuntimeEventPayload>) {
        self.refresh_cached_snapshots(host_session);
        log::debug!(
            "[runtime_integration] dispatch runtime event through session owner: event={:?}",
            event.event_name()
        );

        let receipt = match self.runtime.dispatch_event(event.clone()) {
            Ok(receipt) => receipt,
            Err(error) => {
                log::debug!(
                    "[runtime_integration] failed to queue runtime event: event={:?}, error={:?}",
                    event.event_name(),
                    error
                );
                return (RuntimeOutcomeProjector::project_error(&error), Vec::new());
            }
        };

        let mut projected = RuntimeDispatchOutcome::default();
        let mut follow_up_events = Vec::new();
        let result_future = receipt.await_result();
        tokio::pin!(result_future);

        loop {
            tokio::select! {
                result = &mut result_future => {
                    match result {
                        Ok(report) => {
                            log::debug!(
                                "[runtime_integration] runtime dispatch completed: event={:?}, handler_count={}",
                                report.event,
                                report.handler_count
                            );
                            merge_dispatch_outcome(
                                &mut projected,
                                RuntimeOutcomeProjector::project_report(&report),
                            );
                        }
                        Err(error) => {
                            log::debug!(
                                "[runtime_integration] runtime dispatch failed: event={:?}, error={:?}",
                                event.event_name(),
                                error
                            );
                            merge_dispatch_outcome(
                                &mut projected,
                                RuntimeOutcomeProjector::project_error(&error),
                            );
                        }
                    }
                    break;
                }
                request = self.command_receiver.recv() => {
                    let Some(request) = request else {
                        log::debug!("[runtime_integration] host command channel closed while dispatch was in flight");
                        break;
                    };
                    log::debug!(
                        "[runtime_integration] servicing runtime host command request: command={}",
                        request.name
                    );
                    match host_session.execute_host_command(&request.name) {
                        Ok(effect) => {
                            self.refresh_cached_snapshots(host_session);
                            merge_dispatch_outcome(
                                &mut projected,
                                RuntimeDispatchOutcome {
                                    transient_message: effect.transient_message.clone(),
                                    requires_redraw: effect.transient_message.is_some()
                                        || !effect.presentation_intents.is_empty(),
                                    shutdown_intent: effect.shutdown_intent,
                                    presentation_intents: effect.presentation_intents.clone(),
                                },
                            );
                            follow_up_events.extend(effect.follow_up_events.clone());
                            let _ = request.reply.send(Ok(()));
                        }
                        Err(error) => {
                            let _ = request.reply.send(Err(error));
                        }
                    }
                }
                request = self.input_prompt_receiver.recv() => {
                    let Some(request) = request else {
                        log::debug!("[runtime_integration] input prompt channel closed while dispatch was in flight");
                        break;
                    };
                    log::info!(
                        "[runtime_integration][input] servicing runtime input prompt during event dispatch: title={}, placeholder_present={}",
                        request.request.title,
                        request.request.placeholder.is_some()
                    );
                    match host_session.request_input_prompt(request.request) {
                        Ok(RuntimeInputPromptHostResponse::Completed(response)) => {
                            projected.requires_redraw = true;
                            let _ = request.reply.send(Ok(response));
                        }
                        Ok(RuntimeInputPromptHostResponse::Pending) => {
                            log::debug!("[runtime_integration][input] host left runtime input prompt pending during event dispatch");
                            self.pending_input_prompt_reply = Some(request.reply);
                            projected.requires_redraw = true;
                        }
                        Err(error) => {
                            let _ = request.reply.send(Err(error));
                        }
                    }
                }
                request = self.lsif_request_receiver.recv() => {
                    let Some(request) = request else {
                        log::debug!("[runtime_integration] LSIF request channel closed while dispatch was in flight");
                        break;
                    };
                    log::info!(
                        "[runtime_integration][lsif] servicing runtime LSIF request during event dispatch: method={}",
                        request.request.method
                    );
                    let result = host_session.execute_lsif_request(request.request);
                    if result.is_ok() {
                        self.refresh_cached_snapshots(host_session);
                    }
                    let _ = request.reply.send(result);
                }
                request = self.filer_operation_receiver.recv() => {
                    let Some(request) = request else {
                        log::debug!("[runtime_integration] filer operation channel closed while dispatch was in flight");
                        break;
                    };
                    log::info!(
                        "[runtime_integration][filer] servicing runtime filer operation request during event dispatch: operation={:?}",
                        request.operation
                    );
                    match host_session.execute_filer_operation(request.operation) {
                        Ok(report) => {
                            self.refresh_cached_snapshots(host_session);
                            projected.requires_redraw = true;
                            let _ = request.reply.send(Ok(report));
                        }
                        Err(error) => {
                            let _ = request.reply.send(Err(error));
                        }
                    }
                }
                request = self.filer_list_receiver.recv() => {
                    let Some(request) = request else {
                        log::debug!("[runtime_integration] filer list channel closed while dispatch was in flight");
                        break;
                    };
                    log::info!(
                        "[runtime_integration][filer] servicing runtime filer list request during event dispatch: path={}, options={:?}",
                        request.path.display(),
                        request.options
                    );
                    match host_session.list_filer_entries(request.path, request.options) {
                        Ok(entries) => {
                            self.refresh_cached_snapshots(host_session);
                            projected.requires_redraw = true;
                            let _ = request.reply.send(Ok(entries));
                        }
                        Err(error) => {
                            let _ = request.reply.send(Err(error));
                        }
                    }
                }
                request = self.float_open_receiver.recv() => {
                    let Some(request) = request else {
                        log::debug!("[runtime_integration] float open channel closed while dispatch was in flight");
                        break;
                    };
                    log::info!(
                        "[runtime_integration][window] servicing runtime openFloat request during event dispatch: content={:?}",
                        request.request.content
                    );
                    match host_session.open_float(request.request) {
                        Ok(snapshot) => {
                            self.refresh_cached_snapshots(host_session);
                            projected.requires_redraw = true;
                            let _ = request.reply.send(Ok(snapshot));
                        }
                        Err(error) => {
                            let _ = request.reply.send(Err(error));
                        }
                    }
                }
                request = self.float_focus_receiver.recv() => {
                    let Some(request) = request else {
                        log::debug!("[runtime_integration] float focus channel closed while dispatch was in flight");
                        break;
                    };
                    log::info!(
                        "[runtime_integration][window] servicing runtime {} request during event dispatch: id={}",
                        request.operation,
                        request.id
                    );
                    match host_session.focus_float(request.id) {
                        Ok(focused) => {
                            self.refresh_cached_snapshots(host_session);
                            projected.requires_redraw |= focused;
                            let _ = request.reply.send(Ok(focused));
                        }
                        Err(error) => {
                            let _ = request.reply.send(Err(error));
                        }
                    }
                }
                request = self.float_close_receiver.recv() => {
                    let Some(request) = request else {
                        log::debug!("[runtime_integration] float close channel closed while dispatch was in flight");
                        break;
                    };
                    log::info!(
                        "[runtime_integration][window] servicing runtime {} request during event dispatch: id={}",
                        request.operation,
                        request.id
                    );
                    match host_session.close_float(request.id) {
                        Ok(closed) => {
                            self.refresh_cached_snapshots(host_session);
                            projected.requires_redraw |= closed;
                            let _ = request.reply.send(Ok(closed));
                        }
                        Err(error) => {
                            let _ = request.reply.send(Err(error));
                        }
                    }
                }
                request = self.float_snapshots_receiver.recv() => {
                    let Some(request) = request else {
                        log::debug!("[runtime_integration] float snapshots channel closed while dispatch was in flight");
                        break;
                    };
                    log::info!(
                        "[runtime_integration][window] servicing runtime floats snapshot request during event dispatch"
                    );
                    let _ = request.reply.send(host_session.list_float_snapshots());
                }
            }
        }

        (projected, follow_up_events)
    }

    fn refresh_cached_snapshots<H: RuntimeHostSession>(&self, host_session: &mut H) {
        let buffer = host_session.current_buffer_snapshot();
        let window = host_session.current_window_snapshot();
        let editor = host_session.current_editor_snapshot();
        let current_filer_entry = match host_session.current_filer_entry() {
            Ok(entry) => entry,
            Err(error) => {
                log::debug!(
                    "[runtime_integration] failed to refresh current filer entry snapshot: {:?}",
                    error
                );
                None
            }
        };
        log::debug!(
            "[runtime_integration] refreshing cached runtime snapshots: buffer_id={}, window_id={}, mode={:?}, current_filer_entry={}",
            buffer.id,
            window.id,
            editor.mode,
            current_filer_entry.is_some()
        );
        *self
            .snapshots
            .lock()
            .expect("runtime snapshots mutex should not poison") = CachedRuntimeSnapshots {
            buffer,
            window,
            editor,
            current_filer_entry,
        };
    }

    async fn await_runtime_command<H: RuntimeHostSession>(
        &mut self,
        name: String,
        mut receiver: oneshot::Receiver<Result<(), RuntimeCommandError>>,
        host_session: &mut H,
    ) -> RuntimeCommandDispatchAggregate {
        let mut projected = RuntimeDispatchOutcome::default();
        let mut follow_up_events = Vec::new();
        let mut pending_runtime_command = None;

        loop {
            tokio::select! {
                result = &mut receiver => {
                    match result {
                        Ok(Ok(())) => {
                            log::debug!(
                                "[runtime_integration] runtime command completed: command={}",
                                name
                            );
                        }
                        Ok(Err(error)) => {
                            log::debug!(
                                "[runtime_integration] runtime command failed: command={}, error={:?}",
                                name,
                                error
                            );
                            projected.transient_message = Some(format!("Runtime command failed: {:?}", error));
                            projected.requires_redraw = true;
                        }
                        Err(_) => {
                            log::debug!(
                                "[runtime_integration] runtime command worker stopped before reply: command={}",
                                name
                            );
                            projected.transient_message = Some("Runtime command worker stopped".to_string());
                            projected.requires_redraw = true;
                        }
                    }
                    break;
                }
                request = self.command_receiver.recv() => {
                    let Some(request) = request else {
                        log::debug!("[runtime_integration] host command channel closed while runtime command was in flight");
                        break;
                    };
                    log::info!(
                        "[runtime_integration][host_command] servicing runtime host command request during command execution: command={}",
                        request.name
                    );
                    match host_session.execute_host_command(&request.name) {
                        Ok(effect) => {
                            self.refresh_cached_snapshots(host_session);
                            merge_dispatch_outcome(
                                &mut projected,
                                RuntimeDispatchOutcome {
                                    transient_message: effect.transient_message.clone(),
                                    requires_redraw: effect.transient_message.is_some()
                                        || !effect.presentation_intents.is_empty(),
                                    shutdown_intent: effect.shutdown_intent,
                                    presentation_intents: effect.presentation_intents.clone(),
                                },
                            );
                            follow_up_events.extend(effect.follow_up_events.clone());
                            let _ = request.reply.send(Ok(()));
                        }
                        Err(error) => {
                            let _ = request.reply.send(Err(error));
                        }
                    }
                }
                request = self.input_prompt_receiver.recv() => {
                    let Some(request) = request else {
                        log::debug!("[runtime_integration] input prompt channel closed while runtime command was in flight");
                        break;
                    };
                    log::info!(
                        "[runtime_integration][input] servicing runtime input prompt during command execution: title={}, placeholder_present={}",
                        request.request.title,
                        request.request.placeholder.is_some()
                    );
                    match host_session.request_input_prompt(request.request) {
                        Ok(RuntimeInputPromptHostResponse::Completed(response)) => {
                            projected.requires_redraw = true;
                            let _ = request.reply.send(Ok(response));
                        }
                        Ok(RuntimeInputPromptHostResponse::Pending) => {
                            log::debug!("[runtime_integration][input] host left runtime input prompt pending during command execution");
                            self.pending_input_prompt_reply = Some(request.reply);
                            projected.requires_redraw = true;
                            pending_runtime_command = Some(PendingRuntimeCommand {
                                name: name.clone(),
                                receiver,
                            });
                            break;
                        }
                        Err(error) => {
                            let _ = request.reply.send(Err(error));
                        }
                    }
                }
                request = self.lsif_request_receiver.recv() => {
                    let Some(request) = request else {
                        log::debug!("[runtime_integration] LSIF request channel closed while runtime command was in flight");
                        break;
                    };
                    log::info!(
                        "[runtime_integration][lsif] servicing runtime LSIF request during command execution: method={}",
                        request.request.method
                    );
                    let result = host_session.execute_lsif_request(request.request);
                    if result.is_ok() {
                        self.refresh_cached_snapshots(host_session);
                    }
                    let _ = request.reply.send(result);
                }
                request = self.filer_operation_receiver.recv() => {
                    let Some(request) = request else {
                        log::debug!("[runtime_integration] filer operation channel closed while runtime command was in flight");
                        break;
                    };
                    log::info!(
                        "[runtime_integration][filer] servicing runtime filer operation request during command execution: operation={:?}",
                        request.operation
                    );
                    match host_session.execute_filer_operation(request.operation) {
                        Ok(report) => {
                            self.refresh_cached_snapshots(host_session);
                            projected.requires_redraw = true;
                            let _ = request.reply.send(Ok(report));
                        }
                        Err(error) => {
                            let _ = request.reply.send(Err(error));
                        }
                    }
                }
                request = self.filer_list_receiver.recv() => {
                    let Some(request) = request else {
                        log::debug!("[runtime_integration] filer list channel closed while runtime command was in flight");
                        break;
                    };
                    log::info!(
                        "[runtime_integration][filer] servicing runtime filer list request during command execution: path={}, options={:?}",
                        request.path.display(),
                        request.options
                    );
                    match host_session.list_filer_entries(request.path, request.options) {
                        Ok(entries) => {
                            self.refresh_cached_snapshots(host_session);
                            projected.requires_redraw = true;
                            let _ = request.reply.send(Ok(entries));
                        }
                        Err(error) => {
                            let _ = request.reply.send(Err(error));
                        }
                    }
                }
                request = self.float_open_receiver.recv() => {
                    let Some(request) = request else {
                        log::debug!("[runtime_integration] float open channel closed while runtime command was in flight");
                        break;
                    };
                    log::info!(
                        "[runtime_integration][window] servicing runtime openFloat request during command execution: content={:?}",
                        request.request.content
                    );
                    match host_session.open_float(request.request) {
                        Ok(snapshot) => {
                            self.refresh_cached_snapshots(host_session);
                            projected.requires_redraw = true;
                            let _ = request.reply.send(Ok(snapshot));
                        }
                        Err(error) => {
                            let _ = request.reply.send(Err(error));
                        }
                    }
                }
                request = self.float_focus_receiver.recv() => {
                    let Some(request) = request else {
                        log::debug!("[runtime_integration] float focus channel closed while runtime command was in flight");
                        break;
                    };
                    log::info!(
                        "[runtime_integration][window] servicing runtime {} request during command execution: id={}",
                        request.operation,
                        request.id
                    );
                    match host_session.focus_float(request.id) {
                        Ok(focused) => {
                            self.refresh_cached_snapshots(host_session);
                            projected.requires_redraw |= focused;
                            let _ = request.reply.send(Ok(focused));
                        }
                        Err(error) => {
                            let _ = request.reply.send(Err(error));
                        }
                    }
                }
                request = self.float_close_receiver.recv() => {
                    let Some(request) = request else {
                        log::debug!("[runtime_integration] float close channel closed while runtime command was in flight");
                        break;
                    };
                    log::info!(
                        "[runtime_integration][window] servicing runtime {} request during command execution: id={}",
                        request.operation,
                        request.id
                    );
                    match host_session.close_float(request.id) {
                        Ok(closed) => {
                            self.refresh_cached_snapshots(host_session);
                            projected.requires_redraw |= closed;
                            let _ = request.reply.send(Ok(closed));
                        }
                        Err(error) => {
                            let _ = request.reply.send(Err(error));
                        }
                    }
                }
                request = self.float_snapshots_receiver.recv() => {
                    let Some(request) = request else {
                        log::debug!("[runtime_integration] float snapshots channel closed while runtime command was in flight");
                        break;
                    };
                    log::info!(
                        "[runtime_integration][window] servicing runtime floats snapshot request during command execution"
                    );
                    let _ = request.reply.send(host_session.list_float_snapshots());
                }
            }
        }

        RuntimeCommandDispatchAggregate {
            outcome: projected,
            follow_up_events,
            pending_runtime_command,
        }
    }
}

struct RuntimeCommandDispatchAggregate {
    outcome: RuntimeDispatchOutcome,
    follow_up_events: Vec<RuntimeEventPayload>,
    pending_runtime_command: Option<PendingRuntimeCommand>,
}

struct PendingRuntimeCommand {
    name: String,
    receiver: oneshot::Receiver<Result<(), RuntimeCommandError>>,
}

fn merge_dispatch_outcome(target: &mut RuntimeDispatchOutcome, next: RuntimeDispatchOutcome) {
    if next.transient_message.is_some() {
        target.transient_message = next.transient_message;
    }
    target.requires_redraw |= next.requires_redraw;
    merge_shutdown_intent(&mut target.shutdown_intent, next.shutdown_intent);
    target
        .presentation_intents
        .extend(next.presentation_intents);
}

fn merge_shutdown_intent(
    target: &mut Option<RuntimeShutdownIntent>,
    next: Option<RuntimeShutdownIntent>,
) {
    match (target.as_ref().copied(), next) {
        (None, Some(intent)) => *target = Some(intent),
        (Some(RuntimeShutdownIntent::UserQuit), Some(RuntimeShutdownIntent::UserForceQuit)) => {
            *target = Some(RuntimeShutdownIntent::UserForceQuit);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::live::{RuntimeCallbackError, RuntimeEventName};

    #[test]
    fn runtime_outcome_projector_requests_redraw_for_callback_failure() {
        let projected =
            RuntimeOutcomeProjector::project_error(&RuntimeDispatchError::CallbackFailed {
                event: RuntimeEventName::BufferOpen,
                handler_index: 0,
                error: RuntimeCallbackError::ScriptFailed {
                    message: "boom".to_string(),
                },
            });

        assert_eq!(
            projected,
            RuntimeDispatchOutcome {
                transient_message: Some(
                    "Runtime callback failed on bufferOpen handler 0: script error: boom"
                        .to_string()
                ),
                requires_redraw: true,
                shutdown_intent: None,
                presentation_intents: Vec::new(),
            }
        );
    }
}
