use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::app_paths::default_init_ts_path;
use crate::callback_registry_seed::CallbackRegistrySeed;
use crate::cli::{ConfigSource, InitialCursorPosition, InputSource, LaunchRequest};
use crate::config_runtime::{
    AppliedKeyMapping, CapabilityLoadResult, ConfigApplyState, ConfigKeyMode, ConfigSourceResult,
    SayaKeyMode, SayaKeymapAction, StartupRegistry, StartupRegistryEntry, apply_config_commands,
    evaluate_capability_source,
};
use crate::core_bridge::CoreBridge;
use crate::editor_session::EditorSessionState;
use crate::session_guard::{SessionGuard, SessionGuardError};
use crate::startup_runtime::{
    StartupModulePrepareResult, collect_startup_registry, prepare_init_module,
};
use crate::swapfile::SwapfileCleanupGuard;
use vim_core_rs::CoreSnapshot;

#[derive(Debug)]
pub struct BootstrapOutcome {
    pub target_path: Option<PathBuf>,
    pub loaded_config: LoadedConfig,
    pub initial_tab_size: u16,
    pub initial_line_numbers: bool,
    pub initial_number_width: u16,
    pub read_only: bool,
    pub startup_registry: StartupRegistrySnapshot,
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
}

pub fn bootstrap_warning_message(warnings: &[BootstrapWarning]) -> Option<String> {
    warnings.iter().find_map(|warning| match warning {
        BootstrapWarning::ConfigLoadFailed { path, message } => {
            let rendered = format!("設定読込に失敗しました ({}): {}", path.display(), message);
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupOptionsSnapshot {
    pub tab_size: u16,
    pub line_numbers: bool,
    pub number_width: u16,
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

#[derive(Debug, Clone)]
struct ResolvedStartupState {
    apply_state: ConfigApplyState,
    startup_registry: StartupRegistrySnapshot,
    callback_registry: CallbackRegistrySeed,
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
                line_numbers: state.line_numbers,
                number_width: normalize_number_width(state.number_width),
            },
            keymaps: state
                .key_mappings
                .iter()
                .cloned()
                .map(startup_keymap_from_applied_mapping)
                .collect(),
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
        EditorSessionState::new_with_options(
            self.target_path.clone(),
            self.initial_tab_size,
            self.initial_line_numbers,
            self.initial_number_width,
            self.read_only,
        )
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
            log::debug!(
                "[bootstrap] loading target contents before terminal enter: {}",
                target_path.display()
            );
            fs::read_to_string(target_path).map_err(|error| BootstrapError::TargetReadFailed {
                path: target_path.clone(),
                message: error.to_string(),
            })?
        }
        InputSource::Stdin => {
            log::debug!("[bootstrap] reading startup buffer contents from stdin");
            let mut initial_text = String::new();
            reader.read_to_string(&mut initial_text).map_err(|error| {
                BootstrapError::StdinReadFailed {
                    message: error.to_string(),
                }
            })?;
            initial_text
        }
        InputSource::Empty => {
            log::debug!("[bootstrap] starting with an empty buffer");
            String::new()
        }
    };

    let mut core_bridge = if let Some(target_path) = target_path.as_ref() {
        CoreBridge::new_with_target_path(target_path, &initial_text)
    } else {
        CoreBridge::new(&initial_text)
    }
    .expect("vim-core-rs session should initialize after preflight session guard acquisition");

    apply_initial_cursor(&mut core_bridge, &request.initial_cursor);
    let initial_snapshot = core_bridge.snapshot();

    if let Some(target_path) = target_path.as_ref() {
        log::debug!(
            "[bootstrap] validated target path before terminal enter: {}",
            target_path.display()
        );
    }

    let mut warnings = Vec::new();
    let loaded_config = load_config_with_fallback(request.config_source, &mut warnings);
    let bootstrap_state = resolve_bootstrap_state(&loaded_config);
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
        callback_registry: bootstrap_state.callback_registry,
        initial_snapshot,
        core_bridge,
        warnings,
        session_guard,
        _swapfile_cleanup_guard: swapfile_cleanup_guard,
    })
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
            registry, commands, ..
        } => {
            let result = apply_config_commands(&commands, &mut state);
            let startup_registry = startup_registry_from_registry(&state, &registry);
            let callback_registry = callback_registry_from_registry(&registry);
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
            };
        }
        CapabilityLoadResult::DefaultUsed => {
            let startup_registry = StartupRegistrySnapshot::from_apply_state(&state);
            let callback_registry = CallbackRegistrySeed::empty();
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
) -> Vec<crate::config_runtime::ConfigCommand> {
    registry
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            StartupRegistryEntry::Option { name, value } => {
                Some(crate::config_runtime::ConfigCommand::SetOption {
                    name: (*name).into(),
                    value: value.clone().into(),
                })
            }
            StartupRegistryEntry::Keymap { mode, lhs, action } => match action {
                SayaKeymapAction::Literal(rhs) => {
                    Some(crate::config_runtime::ConfigCommand::MapKey {
                        mode: (*mode).into(),
                        lhs: lhs.clone(),
                        rhs: rhs.clone(),
                    })
                }
                SayaKeymapAction::RegisteredCommand(_) => None,
            },
            StartupRegistryEntry::Command { .. } | StartupRegistryEntry::Event { .. } => None,
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

    StartupRegistrySnapshot {
        options: StartupOptionsSnapshot {
            tab_size: normalize_tab_size(state.tab_size),
            line_numbers: state.line_numbers,
            number_width: normalize_number_width(state.number_width),
        },
        keymaps,
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

fn map_session_guard_error(error: SessionGuardError) -> BootstrapError {
    match error {
        SessionGuardError::AlreadyInitialized => BootstrapError::SessionAlreadyInitialized,
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    use vim_core_rs::CoreMode;

    use super::startup_registry_from_registry;
    use crate::bootstrap::{
        BootstrapError, BootstrapWarning, LoadedConfig, StartupKeymapAction, StartupKeymapMode,
        StartupKeymapSnapshot, StartupRegistrySnapshot, prepare_launch,
    };
    use crate::cli::{ConfigSource, InputSource, LaunchRequest};
    use crate::config_runtime::{
        AppliedKeyMapping, ConfigApplyState, ConfigKeyMode, SayaKeyMode, SayaKeymapAction,
        StartupRegistry, StartupRegistryEntry,
    };
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
        std::fs::write(&config_path, "saya.options.tabSize = 4;\n").expect("config file");

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
                source: "saya.options.tabSize = 4;\n".to_string(),
            }
        );
        assert_eq!(outcome.initial_tab_size, 8);
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
        std::fs::write(&config_path, "saya.options.lineNumbers = true;\n").expect("config file");

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
                source: "saya.options.lineNumbers = true;\n".to_string(),
            }
        );
        assert!(outcome.initial_line_numbers);
        assert!(outcome.warnings.is_empty());

        std::fs::remove_file(&config_path).expect("cleanup config file");
        std::fs::remove_dir_all(&home_dir).expect("cleanup home dir");
    }

    #[test]
    fn startup_registry_from_apply_state_preserves_keymap_order_and_duplicates() {
        let state = ConfigApplyState {
            tab_size: 8,
            line_numbers: false,
            number_width: 4,
            key_mappings: vec![
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
            ],
        };

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
    fn extracts_initial_tab_size_from_config_file() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let config_path = unique_path("config-tab-size");
        std::fs::write(&config_path, "{ \"tabSize\": 4 }\n").expect("config file");

        let outcome = prepare_launch(LaunchRequest {
            input_source: InputSource::Empty,
            config_source: ConfigSource::File(config_path.clone()),
            ..default_request()
        })
        .expect("existing config should load");

        assert_eq!(outcome.initial_tab_size, 8);
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
