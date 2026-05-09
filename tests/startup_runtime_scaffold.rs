use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex as StdMutex, mpsc as std_mpsc};
use std::time::Duration;

use saya::callback_registry_seed::CallbackRegistrySeed;
use saya::lsp_runtime_bridge::{
    LspRuntimeBridgeRequest, LspRuntimeBridgeResponse, LspRuntimeBridgeSource,
};
use saya::saya_live_runtime::{
    BoxFuture, BufferEventPayload, HostCapabilityBridge, ReadonlyBufferSnapshot,
    ReadonlyEditorSnapshot, ReadonlyWindowSnapshot, RuntimeCommandError, RuntimeEventPayload,
    RuntimeMode, SayaLiveRuntime,
};
use saya::startup_runtime::{
    SayaKeyMode, SayaKeymapAction, StartupModuleLoadResult, StartupModulePrepareResult,
    StartupOptionName, StartupOptionValue, StartupRegistryEntry, collect_startup_registry,
    evaluate_startup_module, load_init_module, prepare_init_module, resolve_init_module_specifier,
};
use saya::theme::{
    MarkdownSemanticStyleKey, SyntaxSemanticStyleKey, ThemeTextStyleDeclaration, UiStyleKey,
};
use serde_json::{Value, json};

fn unique_path(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-startup-runtime-{name}-{nanos}"))
}

struct NoopHostBridge;

impl HostCapabilityBridge for NoopHostBridge {
    fn execute_host_command(&self, _name: &str) -> BoxFuture<Result<(), RuntimeCommandError>> {
        Box::pin(async move { Ok(()) })
    }

    fn current_buffer(&self) -> BoxFuture<ReadonlyBufferSnapshot> {
        Box::pin(async move {
            ReadonlyBufferSnapshot {
                id: 1,
                path: None,
                line_count: 1,
                cursor_row: 0,
                cursor_col: 0,
                current_line: String::new(),
                text: String::new(),
            }
        })
    }

    fn current_window(&self) -> BoxFuture<ReadonlyWindowSnapshot> {
        Box::pin(async move { ReadonlyWindowSnapshot { id: 1 } })
    }

    fn current_editor(&self) -> BoxFuture<ReadonlyEditorSnapshot> {
        Box::pin(async move {
            ReadonlyEditorSnapshot {
                mode: RuntimeMode::Normal,
            }
        })
    }
}

struct FakeLspProtocolHostBridge {
    requests: Arc<StdMutex<Vec<LspRuntimeBridgeRequest>>>,
    host_commands: Arc<StdMutex<Vec<String>>>,
    buffer: ReadonlyBufferSnapshot,
}

impl FakeLspProtocolHostBridge {
    fn new(document_path: &Path, document_text: String) -> Self {
        Self {
            requests: Arc::new(StdMutex::new(Vec::new())),
            host_commands: Arc::new(StdMutex::new(Vec::new())),
            buffer: ReadonlyBufferSnapshot {
                id: 77,
                path: Some(document_path.to_path_buf()),
                line_count: document_text.lines().count(),
                cursor_row: 2,
                cursor_col: 1,
                current_line: "\tprintln(\"saya\")".to_string(),
                text: document_text,
            },
        }
    }

    fn observed_methods(&self) -> Vec<String> {
        self.requests
            .lock()
            .expect("fake LSP protocol requests lock")
            .iter()
            .map(|request| request.method.clone())
            .collect()
    }

    fn observed_requests(&self) -> Vec<LspRuntimeBridgeRequest> {
        self.requests
            .lock()
            .expect("fake LSP protocol requests lock")
            .clone()
    }

    fn observed_host_commands(&self) -> Vec<String> {
        self.host_commands
            .lock()
            .expect("fake LSP protocol host commands lock")
            .clone()
    }
}

impl HostCapabilityBridge for FakeLspProtocolHostBridge {
    fn execute_host_command(&self, name: &str) -> BoxFuture<Result<(), RuntimeCommandError>> {
        self.host_commands
            .lock()
            .expect("fake LSP protocol host commands lock")
            .push(name.to_string());
        Box::pin(async move { Ok(()) })
    }

    fn execute_lsp_request(
        &self,
        request: LspRuntimeBridgeRequest,
    ) -> BoxFuture<Result<LspRuntimeBridgeResponse, RuntimeCommandError>> {
        eprintln!(
            "[saya-lsp-fake-protocol-smoke] typed host bridge request: method={}, source={:?}, client={}, root={:?}",
            request.method, request.source, request.client_name, request.root_uri
        );
        self.requests
            .lock()
            .expect("fake LSP protocol requests lock")
            .push(request.clone());
        Box::pin(async move {
            let result = match request.method.as_str() {
                "initialize" => json!({
                    "capabilities": {
                        "hoverProvider": true,
                        "definitionProvider": true,
                        "referencesProvider": true,
                        "documentSymbolProvider": true,
                        "textDocumentSync": 1,
                        "positionEncoding": "utf-16"
                    },
                    "serverInfo": {
                        "name": "fake-lsp-ci"
                    }
                }),
                "textDocument/hover" => json!({
                    "contents": {
                        "kind": "plaintext",
                        "value": "hover from fake CI server"
                    }
                }),
                "textDocument/documentSymbol" => json!([
                    {
                        "name": "main",
                        "kind": 12,
                        "range": {
                            "start": { "line": 2, "character": 0 },
                            "end": { "line": 4, "character": 1 }
                        },
                        "selectionRange": {
                            "start": { "line": 2, "character": 5 },
                            "end": { "line": 2, "character": 9 }
                        }
                    }
                ]),
                "textDocument/definition" | "textDocument/references" => json!([
                    {
                        "uri": request
                            .text_document
                            .as_ref()
                            .map(|document| document.uri.clone())
                            .unwrap_or_default(),
                        "range": {
                            "start": { "line": 2, "character": 0 },
                            "end": { "line": 2, "character": 4 }
                        }
                    }
                ]),
                _ => Value::Null,
            };
            Ok(LspRuntimeBridgeResponse {
                source: request.source,
                method: request.method,
                result,
            })
        })
    }

    fn current_buffer(&self) -> BoxFuture<ReadonlyBufferSnapshot> {
        let buffer = self.buffer.clone();
        Box::pin(async move { buffer })
    }

    fn current_window(&self) -> BoxFuture<ReadonlyWindowSnapshot> {
        Box::pin(async move { ReadonlyWindowSnapshot { id: 77 } })
    }

    fn current_editor(&self) -> BoxFuture<ReadonlyEditorSnapshot> {
        Box::pin(async move {
            ReadonlyEditorSnapshot {
                mode: RuntimeMode::Normal,
            }
        })
    }
}

struct GoplsSmokeHostBridge {
    session: Arc<StdMutex<GoplsSmokeSession>>,
    buffer: ReadonlyBufferSnapshot,
}

impl GoplsSmokeHostBridge {
    fn new(
        gopls_command: &Path,
        workspace: &Path,
        document_path: &Path,
        document_text: String,
    ) -> Self {
        eprintln!(
            "[saya-lsp-gopls-smoke] starting gopls session: command={}, workspace={}, document={}",
            gopls_command.display(),
            workspace.display(),
            document_path.display()
        );
        let session = GoplsSmokeSession::start(
            gopls_command,
            workspace,
            document_path,
            document_text.clone(),
        )
        .expect("gopls smoke session should start");
        Self {
            session: Arc::new(StdMutex::new(session)),
            buffer: ReadonlyBufferSnapshot {
                id: 42,
                path: Some(document_path.to_path_buf()),
                line_count: document_text.lines().count(),
                cursor_row: 2,
                cursor_col: 0,
                current_line: "main()".to_string(),
                text: document_text,
            },
        }
    }

    fn observed_methods(&self) -> Vec<String> {
        self.session
            .lock()
            .expect("gopls session lock")
            .observed_methods
            .clone()
    }
}

impl HostCapabilityBridge for GoplsSmokeHostBridge {
    fn execute_host_command(&self, name: &str) -> BoxFuture<Result<(), RuntimeCommandError>> {
        let session = self.session.clone();
        let name = name.to_string();
        Box::pin(async move {
            eprintln!("[saya-lsp-gopls-smoke] host bridge command: {name}");
            session
                .lock()
                .expect("gopls session lock")
                .handle_bridge_command(&name)
                .map_err(|message| RuntimeCommandError::CommandFailed { name, message })
        })
    }

    fn execute_lsp_request(
        &self,
        request: LspRuntimeBridgeRequest,
    ) -> BoxFuture<Result<LspRuntimeBridgeResponse, RuntimeCommandError>> {
        let session = self.session.clone();
        Box::pin(async move {
            let method = request.method.clone();
            eprintln!("[saya-lsp-gopls-smoke] typed host bridge request: method={method}");
            session
                .lock()
                .expect("gopls session lock")
                .handle_typed_lsp_request(request)
                .map_err(|message| RuntimeCommandError::CommandFailed {
                    name: "lsp.request".to_string(),
                    message,
                })
        })
    }

    fn current_buffer(&self) -> BoxFuture<ReadonlyBufferSnapshot> {
        let buffer = self.buffer.clone();
        Box::pin(async move { buffer })
    }

    fn current_window(&self) -> BoxFuture<ReadonlyWindowSnapshot> {
        Box::pin(async move { ReadonlyWindowSnapshot { id: 7 } })
    }

    fn current_editor(&self) -> BoxFuture<ReadonlyEditorSnapshot> {
        Box::pin(async move {
            ReadonlyEditorSnapshot {
                mode: RuntimeMode::Normal,
            }
        })
    }
}

struct GoplsSmokeSession {
    child: Child,
    stdin: ChildStdin,
    receiver: std_mpsc::Receiver<Result<Value, String>>,
    next_id: i64,
    document_uri: String,
    document_text: String,
    root_uri: String,
    observed_methods: Vec<String>,
}

impl GoplsSmokeSession {
    fn start(
        gopls_command: &Path,
        workspace: &Path,
        document_path: &Path,
        document_text: String,
    ) -> Result<Self, String> {
        let mut child = Command::new(gopls_command)
            .current_dir(workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("failed to spawn gopls: {error}"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "failed to open gopls stdin".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "failed to open gopls stdout".to_string())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "failed to open gopls stderr".to_string())?;
        let receiver = spawn_lsp_reader(stdout);
        spawn_lsp_stderr_logger(stderr);

        Ok(Self {
            child,
            stdin,
            receiver,
            next_id: 1,
            document_uri: file_uri(document_path),
            document_text,
            root_uri: file_uri(workspace),
            observed_methods: Vec::new(),
        })
    }

    fn handle_bridge_command(&mut self, command: &str) -> Result<(), String> {
        let (bridge_command, payload_text) = command
            .split_once(' ')
            .ok_or_else(|| format!("bridge command payload missing: {command}"))?;
        if bridge_command != "host.lsp" {
            return Err(format!("unexpected bridge command: {bridge_command}"));
        }
        let payload: Value = serde_json::from_str(payload_text)
            .map_err(|error| format!("invalid bridge payload JSON: {error}"))?;
        self.handle_bridge_payload(&payload)
    }

    fn handle_typed_lsp_request(
        &mut self,
        request: LspRuntimeBridgeRequest,
    ) -> Result<LspRuntimeBridgeResponse, String> {
        let method = request.method.clone();
        let payload = serde_json::to_value(&request)
            .map_err(|error| format!("failed to encode typed LSP request: {error}"))?;
        self.handle_bridge_payload(&payload)?;
        Ok(LspRuntimeBridgeResponse {
            source: LspRuntimeBridgeSource::Lsp,
            method,
            result: Value::Null,
        })
    }

    fn handle_bridge_payload(&mut self, payload: &Value) -> Result<(), String> {
        let method = payload
            .get("method")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("bridge payload method missing: {payload}"))?;
        eprintln!("[saya-lsp-gopls-smoke] translate bridge method to gopls: {method}");

        match method {
            "initialize" => {
                let response = self.request(
                    "initialize",
                    json!({
                        "processId": null,
                        "clientInfo": {
                            "name": payload.get("clientName").and_then(Value::as_str).unwrap_or("saya-test")
                        },
                        "rootUri": payload.get("rootUri").and_then(Value::as_str).unwrap_or(&self.root_uri),
                        "capabilities": {
                            "textDocument": {
                                "hover": { "dynamicRegistration": false },
                                "documentSymbol": { "dynamicRegistration": false }
                            }
                        },
                        "trace": payload.get("trace").and_then(Value::as_str).unwrap_or("off")
                    }),
                )?;
                let capabilities = response
                    .get("result")
                    .and_then(|result| result.get("capabilities"))
                    .ok_or_else(|| {
                        format!("initialize response missing capabilities: {response}")
                    })?;
                eprintln!(
                    "[saya-lsp-gopls-smoke] initialize completed: capabilities_present={}",
                    capabilities.is_object()
                );
                Ok(())
            }
            "initialized" => self.notify("initialized", json!({})),
            "textDocument/didOpen" => self.notify(
                "textDocument/didOpen",
                payload.get("params").cloned().unwrap_or_else(|| {
                    json!({
                        "textDocument": {
                            "uri": self.document_uri,
                            "languageId": "go",
                            "version": 1,
                            "text": self.document_text
                        }
                    })
                }),
            ),
            "textDocument/didChange" => self.notify(
                "textDocument/didChange",
                payload.get("params").cloned().unwrap_or_else(|| {
                    json!({
                        "textDocument": {
                            "uri": self.document_uri,
                            "version": 2
                        },
                        "contentChanges": [
                            { "text": self.document_text }
                        ]
                    })
                }),
            ),
            "textDocument/didSave" => self.notify(
                "textDocument/didSave",
                payload.get("params").cloned().unwrap_or_else(|| {
                    json!({
                        "textDocument": { "uri": self.document_uri },
                        "text": self.document_text
                    })
                }),
            ),
            "textDocument/didClose" => self.notify(
                "textDocument/didClose",
                payload.get("params").cloned().unwrap_or_else(|| {
                    json!({
                        "textDocument": { "uri": self.document_uri }
                    })
                }),
            ),
            "textDocument/hover" => {
                let response =
                    self.request(method, self.text_document_position_params(&payload))?;
                eprintln!("[saya-lsp-gopls-smoke] hover response observed: {response}");
                Ok(())
            }
            "textDocument/documentSymbol" => {
                let response = self.request(
                    method,
                    json!({
                        "textDocument": { "uri": self.document_uri }
                    }),
                )?;
                let symbol_count = response
                    .get("result")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len);
                eprintln!(
                    "[saya-lsp-gopls-smoke] documentSymbol response observed: symbol_count={symbol_count}"
                );
                if symbol_count == 0 {
                    return Err(format!("gopls returned no document symbols: {response}"));
                }
                Ok(())
            }
            "shutdown" => {
                let response = self.request("shutdown", Value::Null)?;
                eprintln!("[saya-lsp-gopls-smoke] shutdown response observed: {response}");
                self.notify("exit", Value::Null)
            }
            other => Err(format!("unexpected LSP method in smoke bridge: {other}")),
        }
    }

    fn text_document_position_params(&self, payload: &Value) -> Value {
        let position = payload.get("position").cloned().unwrap_or_else(|| {
            json!({
                "line": 2,
                "character": 0
            })
        });
        json!({
            "textDocument": { "uri": self.document_uri },
            "position": position
        })
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.observed_methods.push(method.to_string());
        self.write_message(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        }))?;
        eprintln!("[saya-lsp-gopls-smoke] sent request: id={id}, method={method}");

        loop {
            let message = self
                .receiver
                .recv_timeout(Duration::from_secs(10))
                .map_err(|error| format!("timed out waiting for {method} response: {error}"))??;
            if message.get("id").and_then(Value::as_i64) == Some(id) {
                if let Some(error) = message.get("error") {
                    return Err(format!("gopls returned error for {method}: {error}"));
                }
                eprintln!("[saya-lsp-gopls-smoke] received response: id={id}, method={method}");
                return Ok(message);
            }
            eprintln!("[saya-lsp-gopls-smoke] received side message while waiting: {message}");
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        self.observed_methods.push(method.to_string());
        self.write_message(json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        }))?;
        eprintln!("[saya-lsp-gopls-smoke] sent notification: method={method}");
        Ok(())
    }

    fn write_message(&mut self, message: Value) -> Result<(), String> {
        let body = serde_json::to_string(&message)
            .map_err(|error| format!("failed to encode LSP message: {error}"))?;
        write!(self.stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body)
            .map_err(|error| format!("failed to write LSP message: {error}"))?;
        self.stdin
            .flush()
            .map_err(|error| format!("failed to flush LSP message: {error}"))
    }
}

impl Drop for GoplsSmokeSession {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn spawn_lsp_reader(stdout: ChildStdout) -> std_mpsc::Receiver<Result<Value, String>> {
    let (sender, receiver) = std_mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            match read_lsp_message(&mut reader) {
                Ok(Some(message)) => {
                    eprintln!("[saya-lsp-gopls-smoke] received gopls message: {message}");
                    if sender.send(Ok(message)).is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let _ = sender.send(Err(error));
                    break;
                }
            }
        }
    });
    receiver
}

fn spawn_lsp_stderr_logger(stderr: impl Read + Send + 'static) {
    std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            eprintln!("[saya-lsp-gopls-smoke][stderr] {line}");
        }
    });
}

fn read_lsp_message(reader: &mut impl BufRead) -> Result<Option<Value>, String> {
    let mut content_length = None;

    loop {
        let mut line = String::new();
        let bytes = reader
            .read_line(&mut line)
            .map_err(|error| format!("failed to read LSP header: {error}"))?;
        if bytes == 0 {
            return Ok(None);
        }
        if line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.trim_end().split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = Some(
                    value
                        .trim()
                        .parse::<usize>()
                        .map_err(|error| format!("invalid Content-Length header: {error}"))?,
                );
            }
        }
    }

    let content_length =
        content_length.ok_or_else(|| "LSP message missing Content-Length".to_string())?;
    let mut body = vec![0; content_length];
    reader
        .read_exact(&mut body)
        .map_err(|error| format!("failed to read LSP body: {error}"))?;
    serde_json::from_slice(&body).map(Some).map_err(|error| {
        format!(
            "failed to parse LSP body as JSON: {error}; body={}",
            String::from_utf8_lossy(&body)
        )
    })
}

fn file_uri(path: &Path) -> String {
    format!("file://{}", path.to_string_lossy())
}

fn resolve_gopls_command() -> PathBuf {
    if let Ok(output) = Command::new("mise").args(["which", "gopls"]).output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return PathBuf::from(path);
            }
        }
    }
    PathBuf::from("gopls")
}

fn go_mod_version_from_go_version_output(output: &str) -> Option<String> {
    let version = output.split_whitespace().find_map(|part| {
        let version = part.strip_prefix("go")?;
        version
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit())
            .then_some(version)
    })?;
    let mut parts = version.split('.');
    let major = parts.next()?;
    let minor = parts.next()?;
    Some(format!("{major}.{minor}"))
}

#[test]
fn init_ts_path_is_resolved_as_a_file_module_specifier() {
    let current_dir = unique_path("cwd");
    let specifier = resolve_init_module_specifier("init.ts", &current_dir).expect("specifier");

    assert_eq!(
        specifier.as_str(),
        format!("file://{}/init.ts", current_dir.to_string_lossy())
    );
}

#[test]
fn init_ts_module_loads_as_a_local_file_module() {
    let current_dir = unique_path("cwd");
    std::fs::create_dir_all(&current_dir).expect("current dir");
    let config_path = current_dir.join("init.ts");
    std::fs::write(&config_path, "saya.options.tabSize = 4;").expect("config file");

    let result = load_init_module(&config_path, &current_dir);

    match result {
        StartupModuleLoadResult::Success(module) => {
            assert_eq!(
                module.specifier.as_str(),
                format!("file://{}/init.ts", current_dir.to_string_lossy())
            );
            assert_eq!(module.source_text, "saya.options.tabSize = 4;");
            assert_eq!(module.path, config_path);
        }
        other => panic!("Success を返すこと, got: {:?}", other),
    }
}

#[test]
fn init_ts_module_read_failure_is_reported_structurally() {
    let current_dir = unique_path("cwd");
    std::fs::create_dir_all(&current_dir).expect("current dir");
    let config_path = current_dir.join("missing-init.ts");

    let result = load_init_module(&config_path, &current_dir);

    assert!(matches!(
        result,
        StartupModuleLoadResult::ReadFailed { ref path, .. } if path == &config_path
    ));
}

#[test]
fn init_ts_module_transpiles_into_executable_javascript() {
    let current_dir = unique_path("cwd");
    std::fs::create_dir_all(&current_dir).expect("current dir");
    let config_path = current_dir.join("init.ts");
    std::fs::write(
        &config_path,
        r#"
            const tabSize: number = 4;
            saya.options.tabSize = tabSize;
        "#,
    )
    .expect("config file");

    let result = prepare_init_module(&config_path, &current_dir);

    match result {
        StartupModulePrepareResult::Success(module) => {
            assert!(module.executable_source_text.contains("const tabSize = 4;"));
            assert!(
                module
                    .executable_source_text
                    .contains("saya.options.tabSize = tabSize;")
            );
            assert_eq!(module.path, config_path);
        }
        other => panic!("Success を返すこと, got: {:?}", other),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn init_ts_module_can_import_local_typescript_plugin() {
    let current_dir = unique_path("cwd");
    std::fs::create_dir_all(&current_dir).expect("current dir");
    let config_path = current_dir.join("init.ts");
    let plugin_path = current_dir.join("saya-dired.ts");
    std::fs::write(
        &plugin_path,
        r#"
            export interface SayaDiredOptions {
              enterKey?: string;
            }

            export function setupSayaDired(options: SayaDiredOptions = {}): void {
              const enterKey = options.enterKey ?? "<Enter>";
              saya.commands.register("dired.enter", async () => {});
              saya.keymap.set("normal", enterKey, saya.commands.execute("dired.enter"));
            }
        "#,
    )
    .expect("plugin file");
    std::fs::write(
        &config_path,
        r#"
            import { setupSayaDired } from "./saya-dired.ts";
            setupSayaDired();
        "#,
    )
    .expect("config file");

    let prepared = prepare_init_module(&config_path, &current_dir);
    let StartupModulePrepareResult::Success(module) = prepared else {
        panic!("imported local plugin should prepare, got: {:?}", prepared);
    };
    assert!(
        module
            .executable_source_text
            .contains("function setupSayaDired"),
        "plugin function should be inlined into the executable source: {}",
        module.executable_source_text
    );
    assert!(
        !module
            .executable_source_text
            .contains("interface SayaDiredOptions"),
        "type-only plugin declarations must be stripped"
    );

    let registry = collect_startup_registry(&module.executable_source_text)
        .await
        .expect("inlined plugin should evaluate");

    assert!(registry.entries().iter().any(|entry| {
        matches!(
            entry,
            StartupRegistryEntry::Command { name, .. } if name == "dired.enter"
        )
    }));
    assert!(registry.entries().iter().any(|entry| {
        matches!(
            entry,
            StartupRegistryEntry::Keymap {
                mode: SayaKeyMode::Normal,
                lhs,
                action: SayaKeymapAction::RegisteredCommand(command),
            } if lhs == "<Enter>" && command == "dired.enter"
        )
    }));
}

#[tokio::test(flavor = "current_thread")]
async fn init_ts_module_can_import_repository_dired_plugin() {
    let current_dir = unique_path("cwd");
    std::fs::create_dir_all(&current_dir).expect("current dir");
    let config_path = current_dir.join("init.ts");
    let plugin_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/saya-dired.ts");
    std::fs::write(
        &config_path,
        format!(
            r#"
                import {{ setupSayaDired }} from "{}";
                setupSayaDired();
            "#,
            plugin_path.display()
        ),
    )
    .expect("config file");

    let prepared = prepare_init_module(&config_path, &current_dir);
    let StartupModulePrepareResult::Success(module) = prepared else {
        panic!(
            "repository dired plugin should prepare, got: {:?}",
            prepared
        );
    };
    let registry = collect_startup_registry(&module.executable_source_text)
        .await
        .expect("repository dired plugin should evaluate");

    for expected_command in [
        "dired.open",
        "dired.enter",
        "dired.up",
        "dired.refresh",
        "dired.mark",
        "dired.unmark",
        "dired.clearMarks",
        "dired.bulkDeletePreview",
    ] {
        assert!(
            registry.entries().iter().any(|entry| {
                matches!(
                    entry,
                    StartupRegistryEntry::Command { name, .. } if name == expected_command
                )
            }),
            "missing command {expected_command}"
        );
    }
    for (expected_lhs, expected_command) in [
        ("-", "dired.up"),
        ("<Enter>", "dired.enter"),
        ("gr", "dired.refresh"),
        ("m", "dired.mark"),
        ("M", "dired.unmark"),
        ("gM", "dired.clearMarks"),
        ("D", "dired.bulkDeletePreview"),
    ] {
        assert!(
            registry.entries().iter().any(|entry| {
                matches!(
                    entry,
                    StartupRegistryEntry::Keymap {
                        mode: SayaKeyMode::Normal,
                        lhs,
                        action: SayaKeymapAction::RegisteredCommand(command),
                    } if lhs == expected_lhs && command == expected_command
                )
            }),
            "missing keymap {expected_lhs} -> {expected_command}"
        );
    }

    let seed = CallbackRegistrySeed::from_startup_registry(&registry);
    SayaLiveRuntime::spawn_from_seed(Arc::new(NoopHostBridge), seed)
        .expect("repository dired command callbacks should initialize in live runtime");
}

#[tokio::test(flavor = "current_thread")]
async fn init_ts_module_can_import_repository_lsp_client_plugin() {
    let current_dir = unique_path("cwd");
    std::fs::create_dir_all(&current_dir).expect("current dir");
    let config_path = current_dir.join("init.ts");
    let plugin_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/saya-lsp-client.ts");
    std::fs::write(
        &config_path,
        format!(
            r#"
                import {{ createLspMessageParser, encodeLspMessage, lspPositionFromSayaCursor, lspRangeFromSayaRange, parseLsifLine, setupSayaLspClient }} from "{}";
                const framed = encodeLspMessage({{ jsonrpc: "2.0", method: "initialized" }});
                if (!framed.startsWith("Content-Length:")) {{
                    throw new Error("LSP message framing must include Content-Length");
                }}
                const parsedMessages = [];
                const parser = createLspMessageParser((message) => parsedMessages.push(message));
                parser.accept(framed);
                if (parsedMessages.length !== 1 || parsedMessages[0].method !== "initialized") {{
                    throw new Error("LSP parser must recover JSON-RPC messages from framing");
                }}
                const lsifEntry = parseLsifLine('{{"id":1,"type":"vertex","label":"metaData"}}');
                if (lsifEntry.label !== "metaData") {{
                    throw new Error("LSIF parser must preserve vertex labels");
                }}
                const sampleLine = "aé😀e\u0301z";
                const byteCursor = 10;
                if (lspPositionFromSayaCursor(sampleLine, 2, byteCursor, "utf-16").character !== 6) {{
                    throw new Error("LSP UTF-16 position must count UTF-16 code units");
                }}
                if (lspPositionFromSayaCursor(sampleLine, 2, byteCursor, "utf-8").character !== 10) {{
                    throw new Error("LSP UTF-8 position must preserve Saya byte columns");
                }}
                if (lspPositionFromSayaCursor(sampleLine, 2, byteCursor, "utf-32").character !== 5) {{
                    throw new Error("LSP UTF-32 position must count Unicode scalar values");
                }}
                const clamped = lspPositionFromSayaCursor(sampleLine, 2, 999, "utf-8");
                if (clamped.character !== 11) {{
                    throw new Error("LSP position helper must clamp to line end");
                }}
                const range = lspRangeFromSayaRange("abc\né😀z\n", {{
                    start: {{ line: 1, character: 0 }},
                    end: {{ line: 1, character: 6 }},
                }}, "utf-16");
                if (range.start.character !== 0 || range.end.character !== 3) {{
                    throw new Error("LSP range helper must convert editor byte columns");
                }}
                setupSayaLspClient({{
                    bridgeCommand: "host.lsp",
                    clientName: "saya-test",
                    rootUri: "file:///workspace",
                    languageId: "rust",
                    trace: "messages",
                    positionEncoding: "utf-16",
                    enableBufferEvents: true,
                    lsif: {{
                        enabled: true,
                        bridgeCommand: "host.lsif",
                        dumpPath: ".cache/index.lsif",
                    }},
                    commands: {{
                        hover: "code.hover",
                        definition: "code.definition",
                        references: "code.references",
                        documentSymbol: "code.symbols",
                        lsifHover: "index.hover",
                        lsifDefinition: "index.definition",
                    }},
                    keymap: {{
                        hover: "H",
                        definition: "D",
                        references: "R",
                        documentSymbol: "S",
                        lsifHover: "IH",
                        lsifDefinition: "ID",
                    }},
                }});
            "#,
            plugin_path.display()
        ),
    )
    .expect("config file");

    let prepared = prepare_init_module(&config_path, &current_dir);
    let StartupModulePrepareResult::Success(module) = prepared else {
        panic!(
            "repository lsp client plugin should prepare, got: {:?}",
            prepared
        );
    };
    assert!(
        module
            .executable_source_text
            .contains("function setupSayaLspClient"),
        "plugin function should be inlined into the executable source"
    );
    assert!(
        !module
            .executable_source_text
            .contains("interface SayaLspClientOptions"),
        "type-only lsp declarations must be stripped"
    );
    let registry = collect_startup_registry(&module.executable_source_text)
        .await
        .expect("repository lsp client plugin should evaluate");

    for expected_command in [
        "lsp.initialize",
        "lsp.initialized",
        "code.hover",
        "code.definition",
        "code.references",
        "code.symbols",
        "lsp.shutdown",
        "index.hover",
        "index.definition",
    ] {
        assert!(
            registry.entries().iter().any(|entry| {
                matches!(
                    entry,
                    StartupRegistryEntry::Command { name, .. } if name == expected_command
                )
            }),
            "missing command {expected_command}"
        );
    }

    for (expected_lhs, expected_command) in [
        ("H", "code.hover"),
        ("D", "code.definition"),
        ("R", "code.references"),
        ("S", "code.symbols"),
        ("IH", "index.hover"),
        ("ID", "index.definition"),
    ] {
        assert!(
            registry.entries().iter().any(|entry| {
                matches!(
                    entry,
                    StartupRegistryEntry::Keymap {
                        mode: SayaKeyMode::Normal,
                        lhs,
                        action: SayaKeymapAction::RegisteredCommand(command),
                    } if lhs == expected_lhs && command == expected_command
                )
            }),
            "missing lsp keymap {expected_lhs} -> {expected_command}"
        );
    }

    for expected_event in ["bufferOpen", "bufferWritePost"] {
        assert!(
            registry.entries().iter().any(|entry| {
                matches!(
                    entry,
                    StartupRegistryEntry::Event { name, .. } if name == expected_event
                )
            }),
            "missing lsp lifecycle event {expected_event}"
        );
    }

    let hover_callback = registry
        .entries()
        .iter()
        .find_map(|entry| match entry {
            StartupRegistryEntry::Command {
                name,
                callback_source,
            } if name == "code.hover" => Some(callback_source),
            _ => None,
        })
        .expect("hover callback should be registered");
    assert!(hover_callback.contains(r#"const lspVersion = "3.17";"#));
    assert!(hover_callback.contains(r#"const method = "textDocument/hover";"#));
    assert!(hover_callback.contains("host.lsp"));
    assert!(hover_callback.contains("[saya-lsp] dispatch"));

    let lsif_hover_callback = registry
        .entries()
        .iter()
        .find_map(|entry| match entry {
            StartupRegistryEntry::Command {
                name,
                callback_source,
            } if name == "index.hover" => Some(callback_source),
            _ => None,
        })
        .expect("lsif hover callback should be registered");
    assert!(lsif_hover_callback.contains(r#"const lspVersion = "0.6.0";"#));
    assert!(lsif_hover_callback.contains(r#"const source = "lsif";"#));
    assert!(lsif_hover_callback.contains(".cache/index.lsif"));
}

#[tokio::test(flavor = "current_thread")]
async fn repository_lsp_client_plugin_bridge_fake_server_covers_protocol_flow_for_ci() {
    let workspace = unique_path("fake-lsp-workspace");
    std::fs::create_dir_all(&workspace).expect("workspace");
    std::fs::write(
        workspace.join("go.mod"),
        "module example.com/saya_lsp_fake\n\ngo 1.22\n",
    )
    .expect("go.mod");
    let document_text = r#"package main

func main() {
	println("saya")
}
"#
    .to_string();
    let document_path = workspace.join("main.go");
    std::fs::write(&document_path, &document_text).expect("main.go");

    let config_path = workspace.join("init.ts");
    let plugin_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/saya-lsp-client.ts");
    std::fs::write(
        &config_path,
        format!(
            r#"
                import {{ setupSayaLspClient }} from "{}";
                setupSayaLspClient({{
                    bridgeCommand: "host.lsp",
                    clientName: "saya-fake-ci",
                    rootUri: "{}",
                    languageIdByExtension: {{ go: "go" }},
                    trace: "messages",
                    positionEncoding: "utf-16",
                    servers: {{
                        go: {{
                            name: "fake-lsp-ci",
                            command: "fake-lsp-ci",
                            languages: ["go"],
                            rootMarkers: ["go.mod"],
                        }},
                    }},
                    commands: {{
                        hover: "code.hover",
                        documentSymbol: "code.symbols",
                    }},
                }});
            "#,
            plugin_path.display(),
            file_uri(&workspace)
        ),
    )
    .expect("config file");

    let prepared = prepare_init_module(&config_path, &workspace);
    let StartupModulePrepareResult::Success(module) = prepared else {
        panic!(
            "repository lsp client plugin should prepare for fake protocol smoke, got: {:?}",
            prepared
        );
    };
    let registry = collect_startup_registry(&module.executable_source_text)
        .await
        .expect("repository lsp client plugin should evaluate for fake protocol smoke");
    let seed = CallbackRegistrySeed::from_startup_registry(&registry);
    let host_bridge = Arc::new(FakeLspProtocolHostBridge::new(
        &document_path,
        document_text.clone(),
    ));
    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), seed)
        .expect("repository lsp client callbacks should initialize in live runtime");

    runtime
        .execute_command("lsp.initialize")
        .expect("queue initialize")
        .await_result()
        .await
        .expect("fake initialize should complete through typed bridge");
    runtime
        .execute_command("lsp.initialized")
        .expect("queue initialized")
        .await_result()
        .await
        .expect("fake initialized notification should complete through typed bridge");
    runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: host_bridge.buffer.clone(),
        }))
        .expect("queue didOpen")
        .await_result()
        .await
        .expect("fake didOpen should complete through typed bridge");
    runtime
        .execute_command("code.hover")
        .expect("queue hover")
        .await_result()
        .await
        .expect("fake hover should complete through typed bridge");
    runtime
        .execute_command("code.symbols")
        .expect("queue documentSymbol")
        .await_result()
        .await
        .expect("fake documentSymbol should complete through typed bridge");
    runtime
        .dispatch_event(RuntimeEventPayload::BufferChanged(BufferEventPayload {
            buffer: host_bridge.buffer.clone(),
        }))
        .expect("queue didChange")
        .await_result()
        .await
        .expect("fake didChange should complete through typed bridge");
    runtime
        .dispatch_event(RuntimeEventPayload::BufferWritePost(BufferEventPayload {
            buffer: host_bridge.buffer.clone(),
        }))
        .expect("queue didSave")
        .await_result()
        .await
        .expect("fake didSave should complete through typed bridge");
    runtime
        .dispatch_event(RuntimeEventPayload::BufferClosed(BufferEventPayload {
            buffer: host_bridge.buffer.clone(),
        }))
        .expect("queue didClose")
        .await_result()
        .await
        .expect("fake didClose should complete through typed bridge");
    runtime
        .execute_command("lsp.shutdown")
        .expect("queue shutdown")
        .await_result()
        .await
        .expect("fake shutdown should complete through typed bridge");

    let observed_methods = host_bridge.observed_methods();
    eprintln!("[saya-lsp-fake-protocol-smoke] observed methods: {observed_methods:?}");
    for expected in [
        "initialize",
        "initialized",
        "textDocument/didOpen",
        "textDocument/hover",
        "textDocument/documentSymbol",
        "textDocument/didChange",
        "textDocument/didSave",
        "textDocument/didClose",
        "shutdown",
    ] {
        assert!(
            observed_methods.iter().any(|method| method == expected),
            "fake protocol smoke should observe {expected}; observed={observed_methods:?}"
        );
    }

    let requests = host_bridge.observed_requests();
    let initialize = requests
        .iter()
        .find(|request| request.method == "initialize")
        .expect("initialize request should be recorded");
    assert_eq!(initialize.client_name, "fake-lsp-ci");
    assert_eq!(
        initialize.root_uri.as_deref(),
        Some(file_uri(&workspace).as_str())
    );
    assert_eq!(initialize.language_id, "go");
    assert_eq!(initialize.trace, "messages");
    assert_eq!(initialize.position_encoding, "utf-16");
    assert_eq!(
        initialize
            .server
            .as_ref()
            .map(|server| server.command.as_str()),
        Some("fake-lsp-ci")
    );
    assert_eq!(
        initialize
            .params
            .as_ref()
            .and_then(|params| params.pointer("/capabilities/textDocument/hover"))
            .and_then(Value::as_object)
            .map(|hover| hover.is_empty()),
        Some(true)
    );

    let did_change = requests
        .iter()
        .find(|request| request.method == "textDocument/didChange")
        .expect("didChange request should be recorded");
    assert_eq!(
        did_change
            .params
            .as_ref()
            .and_then(|params| params.pointer("/contentChanges/0/text"))
            .and_then(Value::as_str),
        Some(document_text.as_str())
    );
    assert_eq!(
        did_change
            .params
            .as_ref()
            .and_then(|params| params.pointer("/textDocument/version"))
            .and_then(Value::as_u64),
        Some(2)
    );

    let host_commands = host_bridge.observed_host_commands();
    assert!(
        host_commands
            .iter()
            .any(|command| command.starts_with("lsp.floatHover ")),
        "hover response should route to the hover UI command: {host_commands:?}"
    );
    assert!(
        host_commands
            .iter()
            .any(|command| command.starts_with("lsp.floatSymbols ")),
        "documentSymbol response should route to the symbols UI command: {host_commands:?}"
    );
    assert!(
        host_commands
            .iter()
            .all(|command| !command.starts_with("lsp.status ")),
        "fake protocol path should not report language server readiness failures: {host_commands:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires SAYA_RUN_GOPLS_SMOKE=1 plus local go and gopls; use the documented opt-in command"]
async fn repository_lsp_client_plugin_bridge_smoke_reaches_gopls_with_logs() {
    if std::env::var_os("SAYA_RUN_GOPLS_SMOKE").is_none() {
        eprintln!("[saya-lsp-gopls-smoke] skipped because SAYA_RUN_GOPLS_SMOKE=1 is not set");
        return;
    }
    let gopls_command = resolve_gopls_command();
    let gopls_version = Command::new(&gopls_command)
        .arg("version")
        .output()
        .expect("gopls should be installed for LSP smoke verification");
    assert!(
        gopls_version.status.success(),
        "gopls version should succeed: status={:?}, stderr={}",
        gopls_version.status,
        String::from_utf8_lossy(&gopls_version.stderr)
    );
    eprintln!(
        "[saya-lsp-gopls-smoke] detected {}",
        String::from_utf8_lossy(&gopls_version.stdout).trim()
    );
    let go_version = Command::new("go")
        .arg("version")
        .output()
        .expect("go should be installed for gopls smoke verification");
    assert!(
        go_version.status.success(),
        "go version should succeed: status={:?}, stderr={}",
        go_version.status,
        String::from_utf8_lossy(&go_version.stderr)
    );
    let go_version_stdout = String::from_utf8_lossy(&go_version.stdout);
    let go_mod_version = go_mod_version_from_go_version_output(&go_version_stdout)
        .expect("go version output should include major.minor");
    eprintln!(
        "[saya-lsp-gopls-smoke] detected {}; using go.mod go {}",
        go_version_stdout.trim(),
        go_mod_version
    );

    let workspace = unique_path("gopls-workspace");
    std::fs::create_dir_all(&workspace).expect("workspace");
    std::fs::write(
        workspace.join("go.mod"),
        format!("module example.com/saya_lsp_smoke\n\ngo {go_mod_version}\n"),
    )
    .expect("go.mod");
    let document_text = r#"package main

func main() {
	println("saya")
}
"#
    .to_string();
    let document_path = workspace.join("main.go");
    std::fs::write(&document_path, &document_text).expect("main.go");

    let config_path = workspace.join("init.ts");
    let plugin_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/saya-lsp-client.ts");
    std::fs::write(
        &config_path,
        format!(
            r#"
                import {{ setupSayaLspClient }} from "{}";
                setupSayaLspClient({{
                    bridgeCommand: "host.lsp",
                    clientName: "saya-gopls-smoke",
                    rootUri: "{}",
                    languageId: "go",
                    trace: "messages",
                    commands: {{
                        hover: "code.hover",
                        documentSymbol: "code.symbols",
                    }},
                }});
            "#,
            plugin_path.display(),
            file_uri(&workspace)
        ),
    )
    .expect("config file");

    let prepared = prepare_init_module(&config_path, &workspace);
    let StartupModulePrepareResult::Success(module) = prepared else {
        panic!(
            "repository lsp client plugin should prepare for gopls smoke, got: {:?}",
            prepared
        );
    };
    let registry = collect_startup_registry(&module.executable_source_text)
        .await
        .expect("repository lsp client plugin should evaluate for gopls smoke");
    let seed = CallbackRegistrySeed::from_startup_registry(&registry);
    let host_bridge = Arc::new(GoplsSmokeHostBridge::new(
        &gopls_command,
        &workspace,
        &document_path,
        document_text,
    ));
    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), seed)
        .expect("repository lsp client callbacks should initialize in live runtime");

    runtime
        .execute_command("lsp.initialize")
        .expect("queue initialize")
        .await_result()
        .await
        .expect("gopls initialize should complete through bridge");
    runtime
        .execute_command("lsp.initialized")
        .expect("queue initialized")
        .await_result()
        .await
        .expect("gopls initialized notification should complete through bridge");
    runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: host_bridge.buffer.clone(),
        }))
        .expect("queue didOpen")
        .await_result()
        .await
        .expect("gopls didOpen should complete through bridge");
    runtime
        .execute_command("code.hover")
        .expect("queue hover")
        .await_result()
        .await
        .expect("gopls hover should complete through bridge");
    runtime
        .execute_command("code.symbols")
        .expect("queue documentSymbol")
        .await_result()
        .await
        .expect("gopls documentSymbol should complete through bridge");
    runtime
        .dispatch_event(RuntimeEventPayload::BufferChanged(BufferEventPayload {
            buffer: host_bridge.buffer.clone(),
        }))
        .expect("queue didChange")
        .await_result()
        .await
        .expect("gopls didChange should complete through bridge");
    runtime
        .dispatch_event(RuntimeEventPayload::BufferWritePost(BufferEventPayload {
            buffer: host_bridge.buffer.clone(),
        }))
        .expect("queue didSave")
        .await_result()
        .await
        .expect("gopls didSave should complete through bridge");
    runtime
        .dispatch_event(RuntimeEventPayload::BufferClosed(BufferEventPayload {
            buffer: host_bridge.buffer.clone(),
        }))
        .expect("queue didClose")
        .await_result()
        .await
        .expect("gopls didClose should complete through bridge");
    runtime
        .execute_command("lsp.shutdown")
        .expect("queue shutdown")
        .await_result()
        .await
        .expect("gopls shutdown should complete through bridge");

    let observed_methods = host_bridge.observed_methods();
    eprintln!("[saya-lsp-gopls-smoke] completion observed methods: {observed_methods:?}");
    for expected in [
        "initialize",
        "initialized",
        "textDocument/didOpen",
        "textDocument/hover",
        "textDocument/documentSymbol",
        "textDocument/didChange",
        "textDocument/didSave",
        "textDocument/didClose",
        "shutdown",
        "exit",
    ] {
        assert!(
            observed_methods.iter().any(|method| method == expected),
            "gopls smoke should observe {expected}; observed={observed_methods:?}"
        );
    }
}

#[test]
fn repository_lsp_client_plugin_public_helpers_cover_lsp_and_lsif_protocol_shape() {
    let plugin_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/saya-lsp-client.ts");
    let source = std::fs::read_to_string(plugin_path).expect("repository lsp client plugin");

    for expected in [
        "setupSayaLspClient",
        "encodeLspMessage",
        "createLspMessageParser",
        "createLspJsonRpcClient",
        "parseLsifLine",
        "Content-Length",
        "jsonrpc: \"2.0\"",
        "initialize",
        "initialized",
        "shutdown",
        "textDocument/didOpen",
        "textDocument/didChange",
        "textDocument/didSave",
        "textDocument/didClose",
        "textDocument/hover",
        "textDocument/definition",
        "textDocument/references",
        "textDocument/documentSymbol",
        "servers",
        "rootMarkers",
        "initializationOptions",
        "lspPositionFromSayaCursor",
        "lspRangeFromSayaRange",
        "const lspVersion = ",
        "positionEncoding",
        "utf-16",
        "utf-8",
        "utf-32",
        "vertex",
        "edge",
    ] {
        assert!(
            source.contains(expected),
            "lsp client plugin should expose protocol surface item: {expected}"
        );
    }
}

#[test]
fn repository_dired_plugin_public_options_cover_phase6_surface() {
    let plugin_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/saya-dired.ts");
    let source = std::fs::read_to_string(plugin_path).expect("repository dired plugin");

    for expected in [
        "commandName?:",
        "keymap?:",
        "root?:",
        "hiddenFilePolicy?:",
        "sortPolicy?:",
        "filter?:",
        "confirmStrategy?:",
        "SayaDiredCommandNames",
        "SayaDiredKeymap",
        "SayaDiredHiddenFilePolicy",
        "SayaDiredSortPolicy",
        "SayaDiredConfirmStrategy",
    ] {
        assert!(
            source.contains(expected),
            "dired public options should include phase 6 surface item: {expected}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn repository_dired_plugin_phase6_options_affect_registered_surface() {
    let current_dir = unique_path("cwd");
    std::fs::create_dir_all(&current_dir).expect("current dir");
    let config_path = current_dir.join("init.ts");
    let plugin_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/saya-dired.ts");
    std::fs::write(
        &config_path,
        format!(
            r#"
                import {{ setupSayaDired }} from "{}";
                setupSayaDired({{
                    hiddenFilePolicy: "hide",
                    sortPolicy: "size",
                    filter: "rs",
                    confirmStrategy: "disabled",
                    commands: {{
                        refresh: "workspace.refresh",
                        bulkDeletePreview: "workspace.previewDelete",
                    }},
                    keymap: {{
                        refresh: "R",
                        bulkDeletePreview: "X",
                    }},
                }});
            "#,
            plugin_path.display()
        ),
    )
    .expect("config file");

    let prepared = prepare_init_module(&config_path, &current_dir);
    let StartupModulePrepareResult::Success(module) = prepared else {
        panic!(
            "repository dired plugin with custom options should prepare, got: {:?}",
            prepared
        );
    };
    let registry = collect_startup_registry(&module.executable_source_text)
        .await
        .expect("repository dired plugin with custom options should evaluate");

    let refresh_callback = registry
        .entries()
        .iter()
        .find_map(|entry| match entry {
            StartupRegistryEntry::Command {
                name,
                callback_source,
            } if name == "workspace.refresh" => Some(callback_source),
            _ => None,
        })
        .expect("custom refresh command should be registered");
    assert!(refresh_callback.contains("const showHidden = false;"));
    assert!(refresh_callback.contains(r#"const sortBy = "size";"#));
    assert!(refresh_callback.contains(r#"const filter = "rs";"#));

    let preview_callback = registry
        .entries()
        .iter()
        .find_map(|entry| match entry {
            StartupRegistryEntry::Command {
                name,
                callback_source,
            } if name == "workspace.previewDelete" => Some(callback_source),
            _ => None,
        })
        .expect("custom bulk delete preview command should be registered");
    assert!(preview_callback.contains(r#"const confirmStrategy = "disabled";"#));

    for (expected_lhs, expected_command) in
        [("R", "workspace.refresh"), ("X", "workspace.previewDelete")]
    {
        assert!(
            registry.entries().iter().any(|entry| {
                matches!(
                    entry,
                    StartupRegistryEntry::Keymap {
                        mode: SayaKeyMode::Normal,
                        lhs,
                        action: SayaKeymapAction::RegisteredCommand(command),
                    } if lhs == expected_lhs && command == expected_command
                )
            }),
            "missing custom keymap {expected_lhs} -> {expected_command}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn startup_command_callback_source_preserves_line_comment_boundaries() {
    let registry = collect_startup_registry(
        r#"
            saya.commands.register("commented", async () => {
              // This comment must not swallow the executable line below.
              await saya.commands.execute("write");
            });
        "#,
    )
    .await
    .expect("startup module should evaluate");

    let callback_source = registry
        .entries()
        .iter()
        .find_map(|entry| match entry {
            StartupRegistryEntry::Command {
                name,
                callback_source,
            } if name == "commented" => Some(callback_source.as_str()),
            _ => None,
        })
        .expect("command callback should be collected");

    assert!(
        callback_source.contains("// This comment must not swallow"),
        "line comment should be preserved in source: {callback_source}"
    );
    assert!(
        callback_source.contains('\n'),
        "callback source must preserve line boundaries: {callback_source}"
    );
    assert!(
        callback_source.contains("await saya.commands.execute(\"write\")"),
        "executable line after comment should remain visible: {callback_source}"
    );
}

#[test]
fn init_ts_module_transpile_keeps_theme_object_literals_executable() {
    let current_dir = unique_path("cwd");
    std::fs::create_dir_all(&current_dir).expect("current dir");
    let config_path = current_dir.join("init.ts");
    std::fs::write(
        &config_path,
        r##"
            const accent: string = "#7aa2f7";
            saya.theme.palette = { accent };
            saya.theme.markdown = {
                heading: { fg: "accent", bold: true },
                heading2: { fg: "#9ece6a", underline: true },
            };
        "##,
    )
    .expect("config file");

    let result = prepare_init_module(&config_path, &current_dir);

    match result {
        StartupModulePrepareResult::Success(module) => {
            assert!(
                module
                    .executable_source_text
                    .contains("const accent = \"#7aa2f7\";")
            );
            assert!(
                module
                    .executable_source_text
                    .contains("heading: { fg: \"accent\", bold: true }"),
                "object literal values must not be stripped as type annotations"
            );
        }
        other => panic!("Success を返すこと, got: {:?}", other),
    }
}

#[test]
fn init_ts_module_transpile_preserves_multiline_ternary_expressions() {
    let current_dir = unique_path("cwd");
    std::fs::create_dir_all(&current_dir).expect("current dir");
    let config_path = current_dir.join("init.ts");
    std::fs::write(
        &config_path,
        r#"
            const currentPath: string = "/tmp/notes.txt";
            const directory = currentPath.endsWith("/")
                ? currentPath.slice(0, -1)
                : currentPath.replace(/\/[^/]*$/, "") || ".";
            saya.keymap.set("normal", "-", saya.commands.execute("dired.open"));
        "#,
    )
    .expect("config file");

    let result = prepare_init_module(&config_path, &current_dir);

    match result {
        StartupModulePrepareResult::Success(module) => {
            assert!(
                module
                    .executable_source_text
                    .contains("? currentPath.slice(0, -1)\n                : currentPath.replace"),
                "ternary separator must not be stripped as a type annotation: {}",
                module.executable_source_text
            );
            assert!(
                module
                    .executable_source_text
                    .contains("const currentPath = \"/tmp/notes.txt\";"),
                "real type annotations should still be stripped"
            );
        }
        other => panic!("Success を返すこと, got: {:?}", other),
    }
}

#[test]
fn init_ts_module_transpile_failure_is_reported_structurally() {
    let current_dir = unique_path("cwd");
    std::fs::create_dir_all(&current_dir).expect("current dir");
    let config_path = current_dir.join("init.ts");
    std::fs::write(
        &config_path,
        r#"
            const broken: number = ;
        "#,
    )
    .expect("config file");

    let result = prepare_init_module(&config_path, &current_dir);

    assert!(matches!(
        result,
        StartupModulePrepareResult::TranspileFailed { ref path, .. } if path == &config_path
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn startup_saya_namespace_is_available_to_top_level_module_code() {
    evaluate_startup_module(
        r#"
            if (typeof saya === "undefined") {
                throw new Error("saya namespace is missing");
            }
            saya.options.tabSize = 4;
        "#,
    )
    .await
    .expect("startup module should evaluate with saya namespace");
}

#[tokio::test(flavor = "current_thread")]
async fn startup_saya_namespace_exposes_command_reference_helper_without_runtime_capabilities() {
    let result = evaluate_startup_module(
        r#"
            if (typeof saya === "undefined") {
                throw new Error("saya namespace is missing");
            }
            if (typeof saya.commands.execute !== "function") {
                throw new Error("startup command reference helper is missing");
            }
            const commandRef = saya.commands.execute("writeCurrent");
            if (commandRef !== "__SAYA_STARTUP_COMMAND_REF__:writeCurrent") {
                throw new Error(`unexpected command reference: ${commandRef}`);
            }
        "#,
    )
    .await;

    assert!(
        result.is_ok(),
        "startup namespace should expose only command reference helper semantics"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_surface_is_frozen_and_does_not_expose_runtime_api() {
    evaluate_startup_module(
        r#"
            if (!Object.isFrozen(saya)) {
                throw new Error("startup saya surface should be frozen");
            }
            if (!Object.isFrozen(saya.options)) {
                throw new Error("startup options surface should be frozen");
            }
            if (!Object.isFrozen(saya.keymap)) {
                throw new Error("startup keymap surface should be frozen");
            }
            if (!Object.isFrozen(saya.commands)) {
                throw new Error("startup command surface should be frozen");
            }
            if (!Object.isFrozen(saya.events)) {
                throw new Error("startup event surface should be frozen");
            }
            if (!Object.isFrozen(saya.theme)) {
                throw new Error("startup theme surface should be frozen");
            }
            if (typeof saya.buffer !== "undefined") {
                throw new Error("runtime buffer api leaked into startup namespace");
            }
            if (typeof saya.window !== "undefined") {
                throw new Error("runtime window api leaked into startup namespace");
            }
            if (typeof saya.editor !== "undefined") {
                throw new Error("runtime editor api leaked into startup namespace");
            }
            if (typeof saya.commands.execute !== "function") {
                throw new Error("startup command reference helper is missing");
            }
        "#,
    )
    .await
    .expect("startup surface should stay separated from runtime surface");
}

#[tokio::test(flavor = "current_thread")]
async fn startup_theme_palette_and_markdown_styles_are_collected() {
    let registry = collect_startup_registry(
        r##"
            saya.theme.palette = {
                accent: "#7aa2f7",
                heading2: "#9ece6a",
                code: "#ff9e64",
                link: "#2ac3de",
            };
            saya.theme.markdown = {
                heading: { fg: "accent", bold: true },
                heading2: { fg: "heading2", underline: true },
                inlineCode: { fg: "code" },
                link: { fg: "link", underline: true },
            };
        "##,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[
            StartupRegistryEntry::ThemePalette {
                name: "accent".to_string(),
                value: "#7aa2f7".to_string(),
            },
            StartupRegistryEntry::ThemePalette {
                name: "heading2".to_string(),
                value: "#9ece6a".to_string(),
            },
            StartupRegistryEntry::ThemePalette {
                name: "code".to_string(),
                value: "#ff9e64".to_string(),
            },
            StartupRegistryEntry::ThemePalette {
                name: "link".to_string(),
                value: "#2ac3de".to_string(),
            },
            StartupRegistryEntry::ThemeMarkdownStyle {
                key: MarkdownSemanticStyleKey::Heading,
                style: ThemeTextStyleDeclaration {
                    fg: Some("accent".to_string()),
                    bold: Some(true),
                    ..ThemeTextStyleDeclaration::default()
                },
            },
            StartupRegistryEntry::ThemeMarkdownStyle {
                key: MarkdownSemanticStyleKey::Heading2,
                style: ThemeTextStyleDeclaration {
                    fg: Some("heading2".to_string()),
                    underline: Some(true),
                    ..ThemeTextStyleDeclaration::default()
                },
            },
            StartupRegistryEntry::ThemeMarkdownStyle {
                key: MarkdownSemanticStyleKey::InlineCode,
                style: ThemeTextStyleDeclaration {
                    fg: Some("code".to_string()),
                    ..ThemeTextStyleDeclaration::default()
                },
            },
            StartupRegistryEntry::ThemeMarkdownStyle {
                key: MarkdownSemanticStyleKey::Link,
                style: ThemeTextStyleDeclaration {
                    fg: Some("link".to_string()),
                    underline: Some(true),
                    ..ThemeTextStyleDeclaration::default()
                },
            },
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_theme_ui_and_syntax_styles_are_collected() {
    let registry = collect_startup_registry(
        r##"
            saya.theme.palette = {
                fg: "#c0caf5",
                bg: "#24283b",
                comment: "#565f89",
                keyword: "#bb9af7",
            };
            saya.theme.ui = {
                text: { fg: "fg", bg: "bg" },
                statusActive: { fg: "bg", bg: "fg", bold: true },
            };
            saya.theme.syntax = {
                comment: { fg: "comment", italic: true },
                statement: { fg: "keyword", bold: true },
            };
        "##,
    )
    .await
    .expect("startup theme ui and syntax config should evaluate");

    assert!(registry.entries().iter().any(|entry| {
        matches!(
            entry,
            StartupRegistryEntry::ThemeUiStyle {
                key: UiStyleKey::Text,
                style,
            } if style.fg.as_deref() == Some("fg") && style.bg.as_deref() == Some("bg")
        )
    }));
    assert!(registry.entries().iter().any(|entry| {
        matches!(
            entry,
            StartupRegistryEntry::ThemeSyntaxStyle {
                key: SyntaxSemanticStyleKey::Statement,
                style,
            } if style.fg.as_deref() == Some("keyword") && style.bold == Some(true)
        )
    }));
}

#[tokio::test(flavor = "current_thread")]
async fn startup_tab_size_is_collected_in_source_order_and_is_deterministic() {
    let source = r#"
        saya.options.tabSize = 4;
        saya.options.tabSize = 6;
    "#;

    let first = collect_startup_registry(source)
        .await
        .expect("startup registry");
    let second = collect_startup_registry(source)
        .await
        .expect("startup registry");

    assert_eq!(first, second, "same source should yield the same registry");
    assert_eq!(
        first.entries(),
        &[
            StartupRegistryEntry::Option {
                name: StartupOptionName::TabSize,
                value: StartupOptionValue::Number(4),
            },
            StartupRegistryEntry::Option {
                name: StartupOptionName::TabSize,
                value: StartupOptionValue::Number(6),
            },
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_line_numbers_is_collected() {
    let registry = collect_startup_registry(
        r#"
            saya.options.lineNumbers = true;
        "#,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[StartupRegistryEntry::Option {
            name: StartupOptionName::LineNumbers,
            value: StartupOptionValue::Boolean(true),
        }]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_number_width_is_collected() {
    let registry = collect_startup_registry(
        r#"
            saya.options.numberWidth = 6;
        "#,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[StartupRegistryEntry::Option {
            name: StartupOptionName::NumberWidth,
            value: StartupOptionValue::Number(6),
        }]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_syntax_is_collected() {
    let registry = collect_startup_registry(
        r#"
            saya.options.syntax = true;
        "#,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[StartupRegistryEntry::Option {
            name: StartupOptionName::Syntax,
            value: StartupOptionValue::Boolean(true),
        }]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_option_aliases_are_normalized_to_formal_names() {
    let registry = collect_startup_registry(
        r#"
            saya.options.tabstop = 2;
            saya.options.number = true;
            saya.options.nuw = 5;
        "#,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[
            StartupRegistryEntry::Option {
                name: StartupOptionName::TabSize,
                value: StartupOptionValue::Number(2),
            },
            StartupRegistryEntry::Option {
                name: StartupOptionName::LineNumbers,
                value: StartupOptionValue::Boolean(true),
            },
            StartupRegistryEntry::Option {
                name: StartupOptionName::NumberWidth,
                value: StartupOptionValue::Number(5),
            },
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_keymap_is_collected() {
    let registry = collect_startup_registry(
        r#"
            saya.keymap.set("normal", "x", "dd");
        "#,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[StartupRegistryEntry::Keymap {
            mode: SayaKeyMode::Normal,
            lhs: "x".to_string(),
            action: SayaKeymapAction::Literal("dd".to_string()),
        }]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_keymap_registered_command_reference_is_collected() {
    let registry = collect_startup_registry(
        r#"
            saya.keymap.set("normal", "<leader>w", saya.commands.execute("writeCurrent"));
        "#,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[StartupRegistryEntry::Keymap {
            mode: SayaKeyMode::Normal,
            lhs: "<leader>w".to_string(),
            action: SayaKeymapAction::RegisteredCommand("writeCurrent".to_string()),
        }]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_log_file_is_collected() {
    let registry = collect_startup_registry(
        r#"
            saya.log.file = "/tmp/saya-from-init.log";
        "#,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[StartupRegistryEntry::LogFile {
            path: "/tmp/saya-from-init.log".to_string(),
        }]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_command_registration_is_collected() {
    let registry = collect_startup_registry(
        r#"
            saya.commands.register("writeCurrent", () => {
                console.log("write");
            });
        "#,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[StartupRegistryEntry::Command {
            name: "writeCurrent".to_string(),
            callback_source: "() => {\n                console.log(\"write\");\n            }"
                .to_string(),
        }]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_event_subscription_is_collected() {
    let registry = collect_startup_registry(
        r#"
            saya.events.on("bufferOpen", (payload) => {
                console.log(payload);
            });
        "#,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[StartupRegistryEntry::Event {
            name: "bufferOpen".to_string(),
            callback_source: "(payload) => {\n                console.log(payload);\n            }"
                .to_string(),
        }]
    );
}
