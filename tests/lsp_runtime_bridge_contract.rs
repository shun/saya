use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};

use saya::callback_registry_seed::CallbackRegistrySeed;
use saya::lsp_runtime_bridge::{
    LSP_RUNTIME_BRIDGE_DECISION, LspRuntimeBridgeRequest, LspRuntimeBridgeResponse,
    LspRuntimeBridgeSource,
};
use saya::saya_live_runtime::{
    BoxFuture, BufferEventPayload, HostCapabilityBridge, ReadonlyBufferSnapshot,
    ReadonlyEditorSnapshot, ReadonlyWindowSnapshot, RuntimeCommandError, RuntimeEventPayload,
    RuntimeMode, SayaLiveRuntime,
};
use saya::startup_runtime::{
    StartupModulePrepareResult, collect_startup_registry, evaluate_startup_module,
    prepare_init_module,
};
use serde_json::json;

struct RecordingLspBridge {
    requests: Arc<StdMutex<Vec<LspRuntimeBridgeRequest>>>,
    host_commands: Arc<StdMutex<Vec<String>>>,
    lsp_result: Result<LspRuntimeBridgeResponse, RuntimeCommandError>,
    buffer: Arc<StdMutex<ReadonlyBufferSnapshot>>,
}

impl RecordingLspBridge {
    fn new(lsp_result: Result<LspRuntimeBridgeResponse, RuntimeCommandError>) -> Self {
        Self {
            requests: Arc::new(StdMutex::new(Vec::new())),
            host_commands: Arc::new(StdMutex::new(Vec::new())),
            lsp_result,
            buffer: Arc::new(StdMutex::new(ReadonlyBufferSnapshot {
                id: 101,
                path: Some(PathBuf::from("/workspace/src/main.rs")),
                line_count: 12,
                cursor_row: 4,
                cursor_col: 0,
                current_line: "fn main() {}".to_string(),
                text: "fn main() {}\n".to_string(),
            })),
        }
    }

    fn recorded_requests(&self) -> Vec<LspRuntimeBridgeRequest> {
        self.requests
            .lock()
            .expect("recorded requests lock")
            .clone()
    }

    fn recorded_host_commands(&self) -> Vec<String> {
        self.host_commands
            .lock()
            .expect("recorded host commands lock")
            .clone()
    }
}

impl HostCapabilityBridge for RecordingLspBridge {
    fn execute_host_command(&self, name: &str) -> BoxFuture<Result<(), RuntimeCommandError>> {
        self.host_commands
            .lock()
            .expect("recorded host commands lock")
            .push(name.to_string());
        Box::pin(async move { Ok(()) })
    }

    fn execute_lsp_request(
        &self,
        request: LspRuntimeBridgeRequest,
    ) -> BoxFuture<Result<LspRuntimeBridgeResponse, RuntimeCommandError>> {
        self.requests
            .lock()
            .expect("recorded requests lock")
            .push(request);
        let result = self.lsp_result.clone();
        Box::pin(async move { result })
    }

    fn current_buffer(&self) -> BoxFuture<ReadonlyBufferSnapshot> {
        let buffer = self
            .buffer
            .lock()
            .expect("recording bridge buffer lock")
            .clone();
        Box::pin(async move { buffer })
    }

    fn current_window(&self) -> BoxFuture<ReadonlyWindowSnapshot> {
        Box::pin(async move { ReadonlyWindowSnapshot { id: 3 } })
    }

    fn current_editor(&self) -> BoxFuture<ReadonlyEditorSnapshot> {
        Box::pin(async move {
            ReadonlyEditorSnapshot {
                mode: RuntimeMode::Normal,
            }
        })
    }
}

fn lsp_success_response(method: &str) -> LspRuntimeBridgeResponse {
    LspRuntimeBridgeResponse {
        source: LspRuntimeBridgeSource::Lsp,
        method: method.to_string(),
        result: json!({
            "contents": {
                "kind": "plaintext",
                "value": "hover result from typed bridge",
            }
        }),
    }
}

#[test]
fn lsp_runtime_bridge_decision_is_pinned_to_builtin_host_capability() {
    assert!(LSP_RUNTIME_BRIDGE_DECISION.contains("built-in host capability"));
    assert!(LSP_RUNTIME_BRIDGE_DECISION.contains("stable TypeScript runtime API"));
}

fn unique_path(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-lsp-runtime-bridge-{name}-{nanos}.ts"))
}

fn file_uri_for_path(path: &std::path::Path) -> String {
    format!(
        "file://{}",
        path.to_string_lossy()
            .split('/')
            .map(|part| part.replace(' ', "%20"))
            .collect::<Vec<_>>()
            .join("/")
    )
}

#[tokio::test(flavor = "current_thread")]
async fn lsp_preview_plugin_uses_typed_runtime_bridge_and_records_exact_request_shape() {
    let plugin_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/saya-lsp-client.ts");
    let config_path = unique_path("contract-init");
    let source = format!(
        r#"
            import {{ setupSayaLspClient }} from {specifier:?};
            setupSayaLspClient({{
                bridgeCommand: "legacy.host.lsp",
                clientName: "saya-contract",
                rootUri: "file:///workspace",
                languageId: "rust",
                trace: "messages",
                positionEncoding: "utf-16",
                enableBufferEvents: false,
            }});
        "#,
        specifier = plugin_path.to_string_lossy()
    );
    std::fs::write(&config_path, source).expect("LSP contract config should be written");
    let prepared = prepare_init_module(
        &config_path,
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).as_path(),
    );
    let StartupModulePrepareResult::Success(module) = prepared else {
        panic!("LSP contract startup module should prepare");
    };
    evaluate_startup_module(&module.executable_source_text)
        .await
        .expect("LSP contract startup module should evaluate");
    let registry = collect_startup_registry(&module.executable_source_text)
        .await
        .expect("LSP contract startup registry should collect");
    let bridge = Arc::new(RecordingLspBridge::new(Ok(lsp_success_response(
        "textDocument/hover",
    ))));
    let runtime = SayaLiveRuntime::spawn_from_seed(
        bridge.clone(),
        CallbackRegistrySeed::from_startup_registry(&registry),
    )
    .expect("runtime should spawn from LSP registry");

    runtime
        .execute_command("lsp.hover")
        .expect("lsp.hover command should queue")
        .await_result()
        .await
        .expect("lsp.hover command should complete through typed bridge");

    let host_commands = bridge.recorded_host_commands();
    assert_eq!(host_commands.len(), 1);
    assert!(
        host_commands[0].starts_with("lsp.floatHover "),
        "LSP hover responses should be routed to the hover preview surface, not the legacy bridge: {host_commands:?}"
    );
    let requests = bridge.recorded_requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.source, LspRuntimeBridgeSource::Lsp);
    assert_eq!(request.protocol_version, "3.17");
    assert_eq!(request.method, "textDocument/hover");
    assert_eq!(request.client_name, "saya-contract");
    assert_eq!(request.root_uri.as_deref(), Some("file:///workspace"));
    assert_eq!(request.language_id, "rust");
    assert_eq!(request.trace.as_str(), "messages");
    assert_eq!(request.position_encoding.as_str(), "utf-16");
    assert_eq!(
        request
            .text_document
            .as_ref()
            .map(|document| document.uri.as_str()),
        Some("file:///workspace/src/main.rs")
    );
    assert_eq!(request.position.line, 4);
    assert_eq!(request.position.character, 0);
    assert_eq!(request.buffer.id, 101);
    assert_eq!(request.buffer.current_line, "fn main() {}");
    assert_eq!(request.editor.mode, RuntimeMode::Normal);
    assert_eq!(request.event, None);
}

#[tokio::test(flavor = "current_thread")]
async fn lsp_preview_plugin_resolves_relative_buffer_paths_against_root_uri() {
    let plugin_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/saya-lsp-client.ts");
    let config_path = unique_path("relative-uri");
    let source = format!(
        r#"
            import {{ setupSayaLspClient }} from {specifier:?};
            setupSayaLspClient({{
                clientName: "saya-relative-uri",
                rootUri: "file:///workspace",
                languageId: "rust",
                enableBufferEvents: false,
            }});
        "#,
        specifier = plugin_path.to_string_lossy()
    );
    std::fs::write(&config_path, source).expect("LSP relative URI config should be written");
    let prepared = prepare_init_module(
        &config_path,
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).as_path(),
    );
    let StartupModulePrepareResult::Success(module) = prepared else {
        panic!("LSP relative URI startup module should prepare");
    };
    let registry = collect_startup_registry(&module.executable_source_text)
        .await
        .expect("LSP relative URI startup registry should collect");
    let bridge = Arc::new(RecordingLspBridge::new(Ok(lsp_success_response(
        "textDocument/hover",
    ))));
    *bridge.buffer.lock().expect("recording bridge buffer lock") = ReadonlyBufferSnapshot {
        id: 404,
        path: Some(PathBuf::from("src/main.rs")),
        line_count: 1,
        cursor_row: 0,
        cursor_col: 0,
        current_line: "fn main() {}".to_string(),
        text: "fn main() {}\n".to_string(),
    };
    let runtime = SayaLiveRuntime::spawn_from_seed(
        bridge.clone(),
        CallbackRegistrySeed::from_startup_registry(&registry),
    )
    .expect("runtime should spawn from LSP relative URI registry");

    runtime
        .execute_command("lsp.hover")
        .expect("lsp.hover command should queue")
        .await_result()
        .await
        .expect("lsp.hover command should complete through typed bridge");

    let requests = bridge.recorded_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0]
            .text_document
            .as_ref()
            .map(|document| document.uri.as_str()),
        Some("file:///workspace/src/main.rs")
    );
    assert_eq!(
        requests[0]
            .params
            .as_ref()
            .and_then(|params| params.pointer("/textDocument/uri"))
            .and_then(serde_json::Value::as_str),
        Some("file:///workspace/src/main.rs")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn lsp_preview_plugin_routes_feature_responses_to_editor_ui_commands() {
    let plugin_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/saya-lsp-client.ts");
    let config_path = unique_path("feature-response-ui");
    let source = format!(
        r#"
            import {{ setupSayaLspClient }} from {specifier:?};
            setupSayaLspClient({{
                clientName: "saya-feature-ui",
                rootUri: "file:///workspace",
                languageId: "rust",
                enableBufferEvents: false,
            }});
        "#,
        specifier = plugin_path.to_string_lossy()
    );
    std::fs::write(&config_path, source).expect("LSP feature UI config should be written");
    let prepared = prepare_init_module(
        &config_path,
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).as_path(),
    );
    let StartupModulePrepareResult::Success(module) = prepared else {
        panic!("LSP feature UI startup module should prepare");
    };
    let registry = collect_startup_registry(&module.executable_source_text)
        .await
        .expect("LSP feature UI startup registry should collect");

    for (command, method, result, expected_prefix) in [
        (
            "lsp.definition",
            "textDocument/definition",
            json!({
                "uri": "file:///workspace/src/lib.rs",
                "range": { "start": { "line": 2, "character": 4 } }
            }),
            "lsp.gotoDefinition ",
        ),
        (
            "lsp.references",
            "textDocument/references",
            json!([
                {
                    "uri": "file:///workspace/src/main.rs",
                    "range": { "start": { "line": 4, "character": 1 } }
                }
            ]),
            "lsp.floatLocations ",
        ),
        (
            "lsp.documentSymbol",
            "textDocument/documentSymbol",
            json!([
                {
                    "name": "main",
                    "kind": 12,
                    "range": { "start": { "line": 0, "character": 0 } }
                }
            ]),
            "lsp.floatSymbols ",
        ),
    ] {
        let bridge = Arc::new(RecordingLspBridge::new(Ok(LspRuntimeBridgeResponse {
            source: LspRuntimeBridgeSource::Lsp,
            method: method.to_string(),
            result,
        })));
        let runtime = SayaLiveRuntime::spawn_from_seed(
            bridge.clone(),
            CallbackRegistrySeed::from_startup_registry(&registry),
        )
        .expect("runtime should spawn from LSP feature UI registry");

        runtime
            .execute_command(command)
            .expect("LSP feature UI command should queue")
            .await_result()
            .await
            .expect("LSP feature UI command should complete");

        let host_commands = bridge.recorded_host_commands();
        assert_eq!(
            host_commands.len(),
            1,
            "{command} should issue exactly one UI host command"
        );
        assert!(
            host_commands[0].starts_with(expected_prefix),
            "{command} should route to {expected_prefix}, got {host_commands:?}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn lsp_preview_plugin_exposes_daily_coding_commands_with_lsp_317_request_shapes() {
    let plugin_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/saya-lsp-client.ts");
    let config_path = unique_path("daily-coding-commands");
    let source = format!(
        r#"
            import {{ setupSayaLspClient }} from {specifier:?};
            setupSayaLspClient({{
                clientName: "saya-daily-coding",
                rootUri: "file:///workspace",
                languageId: "rust",
                enableBufferEvents: false,
                formattingOptions: {{
                    tabSize: 2,
                    insertSpaces: true,
                    trimTrailingWhitespace: true,
                }},
                renameNewName: "renamed_symbol",
                codeActionKinds: ["quickfix", "source.organizeImports"],
                commands: {{
                    completion: "code.complete",
                    completionResolve: "code.complete.resolve",
                    signatureHelp: "code.signature",
                    formatting: "code.format",
                    rangeFormatting: "code.rangeFormat",
                    rename: "code.rename",
                    codeAction: "code.action",
                    codeActionResolve: "code.action.resolve",
                }},
            }});
        "#,
        specifier = plugin_path.to_string_lossy()
    );
    std::fs::write(&config_path, source).expect("LSP daily coding config should be written");
    let prepared = prepare_init_module(
        &config_path,
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).as_path(),
    );
    let StartupModulePrepareResult::Success(module) = prepared else {
        panic!("LSP daily coding startup module should prepare");
    };
    let registry = collect_startup_registry(&module.executable_source_text)
        .await
        .expect("LSP daily coding startup registry should collect");

    for (command, method, result, expected_prefix) in [
        (
            "code.complete",
            "textDocument/completion",
            json!({
                "items": [
                    {
                        "label": "println!",
                        "kind": 3,
                        "detail": "macro",
                        "documentation": { "kind": "markdown", "value": "Prints a line." }
                    }
                ]
            }),
            "completion.floatMenu ",
        ),
        (
            "code.complete.resolve",
            "completionItem/resolve",
            json!({
                "label": "println!",
                "detail": "resolved macro",
                "documentation": "Resolved docs"
            }),
            "completion.floatMenu ",
        ),
        (
            "code.signature",
            "textDocument/signatureHelp",
            json!({
                "activeSignature": 0,
                "signatures": [
                    {
                        "label": "fn call(value: i32)",
                        "documentation": "signature docs"
                    }
                ]
            }),
            "lsp.floatHover ",
        ),
        (
            "code.format",
            "textDocument/formatting",
            json!([
                {
                    "range": {
                        "start": { "line": 0, "character": 0 },
                        "end": { "line": 0, "character": 10 }
                    },
                    "newText": "fn main() {}"
                }
            ]),
            "lsp.previewWorkspaceEdit ",
        ),
        (
            "code.rangeFormat",
            "textDocument/rangeFormatting",
            json!([]),
            "lsp.previewWorkspaceEdit ",
        ),
        (
            "code.rename",
            "textDocument/rename",
            json!({
                "changes": {
                    "file:///workspace/src/main.rs": [
                        {
                            "range": {
                                "start": { "line": 4, "character": 0 },
                                "end": { "line": 4, "character": 4 }
                            },
                            "newText": "renamed_symbol"
                        }
                    ]
                }
            }),
            "lsp.previewWorkspaceEdit ",
        ),
        (
            "code.action",
            "textDocument/codeAction",
            json!([
                {
                    "title": "Organize Imports",
                    "kind": "source.organizeImports"
                }
            ]),
            "lsp.floatCodeActions ",
        ),
        (
            "code.action.resolve",
            "codeAction/resolve",
            json!({
                "title": "Apply quick fix",
                "kind": "quickfix",
                "edit": { "changes": {} }
            }),
            "lsp.floatCodeActions ",
        ),
    ] {
        let bridge = Arc::new(RecordingLspBridge::new(Ok(LspRuntimeBridgeResponse {
            source: LspRuntimeBridgeSource::Lsp,
            method: method.to_string(),
            result,
        })));
        if method == "textDocument/completion" {
            *bridge.buffer.lock().expect("recording bridge buffer lock") = ReadonlyBufferSnapshot {
                id: 202,
                path: Some(PathBuf::from("/workspace/src/main.rs")),
                line_count: 1,
                cursor_row: 0,
                cursor_col: "module.".len(),
                current_line: "module.".to_string(),
                text: "module.\n".to_string(),
            };
        }
        let runtime = SayaLiveRuntime::spawn_from_seed(
            bridge.clone(),
            CallbackRegistrySeed::from_startup_registry(&registry),
        )
        .expect("runtime should spawn from LSP daily coding registry");

        runtime
            .execute_command(command)
            .expect("LSP daily coding command should queue")
            .await_result()
            .await
            .expect("LSP daily coding command should complete");

        let requests = bridge.recorded_requests();
        assert_eq!(requests.len(), 1, "{command} should send one request");
        let request = &requests[0];
        assert_eq!(request.source, LspRuntimeBridgeSource::Lsp);
        assert_eq!(request.protocol_version, "3.17");
        assert_eq!(request.method, method);
        assert_eq!(
            request
                .text_document
                .as_ref()
                .map(|document| document.uri.as_str()),
            Some("file:///workspace/src/main.rs")
        );
        assert!(
            request.params.is_some(),
            "{method} should include protocol params"
        );
        if method == "textDocument/completion" {
            assert_eq!(
                request
                    .params
                    .as_ref()
                    .and_then(|params| params.pointer("/context/triggerKind"))
                    .and_then(serde_json::Value::as_u64),
                Some(2),
                "completion requests should carry trigger-character context"
            );
            assert_eq!(
                request
                    .params
                    .as_ref()
                    .and_then(|params| params.pointer("/context/triggerCharacter"))
                    .and_then(serde_json::Value::as_str),
                Some("."),
                "completion requests should include the trigger character"
            );
        }

        let host_commands = bridge.recorded_host_commands();
        assert_eq!(
            host_commands.len(),
            1,
            "{command} should issue exactly one UI host command"
        );
        assert!(
            host_commands[0].starts_with(expected_prefix),
            "{command} should route to {expected_prefix}, got {host_commands:?}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn lsp_preview_plugin_routes_lsif_commands_through_typed_runtime_bridge() {
    let plugin_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/saya-lsp-client.ts");
    let config_path = unique_path("lsif-typed-bridge");
    let source = format!(
        r#"
            import {{ setupSayaLspClient }} from {specifier:?};
            setupSayaLspClient({{
                clientName: "saya-lsif-contract",
                rootUri: "file:///workspace",
                languageId: "rust",
                enableBufferEvents: false,
                lsif: {{
                    enabled: true,
                    bridgeCommand: "legacy.host.lsif",
                    dumpPath: ".cache/index.lsif",
                }},
            }});
        "#,
        specifier = plugin_path.to_string_lossy()
    );
    std::fs::write(&config_path, source).expect("LSIF bridge config should be written");
    let prepared = prepare_init_module(
        &config_path,
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).as_path(),
    );
    let StartupModulePrepareResult::Success(module) = prepared else {
        panic!("LSIF bridge startup module should prepare");
    };
    let registry = collect_startup_registry(&module.executable_source_text)
        .await
        .expect("LSIF bridge startup registry should collect");
    let bridge = Arc::new(RecordingLspBridge::new(Ok(LspRuntimeBridgeResponse {
        source: LspRuntimeBridgeSource::Lsif,
        method: "textDocument/hover".to_string(),
        result: json!({
            "contents": {
                "kind": "plaintext",
                "value": "hover result from LSIF index",
            }
        }),
    })));
    let runtime = SayaLiveRuntime::spawn_from_seed(
        bridge.clone(),
        CallbackRegistrySeed::from_startup_registry(&registry),
    )
    .expect("runtime should spawn from LSIF bridge registry");

    runtime
        .execute_command("lsif.hover")
        .expect("lsif.hover command should queue")
        .await_result()
        .await
        .expect("lsif.hover command should complete through typed bridge");

    let requests = bridge.recorded_requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.source, LspRuntimeBridgeSource::Lsif);
    assert_eq!(request.protocol_version, "0.6.0");
    assert_eq!(request.method, "textDocument/hover");
    assert_eq!(request.dump_path, ".cache/index.lsif");

    let host_commands = bridge.recorded_host_commands();
    assert_eq!(host_commands.len(), 1);
    assert!(
        host_commands[0].starts_with("lsp.floatHover "),
        "LSIF hover should route to the hover UI, not legacy bridge command: {host_commands:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn lsp_preview_plugin_reports_language_server_not_ready_with_status_command() {
    let plugin_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/saya-lsp-client.ts");
    let config_path = unique_path("feature-not-ready-status");
    let source = format!(
        r#"
            import {{ setupSayaLspClient }} from {specifier:?};
            setupSayaLspClient({{
                clientName: "saya-feature-status",
                rootUri: "file:///workspace",
                languageId: "rust",
                enableBufferEvents: false,
            }});
        "#,
        specifier = plugin_path.to_string_lossy()
    );
    std::fs::write(&config_path, source).expect("LSP feature status config should be written");
    let prepared = prepare_init_module(
        &config_path,
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).as_path(),
    );
    let StartupModulePrepareResult::Success(module) = prepared else {
        panic!("LSP feature status startup module should prepare");
    };
    let registry = collect_startup_registry(&module.executable_source_text)
        .await
        .expect("LSP feature status startup registry should collect");
    let bridge = Arc::new(RecordingLspBridge::new(Err(
        RuntimeCommandError::CommandFailed {
            name: "lsp.request".to_string(),
            message: "language server is not ready".to_string(),
        },
    )));
    let runtime = SayaLiveRuntime::spawn_from_seed(
        bridge.clone(),
        CallbackRegistrySeed::from_startup_registry(&registry),
    )
    .expect("runtime should spawn from LSP feature status registry");

    runtime
        .execute_command("lsp.hover")
        .expect("lsp.hover command should queue")
        .await_result()
        .await
        .expect_err("LSP failure should still fail the command");

    let host_commands = bridge.recorded_host_commands();
    assert_eq!(host_commands.len(), 1);
    assert!(
        host_commands[0].starts_with("lsp.status "),
        "LSP failures should surface a user-facing status command: {host_commands:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn lsp_preview_plugin_converts_saya_cursor_columns_to_lsp_positions_by_encoding() {
    let plugin_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/saya-lsp-client.ts");
    let config_path = unique_path("position-encoding");
    let source = format!(
        r#"
            import {{ setupSayaLspClient }} from {specifier:?};
            setupSayaLspClient({{
                clientName: "saya-position-encoding",
                rootUri: "file:///workspace",
                languageId: "rust",
                trace: "verbose",
                positionEncoding: "utf-16",
                enableBufferEvents: false,
                commands: {{
                    hover: "lsp.hover.utf16",
                    references: "lsp.references.utf16",
                }},
            }});
            setupSayaLspClient({{
                clientName: "saya-position-encoding",
                rootUri: "file:///workspace",
                languageId: "rust",
                trace: "verbose",
                positionEncoding: "utf-8",
                enableBufferEvents: false,
                commands: {{
                    hover: "lsp.hover.utf8",
                }},
            }});
            setupSayaLspClient({{
                clientName: "saya-position-encoding",
                rootUri: "file:///workspace",
                languageId: "rust",
                trace: "verbose",
                positionEncoding: "utf-32",
                enableBufferEvents: false,
                commands: {{
                    hover: "lsp.hover.utf32",
                }},
            }});
        "#,
        specifier = plugin_path.to_string_lossy()
    );
    std::fs::write(&config_path, source).expect("LSP position config should be written");
    let prepared = prepare_init_module(
        &config_path,
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).as_path(),
    );
    let StartupModulePrepareResult::Success(module) = prepared else {
        panic!("LSP position startup module should prepare");
    };
    let registry = collect_startup_registry(&module.executable_source_text)
        .await
        .expect("LSP position startup registry should collect");
    let bridge = Arc::new(RecordingLspBridge::new(Ok(lsp_success_response(
        "textDocument/hover",
    ))));
    *bridge.buffer.lock().expect("recording bridge buffer lock") = ReadonlyBufferSnapshot {
        id: 303,
        path: Some(PathBuf::from("/workspace/src/main.rs")),
        line_count: 1,
        cursor_row: 0,
        cursor_col: "aé😀e\u{0301}".len(),
        current_line: "aé😀e\u{0301}z".to_string(),
        text: "aé😀e\u{0301}z\n".to_string(),
    };
    let runtime = SayaLiveRuntime::spawn_from_seed(
        bridge.clone(),
        CallbackRegistrySeed::from_startup_registry(&registry),
    )
    .expect("runtime should spawn from LSP position registry");

    for command in [
        "lsp.hover.utf16",
        "lsp.hover.utf8",
        "lsp.hover.utf32",
        "lsp.references.utf16",
    ] {
        runtime
            .execute_command(command)
            .expect("LSP position command should queue")
            .await_result()
            .await
            .expect("LSP position command should complete");
    }

    let requests = bridge.recorded_requests();
    let positions: Vec<_> = requests
        .iter()
        .map(|request| {
            (
                request.method.as_str(),
                request.position_encoding.as_str(),
                request.position.character,
                request
                    .params
                    .as_ref()
                    .and_then(|params| params.pointer("/position/character"))
                    .and_then(|value| value.as_u64())
                    .expect("params position character") as usize,
            )
        })
        .collect();
    assert_eq!(
        positions,
        vec![
            ("textDocument/hover", "utf-16", 6, 6),
            ("textDocument/hover", "utf-8", 10, 10),
            ("textDocument/hover", "utf-32", 5, 5),
            ("textDocument/references", "utf-16", 6, 6),
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn lsp_preview_plugin_supports_language_server_configuration_surface() {
    let plugin_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/saya-lsp-client.ts");
    let workspace = unique_path("configuration-workspace");
    let src_dir = workspace.join("cmd").join("app");
    std::fs::create_dir_all(&src_dir).expect("workspace source dir should be created");
    std::fs::write(workspace.join("go.mod"), "module example.com/saya\n")
        .expect("go.mod should be written");
    let go_file = src_dir.join("main.go");
    std::fs::write(&go_file, "package main\n\nfunc main() {}\n").expect("go file");

    let config_path = unique_path("configuration-surface");
    let source = format!(
        r#"
            import {{ setupSayaLspClient }} from {specifier:?};
            setupSayaLspClient({{
                clientName: "saya-config",
                languageId: "plaintext",
                languageIdByExtension: {{
                    go: "go",
                    rs: "rust",
                }},
                enableBufferEvents: false,
                servers: [
                    {{
                        name: "gopls",
                        command: "gopls",
                        args: ["serve"],
                        languages: ["go"],
                        filePatterns: ["**/*.go"],
                        rootMarkers: ["go.mod", ".git"],
                        initializationOptions: {{ semanticTokens: true }},
                        positionEncoding: "utf-16",
                    }},
                    {{
                        name: "rust-analyzer",
                        command: "rust-analyzer",
                        languages: ["rust"],
                        filePatterns: ["**/*.rs"],
                        rootMarkers: ["Cargo.toml", ".git"],
                    }},
                ],
            }});
        "#,
        specifier = plugin_path.to_string_lossy()
    );
    std::fs::write(&config_path, source).expect("LSP configuration config should be written");
    let prepared = prepare_init_module(
        &config_path,
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).as_path(),
    );
    let StartupModulePrepareResult::Success(module) = prepared else {
        panic!("LSP configuration startup module should prepare");
    };
    let registry = collect_startup_registry(&module.executable_source_text)
        .await
        .expect("LSP configuration startup registry should collect");
    let bridge = Arc::new(RecordingLspBridge::new(Ok(LspRuntimeBridgeResponse {
        source: LspRuntimeBridgeSource::Lsp,
        method: "initialize".to_string(),
        result: json!({
            "capabilities": {
                "hoverProvider": true,
                "positionEncoding": "utf-8"
            }
        }),
    })));
    *bridge.buffer.lock().expect("recording bridge buffer lock") = ReadonlyBufferSnapshot {
        id: 404,
        path: Some(go_file.clone()),
        line_count: 3,
        cursor_row: 2,
        cursor_col: 0,
        current_line: "func main() {}".to_string(),
        text: "package main\n\nfunc main() {}\n".to_string(),
    };
    let runtime = SayaLiveRuntime::spawn_from_seed(
        bridge.clone(),
        CallbackRegistrySeed::from_startup_registry(&registry),
    )
    .expect("runtime should spawn from LSP configuration registry");

    runtime
        .execute_command("lsp.initialize")
        .expect("lsp.initialize command should queue")
        .await_result()
        .await
        .expect("configured initialize should complete");
    runtime
        .execute_command("lsp.hover")
        .expect("lsp.hover command should queue")
        .await_result()
        .await
        .expect("configured hover should complete with negotiated encoding");

    let requests = bridge.recorded_requests();
    assert_eq!(requests.len(), 2);
    let initialize = &requests[0];
    assert_eq!(initialize.client_name, "gopls");
    assert_eq!(initialize.language_id, "go");
    assert_eq!(
        initialize.root_uri.as_deref(),
        Some(file_uri_for_path(&workspace).as_str())
    );
    assert_eq!(
        initialize
            .params
            .as_ref()
            .and_then(|params| params.pointer("/initializationOptions/semanticTokens"))
            .and_then(|value| value.as_bool()),
        Some(true)
    );
    let server = initialize.server.as_ref().expect("configured server");
    assert_eq!(server.name, "gopls");
    assert_eq!(server.command, "gopls");
    assert_eq!(server.args, vec!["serve".to_string()]);
    assert_eq!(
        server.root_markers,
        vec!["go.mod".to_string(), ".git".to_string()]
    );

    let hover = &requests[1];
    assert_eq!(hover.method, "textDocument/hover");
    assert_eq!(hover.client_name, "gopls");
    assert_eq!(hover.position_encoding, "utf-8");
    assert_eq!(
        hover
            .params
            .as_ref()
            .and_then(|params| params.pointer("/position/character"))
            .and_then(|value| value.as_u64()),
        Some(0)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn lsp_preview_plugin_rejects_invalid_configuration_surface_values() {
    let plugin_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/saya-lsp-client.ts");

    for (name, setup_source, expected) in [
        (
            "empty-command-name",
            r#"setupSayaLspClient({ commands: { hover: "" }, enableBufferEvents: false });"#,
            "commands.hover",
        ),
        (
            "invalid-root-uri",
            r#"setupSayaLspClient({ rootUri: "/workspace", enableBufferEvents: false });"#,
            "rootUri",
        ),
        (
            "invalid-server-command",
            r#"setupSayaLspClient({ servers: [{ name: "bad", command: "", languages: ["go"] }], enableBufferEvents: false });"#,
            "servers[0].command",
        ),
    ] {
        let config_path = unique_path(name);
        std::fs::write(
            &config_path,
            format!(
                r#"
                    import {{ setupSayaLspClient }} from {specifier:?};
                    {setup_source}
                "#,
                specifier = plugin_path.to_string_lossy()
            ),
        )
        .expect("invalid LSP configuration fixture should be written");
        let prepared = prepare_init_module(
            &config_path,
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).as_path(),
        );
        let StartupModulePrepareResult::Success(module) = prepared else {
            panic!("invalid LSP configuration module should still prepare");
        };
        let error = collect_startup_registry(&module.executable_source_text)
            .await
            .expect_err("invalid LSP configuration should fail startup evaluation");
        assert!(
            error.contains(expected),
            "expected invalid LSP configuration error to mention {expected}, got {error}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn lsp_preview_plugin_synchronizes_documents_with_full_text_and_versions() {
    let plugin_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/saya-lsp-client.ts");
    let config_path = unique_path("document-sync");
    let source = format!(
        r#"
            import {{ setupSayaLspClient }} from {specifier:?};
            setupSayaLspClient({{
                clientName: "saya-document-sync",
                rootUri: "file:///workspace",
                languageId: "plaintext",
                languageIdByExtension: {{
                    rs: "rust",
                }},
                trace: "verbose",
                positionEncoding: "utf-16",
            }});
        "#,
        specifier = plugin_path.to_string_lossy()
    );
    std::fs::write(&config_path, source).expect("LSP document sync config should be written");
    let prepared = prepare_init_module(
        &config_path,
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).as_path(),
    );
    let StartupModulePrepareResult::Success(module) = prepared else {
        panic!("LSP document sync startup module should prepare");
    };
    let registry = collect_startup_registry(&module.executable_source_text)
        .await
        .expect("LSP document sync startup registry should collect");
    let bridge = Arc::new(RecordingLspBridge::new(Ok(lsp_success_response(
        "textDocument/didOpen",
    ))));
    *bridge.buffer.lock().expect("recording bridge buffer lock") = ReadonlyBufferSnapshot {
        id: 202,
        path: Some(PathBuf::from("/workspace/src/../src/main file.rs")),
        line_count: 2,
        cursor_row: 0,
        cursor_col: 0,
        current_line: "fn main() {".to_string(),
        text: "fn main() {\n    println!(\"hi\");\n}\n".to_string(),
    };
    let buffer = bridge
        .buffer
        .lock()
        .expect("recording bridge buffer lock")
        .clone();
    let runtime = SayaLiveRuntime::spawn_from_seed(
        bridge.clone(),
        CallbackRegistrySeed::from_startup_registry(&registry),
    )
    .expect("runtime should spawn from LSP document sync registry");

    for event in [
        RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: buffer.clone(),
        }),
        RuntimeEventPayload::BufferChanged(BufferEventPayload {
            buffer: buffer.clone(),
        }),
        RuntimeEventPayload::BufferWritePost(BufferEventPayload {
            buffer: buffer.clone(),
        }),
        RuntimeEventPayload::BufferClosed(BufferEventPayload {
            buffer: buffer.clone(),
        }),
    ] {
        runtime
            .dispatch_event(event)
            .expect("LSP document sync event should queue")
            .await_result()
            .await
            .expect("LSP document sync event should complete");
    }

    let requests = bridge.recorded_requests();
    let methods: Vec<_> = requests
        .iter()
        .map(|request| request.method.as_str())
        .collect();
    assert_eq!(
        methods,
        vec![
            "textDocument/didOpen",
            "textDocument/didChange",
            "textDocument/didSave",
            "textDocument/didClose",
        ]
    );

    let opened = requests[0].params.as_ref().expect("didOpen params");
    assert_eq!(
        opened
            .pointer("/textDocument/uri")
            .and_then(|value| value.as_str()),
        Some("file:///workspace/src/main%20file.rs")
    );
    assert_eq!(
        opened
            .pointer("/textDocument/languageId")
            .and_then(|value| value.as_str()),
        Some("rust")
    );
    assert_eq!(
        opened
            .pointer("/textDocument/version")
            .and_then(|value| value.as_i64()),
        Some(1)
    );
    assert_eq!(
        opened
            .pointer("/textDocument/text")
            .and_then(|value| value.as_str()),
        Some("fn main() {\n    println!(\"hi\");\n}\n")
    );

    let changed = requests[1].params.as_ref().expect("didChange params");
    assert_eq!(
        changed
            .pointer("/textDocument/version")
            .and_then(|value| value.as_i64()),
        Some(2)
    );
    assert_eq!(
        changed
            .pointer("/contentChanges/0/text")
            .and_then(|value| value.as_str()),
        Some("fn main() {\n    println!(\"hi\");\n}\n")
    );
    assert!(
        changed.pointer("/contentChanges/0/range").is_none(),
        "first document sync implementation must use full-document changes"
    );

    let saved = requests[2].params.as_ref().expect("didSave params");
    assert_eq!(
        saved
            .pointer("/textDocument/uri")
            .and_then(|value| value.as_str()),
        Some("file:///workspace/src/main%20file.rs")
    );
    assert_eq!(
        saved.pointer("/text").and_then(|value| value.as_str()),
        Some("fn main() {\n    println!(\"hi\");\n}\n")
    );

    let closed = requests[3].params.as_ref().expect("didClose params");
    assert_eq!(
        closed
            .pointer("/textDocument/uri")
            .and_then(|value| value.as_str()),
        Some("file:///workspace/src/main%20file.rs")
    );
    assert!(closed.pointer("/textDocument/text").is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn lsp_runtime_bridge_returns_structured_results_to_typescript() {
    let bridge = Arc::new(RecordingLspBridge::new(Ok(lsp_success_response(
        "textDocument/hover",
    ))));
    let seed = CallbackRegistrySeed::from_startup_entries(vec![saya::startup_runtime::StartupRegistryEntry::Command {
        name: "lsp.result".to_string(),
        callback_source: r#"
            async () => {
                const response = await saya.lsp.request({
                    source: "lsp",
                    lspVersion: "3.17",
                    method: "textDocument/hover",
                    clientName: "saya-contract",
                    rootUri: "file:///workspace",
                    languageId: "rust",
                    trace: "messages",
                    positionEncoding: "utf-16",
                    dumpPath: "",
                    textDocument: { uri: "file:///workspace/src/main.rs" },
                    position: { line: 1, character: 2 },
                    buffer: await saya.buffer.current(),
                    editor: await saya.editor.current(),
                    event: null,
                });
                await saya.commands.execute(`result:${response.method}:${response.result.contents.value}`);
            }
        "#.to_string(),
    }]);
    let runtime = SayaLiveRuntime::spawn_from_seed(bridge.clone(), seed)
        .expect("runtime should spawn from typed LSP seed");

    runtime
        .execute_command("lsp.result")
        .expect("lsp.result command should queue")
        .await_result()
        .await
        .expect("lsp.result command should complete");

    assert_eq!(
        bridge.recorded_host_commands(),
        vec!["result:textDocument/hover:hover result from typed bridge".to_string()]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn lsp_runtime_bridge_rejects_invalid_payloads_before_host_dispatch() {
    let bridge = Arc::new(RecordingLspBridge::new(Ok(lsp_success_response(
        "textDocument/hover",
    ))));
    let seed = CallbackRegistrySeed::from_startup_entries(vec![
        saya::startup_runtime::StartupRegistryEntry::Command {
            name: "lsp.invalid".to_string(),
            callback_source: r#"async () => { await saya.lsp.request({ source: "lsp" }); }"#
                .to_string(),
        },
    ]);
    let runtime = SayaLiveRuntime::spawn_from_seed(bridge.clone(), seed)
        .expect("runtime should spawn from invalid LSP seed");

    let error = runtime
        .execute_command("lsp.invalid")
        .expect("lsp.invalid command should queue")
        .await_result()
        .await
        .expect_err("invalid LSP bridge payload should fail before host dispatch");

    assert!(matches!(
        error,
        RuntimeCommandError::CommandFailed { ref message, .. }
            if message.contains("invalid LSP bridge request")
    ));
    assert_eq!(
        bridge.recorded_requests(),
        Vec::<LspRuntimeBridgeRequest>::new()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn lsp_runtime_bridge_surfaces_host_failures_as_user_safe_command_errors() {
    let bridge = Arc::new(RecordingLspBridge::new(Err(
        RuntimeCommandError::CommandFailed {
            name: "lsp.request".to_string(),
            message: "language server is not ready".to_string(),
        },
    )));
    let seed = CallbackRegistrySeed::from_startup_entries(vec![
        saya::startup_runtime::StartupRegistryEntry::Command {
            name: "lsp.fail".to_string(),
            callback_source: r#"
            async () => {
                await saya.lsp.request({
                    source: "lsp",
                    lspVersion: "3.17",
                    method: "textDocument/hover",
                    clientName: "saya-contract",
                    rootUri: "file:///workspace",
                    languageId: "rust",
                    trace: "messages",
                    positionEncoding: "utf-16",
                    dumpPath: "",
                    textDocument: { uri: "file:///workspace/src/main.rs" },
                    position: { line: 1, character: 2 },
                    buffer: await saya.buffer.current(),
                    editor: await saya.editor.current(),
                    event: null,
                });
            }
        "#
            .to_string(),
        },
    ]);
    let runtime = SayaLiveRuntime::spawn_from_seed(bridge.clone(), seed)
        .expect("runtime should spawn from failing LSP seed");

    let error = runtime
        .execute_command("lsp.fail")
        .expect("lsp.fail command should queue")
        .await_result()
        .await
        .expect_err("host LSP failure should surface as a command error");

    assert_eq!(
        error,
        RuntimeCommandError::CommandFailed {
            name: "lsp.request".to_string(),
            message: "language server is not ready".to_string(),
        }
    );
}
