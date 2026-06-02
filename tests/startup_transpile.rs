mod support;

use std::path::PathBuf;
use std::sync::Arc;

use saya::runtime::callback_registry_seed::CallbackRegistrySeed;
use saya::runtime::live::{
    BoxFuture, HostCapabilityBridge, ReadonlyBufferSnapshot, ReadonlyEditorSnapshot,
    ReadonlyWindowSnapshot, RuntimeCommandError, RuntimeMode, SayaLiveRuntime,
};
use saya::runtime::startup::{
    SayaKeyMode, SayaKeymapAction, StartupModuleLoadResult, StartupModulePrepareResult,
    StartupRegistryEntry, collect_startup_registry, load_init_module, prepare_init_module,
    resolve_init_module_specifier,
};

fn unique_path(name: &str) -> PathBuf {
    support::temp::unique_temp_path("startup-runtime", name)
}

struct EnvVarGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &std::path::Path) -> Self {
        let previous = std::env::var_os(key);
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match self.previous.as_ref() {
            Some(value) => unsafe {
                std::env::set_var(self.key, value);
            },
            None => unsafe {
                std::env::remove_var(self.key);
            },
        }
    }
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

#[test]
fn init_ts_module_transpile_rejects_unsupported_import_specifiers() {
    let current_dir = unique_path("cwd");
    std::fs::create_dir_all(&current_dir).expect("current dir");
    let config_path = current_dir.join("init.ts");
    std::fs::write(
        &config_path,
        r#"
            import { setup } from "npm:some-package";
            setup();
        "#,
    )
    .expect("config file");

    let result = prepare_init_module(&config_path, &current_dir);

    match result {
        StartupModulePrepareResult::TranspileFailed { path, message } => {
            assert_eq!(path, config_path);
            assert!(message.contains("unsupported startup import specifier"));
        }
        other => panic!("unsupported import should fail structurally, got: {other:?}"),
    }
}

#[test]
fn init_ts_module_transpile_rejects_import_cycles() {
    let current_dir = unique_path("cwd");
    std::fs::create_dir_all(&current_dir).expect("current dir");
    let config_path = current_dir.join("init.ts");
    let a_path = current_dir.join("a.ts");
    std::fs::write(&config_path, r#"import "./a.ts";"#).expect("config file");
    std::fs::write(&a_path, r#"import "./init.ts";"#).expect("a file");

    let result = prepare_init_module(&config_path, &current_dir);

    match result {
        StartupModulePrepareResult::TranspileFailed { path, message } => {
            assert_eq!(path, config_path);
            assert!(message.contains("startup module import cycle detected"));
        }
        other => panic!("import cycle should fail structurally, got: {other:?}"),
    }
}

#[test]
fn init_ts_module_transpile_uses_warm_cache_for_matching_sources() {
    let _lock = saya::app::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let current_dir = unique_path("cwd");
    let cache_home = unique_path("cache-home");
    std::fs::create_dir_all(&current_dir).expect("current dir");
    std::fs::create_dir_all(&cache_home).expect("cache dir");
    let _cache_guard = EnvVarGuard::set("XDG_CACHE_HOME", &cache_home);
    let config_path = current_dir.join("init.ts");
    std::fs::write(&config_path, "saya.options.tabstop = 4;").expect("config file");

    let first = prepare_init_module(&config_path, &current_dir);
    let StartupModulePrepareResult::Success(first_module) = first else {
        panic!("first prepare should succeed, got: {first:?}");
    };
    let cache_dir = cache_home.join("saya").join("startup-transpile");
    let js_path = std::fs::read_dir(&cache_dir)
        .expect("cache dir should exist")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .find(|path| path.extension().is_some_and(|extension| extension == "js"))
        .expect("transpiled js cache file");
    std::fs::write(&js_path, "saya.options.tabstop = 9;\n").expect("cache override");

    let second = prepare_init_module(&config_path, &current_dir);

    let StartupModulePrepareResult::Success(second_module) = second else {
        panic!("second prepare should succeed, got: {second:?}");
    };
    assert!(first_module.executable_source_text.contains("tabstop = 4"));
    assert_eq!(
        second_module.executable_source_text,
        "saya.options.tabstop = 9;\n"
    );
}

#[test]
fn init_ts_module_transpile_cache_invalidates_when_source_changes() {
    let _lock = saya::app::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let current_dir = unique_path("cwd");
    let cache_home = unique_path("cache-home");
    std::fs::create_dir_all(&current_dir).expect("current dir");
    std::fs::create_dir_all(&cache_home).expect("cache dir");
    let _cache_guard = EnvVarGuard::set("XDG_CACHE_HOME", &cache_home);
    let config_path = current_dir.join("init.ts");
    std::fs::write(&config_path, "saya.options.tabstop = 4;").expect("config file");
    let first = prepare_init_module(&config_path, &current_dir);
    assert!(matches!(first, StartupModulePrepareResult::Success(_)));
    std::fs::write(&config_path, "saya.options.tabstop = 8;").expect("config update");

    let second = prepare_init_module(&config_path, &current_dir);

    let StartupModulePrepareResult::Success(second_module) = second else {
        panic!("second prepare should succeed, got: {second:?}");
    };
    assert!(second_module.executable_source_text.contains("tabstop = 8"));
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
    assert!(
        !registry
            .entries()
            .iter()
            .any(|entry| { matches!(entry, StartupRegistryEntry::Keymap { .. }) }),
        "setupSayaDired() must not install normal-mode mappings without explicit keymap"
    );

    let seed = CallbackRegistrySeed::from_startup_registry(&registry);
    SayaLiveRuntime::spawn_from_seed(Arc::new(NoopHostBridge), seed)
        .expect("repository dired command callbacks should initialize in live runtime");
}

#[tokio::test(flavor = "current_thread")]
async fn bundled_plugins_with_no_behavior_options_do_not_register_keymaps_or_events() {
    let _lock = saya::app::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let current_dir = unique_path("explicit-config-cwd");
    std::fs::create_dir_all(&current_dir).expect("current dir");
    let config_path = current_dir.join("init.ts");
    let dired_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/saya-dired.ts");
    let lsp_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/saya-lsp-client.ts");
    let completion_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/bundled/completion/index.ts");
    std::fs::write(
        &config_path,
        format!(
            r#"
                import {{ setupSayaCompletion }} from "{completion}";
                import {{ setupSayaDired }} from "{dired}";
                import {{ setupSayaLspClient }} from "{lsp}";
                setupSayaCompletion();
                setupSayaDired();
                setupSayaLspClient();
            "#,
            completion = completion_path.display(),
            dired = dired_path.display(),
            lsp = lsp_path.display(),
        ),
    )
    .expect("config file");

    let prepared = prepare_init_module(&config_path, &current_dir);
    let StartupModulePrepareResult::Success(module) = prepared else {
        panic!("bundled plugin setup should prepare, got: {prepared:?}");
    };
    let registry = collect_startup_registry(&module.executable_source_text)
        .await
        .expect("bundled plugin setup should evaluate");

    assert!(
        !registry
            .entries()
            .iter()
            .any(|entry| matches!(entry, StartupRegistryEntry::Keymap { .. })),
        "bundled setup functions must not install keymaps without explicit options"
    );
    assert!(
        !registry
            .entries()
            .iter()
            .any(|entry| matches!(entry, StartupRegistryEntry::Event { .. })),
        "bundled setup functions must not subscribe to events without explicit options"
    );
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
        source.contains("const session = await saya.lsp.connect")
            && source.contains("session.takeNotifications()")
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
    assert!(
        source.contains("responseMethod === 'textDocument/completion' || responseMethod === 'completionItem/resolve'")
            && source.contains("return response;")
            && !source.contains("executeUiCommand('completion.floatMenu'")
            && !source.contains("await saya.completion.show"),
        "LSP completion responses should return raw results for the bundled completion source pipeline"
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
                module.executable_source_text.contains("heading:")
                    && module.executable_source_text.contains("fg: \"accent\"")
                    && module.executable_source_text.contains("bold: true"),
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
fn init_ts_module_transpile_preserves_expression_values_in_object_literals() {
    let current_dir = unique_path("cwd");
    std::fs::create_dir_all(&current_dir).expect("current dir");
    let config_path = current_dir.join("init.ts");
    std::fs::write(
        &config_path,
        r#"
            const buffer = { cursorRow: 3, cursorCol: 5 };
            const range = {
              start: {
                line: Number(buffer.cursorRow) || 0,
                character: Math.max(Number(buffer.cursorCol) || 0, 1),
              },
              end: { line: Number(buffer.cursorRow) || 0, character: 5 },
            };
            saya.options.tabstop = range.start.character;
        "#,
    )
    .expect("config file");

    let result = prepare_init_module(&config_path, &current_dir);

    match result {
        StartupModulePrepareResult::Success(module) => {
            assert!(
                module
                    .executable_source_text
                    .contains("line: Number(buffer.cursorRow) || 0"),
                "expression-valued object fields must remain executable: {}",
                module.executable_source_text
            );
            assert!(
                module
                    .executable_source_text
                    .contains("character: Math.max(Number(buffer.cursorCol) || 0, 1)")
            );
        }
        other => panic!("Success を返すこと, got: {:?}", other),
    }
}

#[test]
fn init_ts_module_transpile_preserves_compound_assignment_operators() {
    let current_dir = unique_path("cwd");
    std::fs::create_dir_all(&current_dir).expect("current dir");
    let config_path = current_dir.join("init.ts");
    std::fs::write(
        &config_path,
        r#"
            let offset = 1;
            offset += 1;
            offset -= 1;
            saya.options.tabstop = offset;
        "#,
    )
    .expect("config file");

    let result = prepare_init_module(&config_path, &current_dir);

    match result {
        StartupModulePrepareResult::Success(module) => {
            assert!(module.executable_source_text.contains("offset += 1;"));
            assert!(module.executable_source_text.contains("offset -= 1;"));
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
                    .contains("? currentPath.slice(0, -1) : currentPath.replace"),
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
fn init_ts_module_transpile_strips_multiline_type_aliases_from_imports() {
    let current_dir = unique_path("cwd");
    std::fs::create_dir_all(&current_dir).expect("current dir");
    let config_path = current_dir.join("init.ts");
    let types_path = current_dir.join("types.ts");
    std::fs::write(
        &types_path,
        r#"
            export type SourceFilter = (
                result: unknown,
                query: unknown,
            ) => unknown;

            export function setupImported() {
                saya.options.number = true;
            }
        "#,
    )
    .expect("types file");
    std::fs::write(
        &config_path,
        r#"
            import { setupImported } from "./types.ts";
            setupImported();
        "#,
    )
    .expect("config file");

    let result = prepare_init_module(&config_path, &current_dir);

    match result {
        StartupModulePrepareResult::Success(module) => {
            assert!(
                !module.executable_source_text.contains(") => unknown"),
                "multiline type alias remnants must not reach executable JS: {}",
                module.executable_source_text
            );
            assert!(
                module
                    .executable_source_text
                    .contains("function setupImported()"),
                "runtime declarations from the same import should remain"
            );
        }
        other => panic!("Success を返すこと, got: {:?}", other),
    }
}

#[test]
fn init_ts_module_transpile_strips_declare_statements_from_imports() {
    let current_dir = unique_path("cwd");
    std::fs::create_dir_all(&current_dir).expect("current dir");
    let config_path = current_dir.join("init.ts");
    let imported_path = current_dir.join("imported.ts");
    std::fs::write(
        &imported_path,
        r#"
            declare const saya: any;

            export function setupImported() {
                saya.options.number = true;
            }
        "#,
    )
    .expect("imported file");
    std::fs::write(
        &config_path,
        r#"
            import { setupImported } from "./imported.ts";
            setupImported();
        "#,
    )
    .expect("config file");

    let result = prepare_init_module(&config_path, &current_dir);

    match result {
        StartupModulePrepareResult::Success(module) => {
            assert!(
                !module.executable_source_text.contains("declare const"),
                "declare statements must not reach executable JS: {}",
                module.executable_source_text
            );
            assert!(
                module
                    .executable_source_text
                    .contains("function setupImported()"),
                "runtime declarations from the same import should remain"
            );
        }
        other => panic!("Success を返すこと, got: {:?}", other),
    }
}

#[test]
fn init_ts_module_transpile_strips_type_annotations_with_generic_commas() {
    let current_dir = unique_path("cwd");
    std::fs::create_dir_all(&current_dir).expect("current dir");
    let config_path = current_dir.join("init.ts");
    std::fs::write(
        &config_path,
        r#"
            const ranks: WeakMap<object, { distance: number }> = new WeakMap();
            const groups: Map<string, unknown[]> = new Map();
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
                    .contains("const ranks = new WeakMap();"),
                "generic type annotations with commas should be stripped cleanly: {}",
                module.executable_source_text
            );
            assert!(
                module
                    .executable_source_text
                    .contains("const groups = new Map();"),
                "generic type annotations with array values should be stripped cleanly: {}",
                module.executable_source_text
            );
        }
        other => panic!("Success を返すこと, got: {:?}", other),
    }
}

#[test]
fn init_ts_module_transpile_strips_object_return_type_annotations() {
    let current_dir = unique_path("cwd");
    std::fs::create_dir_all(&current_dir).expect("current dir");
    let config_path = current_dir.join("init.ts");
    std::fs::write(
        &config_path,
        r#"
            function currentState(): {
              enabled: boolean;
              label: string;
            } {
              return { enabled: true, label: "ready" };
            }
            saya.options.number = currentState().enabled;
        "#,
    )
    .expect("config file");

    let result = prepare_init_module(&config_path, &current_dir);

    match result {
        StartupModulePrepareResult::Success(module) => {
            assert!(
                module
                    .executable_source_text
                    .contains("function currentState() {"),
                "object return type annotations should be stripped: {}",
                module.executable_source_text
            );
            assert!(
                module.executable_source_text.contains("return {")
                    && module.executable_source_text.contains("enabled: true")
                    && module.executable_source_text.contains("label: \"ready\"")
            );
        }
        other => panic!("Success を返すこと, got: {:?}", other),
    }
}

#[test]
fn init_ts_module_transpile_inlines_shared_imports_once() {
    let current_dir = unique_path("cwd");
    std::fs::create_dir_all(current_dir.join("sources")).expect("current dir");
    let config_path = current_dir.join("init.ts");
    std::fs::write(
        current_dir.join("shared.ts"),
        r#"
            export function sharedHelper() {
              return "shared";
            }
        "#,
    )
    .expect("shared file");
    std::fs::write(
        current_dir.join("sources").join("a.ts"),
        r#"
            import { sharedHelper } from "../shared.ts";
            export function setupA() {
              saya.options.tabstop = sharedHelper().length;
            }
        "#,
    )
    .expect("source a file");
    std::fs::write(
        current_dir.join("sources").join("b.ts"),
        r#"
            import { sharedHelper } from "../shared.ts";
            export function setupB() {
              saya.options.shiftwidth = sharedHelper().length;
            }
        "#,
    )
    .expect("source b file");
    std::fs::write(
        &config_path,
        r#"
            import { setupA } from "./sources/a.ts";
            import { setupB } from "./sources/b.ts";
            setupA();
            setupB();
        "#,
    )
    .expect("config file");

    let result = prepare_init_module(&config_path, &current_dir);

    match result {
        StartupModulePrepareResult::Success(module) => {
            assert_eq!(
                module
                    .executable_source_text
                    .matches("function sharedHelper")
                    .count(),
                1,
                "shared local imports should only be inlined once: {}",
                module.executable_source_text
            );
            assert!(module.executable_source_text.contains("function setupA()"));
            assert!(module.executable_source_text.contains("function setupB()"));
        }
        other => panic!("Success を返すこと, got: {:?}", other),
    }
}

#[test]
fn init_ts_module_transpile_inlines_multiline_static_imports() {
    let current_dir = unique_path("cwd");
    std::fs::create_dir_all(&current_dir).expect("current dir");
    let config_path = current_dir.join("init.ts");
    std::fs::write(
        current_dir.join("shared.ts"),
        r#"
            export function setupImported() {
              saya.options.number = true;
            }
        "#,
    )
    .expect("shared file");
    std::fs::write(
        &config_path,
        r#"
            import {
              setupImported,
            } from "./shared.ts";
            setupImported();
        "#,
    )
    .expect("config file");

    let result = prepare_init_module(&config_path, &current_dir);

    match result {
        StartupModulePrepareResult::Success(module) => {
            assert!(
                !module.executable_source_text.contains("import {"),
                "multiline imports must not reach executable JS: {}",
                module.executable_source_text
            );
            assert!(
                module
                    .executable_source_text
                    .contains("function setupImported()")
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
