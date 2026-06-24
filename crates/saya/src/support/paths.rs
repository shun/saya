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

/// 開発時の TypeScript プラグイン置き場（ワークスペースルートの `plugins/ts`）。
///
/// プラグインは crate から切り出してワークスペースルートに「第一級」で置く。
/// crates/saya から見て 2 つ上がワークスペースルートなので、そこからの相対で解決する
/// （`CARGO_MANIFEST_DIR` は絶対パスなので CWD には依存しない）。開発ソースの置き場を
/// 変える場合は、本番・テストともこの 1 関数だけ直せばよい。
pub fn dev_ts_plugins_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/ts")
}

fn development_bundled_plugin_dir() -> PathBuf {
    dev_ts_plugins_dir().join("bundled")
}

#[cfg(test)]
#[path = "paths_test.rs"]
mod tests;
