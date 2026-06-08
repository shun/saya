use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{Mutex, oneshot};
use tokio::task::JoinHandle;

use crate::features::lsp::runtime_bridge::LspRuntimeServerDefinition;
use crate::runtime::process_pool::{ProcessPool, ProcessPoolError, ProcessSpec, StdioMode};

const READ_BUFFER_SIZE: usize = 64 * 1024;
const MAX_PENDING_NOTIFICATIONS: usize = 256;

#[derive(Debug)]
pub enum ManagedLspSessionError {
    InvalidSpec { detail: String },
    Process(ProcessPoolError),
    Protocol { detail: String },
    Closed { detail: String },
}

impl std::fmt::Display for ManagedLspSessionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSpec { detail } => write!(formatter, "invalid LSP session spec: {detail}"),
            Self::Process(error) => write!(formatter, "{error}"),
            Self::Protocol { detail } => write!(formatter, "LSP protocol error: {detail}"),
            Self::Closed { detail } => write!(formatter, "LSP session closed: {detail}"),
        }
    }
}

impl std::error::Error for ManagedLspSessionError {}

impl From<ProcessPoolError> for ManagedLspSessionError {
    fn from(value: ProcessPoolError) -> Self {
        Self::Process(value)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedLspConnectRequest {
    pub server: LspRuntimeServerDefinition,
    #[serde(default)]
    pub initialize_params: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedLspConnectResponse {
    pub session_id: u32,
    pub initialize_result: Value,
    pub notifications: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedLspRequestResponse {
    pub result: Value,
    pub notifications: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedLspNotifyResponse {
    pub notifications: Vec<Value>,
}

type PendingResponse = oneshot::Sender<Result<Value, ManagedLspSessionError>>;

pub struct ManagedLspSessionPool {
    process_pool: Arc<ProcessPool>,
    sessions: Mutex<HashMap<u32, Arc<ManagedLspSession>>>,
    next_id: AtomicU32,
}

impl ManagedLspSessionPool {
    pub fn new(process_pool: Arc<ProcessPool>) -> Self {
        Self {
            process_pool,
            sessions: Mutex::new(HashMap::new()),
            next_id: AtomicU32::new(1),
        }
    }

    pub async fn connect(
        &self,
        request: ManagedLspConnectRequest,
    ) -> Result<ManagedLspConnectResponse, ManagedLspSessionError> {
        validate_server(&request.server)?;
        let process_handle = self
            .process_pool
            .spawn(ProcessSpec {
                command: request.server.command.clone(),
                args: request.server.args.clone(),
                env: request.server.env.clone(),
                cwd: request.server.cwd.as_ref().map(PathBuf::from),
                stdin: StdioMode::Piped,
                stdout: StdioMode::Piped,
                stderr: StdioMode::Piped,
            })
            .await?;
        let session_id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let session = Arc::new(ManagedLspSession::new(
            session_id,
            process_handle,
            self.process_pool.clone(),
        ));
        session.start_reader().await;
        self.sessions
            .lock()
            .await
            .insert(session_id, session.clone());

        log::info!(
            "[saya_live_runtime][lsp] managed session connecting: session={}, server={}, command={:?}",
            session_id,
            request.server.name,
            request.server.command
        );
        let initialize_result = match session
            .request("initialize", request.initialize_params)
            .await
        {
            Ok(result) => result,
            Err(error) => {
                log::debug!(
                    "[saya_live_runtime][lsp] initialize failed; session will be closed: session={session_id}"
                );
                let _ = self.close(session_id).await;
                return Err(error);
            }
        };
        session
            .notify("initialized", Value::Object(Default::default()))
            .await?;
        let notifications = session.drain_notifications().await;
        log::info!(
            "[saya_live_runtime][lsp] managed session ready: session={}, notifications={}",
            session_id,
            notifications.len()
        );
        Ok(ManagedLspConnectResponse {
            session_id,
            initialize_result,
            notifications,
        })
    }

    pub async fn request(
        &self,
        session_id: u32,
        method: String,
        params: Value,
    ) -> Result<ManagedLspRequestResponse, ManagedLspSessionError> {
        let session = self.session(session_id).await?;
        let result = session.request(&method, params).await?;
        let notifications = session.drain_notifications().await;
        Ok(ManagedLspRequestResponse {
            result,
            notifications,
        })
    }

    pub async fn notify(
        &self,
        session_id: u32,
        method: String,
        params: Value,
    ) -> Result<ManagedLspNotifyResponse, ManagedLspSessionError> {
        let session = self.session(session_id).await?;
        session.notify(&method, params).await?;
        let notifications = session.drain_notifications().await;
        Ok(ManagedLspNotifyResponse { notifications })
    }

    pub async fn close(&self, session_id: u32) -> Result<(), ManagedLspSessionError> {
        let session = self.sessions.lock().await.remove(&session_id);
        if let Some(session) = session {
            session.close().await?;
        }
        Ok(())
    }

    pub async fn shutdown_all(&self) {
        let sessions: Vec<Arc<ManagedLspSession>> = {
            let mut guard = self.sessions.lock().await;
            guard.drain().map(|(_, session)| session).collect()
        };
        log::info!(
            "[saya_live_runtime][lsp] shutdown_all sweeping {} managed session(s)",
            sessions.len()
        );
        for session in sessions {
            if let Err(error) = session.close().await {
                log::debug!("[saya_live_runtime][lsp] managed session close failed: {error}");
            }
        }
    }

    async fn session(
        &self,
        session_id: u32,
    ) -> Result<Arc<ManagedLspSession>, ManagedLspSessionError> {
        self.sessions
            .lock()
            .await
            .get(&session_id)
            .cloned()
            .ok_or_else(|| ManagedLspSessionError::Closed {
                detail: format!("unknown managed LSP session: {session_id}"),
            })
    }
}

pub struct ManagedLspSession {
    id: u32,
    process_handle: u32,
    process_pool: Arc<ProcessPool>,
    next_request_id: AtomicU64,
    pending: Arc<Mutex<HashMap<u64, PendingResponse>>>,
    notifications: Arc<Mutex<Vec<Value>>>,
    reader: Mutex<Option<JoinHandle<()>>>,
}

impl ManagedLspSession {
    fn new(id: u32, process_handle: u32, process_pool: Arc<ProcessPool>) -> Self {
        Self {
            id,
            process_handle,
            process_pool,
            next_request_id: AtomicU64::new(1),
            pending: Arc::new(Mutex::new(HashMap::new())),
            notifications: Arc::new(Mutex::new(Vec::new())),
            reader: Mutex::new(None),
        }
    }

    async fn start_reader(self: &Arc<Self>) {
        let process_pool = self.process_pool.clone();
        let process_handle = self.process_handle;
        let session_id = self.id;
        let pending = self.pending.clone();
        let notifications = self.notifications.clone();
        let task = tokio::spawn(async move {
            read_loop(
                process_pool,
                process_handle,
                session_id,
                pending,
                notifications,
            )
            .await;
        });
        *self.reader.lock().await = Some(task);
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, ManagedLspSessionError> {
        validate_method(method)?;
        let id = self.next_request_id.fetch_add(1, Ordering::SeqCst);
        let message = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().await.insert(id, sender);
        if let Err(error) = self.write_message(&message).await {
            self.pending.lock().await.remove(&id);
            return Err(error);
        }
        log::debug!(
            "[saya_live_runtime][lsp] request sent: session={}, id={}, method={}",
            self.id,
            id,
            method
        );
        receiver
            .await
            .map_err(|_error| ManagedLspSessionError::Closed {
                detail: format!("response channel closed for {method}"),
            })?
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), ManagedLspSessionError> {
        validate_method(method)?;
        let message = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        self.write_message(&message).await?;
        log::debug!(
            "[saya_live_runtime][lsp] notification sent: session={}, method={}",
            self.id,
            method
        );
        Ok(())
    }

    async fn drain_notifications(&self) -> Vec<Value> {
        let mut guard = self.notifications.lock().await;
        guard.drain(..).collect()
    }

    async fn close(&self) -> Result<(), ManagedLspSessionError> {
        log::info!(
            "[saya_live_runtime][lsp] closing managed session: session={}, process_handle={}",
            self.id,
            self.process_handle
        );
        if let Some(task) = self.reader.lock().await.take() {
            task.abort();
        }
        reject_all_pending(
            &self.pending,
            ManagedLspSessionError::Closed {
                detail: "managed session closed".to_string(),
            },
        )
        .await;
        if let Err(error) = self.process_pool.kill(self.process_handle).await {
            log::debug!(
                "[saya_live_runtime][lsp] best-effort kill failed: session={}, error={}",
                self.id,
                error
            );
        }
        Ok(())
    }

    async fn write_message(&self, message: &Value) -> Result<(), ManagedLspSessionError> {
        let body =
            serde_json::to_vec(message).map_err(|error| ManagedLspSessionError::Protocol {
                detail: format!("failed to encode JSON-RPC message: {error}"),
            })?;
        let mut framed = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
        framed.extend_from_slice(&body);
        self.process_pool
            .write_stdin(self.process_handle, &framed)
            .await?;
        Ok(())
    }
}

async fn read_loop(
    process_pool: Arc<ProcessPool>,
    process_handle: u32,
    session_id: u32,
    pending: Arc<Mutex<HashMap<u64, PendingResponse>>>,
    notifications: Arc<Mutex<Vec<Value>>>,
) {
    let mut read_buf = vec![0_u8; READ_BUFFER_SIZE];
    let mut frame_buf = Vec::new();
    loop {
        match process_pool
            .read_stdout(process_handle, &mut read_buf)
            .await
        {
            Ok(Some(n)) => {
                frame_buf.extend_from_slice(&read_buf[..n]);
                while let Some(message) = match try_parse_message(&mut frame_buf) {
                    Ok(message) => message,
                    Err(error) => {
                        log::debug!(
                            "[saya_live_runtime][lsp] protocol parse failed: session={}, error={}",
                            session_id,
                            error
                        );
                        reject_all_pending(&pending, error).await;
                        return;
                    }
                } {
                    route_message(session_id, message, &pending, &notifications).await;
                }
            }
            Ok(None) => {
                log::debug!("[saya_live_runtime][lsp] stdout EOF: session={session_id}");
                reject_all_pending(
                    &pending,
                    ManagedLspSessionError::Closed {
                        detail: "language server stdout closed".to_string(),
                    },
                )
                .await;
                return;
            }
            Err(error) => {
                log::debug!(
                    "[saya_live_runtime][lsp] stdout read failed: session={}, error={}",
                    session_id,
                    error
                );
                reject_all_pending(&pending, ManagedLspSessionError::Process(error)).await;
                return;
            }
        }
    }
}

async fn route_message(
    session_id: u32,
    message: Value,
    pending: &Arc<Mutex<HashMap<u64, PendingResponse>>>,
    notifications: &Arc<Mutex<Vec<Value>>>,
) {
    if let Some(id) = message.get("id").and_then(Value::as_u64) {
        let sender = pending.lock().await.remove(&id);
        if let Some(sender) = sender {
            let response = if let Some(error) = message.get("error") {
                Err(ManagedLspSessionError::Protocol {
                    detail: format!("server returned error for id {id}: {error}"),
                })
            } else {
                Ok(message.get("result").cloned().unwrap_or(Value::Null))
            };
            let _ = sender.send(response);
            log::debug!(
                "[saya_live_runtime][lsp] response routed: session={}, id={}",
                session_id,
                id
            );
            return;
        }
    }
    let mut guard = notifications.lock().await;
    if guard.len() >= MAX_PENDING_NOTIFICATIONS {
        guard.remove(0);
    }
    guard.push(message);
    log::debug!(
        "[saya_live_runtime][lsp] server notification queued: session={}, pending={}",
        session_id,
        guard.len()
    );
}

async fn reject_all_pending(
    pending: &Arc<Mutex<HashMap<u64, PendingResponse>>>,
    error: ManagedLspSessionError,
) {
    let senders: Vec<PendingResponse> = pending
        .lock()
        .await
        .drain()
        .map(|(_, sender)| sender)
        .collect();
    let detail = error.to_string();
    for sender in senders {
        let _ = sender.send(Err(ManagedLspSessionError::Closed {
            detail: detail.clone(),
        }));
    }
}

fn try_parse_message(buffer: &mut Vec<u8>) -> Result<Option<Value>, ManagedLspSessionError> {
    let Some(header_end) = find_header_end(buffer) else {
        return Ok(None);
    };
    let header = std::str::from_utf8(&buffer[..header_end]).map_err(|error| {
        ManagedLspSessionError::Protocol {
            detail: format!("invalid UTF-8 header: {error}"),
        }
    })?;
    let content_length = parse_content_length(header)?;
    let body_start = header_end + 4;
    let body_end = body_start + content_length;
    if buffer.len() < body_end {
        return Ok(None);
    }
    let body = buffer[body_start..body_end].to_vec();
    buffer.drain(..body_end);
    serde_json::from_slice::<Value>(&body)
        .map(Some)
        .map_err(|error| ManagedLspSessionError::Protocol {
            detail: format!("invalid JSON body: {error}"),
        })
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn parse_content_length(header: &str) -> Result<usize, ManagedLspSessionError> {
    for line in header.split("\r\n") {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            return value.trim().parse::<usize>().map_err(|error| {
                ManagedLspSessionError::Protocol {
                    detail: format!("invalid Content-Length value: {error}"),
                }
            });
        }
    }
    Err(ManagedLspSessionError::Protocol {
        detail: "missing Content-Length header".to_string(),
    })
}

fn validate_server(server: &LspRuntimeServerDefinition) -> Result<(), ManagedLspSessionError> {
    if server.name.trim().is_empty() {
        return Err(ManagedLspSessionError::InvalidSpec {
            detail: "server.name must be non-empty".to_string(),
        });
    }
    if server.command.trim().is_empty() {
        return Err(ManagedLspSessionError::InvalidSpec {
            detail: "server.command must be non-empty".to_string(),
        });
    }
    validate_env(&server.env)?;
    Ok(())
}

fn validate_env(env: &BTreeMap<String, String>) -> Result<(), ManagedLspSessionError> {
    for key in env.keys() {
        if key.trim().is_empty() || key.contains('=') || key.contains('\0') {
            return Err(ManagedLspSessionError::InvalidSpec {
                detail: format!("invalid environment variable name: {key:?}"),
            });
        }
    }
    Ok(())
}

fn validate_method(method: &str) -> Result<(), ManagedLspSessionError> {
    if method.trim().is_empty() {
        return Err(ManagedLspSessionError::InvalidSpec {
            detail: "method must be non-empty".to_string(),
        });
    }
    Ok(())
}
