//! 統合テスト: startup boot flow の検証。
//!
//! startup config の式評価と callback registry 生成が、host/application
//! 層の boot flow で成立することを確認する。

mod support;

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use saya::app::bootstrap::{StartupKeymapAction, prepare_launch};
use saya::app::cli::{ConfigSource, InputSource, LaunchRequest};
use saya::runtime::plugin::{PluginCacheRoot, PluginHost, StartupPlan, StartupPlanEntry};
use support::session::launch_serial_lock;

fn unique_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-wave6-boot-flow-{name}-{nanos}"))
}

#[test]
fn formal_boot_flow_uses_deno_core_runtime_for_expression_based_startup_config() {
    let _lock = launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("target.txt");
    let config_path = unique_path("init.ts");

    std::fs::write(&target_path, "alpha\n").expect("target file");
    std::fs::write(
        &config_path,
        r#"
            const computedTabstop = 2 + 2;
            const writeCurrent = "writeCurrent";

            saya.options.tabstop = computedTabstop;
            saya.options.number = true;
            saya.keymap.set("normal", "<leader>w", saya.commands.execute(writeCurrent));
            saya.commands.register(writeCurrent, () => {return saya.commands.execute("write");});
            saya.events.on("bufferOpen", (payload) => {if (payload.buffer.id > 0) {console.log(payload.buffer.id);}});
        "#,
    )
    .expect("config file");

    let outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::File(target_path.clone()),
        config_source: ConfigSource::File(config_path.clone()),
        ..LaunchRequest::default()
    })
    .expect("expression-based startup config should boot");

    assert_eq!(outcome.initial_tab_size, 4);
    assert!(outcome.initial_line_numbers);
    assert_eq!(outcome.startup_registry.keymaps.len(), 1);
    assert_eq!(
        outcome.startup_registry.keymaps[0].action,
        StartupKeymapAction::RegisteredCommand("writeCurrent".to_string())
    );
    assert!(
        outcome
            .callback_registry
            .commands()
            .iter()
            .any(|command| command.name() == "writeCurrent"),
        "config command should stay registered alongside bundled plugin placeholders"
    );
    assert!(
        outcome
            .callback_registry
            .events()
            .iter()
            .any(|event| event.name() == "bufferOpen"),
        "config event should stay registered alongside bundled plugin placeholders"
    );

    std::fs::remove_file(&target_path).expect("remove target");
    std::fs::remove_file(&config_path).expect("remove config");
}

#[test]
fn boot_flow_merges_cached_plugin_startup_plan_headlessly() {
    let _lock = launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let cache_root = unique_path("plugin-cache-root");
    let previous_cache_dir = std::env::var_os("SAYA_CACHE_DIR");
    unsafe {
        std::env::set_var("SAYA_CACHE_DIR", &cache_root);
    }

    let host = PluginHost::new(PluginCacheRoot::new(cache_root.clone()));
    host.write_startup_plan(&StartupPlan {
        version: StartupPlan::CURRENT_VERSION,
        source_hash: "default".to_string(),
        entries: vec![StartupPlanEntry::Command {
            name: "plugin.cached".to_string(),
            callback_source:
                "() => console.info(\"[saya-plugin-host][cache] cached command executed\")"
                    .to_string(),
        }],
    })
    .expect("startup plan should be written");

    let outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::Empty,
        config_source: ConfigSource::File(unique_path("missing-init.ts")),
        ..LaunchRequest::default()
    })
    .expect("default startup should boot with cached plugin startup plan");

    assert!(
        outcome
            .callback_registry
            .commands()
            .iter()
            .any(|command| command.name() == "plugin.cached"
                && command
                    .callback_source()
                    .contains("[saya-plugin-host][cache] cached command executed")),
        "cached plugin command should merge through bootstrap callback registry"
    );

    if let Some(value) = previous_cache_dir {
        unsafe {
            std::env::set_var("SAYA_CACHE_DIR", value);
        }
    } else {
        unsafe {
            std::env::remove_var("SAYA_CACHE_DIR");
        }
    }
    std::fs::remove_dir_all(cache_root).expect("remove cache root");
}

#[test]
fn boot_flow_uses_bundled_manifest_fallback_when_plugin_cache_is_missing() {
    let _lock = launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let cache_root = unique_path("plugin-cache-missing");
    let previous_cache_dir = std::env::var_os("SAYA_CACHE_DIR");
    unsafe {
        std::env::set_var("SAYA_CACHE_DIR", &cache_root);
    }

    let outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::Empty,
        config_source: ConfigSource::File(unique_path("missing-init.ts")),
        ..LaunchRequest::default()
    })
    .expect("default startup should boot with bundled manifest fallback");

    assert!(
        outcome
            .callback_registry
            .commands()
            .iter()
            .any(|command| command.name() == "dired.open"
                && command
                    .callback_source()
                    .contains("plugins/bundled/dired/index.ts")),
        "bundled dired placeholder should be available without plugin cache"
    );
    assert!(
        outcome
            .callback_registry
            .commands()
            .iter()
            .any(|command| command.name() == "lsp.start"),
        "bundled lsp placeholder should be available without plugin cache"
    );
    assert!(
        outcome.warnings.iter().any(|warning| {
            saya::app::bootstrap::bootstrap_warning_message(std::slice::from_ref(warning))
                .is_some_and(|message| message.contains("sync required: reason=cache_missing"))
        }),
        "external disabled path should expose sync required warning"
    );

    if let Some(value) = previous_cache_dir {
        unsafe {
            std::env::set_var("SAYA_CACHE_DIR", value);
        }
    } else {
        unsafe {
            std::env::remove_var("SAYA_CACHE_DIR");
        }
    }
    let _ = std::fs::remove_dir_all(cache_root);
}
