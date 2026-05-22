use std::path::PathBuf;

const APP_DIR_NAME: &str = "saya";
const INIT_FILE_NAME: &str = "init.ts";

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
}
