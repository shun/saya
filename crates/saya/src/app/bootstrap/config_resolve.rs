//! 設定の読み込みフォールバックと起動状態の解決を担当する。

use super::*;

pub(super) fn load_config_with_fallback(
    config_source: ConfigSource,
    warnings: &mut Vec<BootstrapWarning>,
) -> LoadedConfig {
    match config_source {
        ConfigSource::Default => {
            let Some(path) = default_init_ts_path() else {
                log::debug!("[bootstrap] using default config source without resolved config path");
                return LoadedConfig::Default;
            };
            log::debug!(
                "[bootstrap] probing default config source before terminal enter: {}",
                path.display()
            );
            match fs::read_to_string(&path) {
                Ok(source) => LoadedConfig::File { path, source },
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    log::debug!(
                        "[bootstrap] no default config source present, continuing without config: {}",
                        path.display()
                    );
                    LoadedConfig::Default
                }
                Err(error) => {
                    log::debug!(
                        "[bootstrap] default config load failed, falling back to built-in defaults: path={}, error={}",
                        path.display(),
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

pub(super) fn resolve_bootstrap_state(loaded_config: &LoadedConfig) -> ResolvedStartupState {
    let mut state = ConfigApplyState::default_state();
    let mut fallback_warnings = Vec::new();
    match evaluate_bootstrap_capability(loaded_config) {
        CapabilityLoadResult::Success {
            path,
            mut registry,
            commands,
        } => {
            merge_plugin_startup_cache(loaded_config, &mut registry);
            let result = apply_config_commands(&commands, &mut state);
            let startup_registry = startup_registry_from_registry(&state, &registry);
            let callback_registry = callback_registry_from_registry(&registry);
            let resolved_theme = ThemeRegistry::from_startup_registry(&registry).resolve();
            let warnings = startup_warnings_from_registry(&path, &registry);
            log::debug!(
                "[bootstrap] resolved startup state from config: applied={}, errors={}, tab_size={}, line_numbers={}, keymaps={}, commands={}, events={}",
                result.applied_count,
                result.errors.len(),
                state.tab_size,
                state.line_numbers,
                startup_registry.keymaps.len(),
                callback_registry.commands().len(),
                callback_registry.events().len()
            );
            return ResolvedStartupState {
                apply_state: state,
                startup_registry,
                callback_registry,
                resolved_theme,
                warnings,
            };
        }
        CapabilityLoadResult::DefaultUsed => {
            let mut plugin_registry = StartupRegistry::default();
            merge_plugin_startup_cache(loaded_config, &mut plugin_registry);
            let startup_registry = StartupRegistrySnapshot::from_apply_state(&state);
            let callback_registry = callback_registry_from_registry(&plugin_registry);
            let resolved_theme = ResolvedTheme::default();
            let warning_path =
                default_init_ts_path().unwrap_or_else(|| PathBuf::from("<default-init>"));
            let warnings = startup_warnings_from_registry(&warning_path, &plugin_registry);
            log::debug!(
                "[bootstrap] resolved startup state from default config: tab_size={}, line_numbers={}, keymaps={}, plugin_commands={}, plugin_events={}",
                state.tab_size,
                state.line_numbers,
                startup_registry.keymaps.len(),
                callback_registry.commands().len(),
                callback_registry.events().len()
            );
            return ResolvedStartupState {
                apply_state: state,
                startup_registry,
                callback_registry,
                resolved_theme,
                warnings,
            };
        }
        CapabilityLoadResult::ReadFailed { path, message } => {
            log::debug!(
                "[bootstrap] keeping default startup state because config read failed: path={}, message={}",
                path.display(),
                message
            );
            fallback_warnings.push(BootstrapWarning::ConfigLoadFailed { path, message });
        }
        CapabilityLoadResult::EvalFailed { path, message } => {
            log::debug!(
                "[bootstrap] keeping default startup state because config eval failed: path={}, message={}",
                path.display(),
                message
            );
            fallback_warnings.push(BootstrapWarning::ConfigEvalFailed { path, message });
        }
        CapabilityLoadResult::UnsupportedCapability {
            path,
            capability,
            message,
        } => {
            log::debug!(
                "[bootstrap] keeping default startup state because config uses unsupported capability: path={}, capability={}, message={}",
                path.display(),
                capability,
                message
            );
            fallback_warnings.push(BootstrapWarning::ConfigEvalFailed {
                path,
                message: format!("unsupported {capability}: {message}"),
            });
        }
    }

    let startup_registry = StartupRegistrySnapshot::from_apply_state(&state);
    let callback_registry = CallbackRegistrySeed::empty();
    let resolved_theme = ResolvedTheme::default();
    log::debug!(
        "[bootstrap] fallback startup state resolved: tab_size={}, line_numbers={}, keymaps={}, commands={}, events={}",
        state.tab_size,
        state.line_numbers,
        startup_registry.keymaps.len(),
        callback_registry.commands().len(),
        callback_registry.events().len()
    );
    ResolvedStartupState {
        apply_state: state,
        startup_registry,
        callback_registry,
        resolved_theme,
        warnings: fallback_warnings,
    }
}
