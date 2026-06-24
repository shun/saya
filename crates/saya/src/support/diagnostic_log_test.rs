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
