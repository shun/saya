use std::fs;
use std::path::PathBuf;

use crate::cli::{ConfigSource, LaunchRequest};
use crate::config_runtime::{
    ConfigApplyState, ConfigLoadResult, ConfigSourceResult, apply_config_commands, evaluate_config,
};
use crate::core_bridge::CoreBridge;
use crate::session_guard::{SessionGuard, SessionGuardError};
use vim_core_rs::CoreSnapshot;

#[derive(Debug)]
pub struct BootstrapOutcome {
    pub target_path: Option<PathBuf>,
    pub loaded_config: LoadedConfig,
    pub initial_tab_size: u16,
    pub initial_snapshot: CoreSnapshot,
    pub core_bridge: CoreBridge,
    pub warnings: Vec<BootstrapWarning>,
    pub session_guard: SessionGuard,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadedConfig {
    Default,
    File { path: PathBuf, source: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootstrapWarning {
    ConfigLoadFailed { path: PathBuf, message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootstrapError {
    SessionAlreadyInitialized,
    TargetReadFailed { path: PathBuf, message: String },
}

pub fn prepare_launch(request: LaunchRequest) -> Result<BootstrapOutcome, BootstrapError> {
    log::debug!("[bootstrap] startup preflight requested");
    let session_guard = SessionGuard::acquire().map_err(map_session_guard_error)?;
    log::debug!("[bootstrap] session guard acquired");

    let result = prepare_launch_with_guard(request, session_guard);
    if let Err(error) = &result {
        log::debug!("[bootstrap] startup preflight failed: {error:?}");
    }

    result
}

fn prepare_launch_with_guard(
    request: LaunchRequest,
    session_guard: SessionGuard,
) -> Result<BootstrapOutcome, BootstrapError> {
    let initial_text = if let Some(target_path) = request.target_path.as_ref() {
        log::debug!(
            "[bootstrap] loading target contents before terminal enter: {}",
            target_path.display()
        );
        fs::read_to_string(target_path).map_err(|error| BootstrapError::TargetReadFailed {
            path: target_path.clone(),
            message: error.to_string(),
        })?
    } else {
        log::debug!("[bootstrap] starting with an empty buffer");
        String::new()
    };

    let core_bridge = if let Some(target_path) = request.target_path.as_ref() {
        CoreBridge::new_with_target_path(target_path, &initial_text)
    } else {
        CoreBridge::new(&initial_text)
    }
    .expect("vim-core-rs session should initialize after preflight session guard acquisition");
    let initial_snapshot = core_bridge.snapshot();

    if let Some(target_path) = request.target_path.as_ref() {
        log::debug!(
            "[bootstrap] validated target path before terminal enter: {}",
            target_path.display()
        );
    }

    let mut warnings = Vec::new();
    let loaded_config = load_config_with_fallback(request.config_source, &mut warnings);
    let initial_tab_size = resolve_initial_tab_size(&loaded_config);

    log::debug!(
        "[bootstrap] startup preflight completed: warnings={}, target_present={}, mode={:?}, dirty={}, tab_size={}",
        warnings.len(),
        request.target_path.is_some(),
        initial_snapshot.mode,
        initial_snapshot.dirty,
        initial_tab_size
    );

    Ok(BootstrapOutcome {
        target_path: request.target_path,
        loaded_config,
        initial_tab_size,
        initial_snapshot,
        core_bridge,
        warnings,
        session_guard,
    })
}

fn load_config_with_fallback(
    config_source: ConfigSource,
    warnings: &mut Vec<BootstrapWarning>,
) -> LoadedConfig {
    match config_source {
        ConfigSource::Default => {
            log::debug!("[bootstrap] using default config source");
            LoadedConfig::Default
        }
        ConfigSource::File(path) => {
            log::debug!(
                "[bootstrap] loading config source before terminal enter: {}",
                path.display()
            );
            match fs::read_to_string(&path) {
                Ok(source) => LoadedConfig::File { path, source },
                Err(error) => {
                    log::debug!(
                        "[bootstrap] config load failed, falling back to default: {}",
                        error
                    );
                    warnings.push(BootstrapWarning::ConfigLoadFailed {
                        path,
                        message: error.to_string(),
                    });
                    LoadedConfig::Default
                }
            }
        }
    }
}

fn resolve_initial_tab_size(loaded_config: &LoadedConfig) -> u16 {
    let mut state = ConfigApplyState::default_state();
    let source_result = match loaded_config {
        LoadedConfig::Default => ConfigSourceResult::Default,
        LoadedConfig::File { path, source } => ConfigSourceResult::Loaded {
            path: path.clone(),
            source: source.clone(),
        },
    };

    match evaluate_config(&source_result) {
        ConfigLoadResult::Success { commands } => {
            let result = apply_config_commands(&commands, &mut state);
            log::debug!(
                "[bootstrap] resolved initial tab size from config: applied={}, errors={}, tab_size={}",
                result.applied_count,
                result.errors.len(),
                state.tab_size
            );
        }
        ConfigLoadResult::DefaultUsed => {
            log::debug!(
                "[bootstrap] resolved initial tab size from default config: {}",
                state.tab_size
            );
        }
        ConfigLoadResult::ReadFailed { path, message } => {
            log::debug!(
                "[bootstrap] keeping default tab size because config read failed: path={}, message={}",
                path.display(),
                message
            );
        }
        ConfigLoadResult::EvalFailed { path, message } => {
            log::debug!(
                "[bootstrap] keeping default tab size because config eval failed: path={}, message={}",
                path.display(),
                message
            );
        }
    }

    u16::try_from(state.tab_size).unwrap_or(8).max(1)
}

fn map_session_guard_error(error: SessionGuardError) -> BootstrapError {
    match error {
        SessionGuardError::AlreadyInitialized => BootstrapError::SessionAlreadyInitialized,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    use vim_core_rs::CoreMode;

    use crate::bootstrap::{BootstrapError, BootstrapWarning, LoadedConfig, prepare_launch};
    use crate::cli::{ConfigSource, LaunchRequest};
    use crate::session_guard::SessionGuard;

    fn session_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn unique_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        std::env::temp_dir().join(format!("saya-bootstrap-{name}-{nanos}"))
    }

    #[test]
    fn returns_fatal_error_for_unreadable_target_path() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let missing_path = unique_path("missing-target");

        let result = prepare_launch(LaunchRequest {
            target_path: Some(missing_path.clone()),
            config_source: ConfigSource::Default,
        });

        assert!(matches!(
            result,
            Err(BootstrapError::TargetReadFailed { path, .. }) if path == missing_path
        ));
    }

    #[test]
    fn returns_fatal_error_with_readable_message_for_nonexistent_target() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let missing_path = unique_path("nonexistent-readable-msg");

        let result = prepare_launch(LaunchRequest {
            target_path: Some(missing_path.clone()),
            config_source: ConfigSource::Default,
        });

        match result {
            Err(BootstrapError::TargetReadFailed { path, message }) => {
                assert_eq!(path, missing_path);
                assert!(!message.is_empty(), "失敗メッセージは空でない必要がある");
                log::debug!(
                    "[test] nonexistent target error message for display: {}",
                    message
                );
            }
            other => panic!(
                "nonexistent target should return TargetReadFailed, got: {:?}",
                other
            ),
        }
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
            target_path: Some(restricted_path.clone()),
            config_source: ConfigSource::Default,
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
    fn returns_fatal_error_when_target_is_a_directory() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir_path = unique_path("target-is-directory");
        std::fs::create_dir_all(&dir_path).expect("create directory");

        let result = prepare_launch(LaunchRequest {
            target_path: Some(dir_path.clone()),
            config_source: ConfigSource::Default,
        });

        match result {
            Err(BootstrapError::TargetReadFailed { path, message }) => {
                assert_eq!(path, dir_path.clone());
                assert!(
                    !message.is_empty(),
                    "ディレクトリ読み込み失敗メッセージは空でない必要がある"
                );
                log::debug!(
                    "[test] directory target error message for display: {}",
                    message
                );
            }
            other => panic!(
                "directory target should return TargetReadFailed, got: {:?}",
                other
            ),
        }

        let _ = std::fs::remove_dir(&dir_path);
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

        let outcome = prepare_launch(LaunchRequest {
            target_path: None,
            config_source: ConfigSource::File(missing_config.clone()),
        })
        .expect("config failures should not abort startup");

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
    fn releases_session_guard_when_preflight_fails() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let missing_path = unique_path("missing-target");

        let result = prepare_launch(LaunchRequest {
            target_path: Some(missing_path),
            config_source: ConfigSource::Default,
        });
        assert!(result.is_err());

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

        let outcome = prepare_launch(LaunchRequest {
            target_path: None,
            config_source: ConfigSource::File(config_path.clone()),
        })
        .expect("existing config should load");

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
    fn extracts_initial_tab_size_from_config_file() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let config_path = unique_path("config-tab-size");
        std::fs::write(&config_path, "{ \"tabSize\": 4 }\n").expect("config file");

        let outcome = prepare_launch(LaunchRequest {
            target_path: None,
            config_source: ConfigSource::File(config_path.clone()),
        })
        .expect("existing config should load");

        assert_eq!(outcome.initial_tab_size, 4);
        assert!(outcome.warnings.is_empty());

        std::fs::remove_file(config_path).expect("cleanup config file");
    }

    #[test]
    fn starts_new_empty_buffer_when_no_target_path_is_provided() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let outcome = prepare_launch(LaunchRequest {
            target_path: None,
            config_source: ConfigSource::Default,
        })
        .expect("launching without target path should succeed");

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
            target_path: None,
            config_source: ConfigSource::Default,
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
            target_path: Some(target_path.clone()),
            config_source: ConfigSource::Default,
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
}
