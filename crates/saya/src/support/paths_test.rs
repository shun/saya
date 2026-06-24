use super::*;
use crate::app::test_support::launch_serial_lock;
use std::ffi::OsString;
use std::path::Path;

struct EnvVarGuard {
    key: &'static str,
    previous: Option<OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &Path) -> Self {
        let previous = std::env::var_os(key);
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, previous }
    }

    fn remove(key: &'static str) -> Self {
        let previous = std::env::var_os(key);
        unsafe {
            std::env::remove_var(key);
        }
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match self.previous.as_ref() {
            Some(value) => unsafe {
                std::env::set_var(self.key, value);
            },
            None => unsafe {
                std::env::remove_var(self.key);
            },
        }
    }
}

#[test]
fn default_init_ts_path_prefers_xdg_config_home() {
    let _lock = launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _xdg_guard = EnvVarGuard::set("XDG_CONFIG_HOME", Path::new("/tmp/xdg-config"));
    let _home_guard = EnvVarGuard::set("HOME", Path::new("/tmp/home"));

    assert_eq!(
        default_init_ts_path(),
        Some(PathBuf::from("/tmp/xdg-config/saya/init.ts"))
    );
}

#[test]
fn default_init_ts_path_falls_back_to_home_dot_config() {
    let _lock = launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _xdg_guard = EnvVarGuard::remove("XDG_CONFIG_HOME");
    let _home_guard = EnvVarGuard::set("HOME", Path::new("/tmp/home"));

    assert_eq!(
        default_init_ts_path(),
        Some(PathBuf::from("/tmp/home/.config/saya/init.ts"))
    );
}

#[test]
fn cache_dir_prefers_xdg_cache_home() {
    let _lock = launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _xdg_guard = EnvVarGuard::set("XDG_CACHE_HOME", Path::new("/tmp/xdg-cache"));
    let _home_guard = EnvVarGuard::set("HOME", Path::new("/tmp/home"));

    assert_eq!(cache_dir(), Some(PathBuf::from("/tmp/xdg-cache/saya")));
}

#[test]
fn cache_dir_falls_back_to_home_dot_cache() {
    let _lock = launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _xdg_guard = EnvVarGuard::remove("XDG_CACHE_HOME");
    let _home_guard = EnvVarGuard::set("HOME", Path::new("/tmp/home"));

    assert_eq!(cache_dir(), Some(PathBuf::from("/tmp/home/.cache/saya")));
}

#[test]
fn saya_home_prefers_environment_override() {
    let _lock = launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _saya_guard = EnvVarGuard::set("SAYA_HOME", Path::new("/tmp/saya-home"));
    let _home_guard = EnvVarGuard::set("HOME", Path::new("/tmp/home"));

    assert_eq!(saya_home(), Some(PathBuf::from("/tmp/saya-home")));
    assert_eq!(runtime_dir(), Some(PathBuf::from("/tmp/saya-home/runtime")));
}

#[test]
fn bundled_plugin_dir_uses_saya_home_runtime_when_overridden() {
    let _lock = launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _saya_guard = EnvVarGuard::set("SAYA_HOME", Path::new("/tmp/saya-home"));

    assert_eq!(
        bundled_plugin_dir(),
        PathBuf::from("/tmp/saya-home/runtime/plugins/bundled")
    );
}
