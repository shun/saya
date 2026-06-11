//! startup registry の評価と各種スナップショットへの変換を担当する。

use super::*;

pub fn collect_startup_registry_for_plugin_operation(
    config_source: ConfigSource,
) -> Result<Option<(StartupRegistry, String)>, String> {
    let mut warnings = Vec::new();
    let loaded_config = load_config_with_fallback(config_source, &mut warnings);
    let LoadedConfig::File { path, source } = loaded_config else {
        return Ok(None);
    };
    match evaluate_bootstrap_capability_from_path(&path) {
        Some(CapabilityLoadResult::Success { registry, .. }) => {
            Ok(Some((registry, source_hash_for_startup_cache(&source))))
        }
        Some(CapabilityLoadResult::ReadFailed { path, message })
        | Some(CapabilityLoadResult::EvalFailed { path, message }) => {
            Err(format!("{}: {}", path.display(), message))
        }
        Some(CapabilityLoadResult::UnsupportedCapability {
            path,
            capability,
            message,
        }) => Err(format!(
            "{}: unsupported {capability}: {message}",
            path.display()
        )),
        Some(CapabilityLoadResult::DefaultUsed) | None => Ok(None),
    }
}

pub(super) fn evaluate_bootstrap_capability(loaded_config: &LoadedConfig) -> CapabilityLoadResult {
    match loaded_config {
        LoadedConfig::Default => CapabilityLoadResult::DefaultUsed,
        LoadedConfig::File { path, source } => {
            let source_result = ConfigSourceResult::Loaded {
                path: path.clone(),
                source: source.clone(),
            };
            if source.trim_start().starts_with(['{', '[']) {
                return evaluate_capability_source(&source_result);
            }
            evaluate_bootstrap_capability_from_path(path)
                .unwrap_or_else(|| evaluate_capability_source(&source_result))
        }
    }
}

pub(super) fn evaluate_bootstrap_capability_from_path(path: &Path) -> Option<CapabilityLoadResult> {
    if path
        .extension()
        .is_some_and(|extension| extension == "json")
    {
        return None;
    }
    let (resolved_path, current_dir) = resolve_formal_startup_runtime_path(path)?;
    log::debug!(
        "[bootstrap] evaluating formal startup runtime path: config_path={}, current_dir={}",
        resolved_path.display(),
        current_dir.display()
    );

    let prepared = match prepare_init_module(&resolved_path, &current_dir) {
        StartupModulePrepareResult::Success(module) => module,
        StartupModulePrepareResult::ReadFailed { path, message } => {
            return Some(CapabilityLoadResult::ReadFailed { path, message });
        }
        StartupModulePrepareResult::TranspileFailed { path, message } => {
            return Some(CapabilityLoadResult::EvalFailed { path, message });
        }
    };

    match evaluate_startup_registry_on_worker(prepared.executable_source_text.clone()) {
        Ok(registry) => {
            let commands = config_commands_from_registry(&registry);
            log::debug!(
                "[bootstrap] formal startup runtime evaluation succeeded: path={}, registry_entries={}, commands={}",
                prepared.path.display(),
                registry.entries().len(),
                commands.len()
            );
            Some(CapabilityLoadResult::Success {
                path: prepared.path,
                registry,
                commands,
            })
        }
        Err(message) => {
            log::debug!(
                "[bootstrap] formal startup runtime evaluation failed: path={}, error={}",
                prepared.path.display(),
                message
            );
            Some(CapabilityLoadResult::EvalFailed {
                path: prepared.path,
                message,
            })
        }
    }
}

pub(super) fn resolve_formal_startup_runtime_path(path: &Path) -> Option<(PathBuf, PathBuf)> {
    let absolute_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    let current_dir = absolute_path
        .parent()
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())?;
    Some((absolute_path, current_dir))
}

pub(super) fn evaluate_startup_registry_on_worker(
    source_text: String,
) -> Result<StartupRegistry, String> {
    std::thread::Builder::new()
        .name("saya-startup-bootstrap".to_string())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("bootstrap worker should create startup runtime evaluator");
            runtime.block_on(collect_startup_registry(&source_text))
        })
        .expect("bootstrap should spawn startup runtime worker")
        .join()
        .unwrap_or_else(|panic| {
            std::panic::resume_unwind(panic);
        })
}

pub(super) fn config_commands_from_registry(
    registry: &StartupRegistry,
) -> Vec<crate::runtime::config::ConfigCommand> {
    registry
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            StartupRegistryEntry::Option { name, value } => {
                Some(crate::runtime::config::ConfigCommand::SetOption {
                    name: (*name).into(),
                    value: value.clone().into(),
                })
            }
            StartupRegistryEntry::Keymap { mode, lhs, action } => match action {
                SayaKeymapAction::Literal(rhs) => {
                    Some(crate::runtime::config::ConfigCommand::MapKey {
                        mode: (*mode).into(),
                        lhs: lhs.clone(),
                        rhs: rhs.clone(),
                    })
                }
                SayaKeymapAction::RegisteredCommand(_) => None,
            },
            StartupRegistryEntry::Command { .. }
            | StartupRegistryEntry::Event { .. }
            | StartupRegistryEntry::FtPlugin { .. }
            | StartupRegistryEntry::StatusLine { .. }
            | StartupRegistryEntry::ThemePalette { .. }
            | StartupRegistryEntry::ThemeMarkdownStyle { .. }
            | StartupRegistryEntry::ThemeUiStyle { .. }
            | StartupRegistryEntry::ThemeSyntaxStyle { .. }
            | StartupRegistryEntry::ThemeLanguageSyntaxStyle { .. }
            | StartupRegistryEntry::ThemeFilerStyle { .. }
            | StartupRegistryEntry::LogFile { .. }
            | StartupRegistryEntry::LogLevel { .. }
            | StartupRegistryEntry::PluginUse { .. }
            | StartupRegistryEntry::PluginLazy { .. }
            | StartupRegistryEntry::Warning { .. } => None,
        })
        .collect()
}

pub(super) fn startup_warnings_from_registry(
    path: &Path,
    registry: &StartupRegistry,
) -> Vec<BootstrapWarning> {
    registry
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            StartupRegistryEntry::Warning { message } => Some(BootstrapWarning::ConfigWarning {
                path: path.to_path_buf(),
                message: message.clone(),
            }),
            _ => None,
        })
        .collect()
}

pub(super) fn startup_registry_from_registry(
    state: &ConfigApplyState,
    registry: &StartupRegistry,
) -> StartupRegistrySnapshot {
    let keymaps = registry
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            StartupRegistryEntry::Keymap { mode, lhs, action } => Some(
                startup_keymap_from_registry_entry(*mode, lhs.clone(), action.clone()),
            ),
            _ => None,
        })
        .collect();
    let log = startup_log_from_registry(registry);
    let ftplugin = ftplugin_config_from_registry(registry);
    let status_line = registry
        .entries()
        .iter()
        .rev()
        .find_map(|entry| match entry {
            StartupRegistryEntry::StatusLine { config } => Some(config.clone()),
            _ => None,
        })
        .unwrap_or_default();

    StartupRegistrySnapshot {
        options: StartupOptionsSnapshot {
            tab_size: normalize_tab_size(state.tab_size),
            expandtab: state.expandtab,
            shiftwidth: normalize_tab_size(state.shiftwidth),
            softtabstop: normalize_i16(state.softtabstop),
            autoindent: state.autoindent,
            smartindent: state.smartindent,
            ignorecase: state.ignorecase,
            smartcase: state.smartcase,
            syntax: state.syntax,
            scrolloff: normalize_u16(state.scrolloff),
            sidescrolloff: normalize_u16(state.sidescrolloff),
            wrap: state.wrap,
            line_numbers: state.line_numbers,
            relative_number: state.relative_number,
            cursorline: state.cursorline,
            number_width: normalize_number_width(state.number_width),
            laststatus: normalize_u8(state.laststatus),
            message_height: normalize_message_height(state.message_height),
            list: state.list,
            listchars: state.listchars.clone(),
            mermaid_preview_auto: state.mermaid_preview_auto,
            mermaid_preview_background: state.mermaid_preview_background.clone(),
            mermaid_preview_width_percent: normalize_percent(state.mermaid_preview_width_percent),
            mermaid_preview_height_percent: normalize_percent(state.mermaid_preview_height_percent),
            foldmethod: state.foldmethod.clone(),
            foldlevel: normalize_u16(state.foldlevel),
        },
        keymaps,
        ftplugin,
        status_line,
        log,
    }
}

pub(super) fn ftplugin_config_from_registry(registry: &StartupRegistry) -> FtPluginConfig {
    let mut config = default_ftplugin_config();
    for entry in registry.entries() {
        if let StartupRegistryEntry::FtPlugin { action } = entry {
            apply_ftplugin_startup_action(&mut config, action);
        }
    }
    config
}

pub(super) fn startup_log_from_registry(registry: &StartupRegistry) -> StartupLogSnapshot {
    StartupLogSnapshot {
        log_file: registry
            .entries()
            .iter()
            .rev()
            .find_map(|entry| match entry {
                StartupRegistryEntry::LogFile { path } if !path.trim().is_empty() => {
                    Some(PathBuf::from(path))
                }
                _ => None,
            }),
        log_level: registry
            .entries()
            .iter()
            .rev()
            .find_map(|entry| match entry {
                StartupRegistryEntry::LogLevel { level } => Some(*level),
                _ => None,
            }),
    }
}

pub(super) fn callback_registry_from_registry(registry: &StartupRegistry) -> CallbackRegistrySeed {
    CallbackRegistrySeed::from_startup_registry(registry)
}

pub(super) fn startup_keymap_from_applied_mapping(
    mapping: AppliedKeyMapping,
) -> StartupKeymapSnapshot {
    StartupKeymapSnapshot {
        mode: startup_keymap_mode_from_config(mapping.mode),
        lhs: mapping.lhs,
        action: StartupKeymapAction::Literal(mapping.rhs),
    }
}

pub(super) fn startup_keymap_from_registry_entry(
    mode: SayaKeyMode,
    lhs: String,
    action: SayaKeymapAction,
) -> StartupKeymapSnapshot {
    StartupKeymapSnapshot {
        mode: startup_keymap_mode_from_saya(mode),
        lhs,
        action: startup_keymap_action_from_saya(action),
    }
}

pub(super) fn startup_keymap_mode_from_saya(mode: SayaKeyMode) -> StartupKeymapMode {
    match mode {
        SayaKeyMode::Normal => StartupKeymapMode::Normal,
        SayaKeyMode::Insert => StartupKeymapMode::Insert,
        SayaKeyMode::Visual => StartupKeymapMode::Visual,
    }
}

pub(super) fn startup_keymap_mode_from_config(mode: ConfigKeyMode) -> StartupKeymapMode {
    match mode {
        ConfigKeyMode::Normal => StartupKeymapMode::Normal,
        ConfigKeyMode::Insert => StartupKeymapMode::Insert,
    }
}

pub(super) fn startup_keymap_action_from_saya(action: SayaKeymapAction) -> StartupKeymapAction {
    match action {
        SayaKeymapAction::Literal(text) => StartupKeymapAction::Literal(text),
        SayaKeymapAction::RegisteredCommand(command) => {
            StartupKeymapAction::RegisteredCommand(command)
        }
    }
}
