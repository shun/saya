use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use crate::app::cli::{ConfigSource, InitialCursorPosition, InputSource, LaunchRequest};
use crate::app::session::EditorSessionState;
use crate::core::bridge::CoreBridge;
use crate::presentation::theme::{ResolvedTheme, ThemeRegistry};
use crate::runtime::callback_registry_seed::CallbackRegistrySeed;
use crate::runtime::config::{
    AppliedKeyMapping, CapabilityLoadResult, ConfigApplyState, ConfigKeyMode, ConfigSourceResult,
    SayaKeyMode, SayaKeymapAction, StartupRegistry, StartupRegistryEntry, apply_config_commands,
    evaluate_capability_source,
};
use crate::runtime::options::{SayaOptionName, SayaOptionValue};
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
                foldmethod: state.foldmethod.clone(),
                foldlevel: normalize_u16(state.foldlevel),
            },
            keymaps: state
                .key_mappings
                .iter()
                .cloned()
                .map(startup_keymap_from_applied_mapping)
                .collect(),
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
    let target_path = target_path_from_input_source(&request.input_source);
    let initial_text = match &request.input_source {
        InputSource::File(target_path) => {
            let started_at = Instant::now();
            log::debug!(
                "[bootstrap] loading target contents before terminal enter: {}",
                target_path.display()
            );
            let text = fs::read_to_string(target_path).map_err(|error| {
                BootstrapError::TargetReadFailed {
                    path: target_path.clone(),
                    message: error.to_string(),
                }
            })?;
            log::debug!(
                "[PERF][bootstrap] target file read: path={}, bytes={}, elapsed_ms={}",
                target_path.display(),
                text.len(),
                started_at.elapsed().as_millis()
            );
            text
        }
        InputSource::Stdin => {
            let started_at = Instant::now();
            log::debug!("[bootstrap] reading startup buffer contents from stdin");
            let mut initial_text = String::new();
            reader.read_to_string(&mut initial_text).map_err(|error| {
                BootstrapError::StdinReadFailed {
                    message: error.to_string(),
                }
            })?;
            log::debug!(
                "[PERF][bootstrap] stdin read: bytes={}, elapsed_ms={}",
                initial_text.len(),
                started_at.elapsed().as_millis()
            );
            initial_text
        }
        InputSource::Empty => {
            log::debug!("[bootstrap] starting with an empty buffer");
            String::new()
        }
    };

    let core_started_at = Instant::now();
    let mut core_bridge = if let Some(target_path) = target_path.as_ref() {
        CoreBridge::new_with_target_path(target_path, &initial_text)
    } else {
        CoreBridge::new(&initial_text)
    }
    .expect("vim-core-rs session should initialize after preflight session guard acquisition");
    log::debug!(
        "[PERF][bootstrap] core bridge initialized: initial_text_len={}, elapsed_ms={}",
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
        "[bootstrap] startup preflight completed: warnings={}, target_present={}, mode={:?}, dirty={}, tab_size={}, number_width={}",
        warnings.len(),
        target_path.is_some(),
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

fn snapshot_from_light_snapshot(light: &CoreLightSnapshot, text: String) -> CoreSnapshot {
    CoreSnapshot {
        text,
        revision: light.revision,
        dirty: light.dirty,
        mode: light.mode,
        pending_input: light.pending_input.clone(),
        cursor_row: light.cursor_row,
        cursor_col: light.cursor_col,
        pending_host_actions: light.pending_host_actions,
        buffers: light.buffers.clone(),
        windows: light.windows.clone(),
        pum: light.pum.clone(),
    }
}

fn target_path_from_input_source(input_source: &InputSource) -> Option<PathBuf> {
    match input_source {
        InputSource::File(path) => Some(path.clone()),
        InputSource::Empty | InputSource::Stdin => None,
    }
}

fn apply_initial_cursor(core_bridge: &mut CoreBridge, initial_cursor: &InitialCursorPosition) {
    let command = match initial_cursor {
        InitialCursorPosition::None => return,
        InitialCursorPosition::End => "G".to_string(),
        InitialCursorPosition::Line(line_number) => format!("{line_number}G"),
    };
    log::debug!(
        "[bootstrap] applying initial cursor position: {:?} via command={}",
        initial_cursor,
        command
    );
    let _ = core_bridge.dispatch_key(&command);
}

fn load_config_with_fallback(
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

fn resolve_bootstrap_state(loaded_config: &LoadedConfig) -> ResolvedStartupState {
    let mut state = ConfigApplyState::default_state();
    match evaluate_bootstrap_capability(loaded_config) {
        CapabilityLoadResult::Success {
            path,
            registry,
            commands,
        } => {
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
            let startup_registry = StartupRegistrySnapshot::from_apply_state(&state);
            let callback_registry = CallbackRegistrySeed::empty();
            let resolved_theme = ResolvedTheme::default();
            log::debug!(
                "[bootstrap] resolved startup state from default config: tab_size={}, line_numbers={}, keymaps={}",
                state.tab_size,
                state.line_numbers,
                startup_registry.keymaps.len()
            );
            return ResolvedStartupState {
                apply_state: state,
                startup_registry,
                callback_registry,
                resolved_theme,
                warnings: Vec::new(),
            };
        }
        CapabilityLoadResult::ReadFailed { path, message } => {
            log::debug!(
                "[bootstrap] keeping default startup state because config read failed: path={}, message={}",
                path.display(),
                message
            );
        }
        CapabilityLoadResult::EvalFailed { path, message } => {
            log::debug!(
                "[bootstrap] keeping default startup state because config eval failed: path={}, message={}",
                path.display(),
                message
            );
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
        warnings: Vec::new(),
    }
}

fn evaluate_bootstrap_capability(loaded_config: &LoadedConfig) -> CapabilityLoadResult {
    match loaded_config {
        LoadedConfig::Default => CapabilityLoadResult::DefaultUsed,
        LoadedConfig::File { path, source } => {
            let source_result = ConfigSourceResult::Loaded {
                path: path.clone(),
                source: source.clone(),
            };
            evaluate_bootstrap_capability_from_path(path)
                .unwrap_or_else(|| evaluate_capability_source(&source_result))
        }
    }
}

fn evaluate_bootstrap_capability_from_path(path: &Path) -> Option<CapabilityLoadResult> {
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

fn resolve_formal_startup_runtime_path(path: &Path) -> Option<(PathBuf, PathBuf)> {
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

fn evaluate_startup_registry_on_worker(source_text: String) -> Result<StartupRegistry, String> {
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

fn config_commands_from_registry(
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
            | StartupRegistryEntry::ThemePalette { .. }
            | StartupRegistryEntry::ThemeMarkdownStyle { .. }
            | StartupRegistryEntry::ThemeUiStyle { .. }
            | StartupRegistryEntry::ThemeSyntaxStyle { .. }
            | StartupRegistryEntry::LogFile { .. }
            | StartupRegistryEntry::LogLevel { .. }
            | StartupRegistryEntry::Warning { .. } => None,
        })
        .collect()
}

fn startup_warnings_from_registry(
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

fn startup_registry_from_registry(
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
            foldmethod: state.foldmethod.clone(),
            foldlevel: normalize_u16(state.foldlevel),
        },
        keymaps,
        log,
    }
}

fn startup_log_from_registry(registry: &StartupRegistry) -> StartupLogSnapshot {
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

fn callback_registry_from_registry(registry: &StartupRegistry) -> CallbackRegistrySeed {
    CallbackRegistrySeed::from_startup_registry(registry)
}

fn startup_keymap_from_applied_mapping(mapping: AppliedKeyMapping) -> StartupKeymapSnapshot {
    StartupKeymapSnapshot {
        mode: startup_keymap_mode_from_config(mapping.mode),
        lhs: mapping.lhs,
        action: StartupKeymapAction::Literal(mapping.rhs),
    }
}

fn startup_keymap_from_registry_entry(
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

fn startup_keymap_mode_from_saya(mode: SayaKeyMode) -> StartupKeymapMode {
    match mode {
        SayaKeyMode::Normal => StartupKeymapMode::Normal,
        SayaKeyMode::Insert => StartupKeymapMode::Insert,
        SayaKeyMode::Visual => StartupKeymapMode::Visual,
    }
}

fn startup_keymap_mode_from_config(mode: ConfigKeyMode) -> StartupKeymapMode {
    match mode {
        ConfigKeyMode::Normal => StartupKeymapMode::Normal,
        ConfigKeyMode::Insert => StartupKeymapMode::Insert,
    }
}

fn startup_keymap_action_from_saya(action: SayaKeymapAction) -> StartupKeymapAction {
    match action {
        SayaKeymapAction::Literal(text) => StartupKeymapAction::Literal(text),
        SayaKeymapAction::RegisteredCommand(command) => {
            StartupKeymapAction::RegisteredCommand(command)
        }
    }
}

fn normalize_tab_size(tab_size: i64) -> u16 {
    u16::try_from(tab_size).unwrap_or(8).max(1)
}

fn normalize_number_width(number_width: i64) -> u16 {
    u16::try_from(number_width).unwrap_or(4).max(1)
}

fn normalize_message_height(message_height: i64) -> u16 {
    u16::try_from(message_height).unwrap_or(5).max(1)
}

fn normalize_u16(value: i64) -> u16 {
    u16::try_from(value.max(0)).unwrap_or(u16::MAX)
}

fn normalize_i16(value: i64) -> i16 {
    i16::try_from(value).unwrap_or(0)
}

fn normalize_u8(value: i64) -> u8 {
    u8::try_from(value.max(0)).unwrap_or(u8::MAX)
}

fn apply_startup_presentation_to_session_state(
    state: &mut EditorSessionState,
    options: &StartupOptionsSnapshot,
) {
    let presentation_options = [
        (
            SayaOptionName::RelativeNumber,
            SayaOptionValue::Boolean(options.relative_number),
        ),
        (
            SayaOptionName::CursorLine,
            SayaOptionValue::Boolean(options.cursorline),
        ),
        (
            SayaOptionName::ScrollOff,
            SayaOptionValue::Number(i64::from(options.scrolloff)),
        ),
        (
            SayaOptionName::SidescrollOff,
            SayaOptionValue::Number(i64::from(options.sidescrolloff)),
        ),
        (SayaOptionName::Wrap, SayaOptionValue::Boolean(options.wrap)),
        (
            SayaOptionName::LastStatus,
            SayaOptionValue::Number(i64::from(options.laststatus)),
        ),
        (
            SayaOptionName::MessageHeight,
            SayaOptionValue::Number(i64::from(options.message_height)),
        ),
        (SayaOptionName::List, SayaOptionValue::Boolean(options.list)),
        (
            SayaOptionName::ListChars,
            SayaOptionValue::String(options.listchars.clone()),
        ),
        (
            SayaOptionName::FoldMethod,
            SayaOptionValue::String(options.foldmethod.clone()),
        ),
        (
            SayaOptionName::FoldLevel,
            SayaOptionValue::Number(i64::from(options.foldlevel)),
        ),
    ];
    for (name, value) in presentation_options {
        let _ = state.apply_presentation_option(name, value);
    }
}

fn apply_startup_core_options(core_bridge: &mut CoreBridge, options: &StartupOptionsSnapshot) {
    let core_options = [
        (
            SayaOptionName::TabSize,
            SayaOptionValue::Number(i64::from(options.tab_size)),
        ),
        (
            SayaOptionName::ExpandTab,
            SayaOptionValue::Boolean(options.expandtab),
        ),
        (
            SayaOptionName::ShiftWidth,
            SayaOptionValue::Number(i64::from(options.shiftwidth)),
        ),
        (
            SayaOptionName::SoftTabStop,
            SayaOptionValue::Number(i64::from(options.softtabstop)),
        ),
        (
            SayaOptionName::AutoIndent,
            SayaOptionValue::Boolean(options.autoindent),
        ),
        (
            SayaOptionName::SmartIndent,
            SayaOptionValue::Boolean(options.smartindent),
        ),
        (
            SayaOptionName::IgnoreCase,
            SayaOptionValue::Boolean(options.ignorecase),
        ),
        (
            SayaOptionName::SmartCase,
            SayaOptionValue::Boolean(options.smartcase),
        ),
    ];
    for (name, value) in core_options {
        if let Err(error) = core_bridge.set_core_option(name, value) {
            log::debug!(
                "[bootstrap] startup core option application failed and was ignored: name={}, error={:?}",
                name,
                error
            );
        }
    }

    let syntax_command = if options.syntax {
        "syntax on"
    } else {
        "syntax off"
    };
    log::debug!(
        "[bootstrap] applying startup syntax option through Vim core ex command: syntax={}, command={:?}",
        options.syntax,
        syntax_command
    );
    if let Err(error) = core_bridge.apply_ex_command(syntax_command) {
        log::debug!(
            "[bootstrap] startup syntax command application failed and was ignored: command={:?}, error={:?}",
            syntax_command,
            error
        );
    }
}

fn map_session_guard_error(error: SessionGuardError) -> BootstrapError {
    match error {
        SessionGuardError::AlreadyInitialized => BootstrapError::SessionAlreadyInitialized,
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use vim_core_rs::CoreMode;

    use super::startup_registry_from_registry;
    use crate::app::bootstrap::{
        BootstrapError, BootstrapWarning, LoadedConfig, StartupKeymapAction, StartupKeymapMode,
        StartupKeymapSnapshot, StartupRegistrySnapshot, bootstrap_warning_message, prepare_launch,
    };
    use crate::app::cli::{ConfigSource, InputSource, LaunchRequest};
    use crate::runtime::config::{
        AppliedKeyMapping, ConfigApplyState, ConfigKeyMode, SayaKeyMode, SayaKeymapAction,
        StartupRegistry, StartupRegistryEntry,
    };
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

    fn default_request() -> LaunchRequest {
        LaunchRequest::default()
    }

    #[test]
    fn returns_fatal_error_for_unreadable_target_path() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let missing_path = unique_path("missing-target");

        let result = prepare_launch(LaunchRequest {
            input_source: InputSource::File(missing_path.clone()),
            config_source: ConfigSource::Default,
            ..default_request()
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
            input_source: InputSource::File(missing_path.clone()),
            config_source: ConfigSource::Default,
            ..default_request()
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
    fn returns_fatal_error_when_target_is_a_directory() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir_path = unique_path("target-is-directory");
        std::fs::create_dir_all(&dir_path).expect("create directory");

        let result = prepare_launch(LaunchRequest {
            input_source: InputSource::File(dir_path.clone()),
            config_source: ConfigSource::Default,
            ..default_request()
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
            input_source: InputSource::Empty,
            config_source: ConfigSource::File(missing_config.clone()),
            ..default_request()
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
        let missing_path = unique_path("missing-target");

        let result = prepare_launch(LaunchRequest {
            input_source: InputSource::File(missing_path),
            config_source: ConfigSource::Default,
            ..default_request()
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
            input_source: InputSource::Empty,
            config_source: ConfigSource::File(config_path.clone()),
            ..default_request()
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
    fn default_config_source_prefers_xdg_config_home_init_ts() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let xdg_config_home = unique_path("xdg-config-home");
        let config_dir = xdg_config_home.join("saya");
        let config_path = config_dir.join("init.ts");
        std::fs::create_dir_all(&config_dir).expect("xdg config directory");
        std::fs::write(&config_path, "saya.options.tabstop = 4;\n").expect("config file");

        let outcome = with_env_var_set("XDG_CONFIG_HOME", &xdg_config_home, || {
            with_env_var_removed("HOME", || {
                prepare_launch(LaunchRequest {
                    input_source: InputSource::Empty,
                    config_source: ConfigSource::Default,
                    ..default_request()
                })
                .expect("default launch should load XDG config")
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

        let outcome = with_env_var_removed("XDG_CONFIG_HOME", || {
            with_env_var_set("HOME", &home_dir, || {
                prepare_launch(LaunchRequest {
                    input_source: InputSource::Empty,
                    config_source: ConfigSource::Default,
                    ..default_request()
                })
                .expect("default launch should load HOME fallback config")
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

        let outcome = prepare_launch(LaunchRequest {
            input_source: InputSource::Empty,
            config_source: ConfigSource::File(config_path.clone()),
            ..default_request()
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
            input_source: InputSource::Empty,
            config_source: ConfigSource::Default,
            ..default_request()
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
}
