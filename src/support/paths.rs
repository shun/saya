use std::path::PathBuf;

const APP_DIR_NAME: &str = "saya";
const INIT_FILE_NAME: &str = "init.ts";
const RUNTIME_SUBDIR: &str = "runtime";
const BUNDLED_PLUGIN_SUBDIR: &str = "runtime/plugins/bundled";

pub fn default_init_ts_path() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join(INIT_FILE_NAME))
}

pub fn config_dir() -> Option<PathBuf> {
    env_dir("XDG_CONFIG_HOME")
        .or_else(home_dir_fallback)
        .map(|dir| {
            let app_dir = dir.join(APP_DIR_NAME);
            log::debug!(
                "[app_paths] resolved config dir: base={}, app_dir={}",
                dir.display(),
                app_dir.display()
            );
            app_dir
        })
}

pub fn cache_dir() -> Option<PathBuf> {
    env_dir("XDG_CACHE_HOME")
        .or_else(|| home_dir().map(|dir| dir.join(".cache")))
        .map(|dir| {
            let app_dir = dir.join(APP_DIR_NAME);
            log::debug!(
                "[app_paths] resolved cache dir: base={}, app_dir={}",
                dir.display(),
                app_dir.display()
            );
            app_dir
        })
}

pub fn saya_home() -> Option<PathBuf> {
    if let Some(dir) = env_dir("SAYA_HOME") {
        log::debug!("[app_paths] resolved SAYA_HOME from env: {}", dir.display());
        return Some(dir);
    }

    let candidates = saya_home_candidates();
    if let Some(existing) = candidates.iter().find(|path| path.is_dir()) {
        log::debug!(
            "[app_paths] resolved SAYA_HOME from existing candidate: {}",
            existing.display()
        );
        return Some(existing.clone());
    }

    candidates.into_iter().next().map(|fallback| {
        log::debug!(
            "[app_paths] resolved SAYA_HOME from fallback candidate: {}",
            fallback.display()
        );
        fallback
    })
}

pub fn runtime_dir() -> Option<PathBuf> {
    saya_home().map(|dir| dir.join(RUNTIME_SUBDIR))
}

pub fn bundled_plugin_dir() -> PathBuf {
    if let Some(dir) = env_dir("SAYA_HOME") {
        let bundled = dir.join(BUNDLED_PLUGIN_SUBDIR);
        log::debug!(
            "[app_paths] resolved bundled plugin dir from SAYA_HOME: {}",
            bundled.display()
        );
        return bundled;
    }

    for home in saya_home_candidates() {
        let bundled = home.join(BUNDLED_PLUGIN_SUBDIR);
        if bundled.is_dir() {
            log::debug!(
                "[app_paths] resolved bundled plugin dir from SAYA_HOME candidate: {}",
                bundled.display()
            );
            return bundled;
        }
    }

    let development = development_bundled_plugin_dir();
    log::debug!(
        "[app_paths] resolved bundled plugin dir from development fallback: {}",
        development.display()
    );
    development
}

fn env_dir(key: &str) -> Option<PathBuf> {
    let value = std::env::var_os(key)?;
    if value.is_empty() {
        log::debug!("[app_paths] ignoring empty environment variable: {}", key);
        return None;
    }
    Some(PathBuf::from(value))
}

fn home_dir() -> Option<PathBuf> {
    env_dir("HOME")
}

fn home_dir_fallback() -> Option<PathBuf> {
    home_dir().map(|dir| dir.join(".config"))
}

fn saya_home_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(dir) = executable_relative_saya_home() {
        candidates.push(dir);
    }

    if let Some(value) = option_env!("SAYA_DEFAULT_HOME").filter(|value| !value.is_empty()) {
        candidates.push(PathBuf::from(value));
    }

    if let Some(home) = home_dir() {
        candidates.push(home.join(".local").join("share").join(APP_DIR_NAME));
    }

    candidates.push(PathBuf::from("/usr/local/share").join(APP_DIR_NAME));
    candidates.push(PathBuf::from("/usr/share").join(APP_DIR_NAME));
    candidates
}

fn executable_relative_saya_home() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let bin_dir = executable.parent()?;
    let prefix = bin_dir.parent()?;
    Some(prefix.join("share").join(APP_DIR_NAME))
}

fn development_bundled_plugin_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/bundled")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::support::session_guard::test_lock;
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
        let _lock = test_lock()
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
        let _lock = test_lock()
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
        let _lock = test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _xdg_guard = EnvVarGuard::set("XDG_CACHE_HOME", Path::new("/tmp/xdg-cache"));
        let _home_guard = EnvVarGuard::set("HOME", Path::new("/tmp/home"));

        assert_eq!(cache_dir(), Some(PathBuf::from("/tmp/xdg-cache/saya")));
    }

    #[test]
    fn cache_dir_falls_back_to_home_dot_cache() {
        let _lock = test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _xdg_guard = EnvVarGuard::remove("XDG_CACHE_HOME");
        let _home_guard = EnvVarGuard::set("HOME", Path::new("/tmp/home"));

        assert_eq!(cache_dir(), Some(PathBuf::from("/tmp/home/.cache/saya")));
    }

    #[test]
    fn saya_home_prefers_environment_override() {
        let _lock = test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _saya_guard = EnvVarGuard::set("SAYA_HOME", Path::new("/tmp/saya-home"));
        let _home_guard = EnvVarGuard::set("HOME", Path::new("/tmp/home"));

        assert_eq!(saya_home(), Some(PathBuf::from("/tmp/saya-home")));
        assert_eq!(runtime_dir(), Some(PathBuf::from("/tmp/saya-home/runtime")));
    }

    #[test]
    fn bundled_plugin_dir_uses_saya_home_runtime_when_overridden() {
        let _lock = test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _saya_guard = EnvVarGuard::set("SAYA_HOME", Path::new("/tmp/saya-home"));

        assert_eq!(
            bundled_plugin_dir(),
            PathBuf::from("/tmp/saya-home/runtime/plugins/bundled")
        );
    }
}
