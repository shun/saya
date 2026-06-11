use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use vim_core_rs::CoreMode;

use super::{merge_plugin_startup_cache, startup_registry_from_registry};
use crate::app::bootstrap::{
    BootstrapError, BootstrapWarning, LoadedConfig, StartupKeymapAction, StartupKeymapMode,
    StartupKeymapSnapshot, StartupRegistrySnapshot, bootstrap_warning_message, prepare_launch,
};
use crate::app::cli::{ConfigSource, InputSource, LaunchRequest};
use crate::runtime::config::{
    AppliedKeyMapping, ConfigApplyState, ConfigKeyMode, SayaKeyMode, SayaKeymapAction,
    StartupRegistry, StartupRegistryEntry,
};
use crate::runtime::plugin::{LazyIndex, LazyTarget, PluginHost};
use crate::support::session_guard::{SessionGuard, test_lock as session_test_lock};

fn unique_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-bootstrap-{name}-{nanos}"))
}

fn with_env_var_removed<T>(key: &str, f: impl FnOnce() -> T) -> T {
    let original = std::env::var_os(key);
    unsafe {
        std::env::remove_var(key);
    }
    let result = f();
    match original {
        Some(value) => unsafe {
            std::env::set_var(key, value);
        },
        None => unsafe {
            std::env::remove_var(key);
        },
    }
    result
}

fn with_env_var_set<T>(key: &str, value: &Path, f: impl FnOnce() -> T) -> T {
    let original = std::env::var_os(key);
    unsafe {
        std::env::set_var(key, value);
    }
    let result = f();
    match original {
        Some(value) => unsafe {
            std::env::set_var(key, value);
        },
        None => unsafe {
            std::env::remove_var(key);
        },
    }
    result
}

fn with_isolated_plugin_cache<T>(name: &str, f: impl FnOnce() -> T) -> T {
    let cache_root = unique_path(name);
    let host = PluginHost::new(crate::runtime::plugin::PluginCacheRoot::new(
        cache_root.clone(),
    ));
    let mut commands = BTreeMap::new();
    commands.insert(
        "__test.noop".to_string(),
        LazyTarget {
            plugin: "__test".to_string(),
            module: "__test.ts".to_string(),
            export_name: "setup".to_string(),
        },
    );
    host.write_lazy_index(&LazyIndex {
        version: LazyIndex::CURRENT_VERSION,
        commands,
        events: BTreeMap::new(),
    })
    .expect("isolated lazy plugin cache should be writable");

    let result = with_env_var_set("SAYA_CACHE_DIR", &cache_root, f);
    let _ = std::fs::remove_dir_all(cache_root);
    result
}

fn default_request() -> LaunchRequest {
    LaunchRequest::default()
}

#[test]
fn opens_nonexistent_target_as_named_empty_buffer() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let missing_path = unique_path("missing-target");

    let result = prepare_launch(LaunchRequest {
        input_source: InputSource::File(missing_path.clone()),
        config_source: ConfigSource::Default,
        ..default_request()
    });

    let outcome = result.expect("nonexistent target should open as a new named buffer");
    assert_eq!(outcome.target_path, Some(missing_path.clone()));
    assert_eq!(outcome.initial_snapshot.text, "\n");
    assert!(
        !outcome.initial_snapshot.dirty,
        "opening a new named buffer must start clean until the user edits it"
    );
    assert!(
        !missing_path.exists(),
        "startup must not create the file until the user writes the buffer"
    );
}

#[test]
fn returns_fatal_error_for_permission_denied_target() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let restricted_path = unique_path("permission-denied-target");

    // 読み取り不能ファイルを作成
    std::fs::write(&restricted_path, "restricted content").expect("create file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = std::fs::Permissions::from_mode(0o000);
        std::fs::set_permissions(&restricted_path, permissions).expect("set permissions");
    }

    let result = prepare_launch(LaunchRequest {
        input_source: InputSource::File(restricted_path.clone()),
        config_source: ConfigSource::Default,
        ..default_request()
    });

    // Unix環境では権限不足のエラーになるはず
    #[cfg(unix)]
    {
        match &result {
            Err(BootstrapError::TargetReadFailed { path, message }) => {
                assert_eq!(path, &restricted_path);
                assert!(
                    !message.is_empty(),
                    "権限不足の失敗メッセージは空でない必要がある"
                );
                log::debug!(
                    "[test] permission denied error message for display: {}",
                    message
                );
            }
            other => panic!(
                "permission denied target should return TargetReadFailed, got: {:?}",
                other
            ),
        }
    }

    // テスト後にクリーンアップ（権限を戻してから削除）
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = std::fs::Permissions::from_mode(0o644);
        let _ = std::fs::set_permissions(&restricted_path, permissions);
    }
    let _ = std::fs::remove_file(&restricted_path);
}

#[test]
fn opens_directory_target_as_initial_dired_buffer() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let dir_path = unique_path("target-is-directory");
    let nested_path = dir_path.join("src");
    let readme_path = dir_path.join("README.md");
    std::fs::create_dir_all(&nested_path).expect("create directory");
    std::fs::write(&readme_path, "hello\n").expect("create directory entry file");

    let outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::File(dir_path.clone()),
        config_source: ConfigSource::Default,
        ..default_request()
    })
    .expect("directory target should bootstrap as dired buffer");

    assert_eq!(outcome.target_path, Some(dir_path.clone()));
    assert!(
        outcome.initial_snapshot.text.contains("README.md\n"),
        "directory startup snapshot should project file entry: {:?}",
        outcome.initial_snapshot.text
    );
    assert!(
        outcome.initial_snapshot.text.contains("src/\n"),
        "directory startup snapshot should project nested directory entry: {:?}",
        outcome.initial_snapshot.text
    );
    let session_state = outcome.editor_session_state();
    let directory_buffer = session_state
        .directory_buffer()
        .expect("directory target should initialize directory buffer metadata");
    assert_eq!(directory_buffer.root_path, dir_path);
    assert_eq!(directory_buffer.display_text, outcome.initial_snapshot.text);

    let _ = std::fs::remove_dir_all(directory_buffer.root_path.clone());
}

#[test]
fn bootstrap_error_target_read_failed_contains_path_and_message_for_display() {
    // BootstrapError::TargetReadFailed が表示用のパスとメッセージを保持していることを検証
    let error = BootstrapError::TargetReadFailed {
        path: PathBuf::from("/some/missing/file.txt"),
        message: "No such file or directory (os error 2)".to_string(),
    };

    match &error {
        BootstrapError::TargetReadFailed { path, message } => {
            assert_eq!(path, &PathBuf::from("/some/missing/file.txt"));
            assert!(message.contains("os error"));
        }
        _ => unreachable!(),
    }
}

#[test]
fn falls_back_to_default_config_with_warning_when_config_cannot_be_read() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let missing_config = unique_path("missing-config");

    let outcome = with_isolated_plugin_cache("missing-config-plugin-cache", || {
        prepare_launch(LaunchRequest {
            input_source: InputSource::Empty,
            config_source: ConfigSource::File(missing_config.clone()),
            ..default_request()
        })
        .expect("config failures should not abort startup")
    });

    assert_eq!(outcome.loaded_config, LoadedConfig::Default);
    assert_eq!(
        outcome.warnings,
        vec![BootstrapWarning::ConfigLoadFailed {
            path: missing_config,
            message: "No such file or directory (os error 2)".to_string(),
        }]
    );
}

#[test]
fn bootstrap_warning_message_distinguishes_config_load_failure_from_config_warning() {
    let config_path = PathBuf::from("/tmp/init.ts");

    let load_failure = bootstrap_warning_message(&[BootstrapWarning::ConfigLoadFailed {
        path: config_path.clone(),
        message: "No such file or directory".to_string(),
    }])
    .expect("load failure warning should render");
    assert!(
        load_failure.starts_with("Failed to read startup config"),
        "config load failure should say that the config file could not be read: {load_failure}"
    );

    let eval_failure = bootstrap_warning_message(&[BootstrapWarning::ConfigEvalFailed {
        path: config_path.clone(),
        message: "Uncaught SyntaxError: Unexpected token ')'".to_string(),
    }])
    .expect("eval failure warning should render");
    assert!(
        eval_failure.starts_with("Failed to evaluate startup config"),
        "config eval failure should say that startup evaluation failed: {eval_failure}"
    );

    let partial_warning = bootstrap_warning_message(&[BootstrapWarning::ConfigWarning {
        path: config_path,
        message: "unsupported startup option: saya.options.lineNumbers".to_string(),
    }])
    .expect("partial warning should render");
    assert!(
        partial_warning.starts_with("Ignored startup config entry"),
        "startup option warning should not imply total config load failure: {partial_warning}"
    );
}

#[test]
fn releases_session_guard_when_preflight_fails() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let restricted_path = unique_path("guard-release-permission-denied-target");
    std::fs::write(&restricted_path, "restricted content").expect("create restricted file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&restricted_path, std::fs::Permissions::from_mode(0o000))
            .expect("restrict permissions");
    }

    let result = prepare_launch(LaunchRequest {
        input_source: InputSource::File(restricted_path.clone()),
        config_source: ConfigSource::Default,
        ..default_request()
    });
    assert!(result.is_err());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&restricted_path, std::fs::Permissions::from_mode(0o644))
            .expect("restore permissions");
    }
    std::fs::remove_file(&restricted_path).expect("cleanup restricted file");

    let reacquired = SessionGuard::acquire();
    assert!(
        reacquired.is_ok(),
        "session guard must be released on failure"
    );
}

#[test]
fn loads_config_file_without_warning_when_it_exists() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let config_path = unique_path("config-ok");
    std::fs::write(&config_path, "export default {};\n").expect("config file");

    let outcome = with_isolated_plugin_cache("config-ok-plugin-cache", || {
        prepare_launch(LaunchRequest {
            input_source: InputSource::Empty,
            config_source: ConfigSource::File(config_path.clone()),
            ..default_request()
        })
        .expect("existing config should load")
    });

    assert_eq!(
        outcome.loaded_config,
        LoadedConfig::File {
            path: config_path.clone(),
            source: "export default {};\n".to_string(),
        }
    );
    assert_eq!(outcome.initial_tab_size, 8);
    assert!(outcome.warnings.is_empty());

    std::fs::remove_file(config_path).expect("cleanup config file");
}

#[test]
fn default_config_source_prefers_xdg_config_home_init_ts() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let xdg_config_home = unique_path("xdg-config-home");
    let config_dir = xdg_config_home.join("saya");
    let config_path = config_dir.join("init.ts");
    std::fs::create_dir_all(&config_dir).expect("xdg config directory");
    std::fs::write(&config_path, "saya.options.tabstop = 4;\n").expect("config file");

    let outcome = with_isolated_plugin_cache("xdg-config-plugin-cache", || {
        with_env_var_set("XDG_CONFIG_HOME", &xdg_config_home, || {
            with_env_var_removed("HOME", || {
                prepare_launch(LaunchRequest {
                    input_source: InputSource::Empty,
                    config_source: ConfigSource::Default,
                    ..default_request()
                })
                .expect("default launch should load XDG config")
            })
        })
    });

    assert_eq!(
        outcome.loaded_config,
        LoadedConfig::File {
            path: config_path.clone(),
            source: "saya.options.tabstop = 4;\n".to_string(),
        }
    );
    assert_eq!(outcome.initial_tab_size, 4);
    assert!(outcome.warnings.is_empty());

    std::fs::remove_file(&config_path).expect("cleanup config file");
    std::fs::remove_dir_all(&xdg_config_home).expect("cleanup xdg config home");
}

#[test]
fn default_config_source_falls_back_to_home_dot_config_init_ts() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let home_dir = unique_path("home-dir");
    let config_dir = home_dir.join(".config").join("saya");
    let config_path = config_dir.join("init.ts");
    std::fs::create_dir_all(&config_dir).expect("home config directory");
    std::fs::write(&config_path, "saya.options.number = true;\n").expect("config file");

    let outcome = with_isolated_plugin_cache("home-config-plugin-cache", || {
        with_env_var_removed("XDG_CONFIG_HOME", || {
            with_env_var_set("HOME", &home_dir, || {
                prepare_launch(LaunchRequest {
                    input_source: InputSource::Empty,
                    config_source: ConfigSource::Default,
                    ..default_request()
                })
                .expect("default launch should load HOME fallback config")
            })
        })
    });

    assert_eq!(
        outcome.loaded_config,
        LoadedConfig::File {
            path: config_path.clone(),
            source: "saya.options.number = true;\n".to_string(),
        }
    );
    assert!(outcome.initial_line_numbers);
    assert!(outcome.warnings.is_empty());

    std::fs::remove_file(&config_path).expect("cleanup config file");
    std::fs::remove_dir_all(&home_dir).expect("cleanup home dir");
}

#[test]
fn startup_registry_from_apply_state_preserves_keymap_order_and_duplicates() {
    let mut state = ConfigApplyState::default_state();
    state.key_mappings = vec![
        AppliedKeyMapping {
            mode: ConfigKeyMode::Normal,
            lhs: "x".to_string(),
            rhs: "dd".to_string(),
        },
        AppliedKeyMapping {
            mode: ConfigKeyMode::Normal,
            lhs: "x".to_string(),
            rhs: "yy".to_string(),
        },
    ];

    let startup_registry = StartupRegistrySnapshot::from_apply_state(&state);

    assert_eq!(
        startup_registry.keymaps,
        vec![
            StartupKeymapSnapshot {
                mode: StartupKeymapMode::Normal,
                lhs: "x".to_string(),
                action: StartupKeymapAction::Literal("dd".to_string()),
            },
            StartupKeymapSnapshot {
                mode: StartupKeymapMode::Normal,
                lhs: "x".to_string(),
                action: StartupKeymapAction::Literal("yy".to_string()),
            },
        ],
        "same lhs should remain duplicated in registration order"
    );
}

#[test]
fn startup_registry_from_registry_preserves_keymap_order_and_registered_command_actions() {
    let mut registry = StartupRegistry::default();
    registry.push(StartupRegistryEntry::Keymap {
        mode: SayaKeyMode::Normal,
        lhs: "<leader>w".to_string(),
        action: SayaKeymapAction::Literal("write".to_string()),
    });
    registry.push(StartupRegistryEntry::Keymap {
        mode: SayaKeyMode::Insert,
        lhs: "<C-s>".to_string(),
        action: SayaKeymapAction::RegisteredCommand("saveBuffer".to_string()),
    });
    registry.push(StartupRegistryEntry::Keymap {
        mode: SayaKeyMode::Normal,
        lhs: "<leader>w".to_string(),
        action: SayaKeymapAction::Literal("write!".to_string()),
    });

    let state = ConfigApplyState::default_state();
    let startup_registry = startup_registry_from_registry(&state, &registry);

    assert_eq!(
        startup_registry.keymaps,
        vec![
            StartupKeymapSnapshot {
                mode: StartupKeymapMode::Normal,
                lhs: "<leader>w".to_string(),
                action: StartupKeymapAction::Literal("write".to_string()),
            },
            StartupKeymapSnapshot {
                mode: StartupKeymapMode::Insert,
                lhs: "<C-s>".to_string(),
                action: StartupKeymapAction::RegisteredCommand("saveBuffer".to_string()),
            },
            StartupKeymapSnapshot {
                mode: StartupKeymapMode::Normal,
                lhs: "<leader>w".to_string(),
                action: StartupKeymapAction::Literal("write!".to_string()),
            },
        ],
        "startup registry must preserve registration order and duplicates"
    );
}

#[test]
fn plugin_fallback_does_not_override_config_registered_lsp_command() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let cache_root = unique_path("plugin-fallback-no-override-cache");
    let mut registry = StartupRegistry::default();
    registry.push(StartupRegistryEntry::Command {
            name: "lsp.hover".to_string(),
            callback_source: "async () => { await saya.commands.execute('lsp.floatHover {\"result\":{\"contents\":\"config hover\"}}'); }".to_string(),
        });

    with_env_var_set("SAYA_CACHE_DIR", &cache_root, || {
        merge_plugin_startup_cache(
            &LoadedConfig::File {
                path: PathBuf::from("init.ts"),
                source: "setupSayaLspClient({});".to_string(),
            },
            &mut registry,
        );
    });

    let hover_commands = registry
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            StartupRegistryEntry::Command {
                name,
                callback_source,
            } if name == "lsp.hover" => Some(callback_source),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        hover_commands.len(),
        1,
        "plugin fallback must not add a duplicate lsp.hover command after user config"
    );
    assert!(
        hover_commands[0].contains("config hover"),
        "user-configured lsp.hover callback must stay active: {:?}",
        hover_commands
    );

    let _ = std::fs::remove_dir_all(cache_root);
}

#[test]
fn startup_registry_from_registry_uses_last_log_file() {
    let mut registry = StartupRegistry::default();
    registry.push(StartupRegistryEntry::LogFile {
        path: "/tmp/saya-old.log".to_string(),
    });
    registry.push(StartupRegistryEntry::LogFile {
        path: "/tmp/saya-new.log".to_string(),
    });

    let state = ConfigApplyState::default_state();
    let startup_registry = startup_registry_from_registry(&state, &registry);

    assert_eq!(
        startup_registry.log.log_file,
        Some(PathBuf::from("/tmp/saya-new.log"))
    );
}

#[test]
fn startup_registry_from_registry_uses_last_log_level() {
    let mut registry = StartupRegistry::default();
    registry.push(StartupRegistryEntry::LogLevel {
        level: log::LevelFilter::Debug,
    });
    registry.push(StartupRegistryEntry::LogLevel {
        level: log::LevelFilter::Warn,
    });

    let state = ConfigApplyState::default_state();
    let startup_registry = startup_registry_from_registry(&state, &registry);

    assert_eq!(startup_registry.log.log_level, Some(log::LevelFilter::Warn));
}

#[test]
fn extracts_initial_tab_size_from_config_file() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let config_path = unique_path("config-tab-size");
    std::fs::write(&config_path, "saya.options.tabstop = 4;\n").expect("config file");

    let outcome = with_isolated_plugin_cache("config-tab-size-plugin-cache", || {
        prepare_launch(LaunchRequest {
            input_source: InputSource::Empty,
            config_source: ConfigSource::File(config_path.clone()),
            ..default_request()
        })
        .expect("existing config should load")
    });

    assert_eq!(outcome.initial_tab_size, 4);
    assert!(outcome.warnings.is_empty());

    std::fs::remove_file(config_path).expect("cleanup config file");
}

#[test]
fn starts_new_empty_buffer_when_no_target_path_is_provided() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let outcome = with_isolated_plugin_cache("empty-buffer-plugin-cache", || {
        prepare_launch(LaunchRequest {
            input_source: InputSource::Empty,
            config_source: ConfigSource::Default,
            ..default_request()
        })
        .expect("launching without target path should succeed")
    });

    assert_eq!(outcome.target_path, None);
    assert_eq!(outcome.initial_snapshot.text, "\n");
    assert!(!outcome.initial_snapshot.dirty);
    assert_eq!(outcome.initial_snapshot.mode, CoreMode::Normal);
    assert!(outcome.warnings.is_empty());
}

#[test]
fn new_buffer_allows_later_target_path_attachment_for_save_flow() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::Empty,
        config_source: ConfigSource::Default,
        ..default_request()
    })
    .expect("launching without target path should succeed");

    let save_path = unique_path("new-buffer-save-target");
    outcome
        .core_bridge
        .attach_target_path(&save_path)
        .expect("should be able to attach save path to new buffer");

    let snapshot = outcome.core_bridge.snapshot();
    let active_buffer = snapshot
        .buffers
        .iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist");

    assert_eq!(active_buffer.name, save_path.display().to_string());
}

#[test]
fn loads_existing_target_file_into_live_core_session() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("target file ok");
    let target_text = "hello saya\nsecond line\n";
    std::fs::write(&target_path, target_text).expect("target file");

    let outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::File(target_path.clone()),
        config_source: ConfigSource::Default,
        ..default_request()
    })
    .expect("existing target should load");

    let snapshot = outcome.core_bridge.snapshot();

    assert_eq!(outcome.target_path, Some(target_path.clone()));
    assert_eq!(snapshot.text, target_text);
    assert_eq!(outcome.initial_snapshot.text, target_text);
    assert_eq!(outcome.initial_snapshot.mode, CoreMode::Normal);
    assert!(!outcome.initial_snapshot.dirty);
    assert_eq!(outcome.initial_snapshot.text, snapshot.text);
    assert_eq!(
        outcome
            .initial_snapshot
            .buffers
            .iter()
            .find(|buffer| buffer.is_active)
            .expect("active buffer should exist")
            .name,
        target_path.display().to_string()
    );

    std::fs::remove_file(target_path).expect("cleanup target file");
}
