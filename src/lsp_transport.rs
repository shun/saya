//! LSP transport support is implemented separately from floating windows.

use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};

#[derive(Debug, Clone)]
pub struct LspServerConfig {
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<PathBuf>,
    pub request_timeout: Duration,
    pub startup_timeout: Duration,
    pub shutdown_timeout: Duration,
}

impl LspServerConfig {
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: None,
            request_timeout: Duration::from_secs(10),
            startup_timeout: Duration::from_secs(10),
            shutdown_timeout: Duration::from_secs(5),
        }
    }
}

#[derive(Debug)]
pub enum LspTransportError {
    Io {
        context: &'static str,
        message: String,
    },
    Json {
        context: &'static str,
        message: String,
    },
    MissingContentLength,
    MalformedHeader {
        line: String,
        message: String,
    },
    ServerError {
        id: u64,
        code: Option<i64>,
        message: String,
    },
    Timeout {
        phase: &'static str,
        method: String,
        timeout: Duration,
    },
    UnexpectedMessage {
        message: String,
    },
    ProcessExited,
}

impl std::fmt::Display for LspTransportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LspTransportError::Io { context, message } => {
                write!(formatter, "{context}: {message}")
            }
            LspTransportError::Json { context, message } => {
                write!(formatter, "{context}: {message}")
            }
            LspTransportError::MissingContentLength => {
                write!(formatter, "LSP message is missing Content-Length")
            }
            LspTransportError::MalformedHeader { line, message } => {
                write!(formatter, "malformed LSP header {line:?}: {message}")
            }
            LspTransportError::ServerError { id, code, message } => {
                write!(
                    formatter,
                    "LSP server returned an error: id={id}, code={code:?}, message={message}"
                )
            }
            LspTransportError::Timeout {
                phase,
                method,
                timeout,
            } => {
                write!(
                    formatter,
                    "timed out during {phase}: method={method}, timeout_ms={}",
                    timeout.as_millis()
                )
            }
            LspTransportError::UnexpectedMessage { message } => {
                write!(formatter, "unexpected LSP message: {message}")
            }
            LspTransportError::ProcessExited => write!(formatter, "LSP process exited"),
        }
    }
}

impl std::error::Error for LspTransportError {}

#[derive(Debug)]
pub struct LspTransportClient {
    config: LspServerConfig,
    child: Arc<Mutex<Child>>,
    stdin: Arc<Mutex<ChildStdin>>,
    receiver: Receiver<Result<LspWireMessage, LspTransportError>>,
    next_id: u64,
    pending_responses: BTreeMap<u64, Value>,
    notifications: VecDeque<Value>,
    diagnostic_events: Arc<Mutex<Vec<String>>>,
    shutdown_started: bool,
}

impl LspTransportClient {
    pub fn start(config: LspServerConfig) -> Result<Self, LspTransportError> {
        let diagnostic_events = Arc::new(Mutex::new(Vec::new()));
        Self::start_with_diagnostic_events(config, diagnostic_events)
    }

    pub(crate) fn start_with_diagnostic_events(
        config: LspServerConfig,
        diagnostic_events: Arc<Mutex<Vec<String>>>,
    ) -> Result<Self, LspTransportError> {
        emit_transport_event_fields(
            &diagnostic_events,
            "process_start",
            format!(
                "process start: command={}, args={:?}, cwd={}",
                config.command,
                config.args,
                config
                    .cwd
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "<inherit>".to_string())
            ),
            vec![
                ("command", Value::String(config.command.clone())),
                ("args", json!(config.args)),
                (
                    "cwd",
                    config
                        .cwd
                        .as_ref()
                        .map(|path| Value::String(path.display().to_string()))
                        .unwrap_or(Value::Null),
                ),
            ],
        );

        let mut command = Command::new(&config.command);
        command.args(&config.args);
        if let Some(cwd) = config.cwd.as_ref() {
            command.current_dir(cwd);
        }
        for (key, value) in &config.env {
            command.env(key, value);
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command.spawn().map_err(|error| LspTransportError::Io {
            context: "failed to spawn LSP process",
            message: error.to_string(),
        })?;
        let stdin = child.stdin.take().ok_or_else(|| LspTransportError::Io {
            context: "failed to open LSP stdin",
            message: "child stdin was not piped".to_string(),
        })?;
        let stdout = child.stdout.take().ok_or_else(|| LspTransportError::Io {
            context: "failed to open LSP stdout",
            message: "child stdout was not piped".to_string(),
        })?;
        let stderr = child.stderr.take().ok_or_else(|| LspTransportError::Io {
            context: "failed to open LSP stderr",
            message: "child stderr was not piped".to_string(),
        })?;

        let (sender, receiver) = mpsc::channel();
        spawn_lsp_stdout_reader(sender, stdout, diagnostic_events.clone());
        spawn_lsp_stderr_logger(stderr, diagnostic_events.clone());

        Ok(Self {
            config,
            child: Arc::new(Mutex::new(child)),
            stdin: Arc::new(Mutex::new(stdin)),
            receiver,
            next_id: 1,
            pending_responses: BTreeMap::new(),
            notifications: VecDeque::new(),
            diagnostic_events,
            shutdown_started: false,
        })
    }

    pub fn initialize(&mut self, params: Value) -> Result<Value, LspTransportError> {
        self.request_with_timeout("initialize", params, self.config.startup_timeout, "startup")
    }

    pub fn request(&mut self, method: &str, params: Value) -> Result<Value, LspTransportError> {
        self.request_with_timeout(method, params, self.config.request_timeout, "request")
    }

    pub fn send_request(&mut self, method: &str, params: Value) -> Result<u64, LspTransportError> {
        let id = self.next_id;
        self.next_id += 1;
        let redacted_params = redact_lsp_value(&params);
        self.write_message(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))?;
        emit_transport_event_fields(
            &self.diagnostic_events,
            "request_send",
            format!("request send: id={id}, method={method}"),
            vec![
                ("requestId", json!(id)),
                ("method", Value::String(method.to_string())),
                ("params", redacted_params),
            ],
        );
        Ok(id)
    }

    pub fn wait_for_response_by_id(
        &mut self,
        id: u64,
        method: &str,
    ) -> Result<Value, LspTransportError> {
        self.wait_for_response(id, method, self.config.request_timeout, "request")
    }

    pub fn send_notification(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<(), LspTransportError> {
        let mut message = Map::new();
        message.insert("jsonrpc".to_string(), Value::String("2.0".to_string()));
        message.insert("method".to_string(), Value::String(method.to_string()));
        let redacted_params = redact_lsp_value(&params);
        if !params.is_null() {
            message.insert("params".to_string(), params);
        }
        self.write_message(Value::Object(message))?;
        emit_transport_event_fields(
            &self.diagnostic_events,
            "notification_send",
            format!("notification send: method={method}"),
            vec![
                ("method", Value::String(method.to_string())),
                ("params", redacted_params),
            ],
        );
        Ok(())
    }

    pub fn drain_notifications(&mut self) -> Result<Vec<Value>, LspTransportError> {
        self.drain_ready_messages()?;
        Ok(self.notifications.drain(..).collect())
    }

    pub fn diagnostic_events(&self) -> Vec<String> {
        self.diagnostic_events
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }

    pub fn shutdown(&mut self) -> Result<(), LspTransportError> {
        if self.shutdown_started {
            return Ok(());
        }
        self.shutdown_started = true;
        emit_transport_event_fields(
            &self.diagnostic_events,
            "process_shutdown_start",
            "shutdown start".to_string(),
            Vec::new(),
        );

        let shutdown_result = self.request_with_timeout(
            "shutdown",
            Value::Null,
            self.config.shutdown_timeout,
            "shutdown",
        );
        if let Err(error) = shutdown_result {
            emit_transport_event_fields(
                &self.diagnostic_events,
                "process_shutdown_error",
                format!("shutdown graceful failed: error={error}"),
                vec![("error", Value::String(error.to_string()))],
            );
            let _ = self.kill_process();
            return Err(error);
        }

        self.send_notification("exit", Value::Null)?;
        self.wait_for_exit_or_kill()
    }

    fn request_with_timeout(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
        phase: &'static str,
    ) -> Result<Value, LspTransportError> {
        let id = self.send_request(method, params)?;
        self.wait_for_response(id, method, timeout, phase)
    }

    fn wait_for_response(
        &mut self,
        id: u64,
        method: &str,
        timeout: Duration,
        phase: &'static str,
    ) -> Result<Value, LspTransportError> {
        if let Some(response) = self.pending_responses.remove(&id) {
            emit_transport_event_fields(
                &self.diagnostic_events,
                "response_route",
                format!("response route: id={id}, method={method}, source=pending"),
                vec![
                    ("requestId", json!(id)),
                    ("method", Value::String(method.to_string())),
                    ("source", Value::String("pending".to_string())),
                ],
            );
            return response_result(id, response);
        }

        let started_at = Instant::now();
        loop {
            let Some(remaining) = timeout.checked_sub(started_at.elapsed()) else {
                emit_transport_event_fields(
                    &self.diagnostic_events,
                    "timeout",
                    format!("timeout: phase={phase}, method={method}, id={id}"),
                    vec![
                        ("phase", Value::String(phase.to_string())),
                        ("method", Value::String(method.to_string())),
                        ("requestId", json!(id)),
                        ("timeoutMs", json!(timeout.as_millis())),
                    ],
                );
                return Err(LspTransportError::Timeout {
                    phase,
                    method: method.to_string(),
                    timeout,
                });
            };

            match self.receiver.recv_timeout(remaining) {
                Ok(Ok(LspWireMessage::Response {
                    id: response_id,
                    message,
                })) if response_id == id => {
                    emit_transport_event_fields(
                        &self.diagnostic_events,
                        "response_route",
                        format!("response route: id={id}, method={method}"),
                        vec![
                            ("requestId", json!(id)),
                            ("method", Value::String(method.to_string())),
                            ("source", Value::String("live".to_string())),
                        ],
                    );
                    return response_result(id, message);
                }
                Ok(Ok(LspWireMessage::Response {
                    id: response_id,
                    message,
                })) => {
                    self.pending_responses.insert(response_id, message);
                }
                Ok(Ok(LspWireMessage::Notification(message))) => {
                    self.notifications.push_back(message);
                }
                Ok(Ok(LspWireMessage::ServerRequest(message))) => {
                    self.notifications.push_back(message);
                }
                Ok(Err(error)) => {
                    emit_transport_event_fields(
                        &self.diagnostic_events,
                        "transport_error",
                        format!("transport error: error={error}"),
                        vec![
                            ("method", Value::String(method.to_string())),
                            ("requestId", json!(id)),
                            ("error", Value::String(error.to_string())),
                        ],
                    );
                    return Err(error);
                }
                Err(RecvTimeoutError::Timeout) => {
                    emit_transport_event_fields(
                        &self.diagnostic_events,
                        "timeout",
                        format!("timeout: phase={phase}, method={method}, id={id}"),
                        vec![
                            ("phase", Value::String(phase.to_string())),
                            ("method", Value::String(method.to_string())),
                            ("requestId", json!(id)),
                            ("timeoutMs", json!(timeout.as_millis())),
                        ],
                    );
                    return Err(LspTransportError::Timeout {
                        phase,
                        method: method.to_string(),
                        timeout,
                    });
                }
                Err(RecvTimeoutError::Disconnected) => {
                    emit_transport_event_fields(
                        &self.diagnostic_events,
                        "process_exited",
                        format!("process exited before response: method={method}, id={id}"),
                        vec![
                            ("method", Value::String(method.to_string())),
                            ("requestId", json!(id)),
                        ],
                    );
                    return Err(LspTransportError::ProcessExited);
                }
            }
        }
    }

    fn drain_ready_messages(&mut self) -> Result<(), LspTransportError> {
        loop {
            match self.receiver.try_recv() {
                Ok(Ok(LspWireMessage::Response { id, message })) => {
                    self.pending_responses.insert(id, message);
                }
                Ok(Ok(LspWireMessage::Notification(message))) => {
                    self.notifications.push_back(message);
                }
                Ok(Ok(LspWireMessage::ServerRequest(message))) => {
                    self.notifications.push_back(message);
                }
                Ok(Err(error)) => return Err(error),
                Err(TryRecvError::Empty) => return Ok(()),
                Err(TryRecvError::Disconnected) => return Ok(()),
            }
        }
    }

    fn write_message(&mut self, message: Value) -> Result<(), LspTransportError> {
        let frame = encode_lsp_message(&message)?;
        let mut stdin = self.stdin.lock().map_err(|error| LspTransportError::Io {
            context: "failed to lock LSP stdin",
            message: error.to_string(),
        })?;
        stdin
            .write_all(&frame)
            .and_then(|_| stdin.flush())
            .map_err(|error| LspTransportError::Io {
                context: "failed to write LSP message",
                message: error.to_string(),
            })
    }

    fn wait_for_exit_or_kill(&mut self) -> Result<(), LspTransportError> {
        let started_at = Instant::now();
        loop {
            let exited = {
                let mut child = self.child.lock().map_err(|error| LspTransportError::Io {
                    context: "failed to lock LSP process",
                    message: error.to_string(),
                })?;
                child.try_wait().map_err(|error| LspTransportError::Io {
                    context: "failed to wait for LSP process",
                    message: error.to_string(),
                })?
            };
            if let Some(status) = exited {
                emit_transport_event_fields(
                    &self.diagnostic_events,
                    "process_shutdown_complete",
                    format!("shutdown complete: exit_status={status}"),
                    vec![("exitStatus", Value::String(status.to_string()))],
                );
                return Ok(());
            }
            if started_at.elapsed() >= self.config.shutdown_timeout {
                emit_transport_event_fields(
                    &self.diagnostic_events,
                    "process_shutdown_timeout",
                    "shutdown timeout; killing process".to_string(),
                    vec![("timeoutMs", json!(self.config.shutdown_timeout.as_millis()))],
                );
                self.kill_process()?;
                return Err(LspTransportError::Timeout {
                    phase: "shutdown",
                    method: "exit".to_string(),
                    timeout: self.config.shutdown_timeout,
                });
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn kill_process(&mut self) -> Result<(), LspTransportError> {
        let mut child = self.child.lock().map_err(|error| LspTransportError::Io {
            context: "failed to lock LSP process",
            message: error.to_string(),
        })?;
        if child
            .try_wait()
            .map_err(|error| LspTransportError::Io {
                context: "failed to inspect LSP process",
                message: error.to_string(),
            })?
            .is_none()
        {
            child.kill().map_err(|error| LspTransportError::Io {
                context: "failed to kill LSP process",
                message: error.to_string(),
            })?;
            let _ = child.wait();
            emit_transport_event_fields(
                &self.diagnostic_events,
                "process_killed",
                "process killed".to_string(),
                Vec::new(),
            );
        }
        Ok(())
    }
}

impl Drop for LspTransportClient {
    fn drop(&mut self) {
        let _ = self.kill_process();
    }
}

#[derive(Debug)]
enum LspWireMessage {
    Response { id: u64, message: Value },
    Notification(Value),
    ServerRequest(Value),
}

pub fn encode_lsp_message(message: &Value) -> Result<Vec<u8>, LspTransportError> {
    let body = serde_json::to_vec(message).map_err(|error| LspTransportError::Json {
        context: "failed to encode LSP message",
        message: error.to_string(),
    })?;
    let mut frame = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    frame.extend_from_slice(&body);
    Ok(frame)
}

pub fn read_lsp_message(reader: &mut impl BufRead) -> Result<Option<Value>, LspTransportError> {
    let mut content_length = None;

    loop {
        let mut line = String::new();
        let bytes = reader
            .read_line(&mut line)
            .map_err(|error| LspTransportError::Io {
                context: "failed to read LSP header",
                message: error.to_string(),
            })?;
        if bytes == 0 {
            return Ok(None);
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let Some((name, value)) = trimmed.split_once(':') else {
            return Err(LspTransportError::MalformedHeader {
                line: trimmed.to_string(),
                message: "missing ':' separator".to_string(),
            });
        };
        if name.eq_ignore_ascii_case("content-length") {
            content_length = Some(value.trim().parse::<usize>().map_err(|error| {
                LspTransportError::MalformedHeader {
                    line: trimmed.to_string(),
                    message: error.to_string(),
                }
            })?);
        }
    }

    let content_length = content_length.ok_or(LspTransportError::MissingContentLength)?;
    let mut body = vec![0; content_length];
    reader
        .read_exact(&mut body)
        .map_err(|error| LspTransportError::Io {
            context: "failed to read LSP body",
            message: error.to_string(),
        })?;
    serde_json::from_slice(&body)
        .map_err(|error| LspTransportError::Json {
            context: "failed to decode LSP body",
            message: error.to_string(),
        })
        .map(Some)
}

fn spawn_lsp_stdout_reader<R>(
    sender: Sender<Result<LspWireMessage, LspTransportError>>,
    stdout: R,
    diagnostic_events: Arc<Mutex<Vec<String>>>,
) where
    R: Read + Send + 'static,
{
    thread::Builder::new()
        .name("saya-lsp-stdout-reader".to_string())
        .spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_lsp_message(&mut reader) {
                    Ok(Some(message)) => {
                        let wire_message = classify_lsp_message(message);
                        match &wire_message {
                            LspWireMessage::Response { id, .. } => emit_transport_event_fields(
                                &diagnostic_events,
                                "response_receive",
                                format!("response receive: id={id}"),
                                vec![("requestId", json!(id))],
                            ),
                            LspWireMessage::Notification(message) => emit_transport_event_fields(
                                &diagnostic_events,
                                "notification_receive",
                                format!(
                                    "notification receive: method={}",
                                    message
                                        .get("method")
                                        .and_then(Value::as_str)
                                        .unwrap_or("<unknown>")
                                ),
                                vec![(
                                    "method",
                                    Value::String(
                                        message
                                            .get("method")
                                            .and_then(Value::as_str)
                                            .unwrap_or("<unknown>")
                                            .to_string(),
                                    ),
                                )],
                            ),
                            LspWireMessage::ServerRequest(message) => emit_transport_event_fields(
                                &diagnostic_events,
                                "server_request_receive",
                                format!(
                                    "server request receive: method={}",
                                    message
                                        .get("method")
                                        .and_then(Value::as_str)
                                        .unwrap_or("<unknown>")
                                ),
                                vec![(
                                    "method",
                                    Value::String(
                                        message
                                            .get("method")
                                            .and_then(Value::as_str)
                                            .unwrap_or("<unknown>")
                                            .to_string(),
                                    ),
                                )],
                            ),
                        }
                        if sender.send(Ok(wire_message)).is_err() {
                            break;
                        }
                    }
                    Ok(None) => {
                        emit_transport_event(
                            &diagnostic_events,
                            "process stdout closed".to_string(),
                        );
                        let _ = sender.send(Err(LspTransportError::ProcessExited));
                        break;
                    }
                    Err(error) => {
                        emit_transport_event(
                            &diagnostic_events,
                            format!("reader error: error={error}"),
                        );
                        let _ = sender.send(Err(error));
                        break;
                    }
                }
            }
        })
        .expect("LSP stdout reader thread should start");
}

fn spawn_lsp_stderr_logger<R>(stderr: R, diagnostic_events: Arc<Mutex<Vec<String>>>)
where
    R: Read + Send + 'static,
{
    thread::Builder::new()
        .name("saya-lsp-stderr-reader".to_string())
        .spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                emit_transport_event(&diagnostic_events, format!("stderr: {line}"));
            }
        })
        .expect("LSP stderr reader thread should start");
}

fn classify_lsp_message(message: Value) -> LspWireMessage {
    if message.get("id").is_some() && message.get("method").is_none() {
        let id = message
            .get("id")
            .and_then(Value::as_u64)
            .or_else(|| {
                message
                    .get("id")
                    .and_then(Value::as_i64)
                    .and_then(|id| u64::try_from(id).ok())
            })
            .unwrap_or(0);
        return LspWireMessage::Response { id, message };
    }

    if message.get("method").is_some() && message.get("id").is_none() {
        return LspWireMessage::Notification(message);
    }

    LspWireMessage::ServerRequest(message)
}

fn response_result(id: u64, message: Value) -> Result<Value, LspTransportError> {
    if let Some(error) = message.get("error") {
        return Err(LspTransportError::ServerError {
            id,
            code: error.get("code").and_then(Value::as_i64),
            message: error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown LSP server error")
                .to_string(),
        });
    }
    Ok(message)
}

fn redact_lsp_value(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    if key == "text" {
                        (key.clone(), Value::String("<redacted>".to_string()))
                    } else {
                        (key.clone(), redact_lsp_value(value))
                    }
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(redact_lsp_value).collect()),
        _ => value.clone(),
    }
}

fn emit_transport_event(diagnostic_events: &Arc<Mutex<Vec<String>>>, message: String) {
    let event = message
        .split_once(':')
        .map(|(event, _)| event)
        .unwrap_or(message.as_str())
        .replace(' ', "_");
    emit_transport_event_fields(diagnostic_events, &event, message, Vec::new());
}

fn emit_transport_event_fields(
    diagnostic_events: &Arc<Mutex<Vec<String>>>,
    event: &str,
    message: String,
    fields: Vec<(&str, Value)>,
) {
    log::debug!("[lsp_transport] {message}");
    let mut payload = Map::new();
    payload.insert(
        "target".to_string(),
        Value::String("lsp_transport".to_string()),
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
