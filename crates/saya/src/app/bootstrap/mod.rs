use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use std::{collections::hash_map::DefaultHasher, hash::Hash, hash::Hasher};

use crate::app::cli::{ConfigSource, InitialCursorPosition, InputSource, LaunchRequest};
use crate::app::ftplugin::{
    apply_ftplugin_startup_action, default_ftplugin_config, resolve_ftplugin_for_path,
};
use crate::app::session::{
    DirectoryBufferListingOptions, EditorSessionState, read_directory_buffer_state_with_options,
};
use crate::core::bridge::CoreBridge;
use crate::presentation::theme::{ResolvedTheme, ThemeRegistry};
use crate::runtime::callback_registry_seed::CallbackRegistrySeed;
use crate::runtime::config::{
    AppliedKeyMapping, CapabilityLoadResult, ConfigApplyState, ConfigKeyMode, ConfigSourceResult,
    FtPluginConfig, SayaKeyMode, SayaKeymapAction, StartupRegistry, StartupRegistryEntry,
    StatusLineConfig, apply_config_commands, evaluate_capability_source,
};
use crate::runtime::options::{SayaOptionName, SayaOptionValue};
use crate::runtime::plugin::{PluginHost, StartupPlanValidation};
use crate::runtime::startup::{
    StartupModulePrepareResult, collect_startup_registry, prepare_init_module,
};
use crate::support::paths::default_init_ts_path;
use crate::support::session_guard::{SessionGuard, SessionGuardError};
use crate::support::swapfile::SwapfileCleanupGuard;
use vim_core_rs::{CoreLightSnapshot, CoreSnapshot};

const INITIAL_SNAPSHOT_TEXT_INLINE_LIMIT: usize = 1024 * 1024;

#[derive(Debug)]
pub struct BootstrapOutcome {
    pub target_path: Option<PathBuf>,
    pub loaded_config: LoadedConfig,
    pub initial_tab_size: u16,
    pub initial_line_numbers: bool,
    pub initial_number_width: u16,
    pub read_only: bool,
    pub startup_registry: StartupRegistrySnapshot,
    pub resolved_theme: ResolvedTheme,
    pub callback_registry: CallbackRegistrySeed,
    pub initial_snapshot: CoreSnapshot,
    pub core_bridge: CoreBridge,
    pub warnings: Vec<BootstrapWarning>,
    pub session_guard: SessionGuard,
    _swapfile_cleanup_guard: SwapfileCleanupGuard,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadedConfig {
    Default,
    File { path: PathBuf, source: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootstrapWarning {
    ConfigLoadFailed { path: PathBuf, message: String },
    ConfigEvalFailed { path: PathBuf, message: String },
    ConfigWarning { path: PathBuf, message: String },
}

pub fn bootstrap_warning_message(warnings: &[BootstrapWarning]) -> Option<String> {
    warnings.iter().find_map(|warning| match warning {
        BootstrapWarning::ConfigLoadFailed { path, message } => {
            let rendered = format!(
                "Failed to read startup config ({}): {}",
                path.display(),
                message
            );
            log::debug!(
                "[bootstrap] projecting startup warning into host message line: {}",
                rendered
            );
            Some(rendered)
        }
        BootstrapWarning::ConfigEvalFailed { path, message } => {
            let rendered = format!(
                "Failed to evaluate startup config ({}): {}",
                path.display(),
                message
            );
            log::debug!(
                "[bootstrap] projecting startup warning into host message line: {}",
                rendered
            );
            Some(rendered)
        }
        BootstrapWarning::ConfigWarning { path, message } => {
            let rendered = format!(
                "Ignored startup config entry ({}): {}",
                path.display(),
                message
            );
            log::debug!(
                "[bootstrap] projecting startup warning into host message line: {}",
                rendered
            );
            Some(rendered)
        }
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootstrapError {
    SessionAlreadyInitialized,
    StdinReadFailed { message: String },
    TargetReadFailed { path: PathBuf, message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupRegistrySnapshot {
    pub options: StartupOptionsSnapshot,
    pub keymaps: Vec<StartupKeymapSnapshot>,
    pub ftplugin: FtPluginConfig,
    pub status_line: StatusLineConfig,
    pub log: StartupLogSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupOptionsSnapshot {
    pub tab_size: u16,
    pub expandtab: bool,
    pub shiftwidth: u16,
    pub softtabstop: i16,
    pub autoindent: bool,
    pub smartindent: bool,
    pub ignorecase: bool,
    pub smartcase: bool,
    pub syntax: bool,
    pub scrolloff: u16,
    pub sidescrolloff: u16,
    pub wrap: bool,
    pub line_numbers: bool,
    pub relative_number: bool,
    pub cursorline: bool,
    pub number_width: u16,
    pub laststatus: u8,
    pub message_height: u16,
    pub list: bool,
    pub listchars: String,
    pub mermaid_preview_auto: bool,
    pub mermaid_preview_background: String,
    pub mermaid_preview_width_percent: u16,
    pub mermaid_preview_height_percent: u16,
    pub foldmethod: String,
    pub foldlevel: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupKeymapSnapshot {
    pub mode: StartupKeymapMode,
    pub lhs: String,
    pub action: StartupKeymapAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupKeymapMode {
    Normal,
    Insert,
    Visual,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupKeymapAction {
    Literal(String),
    RegisteredCommand(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartupLogSnapshot {
    pub log_file: Option<PathBuf>,
    pub log_level: Option<log::LevelFilter>,
}

#[derive(Debug, Clone)]
struct ResolvedStartupState {
    apply_state: ConfigApplyState,
    startup_registry: StartupRegistrySnapshot,
    callback_registry: CallbackRegistrySeed,
    resolved_theme: ResolvedTheme,
    warnings: Vec<BootstrapWarning>,
}

#[derive(Debug)]
struct InitialBuffer {
    target_path: Option<PathBuf>,
    text: String,
    source: InitialBufferSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InitialBufferSource {
    Empty,
    File,
    NewFile,
    Directory,
    Stdin,
}

impl StartupRegistrySnapshot {
    pub fn from_apply_state(state: &ConfigApplyState) -> Self {
        log::debug!(
            "[bootstrap] building startup registry from apply state: tab_size={}, line_numbers={}, number_width={}, keymaps={}",
            state.tab_size,
            state.line_numbers,
            state.number_width,
            state.key_mappings.len()
        );
        Self {
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
                mermaid_preview_width_percent: normalize_percent(
                    state.mermaid_preview_width_percent,
                ),
                mermaid_preview_height_percent: normalize_percent(
                    state.mermaid_preview_height_percent,
                ),
                foldmethod: state.foldmethod.clone(),
                foldlevel: normalize_u16(state.foldlevel),
            },
            keymaps: state
                .key_mappings
                .iter()
                .cloned()
                .map(startup_keymap_from_applied_mapping)
                .collect(),
            ftplugin: default_ftplugin_config(),
            status_line: StatusLineConfig::default(),
            log: StartupLogSnapshot::default(),
        }
    }
}

impl BootstrapOutcome {
    pub fn editor_session_state(&self) -> EditorSessionState {
        log::debug!(
            "[bootstrap] materializing editor session state from startup options: tab_size={}, line_numbers={}, number_width={}",
            self.initial_tab_size,
            self.initial_line_numbers,
            self.initial_number_width
        );
        let mut state = EditorSessionState::new_with_options(
            self.target_path.clone(),
            self.initial_tab_size,
            self.initial_line_numbers,
            self.initial_number_width,
            self.read_only,
        );
        apply_startup_presentation_to_session_state(&mut state, &self.startup_registry.options);
        state.set_resolved_theme(self.resolved_theme.clone());
        state.set_status_line_config(self.startup_registry.status_line.clone());
        if let Some(ftplugin) =
            resolve_ftplugin_for_path(self.target_path.as_deref(), &self.startup_registry.ftplugin)
        {
            state.set_filetype(Some(ftplugin.filetype.to_string()));
        }
        state
    }
}

pub fn launch_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub fn prepare_launch(request: LaunchRequest) -> Result<BootstrapOutcome, BootstrapError> {
    let mut stdin = std::io::stdin().lock();
    prepare_launch_with_reader(request, &mut stdin)
}

pub fn prepare_launch_with_reader<R: Read>(
    request: LaunchRequest,
    reader: &mut R,
) -> Result<BootstrapOutcome, BootstrapError> {
    log::debug!("[bootstrap] startup preflight requested");
    let session_guard = SessionGuard::acquire().map_err(map_session_guard_error)?;
    log::debug!("[bootstrap] session guard acquired");

    let result = prepare_launch_with_guard(request, session_guard, reader);
    if let Err(error) = &result {
        log::debug!("[bootstrap] startup preflight failed: {error:?}");
    }

    result
}

fn prepare_launch_with_guard<R: Read>(
    request: LaunchRequest,
    session_guard: SessionGuard,
    reader: &mut R,
) -> Result<BootstrapOutcome, BootstrapError> {
    let initial_buffer = load_initial_buffer(&request.input_source, reader)?;
    let target_path = initial_buffer.target_path;
    let initial_text = initial_buffer.text;

    let core_started_at = Instant::now();
    let mut core_bridge = if let Some(target_path) = target_path.as_ref() {
        CoreBridge::new_with_target_path(target_path, &initial_text)
    } else {
        CoreBridge::new(&initial_text)
    }
    .expect("vim-core-rs session should initialize after preflight session guard acquisition");
    log::debug!(
        "[PERF][bootstrap] core bridge initialized: source={:?}, initial_text_len={}, elapsed_ms={}",
        initial_buffer.source,
        initial_text.len(),
        core_started_at.elapsed().as_millis()
    );

    apply_initial_cursor(&mut core_bridge, &request.initial_cursor);

    if let Some(target_path) = target_path.as_ref() {
        log::debug!(
            "[bootstrap] validated target path before terminal enter: {}",
            target_path.display()
        );
    }

    let config_started_at = Instant::now();
    let mut warnings = Vec::new();
    let loaded_config = load_config_with_fallback(request.config_source, &mut warnings);
    let bootstrap_state = resolve_bootstrap_state(&loaded_config);
    warnings.extend(bootstrap_state.warnings.clone());
    apply_startup_core_options(&mut core_bridge, &bootstrap_state.startup_registry.options);
    apply_startup_ftplugin_options(
        &mut core_bridge,
        target_path.as_deref(),
        &bootstrap_state.startup_registry.ftplugin,
    );
    log::debug!(
        "[PERF][bootstrap] config resolved and core options applied: elapsed_ms={}",
        config_started_at.elapsed().as_millis()
    );
    let snapshot_started_at = Instant::now();
    let initial_light_snapshot = core_bridge.light_snapshot();
    let initial_snapshot_text_len = initial_text.len();
    let initial_snapshot_text = if initial_snapshot_text_len == 0 {
        "\n".to_string()
    } else if initial_snapshot_text_len <= INITIAL_SNAPSHOT_TEXT_INLINE_LIMIT {
        initial_text
    } else {
        log::debug!(
            "[bootstrap] omitting large initial snapshot text from BootstrapOutcome: text_len={}, inline_limit={}",
            initial_snapshot_text_len,
            INITIAL_SNAPSHOT_TEXT_INLINE_LIMIT
        );
        String::new()
    };
    let initial_snapshot =
        snapshot_from_light_snapshot(&initial_light_snapshot, initial_snapshot_text);
    log::debug!(
        "[PERF][bootstrap] initial snapshot assembled from light snapshot: source_text_len={}, retained_text_len={}, elapsed_ms={}",
        initial_snapshot_text_len,
        initial_snapshot.text.len(),
        snapshot_started_at.elapsed().as_millis()
    );
    let initial_tab_size = bootstrap_state.startup_registry.options.tab_size;
    let initial_number_width = bootstrap_state.startup_registry.options.number_width;

    log::debug!(
        "[bootstrap] startup preflight completed: warnings={}, target_present={}, source={:?}, mode={:?}, dirty={}, tab_size={}, number_width={}",
        warnings.len(),
        target_path.is_some(),
        initial_buffer.source,
        initial_snapshot.mode,
        initial_snapshot.dirty,
        initial_tab_size,
        initial_number_width
    );
    let swapfile_cleanup_guard = SwapfileCleanupGuard::new(target_path.clone());

    Ok(BootstrapOutcome {
        target_path,
        loaded_config,
        initial_tab_size,
        initial_line_numbers: bootstrap_state.apply_state.line_numbers,
        initial_number_width,
        read_only: request.read_only,
        startup_registry: bootstrap_state.startup_registry,
        resolved_theme: bootstrap_state.resolved_theme,
        callback_registry: bootstrap_state.callback_registry,
        initial_snapshot,
        core_bridge,
        warnings,
        session_guard,
        _swapfile_cleanup_guard: swapfile_cleanup_guard,
    })
}

mod config_resolve;
mod initial_buffer;
mod plugin_cache;
mod registry;
mod startup_apply;

#[cfg(test)]
mod tests;

use config_resolve::*;
use initial_buffer::*;
use plugin_cache::*;
use registry::*;
use startup_apply::*;

pub use registry::collect_startup_registry_for_plugin_operation;
