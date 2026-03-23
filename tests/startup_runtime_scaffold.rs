use std::path::PathBuf;

use saya::startup_runtime::{
    SayaKeyMode, SayaKeymapAction, StartupModuleLoadResult, StartupModulePrepareResult,
    StartupOptionName, StartupOptionValue, StartupRegistryEntry, collect_startup_registry,
    evaluate_startup_module, load_init_module, prepare_init_module, resolve_init_module_specifier,
};

fn unique_path(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-startup-runtime-{name}-{nanos}"))
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
async fn startup_option_aliases_are_normalized_to_formal_names() {
    let registry = collect_startup_registry(
        r#"
            saya.options.tabstop = 2;
            saya.options.number = true;
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
            callback_source: "() => { console.log(\"write\"); }".to_string(),
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
            callback_source: "(payload) => { console.log(payload); }".to_string(),
        }]
    );
}
