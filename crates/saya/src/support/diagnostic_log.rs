use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use log::{LevelFilter, Log, Metadata, Record, SetLoggerError};

use crate::support::paths::cache_dir;

const LOG_FILE_ENV: &str = "SAYA_LOG_FILE";
const LOG_ENABLE_ENV: &str = "SAYA_LOG";
const BUFFER_LIMIT: usize = 2000;

static LOGGER: OnceLock<FileDiagnosticLogger> = OnceLock::new();

pub fn init_from_env() -> Result<Option<PathBuf>, DiagnosticLogInitError> {
    let logger = LOGGER.get_or_init(FileDiagnosticLogger::default);
    log::set_logger(logger).map_err(DiagnosticLogInitError::SetLogger)?;
    log::set_max_level(LevelFilter::Trace);

    let level = diagnostic_log_level_from_env();
    let Some((path, locked_by_env)) = diagnostic_log_path_from_env() else {
        logger.configure_level(level);
        return Ok(None);
    };

    logger.configure_file(&path, level, locked_by_env)?;
    log::info!(
        "[diagnostic_log] file logger initialized: path={}",
        path.display()
    );

    Ok(Some(path))
}

pub fn configure_from_startup(
    log_file: Option<&Path>,
    log_level: Option<LevelFilter>,
) -> Result<Option<PathBuf>, DiagnosticLogInitError> {
    let logger = LOGGER.get_or_init(FileDiagnosticLogger::default);
    if let Some(level) = log_level {
        logger.configure_level(level);
        log::info!("[diagnostic_log] startup log level configured: level={level}");
    }
    let Some(path) = log_file else {
        return Ok(None);
    };
    let configured = logger.configure_file_from_startup(path)?;
    if let Some(path) = configured.as_ref() {
        log::info!(
            "[diagnostic_log] startup log file configured: path={}",
            path.display()
        );
    }
    Ok(configured)
}

fn open_log_file(path: &Path) -> Result<File, DiagnosticLogInitError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| DiagnosticLogInitError::CreateDir {
            path: parent.to_path_buf(),
            message: error.to_string(),
        })?;
    }

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| DiagnosticLogInitError::OpenFile {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
    Ok(file)
}

fn diagnostic_log_path_from_env() -> Option<(PathBuf, bool)> {
    let file = std::env::var_os(LOG_FILE_ENV);
    if let Some(value) = file
        && !value.is_empty()
    {
        return Some((PathBuf::from(value), true));
    }

    let enabled = std::env::var_os(LOG_ENABLE_ENV)?;
    if !diagnostic_log_enabled_value(&enabled.to_string_lossy()) {
        return None;
    }

    cache_dir().map(|dir| (dir.join("sy.log"), false))
}

fn diagnostic_log_enabled_value(value: &str) -> bool {
    !matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "" | "0" | "false" | "off" | "no"
    )
}

fn diagnostic_log_level_from_env() -> LevelFilter {
    let Ok(value) = std::env::var(LOG_ENABLE_ENV) else {
        return LevelFilter::Debug;
    };
    match value.trim().to_ascii_lowercase().as_str() {
        "error" => LevelFilter::Error,
        "warn" => LevelFilter::Warn,
        "info" => LevelFilter::Info,
        "trace" => LevelFilter::Trace,
        "debug" | "1" | "true" | "yes" | "on" => LevelFilter::Debug,
        _ => LevelFilter::Debug,
    }
}

#[derive(Debug)]
pub enum DiagnosticLogInitError {
    CreateDir { path: PathBuf, message: String },
    OpenFile { path: PathBuf, message: String },
    SetLogger(SetLoggerError),
}

impl std::fmt::Display for DiagnosticLogInitError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiagnosticLogInitError::CreateDir { path, message } => {
                write!(
                    formatter,
                    "failed to create diagnostic log directory {}: {}",
                    path.display(),
                    message
                )
            }
            DiagnosticLogInitError::OpenFile { path, message } => {
                write!(
                    formatter,
                    "failed to open diagnostic log file {}: {}",
                    path.display(),
                    message
                )
            }
            DiagnosticLogInitError::SetLogger(error) => {
                write!(formatter, "failed to install diagnostic logger: {error}")
            }
        }
    }
}

impl std::error::Error for DiagnosticLogInitError {}

#[derive(Default)]
struct FileDiagnosticLogger {
    state: Mutex<FileDiagnosticLoggerState>,
}

struct FileDiagnosticLoggerState {
    file: Option<File>,
    level: LevelFilter,
    buffer: Vec<BufferedLogLine>,
    locked_by_env: bool,
}

struct BufferedLogLine {
    level: log::Level,
    line: String,
}

impl Default for FileDiagnosticLoggerState {
    fn default() -> Self {
        Self {
            file: None,
            level: LevelFilter::Debug,
            buffer: Vec::new(),
            locked_by_env: false,
        }
    }
}

impl FileDiagnosticLogger {
    fn configure_level(&self, level: LevelFilter) {
        if let Ok(mut state) = self.state.lock() {
            state.level = level;
        }
    }

    fn configure_file(
        &self,
        path: &Path,
        level: LevelFilter,
        locked_by_env: bool,
    ) -> Result<(), DiagnosticLogInitError> {
        let mut file = open_log_file(path)?;
        if let Ok(mut state) = self.state.lock() {
            for buffered in state.buffer.drain(..) {
                if buffered.level <= level {
                    let _ = writeln!(file, "{}", buffered.line);
                }
            }
            let _ = file.flush();
            state.file = Some(file);
            state.level = level;
            state.locked_by_env = locked_by_env;
        }
        Ok(())
    }

    fn configure_file_from_startup(
        &self,
        path: &Path,
    ) -> Result<Option<PathBuf>, DiagnosticLogInitError> {
        let should_skip = self
            .state
            .lock()
            .map(|state| state.locked_by_env)
            .unwrap_or(false);
        if should_skip {
            log::info!(
                "[diagnostic_log] startup log file ignored because environment already configured the log file: path={}",
                path.display()
            );
            return Ok(None);
        }
        let level = self
            .state
            .lock()
            .map(|state| state.level)
            .unwrap_or(LevelFilter::Debug);
        self.configure_file(path, level, false)?;
        Ok(Some(path.to_path_buf()))
    }
}

impl Log for FileDiagnosticLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        self.state
            .lock()
            .map(|state| metadata.level() <= state.level)
            .unwrap_or(false)
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis().to_string())
            .unwrap_or_else(|_| "0".to_string());

        let line = format!(
            "{} [{}] {} - {}",
            timestamp,
            record.level(),
            record.target(),
            record.args()
        );

        if let Ok(mut state) = self.state.lock() {
            if let Some(file) = state.file.as_mut() {
                let _ = writeln!(file, "{line}");
                let _ = file.flush();
            } else {
                if state.buffer.len() >= BUFFER_LIMIT {
                    state.buffer.remove(0);
                }
                state.buffer.push(BufferedLogLine {
                    level: record.level(),
                    line,
                });
            }
        }
    }

    fn flush(&self) {
        if let Ok(mut state) = self.state.lock()
            && let Some(file) = state.file.as_mut()
        {
            let _ = file.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn diagnostic_log_enabled_value_accepts_boolean_like_values() {
        assert!(diagnostic_log_enabled_value("1"));
        assert!(diagnostic_log_enabled_value("debug"));
        assert!(!diagnostic_log_enabled_value("0"));
        assert!(!diagnostic_log_enabled_value("off"));
    }

    #[test]
    fn diagnostic_log_level_from_env_defaults_to_debug_for_unknown_values() {
        let _lock = env_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _guard = saya_env_guard(LOG_ENABLE_ENV, Some("verbose"));

        assert_eq!(diagnostic_log_level_from_env(), LevelFilter::Debug);
    }

    struct EnvGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    fn saya_env_guard(key: &'static str, value: Option<&str>) -> EnvGuard {
        let previous = std::env::var_os(key);
        unsafe {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        EnvGuard { key, previous }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.previous {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }
}
