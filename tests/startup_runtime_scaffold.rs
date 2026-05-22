use std::path::PathBuf;
use std::sync::Arc;

use saya::presentation::theme::{
    MarkdownSemanticStyleKey, SyntaxSemanticStyleKey, ThemeTextStyleDeclaration, UiStyleKey,
};
use saya::runtime::callback_registry_seed::CallbackRegistrySeed;
use saya::runtime::live::{
    BoxFuture, HostCapabilityBridge, ReadonlyBufferSnapshot, ReadonlyEditorSnapshot,
    ReadonlyWindowSnapshot, RuntimeCommandError, RuntimeMode, SayaLiveRuntime,
};
use saya::runtime::startup::{
    SayaKeyMode, SayaKeymapAction, StartupModuleLoadResult, StartupModulePrepareResult,
    StartupOptionName, StartupOptionValue, StartupPluginSource, StartupRegistryEntry,
    collect_startup_registry, evaluate_startup_module, load_init_module, prepare_init_module,
    resolve_init_module_specifier,
};

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
    std::fs::write(&config_path, "saya.options.tabstop = 4;").expect("config file");

    let result = load_init_module(&config_path, &current_dir);

    match result {
        StartupModuleLoadResult::Success(module) => {
            assert_eq!(
                module.specifier.as_str(),
                format!("file://{}/init.ts", current_dir.to_string_lossy())
            );
            assert_eq!(module.source_text, "saya.options.tabstop = 4;");
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
            const tabstop: number = 4;
            saya.options.tabstop = tabstop;
        "#,
    )
    .expect("config file");

    let result = prepare_init_module(&config_path, &current_dir);

    match result {
        StartupModulePrepareResult::Success(module) => {
            assert!(module.executable_source_text.contains("const tabstop = 4;"));
            assert!(
                module
                    .executable_source_text
                    .contains("saya.options.tabstop = tabstop;")
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
    let _lock = saya::app::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
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
async fn init_ts_module_can_import_repository_agent_plugin() {
    let _lock = saya::app::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let current_dir = unique_path("cwd");
    std::fs::create_dir_all(&current_dir).expect("current dir");
    let config_path = current_dir.join("init.ts");
    let plugin_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/saya-agent.ts");
    std::fs::write(
        &config_path,
        format!(
            r#"
                import {{ setupSayaAgent }} from "{}";
                setupSayaAgent({{
                    defaultTool: "codex",
                    layout: {{ position: "right", size: "35%" }},
                }});
            "#,
            plugin_path.display()
        ),
    )
    .expect("config file");

    let prepared = prepare_init_module(&config_path, &current_dir);
    let StartupModulePrepareResult::Success(module) = prepared else {
        panic!(
            "repository agent plugin should prepare, got: {:?}",
            prepared
        );
    };
    let registry = collect_startup_registry(&module.executable_source_text)
        .await
        .expect("repository agent plugin should evaluate");

    for expected_command in [
        "panel.toggle",
        "panel.focus",
        "panel.unfocus",
        "panel.close",
        "panel.detach",
        "agent.sendCurrentFile",
        "agent.sendCurrentLine",
        "agent.sendSelectedRange",
        "agent.sendPrompt",
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
    for removed_command in ["agent.toggle", "agent.focus", "agent.close", "agent.detach"] {
        assert!(
            !registry.entries().iter().any(|entry| {
                matches!(
                    entry,
                    StartupRegistryEntry::Command { name, .. } if name == removed_command
                )
            }),
            "removed compatibility command should not be registered: {removed_command}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn init_ts_module_can_import_repository_lsp_client_plugin() {
    let _lock = saya::app::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
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
                try {{
                    setupSayaLspClient({{ ui: {{ popups: {{ hover: {{ width: "101%" }} }} }} }});
                    throw new Error("invalid LSP popup percentage should be rejected");
                }} catch (error) {{
                    if (!String(error && error.message).includes("1% through 100%")) {{
                        throw error;
                    }}
                }}
                try {{
                    setupSayaLspClient({{ ui: {{ popups: {{ hover: {{ width: 12.5 }} }} }} }});
                    throw new Error("fractional LSP popup width should be rejected");
                }} catch (error) {{
                    if (!String(error && error.message).includes("positive integer")) {{
                        throw error;
                    }}
                }}
                try {{
                    setupSayaLspClient({{ ui: {{ popups: {{ hover: {{ basis: "screen" }} }} }} }});
                    throw new Error("unknown LSP popup basis should be rejected");
                }} catch (error) {{
                    if (!String(error && error.message).includes("basis must be")) {{
                        throw error;
                    }}
                }}
                try {{
                    setupSayaLspClient({{ ui: {{ popups: {{ hovre: {{ width: 10 }} }} }} }});
                    throw new Error("misspelled LSP popup key should be rejected");
                }} catch (error) {{
                    if (!String(error && error.message).includes("unknown ui.popups.hovre")) {{
                        throw error;
                    }}
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
                    ui: {{
                        popups: {{
                            hover: {{ width: "60%", height: "35%" }},
                            diagnostics: {{ width: 72, height: 12 }},
                            locations: {{ width: "80%", height: "50%", basis: "editor" }},
                            symbols: {{ width: "70%", height: "60%", basis: "window" }},
                            signatureHelp: {{ width: 88, height: 14 }},
                        }},
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
        module.executable_source_text.contains("setupSayaLspClient"),
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
    assert!(hover_callback.contains(r#""hover":{"width":"60%","height":"35%","basis":"window"}"#));
    assert!(
        hover_callback.contains(r#""signatureHelp":{"width":88,"height":14,"basis":"window"}"#)
    );
    assert!(hover_callback.contains(r#""diagnostics":{"width":72,"height":12,"basis":"window"}"#));
    assert!(hover_callback.contains("kind: 'signatureHelp'"));

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

#[test]
fn lsp_client_shim_exposes_normal_typescript_named_exports() {
    let plugin_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/saya-lsp-client.ts");
    let source = std::fs::read_to_string(plugin_path).expect("lsp client shim should be readable");

    assert!(
        source.contains("export {")
            && source.contains("setupSayaLspClient")
            && source.contains("from \"./bundled/lsp-client/index.ts\""),
        "lsp client shim should expose named exports for external TypeScript language servers"
    );
}

#[test]
fn lsp_client_manager_routes_server_notifications_to_ui_commands() {
    let plugin_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/bundled/lsp-client/index.ts");
    let source = std::fs::read_to_string(plugin_path).expect("lsp client should be readable");

    assert!(
        source.contains("session.onNotification(function (message)")
            && source.contains("pendingNotifications.push")
            && source.contains("api.drainNotifications")
            && source.contains("for (const notification of manager.drainNotifications())")
            && source.find("for (const notification of manager.drainNotifications())")
                < source.find("await routeFeatureResponse(response)")
            && source.contains("textDocument/publishDiagnostics"),
        "LSP server notifications such as publishDiagnostics should update UI state before feature responses render"
    );
    assert!(
        source.contains("export type SayaPopupSizeValue")
            && source.contains("interface SayaLspPopupUiOptions")
            && source.contains("ui?:")
            && source.contains("normalizedPopupUi"),
        "LSP client source should expose and transport popup UI configuration"
    );
    assert!(
        source.contains(
            "lsp.nextDiagnostic ${JSON.stringify({ ui: normalizedPopupUi.diagnostics })}"
        ) && source.contains(
            "lsp.previousDiagnostic ${JSON.stringify({ ui: normalizedPopupUi.diagnostics })}"
        ),
        "diagnostic navigation commands should carry diagnostics popup UI configuration"
    );
}

#[test]
fn repository_lsp_client_plugin_public_helpers_cover_lsp_and_lsif_protocol_shape() {
    // プラグインは plugins/saya-lsp-client.ts （シム）から
    // plugins/bundled/lsp-client/ 配下のモジュール群へ分割されている。
    // 物理ファイル構造ではなく、展開後の実行ソースに API surface が
    // 含まれていることを検証する。
    let current_dir = unique_path("lsp-surface-cwd");
    std::fs::create_dir_all(&current_dir).expect("current dir");
    let plugin_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/saya-lsp-client.ts");
    let config_path = current_dir.join("init.ts");
    std::fs::write(
        &config_path,
        format!(
            r#"import {{ setupSayaLspClient, createLspMessageParser, encodeLspMessage, createLspJsonRpcClient, lspPositionFromSayaCursor, lspRangeFromSayaRange, parseLsifLine }} from "{}";
"#,
            plugin_path.display()
        ),
    )
    .expect("config file");

    let prepared = prepare_init_module(&config_path, &current_dir);
    let StartupModulePrepareResult::Success(module) = prepared else {
        panic!(
            "repository lsp client plugin shim should prepare, got: {:?}",
            prepared
        );
    };
    let source = module.executable_source_text;

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
        "normalizedPopupUi",
        "ui: normalizedPopupUi.hover",
        "ui: normalizedPopupUi.locations",
        "ui: normalizedPopupUi.symbols",
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
    let plugin_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/bundled/dired/index.ts");
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
fn init_ts_module_transpile_preserves_identifier_values_in_callback_object_literals() {
    let current_dir = unique_path("cwd");
    std::fs::create_dir_all(&current_dir).expect("current dir");
    let config_path = current_dir.join("init.ts");
    std::fs::write(
        &config_path,
        r#"
            saya.events.on("bufferOpen", async () => {
                const panelPosition = "right";
                const panelSize = "50%";
                await saya.panel.open({
                    id: "terminal-demo",
                    position: panelPosition,
                    size: panelSize,
                    focus: true,
                });
            });
        "#,
    )
    .expect("config file");

    let result = prepare_init_module(&config_path, &current_dir);

    match result {
        StartupModulePrepareResult::Success(module) => {
            assert!(
                module
                    .executable_source_text
                    .contains("position: panelPosition"),
                "identifier-valued object fields must not be stripped as type annotations: {}",
                module.executable_source_text
            );
            assert!(
                module.executable_source_text.contains("size: panelSize"),
                "identifier-valued object fields must not be stripped as type annotations: {}",
                module.executable_source_text
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
            saya.options.tabstop = 4;
        "#,
    )
    .await
    .expect("startup module should evaluate with saya namespace");
}

#[tokio::test(flavor = "current_thread")]
async fn startup_unknown_option_warns_without_failing_evaluation() {
    let registry = collect_startup_registry("saya.options.unknownoption = 4;")
        .await
        .expect("unknown option should warn without failing startup evaluation");

    assert!(
        registry.entries().iter().any(|entry| matches!(
            entry,
            StartupRegistryEntry::Warning { message }
                if message.contains("saya.options.unknownoption")
        )),
        "unknown option should be collected as a warning: {:?}",
        registry.entries()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_plugins_use_collects_local_plugin_declaration() {
    let registry = collect_startup_registry(
        r#"
            saya.plugins.use([
                { local: "~/.config/saya/plugins/workspace-tools" },
            ]);
        "#,
    )
    .await
    .expect("startup plugin use declaration should evaluate");

    assert!(registry.entries().iter().any(|entry| matches!(
        entry,
        StartupRegistryEntry::PluginUse { declaration }
            if declaration.name == "workspace-tools"
                && declaration.source == StartupPluginSource::Local {
                    path: "~/.config/saya/plugins/workspace-tools".to_string()
                }
                && declaration.module == "mod.ts"
                && declaration.setup == "setup"
                && declaration.commands.is_empty()
                && declaration.events.is_empty()
    )));
}

#[tokio::test(flavor = "current_thread")]
async fn startup_plugins_lazy_collects_github_triggers_and_options() {
    let registry = collect_startup_registry(
        r#"
            saya.plugins.lazy([
                {
                    github: "shun/saya-git-tools",
                    rev: "v0.1.0",
                    commands: ["GitStatus", "GitBlame"],
                    events: ["bufferOpen"],
                    options: { trace: "messages" },
                },
            ]);
        "#,
    )
    .await
    .expect("startup plugin lazy declaration should evaluate");

    assert!(registry.entries().iter().any(|entry| matches!(
        entry,
        StartupRegistryEntry::PluginLazy { declaration }
            if declaration.name == "saya-git-tools"
                && declaration.source == StartupPluginSource::Github {
                    repo: "shun/saya-git-tools".to_string(),
                    rev: Some("v0.1.0".to_string())
                }
                && declaration.commands == ["GitStatus".to_string(), "GitBlame".to_string()]
                && declaration.events == ["bufferOpen".to_string()]
                && declaration.options.as_ref()
                    .and_then(|options| options.get("trace"))
                    .and_then(|value| value.as_str()) == Some("messages")
    )));
}

#[tokio::test(flavor = "current_thread")]
async fn startup_plugins_rejects_ambiguous_source_declaration() {
    let error = collect_startup_registry(
        r#"
            saya.plugins.use([
                { local: "./plugins/a", github: "owner/repo" },
            ]);
        "#,
    )
    .await
    .expect_err("ambiguous plugin source should fail startup evaluation");

    assert!(
        error.contains("exactly one of local or github"),
        "unexpected error: {error}"
    );
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
            if (!Object.isFrozen(saya.plugins)) {
                throw new Error("startup plugins surface should be frozen");
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
        saya.options.tabstop = 4;
        saya.options.tabstop = 6;
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
async fn startup_number_is_collected() {
    let registry = collect_startup_registry(
        r#"
            saya.options.number = true;
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
async fn startup_numberwidth_is_collected() {
    let registry = collect_startup_registry(
        r#"
            saya.options.numberwidth = 6;
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
async fn startup_cmdheight_is_collected() {
    let registry = collect_startup_registry(
        r#"
            saya.options.cmdheight = 3;
        "#,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[StartupRegistryEntry::Option {
            name: StartupOptionName::MessageHeight,
            value: StartupOptionValue::Number(3),
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
async fn startup_log_level_is_collected() {
    let registry = collect_startup_registry(
        r#"
            saya.log.level = "warn";
        "#,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[StartupRegistryEntry::LogLevel {
            level: log::LevelFilter::Warn,
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
