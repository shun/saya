use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{Map, Value, json};

use crate::lsif_index::LsifIndexCache;
use crate::lsp_runtime_bridge::{
    LspRuntimeBridgeRequest, LspRuntimeBridgeResponse, LspRuntimeBridgeSource,
    LspRuntimeServerDefinition,
};
use crate::lsp_transport::{LspServerConfig, LspTransportClient, LspTransportError};
use crate::saya_live_runtime::RuntimeCommandError;

#[derive(Debug, Clone)]
pub struct LspSessionRequestOptions {
    pub request_timeout: Duration,
    pub startup_timeout: Duration,
    pub shutdown_timeout: Duration,
}

impl Default for LspSessionRequestOptions {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(10),
            startup_timeout: Duration::from_secs(10),
            shutdown_timeout: Duration::from_secs(5),
        }
    }
}

#[derive(Debug)]
pub struct LspSessionRequestHandle {
    receiver: Receiver<Result<LspRuntimeBridgeResponse, RuntimeCommandError>>,
    cancelled: Arc<AtomicBool>,
}

impl LspSessionRequestHandle {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn recv_timeout(
        self,
        timeout: Duration,
    ) -> Result<Result<LspRuntimeBridgeResponse, RuntimeCommandError>, RecvTimeoutError> {
        self.receiver.recv_timeout(timeout)
    }
}

#[derive(Clone, Default)]
pub struct LspSessionManager {
    sessions: Arc<Mutex<HashMap<LspSessionKey, LspSessionHandle>>>,
    lsif_cache: Arc<Mutex<LsifIndexCache>>,
    diagnostic_events: Arc<Mutex<Vec<String>>>,
}

impl LspSessionManager {
    pub fn submit(
        &self,
        request: LspRuntimeBridgeRequest,
        options: LspSessionRequestOptions,
    ) -> LspSessionRequestHandle {
        let (reply, receiver) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let command = LspSessionCommand {
            request,
            options,
            reply,
            cancelled: cancelled.clone(),
        };

        if command.request.source == LspRuntimeBridgeSource::Lsif {
            self.submit_lsif_command(command);
            return LspSessionRequestHandle {
                receiver,
                cancelled,
            };
        }

        self.submit_command(command);
        LspSessionRequestHandle {
            receiver,
            cancelled,
        }
    }

    pub fn execute_blocking(
        &self,
        request: LspRuntimeBridgeRequest,
    ) -> Result<LspRuntimeBridgeResponse, RuntimeCommandError> {
        let handle = self.submit(request, LspSessionRequestOptions::default());
        handle.recv_timeout(Duration::from_secs(30)).map_err(|_| {
            RuntimeCommandError::CommandFailed {
                name: "lsp.request".to_string(),
                message: "timed out waiting for LSP session worker".to_string(),
            }
        })?
    }

    pub fn diagnostic_events(&self) -> Vec<String> {
        self.diagnostic_events
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }

    fn submit_command(&self, command: LspSessionCommand) {
        let key = match LspSessionKey::from_request(&command.request) {
            Ok(key) => key,
            Err(error) => {
                let _ = command.reply.send(Err(error));
                return;
            }
        };

        let sender = {
            let mut sessions = self
                .sessions
                .lock()
                .expect("LSP session map mutex should not poison");
            sessions
                .entry(key.clone())
                .or_insert_with(|| {
                    emit_session_event(
                        &self.diagnostic_events,
                        format!(
                            "session start: server={}, root={}",
                            key.server_name, key.root_uri
                        ),
                    );
                    spawn_session_worker(key.clone(), self.diagnostic_events.clone())
                })
                .sender
                .clone()
        };

        if let Err(error) = sender.send(command) {
            emit_session_event(
                &self.diagnostic_events,
                format!(
                    "session worker channel closed; dropping stale session: server={}, root={}",
                    key.server_name, key.root_uri
                ),
            );
            if let Ok(mut sessions) = self.sessions.lock() {
                sessions.remove(&key);
            }
            let retry = error.0;
            let retry_sender = {
                let mut sessions = self
                    .sessions
                    .lock()
                    .expect("LSP session map mutex should not poison");
                sessions
                    .entry(key.clone())
                    .or_insert_with(|| {
                        emit_session_event(
                            &self.diagnostic_events,
                            format!(
                                "session restart: server={}, root={}",
                                key.server_name, key.root_uri
                            ),
                        );
                        spawn_session_worker(key.clone(), self.diagnostic_events.clone())
                    })
                    .sender
                    .clone()
            };
            if let Err(error) = retry_sender.send(retry) {
                let _ = error
                    .0
                    .reply
                    .send(Err(command_failed("LSP session worker is unavailable")));
            }
        } else {
            emit_session_event(
                &self.diagnostic_events,
                format!(
                    "session reuse: server={}, root={}",
                    key.server_name, key.root_uri
                ),
            );
        }
    }

    fn submit_lsif_command(&self, command: LspSessionCommand) {
        let cache = self.lsif_cache.clone();
        let diagnostic_events = self.diagnostic_events.clone();
        thread::Builder::new()
            .name("saya-lsif-lookup".to_string())
            .spawn(move || {
                if command.cancelled.load(Ordering::SeqCst) {
                    emit_session_event(
                        &diagnostic_events,
                        format!(
                            "lsif request cancelled before dispatch: method={}",
                            command.request.method
                        ),
                    );
                    let _ = command
                        .reply
                        .send(Err(command_failed("cancelled LSIF request")));
                    return;
                }
                emit_session_event(
                    &diagnostic_events,
                    format!(
                        "lsif request dispatch: method={}, dump={}",
                        command.request.method, command.request.dump_path
                    ),
                );
                let result = cache
                    .lock()
                    .map_err(|_| command_failed("LSIF index cache mutex poisoned"))
                    .and_then(|mut cache| {
                        cache.execute_request(command.request, &diagnostic_events)
                    });
                let _ = command.reply.send(result);
            })
            .expect("LSIF lookup worker thread should start");
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LspSessionKey {
    server_name: String,
    root_uri: String,
    command: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
    cwd: Option<String>,
}

impl LspSessionKey {
    fn from_request(request: &LspRuntimeBridgeRequest) -> Result<Self, RuntimeCommandError> {
        let Some(server) = request.server.as_ref() else {
            return Err(RuntimeCommandError::CommandFailed {
                name: "lsp.request".to_string(),
                message: "LSP request is missing server definition".to_string(),
            });
        };
        let root_uri =
            request
                .root_uri
                .clone()
                .ok_or_else(|| RuntimeCommandError::CommandFailed {
                    name: "lsp.request".to_string(),
                    message: "LSP request is missing rootUri".to_string(),
                })?;
        Ok(Self {
            server_name: server.name.clone(),
            root_uri,
            command: server.command.clone(),
            args: server.args.clone(),
            env: server
                .env
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            cwd: server.cwd.clone(),
        })
    }
}

#[derive(Clone)]
struct LspSessionHandle {
    sender: Sender<LspSessionCommand>,
}

struct LspSessionCommand {
    request: LspRuntimeBridgeRequest,
    options: LspSessionRequestOptions,
    reply: Sender<Result<LspRuntimeBridgeResponse, RuntimeCommandError>>,
    cancelled: Arc<AtomicBool>,
}

struct QueuedLspSessionCommand {
    request: LspRuntimeBridgeRequest,
    reply: Option<Sender<Result<LspRuntimeBridgeResponse, RuntimeCommandError>>>,
    cancelled: Arc<AtomicBool>,
}

struct LspSessionWorker {
    key: LspSessionKey,
    client: Option<LspTransportClient>,
    initialized: bool,
    open_documents: HashSet<String>,
    deferred: VecDeque<QueuedLspSessionCommand>,
    diagnostic_events: Arc<Mutex<Vec<String>>>,
}

fn spawn_session_worker(
    key: LspSessionKey,
    diagnostic_events: Arc<Mutex<Vec<String>>>,
) -> LspSessionHandle {
    let (sender, receiver) = mpsc::channel::<LspSessionCommand>();
    thread::Builder::new()
        .name(format!("saya-lsp-session-{}", key.server_name))
        .spawn(move || {
            let mut worker = LspSessionWorker {
                key,
                client: None,
                initialized: false,
                open_documents: HashSet::new(),
                deferred: VecDeque::new(),
                diagnostic_events,
            };
            worker.run(receiver);
        })
        .expect("LSP session worker thread should start");
    LspSessionHandle { sender }
}

impl LspSessionWorker {
    fn run(&mut self, receiver: Receiver<LspSessionCommand>) {
        while let Ok(command) = receiver.recv() {
            if command.cancelled.load(Ordering::SeqCst) {
                emit_session_event(
                    &self.diagnostic_events,
                    format!(
                        "request cancelled before dispatch: method={}",
                        command.request.method
                    ),
                );
                let _ = command
                    .reply
                    .send(Err(command_failed("cancelled LSP request")));
                continue;
            }

            let method = command.request.method.clone();
            if !self.initialized && method != "initialize" {
                if is_lsp_notification_method(&method) {
                    self.apply_preinitialize_notification(&command.request);
                    self.deferred.push_back(QueuedLspSessionCommand {
                        request: command.request,
                        reply: None,
                        cancelled: command.cancelled,
                    });
                    let _ = command.reply.send(Ok(queued_response(&method)));
                    continue;
                }
                emit_session_event(
                    &self.diagnostic_events,
                    format!("request queued until initialize completes: method={method}"),
                );
                self.deferred.push_back(QueuedLspSessionCommand {
                    request: command.request,
                    reply: Some(command.reply),
                    cancelled: command.cancelled,
                });
                continue;
            }

            let should_shutdown = method == "shutdown";
            let result = self.execute_request(command.request, command.options.clone());
            let _ = command.reply.send(result);
            if should_shutdown {
                break;
            }
            if self.initialized {
                self.flush_deferred(command.options);
            }
        }
        emit_session_event(
            &self.diagnostic_events,
            format!(
                "session worker stopped: server={}, root={}",
                self.key.server_name, self.key.root_uri
            ),
        );
    }

    fn flush_deferred(&mut self, options: LspSessionRequestOptions) {
        while let Some(command) = self.deferred.pop_front() {
            if command.cancelled.load(Ordering::SeqCst) {
                emit_session_event(
                    &self.diagnostic_events,
                    format!(
                        "queued request cancelled before dispatch: method={}",
                        command.request.method
                    ),
                );
                if let Some(reply) = command.reply {
                    let _ = reply.send(Err(command_failed("cancelled LSP request")));
                }
                continue;
            }
            let result = self.execute_request(command.request, options.clone());
            if let Some(reply) = command.reply {
                let _ = reply.send(result);
            }
        }
    }

    fn execute_request(
        &mut self,
        request: LspRuntimeBridgeRequest,
        options: LspSessionRequestOptions,
    ) -> Result<LspRuntimeBridgeResponse, RuntimeCommandError> {
        request.validate()?;
        let method = request.method.clone();
        emit_session_event_fields(
            &self.diagnostic_events,
            "request_dispatch",
            format!(
                "request dispatch: method={method}, server={}, root={}",
                self.key.server_name, self.key.root_uri
            ),
            vec![
                ("method", Value::String(method.clone())),
                ("serverName", Value::String(self.key.server_name.clone())),
                ("workspaceRoot", Value::String(self.key.root_uri.clone())),
                ("trace", Value::String(normalize_lsp_trace(&request.trace))),
            ],
        );

        match method.as_str() {
            "initialize" => self.initialize(request, options),
            "shutdown" => self.shutdown(request),
            "initialized"
            | "textDocument/didOpen"
            | "textDocument/didSave"
            | "textDocument/didClose" => self.send_notification_request(request),
            _ => self.send_document_request(request),
        }
    }

    fn initialize(
        &mut self,
        request: LspRuntimeBridgeRequest,
        options: LspSessionRequestOptions,
    ) -> Result<LspRuntimeBridgeResponse, RuntimeCommandError> {
        let server = request
            .server
            .as_ref()
            .ok_or_else(|| RuntimeCommandError::CommandFailed {
                name: "lsp.request".to_string(),
                message: "LSP initialize request is missing server definition".to_string(),
            })?;
        if self.client.is_none() {
            self.client = Some(
                LspTransportClient::start_with_diagnostic_events(
                    server_config(server, &options),
                    self.diagnostic_events.clone(),
                )
                .map_err(transport_error)?,
            );
        }
        let params = initialize_params_with_trace(
            request.params.clone().unwrap_or(Value::Null),
            normalize_lsp_trace(&request.trace),
        );
        let result = self
            .client
            .as_mut()
            .expect("LSP client should be present after start")
            .initialize(params)
            .map_err(transport_error)?;
        self.initialized = true;
        emit_session_event(
            &self.diagnostic_events,
            format!("initialize complete: server={}", self.key.server_name),
        );
        Ok(LspRuntimeBridgeResponse {
            source: LspRuntimeBridgeSource::Lsp,
            method: request.method,
            result,
        })
    }

    fn shutdown(
        &mut self,
        request: LspRuntimeBridgeRequest,
    ) -> Result<LspRuntimeBridgeResponse, RuntimeCommandError> {
        if let Some(client) = self.client.as_mut() {
            client.shutdown().map_err(transport_error)?;
        }
        self.client = None;
        self.initialized = false;
        self.open_documents.clear();
        emit_session_event(
            &self.diagnostic_events,
            format!("shutdown complete: server={}", self.key.server_name),
        );
        Ok(LspRuntimeBridgeResponse {
            source: LspRuntimeBridgeSource::Lsp,
            method: request.method,
            result: json!({ "result": null }),
        })
    }

    fn send_notification_request(
        &mut self,
        request: LspRuntimeBridgeRequest,
    ) -> Result<LspRuntimeBridgeResponse, RuntimeCommandError> {
        self.apply_document_lifecycle(&request)?;
        let params = request.params.clone().unwrap_or(Value::Null);
        self.client_mut()?
            .send_notification(&request.method, params)
            .map_err(transport_error)?;
        let _ = self.client_mut()?.drain_notifications();
        Ok(LspRuntimeBridgeResponse {
            source: LspRuntimeBridgeSource::Lsp,
            method: request.method,
            result: json!({ "result": null }),
        })
    }

    fn send_document_request(
        &mut self,
        request: LspRuntimeBridgeRequest,
    ) -> Result<LspRuntimeBridgeResponse, RuntimeCommandError> {
        let document_uri = request
            .text_document
            .as_ref()
            .map(|document| document.uri.clone());
        if let Some(uri) = document_uri.as_ref() {
            if !self.open_documents.contains(uri) {
                return Err(command_failed(format!(
                    "LSP document is not open: method={}, uri={uri}",
                    request.method
                )));
            }
        }
        let params = request.params.clone().unwrap_or(Value::Null);
        let result = self
            .client_mut()?
            .request(&request.method, params)
            .map_err(transport_error)?;
        let _ = self.client_mut()?.drain_notifications();
        Ok(LspRuntimeBridgeResponse {
            source: LspRuntimeBridgeSource::Lsp,
            method: request.method,
            result,
        })
    }

    fn client_mut(&mut self) -> Result<&mut LspTransportClient, RuntimeCommandError> {
        self.client
            .as_mut()
            .ok_or_else(|| RuntimeCommandError::CommandFailed {
                name: "lsp.request".to_string(),
                message: "LSP server is not initialized".to_string(),
            })
    }

    fn apply_preinitialize_notification(&mut self, request: &LspRuntimeBridgeRequest) {
        if let Err(error) = self.apply_document_lifecycle(request) {
            emit_session_event(
                &self.diagnostic_events,
                format!(
                    "failed to apply preinitialize notification lifecycle: method={}, error={:?}",
                    request.method, error
                ),
            );
        }
    }

    fn apply_document_lifecycle(
        &mut self,
        request: &LspRuntimeBridgeRequest,
    ) -> Result<(), RuntimeCommandError> {
        match request.method.as_str() {
            "textDocument/didOpen" => {
                let Some(uri) = document_uri_from_request(request) else {
                    return Err(command_failed("LSP didOpen is missing textDocument.uri"));
                };
                self.open_documents.insert(uri.clone());
                emit_session_event(&self.diagnostic_events, format!("document open: uri={uri}"));
            }
            "textDocument/didSave" => {
                let Some(uri) = document_uri_from_request(request) else {
                    return Err(command_failed("LSP didSave is missing textDocument.uri"));
                };
                if !self.open_documents.contains(&uri) {
                    return Err(command_failed(format!(
                        "LSP document is not open: method={}, uri={uri}",
                        request.method
                    )));
                }
            }
            "textDocument/didClose" => {
                let Some(uri) = document_uri_from_request(request) else {
                    return Err(command_failed("LSP didClose is missing textDocument.uri"));
                };
                self.open_documents.remove(&uri);
                emit_session_event(
                    &self.diagnostic_events,
                    format!("document close: uri={uri}"),
                );
            }
            _ => {}
        }
        Ok(())
    }
}

fn server_config(
    server: &LspRuntimeServerDefinition,
    options: &LspSessionRequestOptions,
) -> LspServerConfig {
    LspServerConfig {
        command: server.command.clone(),
        args: server.args.clone(),
        env: server
            .env
            .iter()
            .map(clone_pair)
            .collect::<BTreeMap<_, _>>(),
        cwd: server.cwd.as_ref().map(PathBuf::from),
        request_timeout: options.request_timeout,
        startup_timeout: options.startup_timeout,
        shutdown_timeout: options.shutdown_timeout,
    }
}

fn clone_pair((key, value): (&String, &String)) -> (String, String) {
    (key.clone(), value.clone())
}

fn document_uri_from_request(request: &LspRuntimeBridgeRequest) -> Option<String> {
    request
        .params
        .as_ref()
        .and_then(|params| params.pointer("/textDocument/uri"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            request
                .text_document
                .as_ref()
                .map(|document| document.uri.clone())
        })
}

fn is_lsp_notification_method(method: &str) -> bool {
    matches!(
        method,
        "initialized" | "textDocument/didOpen" | "textDocument/didSave" | "textDocument/didClose"
    )
}

fn queued_response(method: &str) -> LspRuntimeBridgeResponse {
    LspRuntimeBridgeResponse {
        source: LspRuntimeBridgeSource::Lsp,
        method: method.to_string(),
        result: json!({ "queued": true }),
    }
}

fn normalize_lsp_trace(trace: &str) -> String {
    match trace {
        "messages" | "verbose" => trace.to_string(),
        _ => "off".to_string(),
    }
}

fn initialize_params_with_trace(params: Value, trace: String) -> Value {
    match params {
        Value::Object(mut object) => {
            object.insert("trace".to_string(), Value::String(trace));
            Value::Object(object)
        }
        _ => json!({ "trace": trace }),
    }
}

fn command_failed(message: impl Into<String>) -> RuntimeCommandError {
    RuntimeCommandError::CommandFailed {
        name: "lsp.request".to_string(),
        message: message.into(),
    }
}

fn transport_error(error: LspTransportError) -> RuntimeCommandError {
    command_failed(format!("LSP transport failed: {error}"))
}

fn emit_session_event(diagnostic_events: &Arc<Mutex<Vec<String>>>, message: String) {
    let event = message
        .split_once(':')
        .map(|(event, _)| event)
        .unwrap_or(message.as_str())
        .replace(' ', "_");
    emit_session_event_fields(diagnostic_events, &event, message, Vec::new());
}

fn emit_session_event_fields(
    diagnostic_events: &Arc<Mutex<Vec<String>>>,
    event: &str,
    message: String,
    fields: Vec<(&str, Value)>,
) {
    log::debug!("[lsp_session] {message}");
    let mut payload = Map::new();
    payload.insert(
        "target".to_string(),
        Value::String("lsp_session".to_string()),
    );
    payload.insert("event".to_string(), Value::String(event.to_string()));
    payload.insert("message".to_string(), Value::String(message));
    for (key, value) in fields {
        payload.insert(key.to_string(), value);
    }
    let line = Value::Object(payload).to_string();
    if let Ok(mut events) = diagnostic_events.lock() {
        events.push(line);
    }
}
