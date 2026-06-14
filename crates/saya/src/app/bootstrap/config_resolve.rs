//! 設定の読み込みフォールバックと起動状態の解決を担当する。

use super::*;

/// `ConfigSource::Default` を解決するときの設定ディレクトリ基点。
///
/// `Injected` を渡したテストは実ホームを読まずに `init.ts` の有無を制御できる。
/// `EnvResolved` は本番経路で、`default_init_ts_path()`（実ホーム解決）を使う。
pub(super) enum DefaultConfigBase<'a> {
    EnvResolved,
    Injected(&'a Path),
}

#[cfg(test)]
pub(super) mod hermetic_guard {
    use std::cell::Cell;

    thread_local! {
        /// このスレッドで「ホーム解決経路を踏んだら隔離漏れとして失敗させる」ガードが
        /// 有効かどうか。bootstrap の密閉テストだけがこれを有効化し、`ConfigSource::Default`
        /// が実ホーム解決へ落ちた瞬間にパニックさせる。main.rs などスコープ外のテストには影響しない。
        static ACTIVE: Cell<bool> = const { Cell::new(false) };
    }

    /// ガードを有効化し、Drop で元の状態へ戻す RAII ハンドル。
    pub(in crate::app::bootstrap) struct GuardScope {
        previous: bool,
    }

    impl GuardScope {
        pub(in crate::app::bootstrap) fn activate() -> Self {
            let previous = ACTIVE.with(|active| active.replace(true));
            Self { previous }
        }
    }

    impl Drop for GuardScope {
        fn drop(&mut self) {
            let previous = self.previous;
            ACTIVE.with(|active| active.set(previous));
        }
    }

    pub(super) fn is_active() -> bool {
        ACTIVE.with(|active| active.get())
    }
}

fn resolve_default_init_ts_path(base: DefaultConfigBase<'_>) -> Option<PathBuf> {
    match base {
        DefaultConfigBase::Injected(dir) => {
            let path = dir.join("saya").join("init.ts");
            log::debug!(
                "[bootstrap] resolving ConfigSource::Default from injected config dir (hermetic): {}",
                path.display()
            );
            Some(path)
        }
        DefaultConfigBase::EnvResolved => {
            // 番人: 密閉テストのスコープ内で実ホーム解決経路を踏んだら、隔離漏れとして即失敗させる。
            // 本番ビルドおよびスコープ外のテストには影響しない。
            #[cfg(test)]
            if hermetic_guard::is_active() {
                panic!(
                    "[bootstrap][hermetic-guard] ConfigSource::Default reached env-based home resolution \
                     inside a hermetic bootstrap test. Inject `default_config_dir` (e.g. via \
                     hermetic_request()/hermetic_default_config_request()) so tests never read the real \
                     user's ~/.config/saya/init.ts."
                );
            }
            let path = default_init_ts_path();
            log::debug!(
                "[bootstrap] resolving ConfigSource::Default from environment (real home): {:?}",
                path.as_ref().map(|p| p.display())
            );
            path
        }
    }
}

pub(super) fn load_config_with_fallback(
    config_source: ConfigSource,
    default_config_base: DefaultConfigBase<'_>,
    warnings: &mut Vec<BootstrapWarning>,
) -> LoadedConfig {
    match config_source {
        ConfigSource::Default => {
            let Some(path) = resolve_default_init_ts_path(default_config_base) else {
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

pub(super) fn resolve_bootstrap_state(
    loaded_config: &LoadedConfig,
    plugin_cache_root: Option<&PluginCacheRoot>,
) -> ResolvedStartupState {
    let mut state = ConfigApplyState::default_state();
    let mut fallback_warnings = Vec::new();
    match evaluate_bootstrap_capability(loaded_config) {
        CapabilityLoadResult::Success {
            path,
            mut registry,
            commands,
        } => {
            merge_plugin_startup_cache(loaded_config, plugin_cache_root, &mut registry);
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
            merge_plugin_startup_cache(loaded_config, plugin_cache_root, &mut plugin_registry);
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
