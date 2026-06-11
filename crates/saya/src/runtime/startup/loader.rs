//! `init.ts` の解決・読み込み・実行準備を行う loader 層。

use super::*;

/// `init.ts` を `deno_core` へ渡す前の最小解決ヘルパー。
pub fn resolve_init_module_specifier(
    specifier: &str,
    current_dir: &Path,
) -> Result<deno_core::url::Url, deno_core::anyhow::Error> {
    log::debug!(
        "[startup_runtime] resolve init module specifier: specifier={}, current_dir={}",
        specifier,
        current_dir.display()
    );
    deno_core::resolve_url_or_path(specifier, current_dir).map_err(Into::into)
}

/// `init.ts` を local file module として読み込んだ結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupModuleLoadResult {
    Success(StartupModuleSource),
    ReadFailed { path: PathBuf, message: String },
}

/// 読み込み成功時の startup module 情報。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupModuleSource {
    pub path: PathBuf,
    pub specifier: deno_core::url::Url,
    pub source_text: String,
}

/// `deno_core` に渡せる executable module の準備結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupModulePrepareResult {
    Success(PreparedStartupModule),
    ReadFailed { path: PathBuf, message: String },
    TranspileFailed { path: PathBuf, message: String },
}

/// transpile 後の startup module 情報。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedStartupModule {
    pub path: PathBuf,
    pub specifier: deno_core::url::Url,
    pub source_text: String,
    pub executable_source_text: String,
}

/// `init.ts` を読み込み、`deno_core` に渡せる local file module へ正規化する。
pub fn load_init_module(path: &Path, current_dir: &Path) -> StartupModuleLoadResult {
    log::debug!(
        "[startup_runtime] load init module: path={}, current_dir={}",
        path.display(),
        current_dir.display()
    );

    let specifier = match path.to_str() {
        Some(specifier) => match deno_core::resolve_url_or_path(specifier, current_dir) {
            Ok(specifier) => specifier,
            Err(error) => {
                log::debug!(
                    "[startup_runtime] init module specifier resolution failed: path={}, error={}",
                    path.display(),
                    error
                );
                return StartupModuleLoadResult::ReadFailed {
                    path: path.to_path_buf(),
                    message: error.to_string(),
                };
            }
        },
        None => {
            let message = "config path に有効な UTF-8 を含める必要があります".to_string();
            log::debug!(
                "[startup_runtime] init module path is not valid UTF-8: path={}",
                path.display()
            );
            return StartupModuleLoadResult::ReadFailed {
                path: path.to_path_buf(),
                message,
            };
        }
    };

    match fs::read_to_string(path) {
        Ok(source_text) => {
            log::debug!(
                "[startup_runtime] init module read success: path={}, len={}",
                path.display(),
                source_text.len()
            );
            StartupModuleLoadResult::Success(StartupModuleSource {
                path: path.to_path_buf(),
                specifier,
                source_text,
            })
        }
        Err(error) => {
            log::debug!(
                "[startup_runtime] init module read failed: path={}, error={}",
                path.display(),
                error
            );
            StartupModuleLoadResult::ReadFailed {
                path: path.to_path_buf(),
                message: error.to_string(),
            }
        }
    }
}

/// `init.ts` を runtime 評価可能な executable module へ変換する。
pub fn prepare_init_module(path: &Path, current_dir: &Path) -> StartupModulePrepareResult {
    log::debug!(
        "[startup_runtime] prepare init module: path={}, current_dir={}",
        path.display(),
        current_dir.display()
    );
    let prepare_started = Instant::now();

    let loaded = match load_init_module(path, current_dir) {
        StartupModuleLoadResult::Success(module) => module,
        StartupModuleLoadResult::ReadFailed { path, message } => {
            return StartupModulePrepareResult::ReadFailed { path, message };
        }
    };

    match transpile_typescript_module(&loaded, prepare_started) {
        Ok(executable_source_text) => {
            log::debug!(
                "[startup_runtime] init module transpile success: path={}, output_len={}",
                loaded.path.display(),
                executable_source_text.len()
            );
            StartupModulePrepareResult::Success(PreparedStartupModule {
                path: loaded.path,
                specifier: loaded.specifier,
                source_text: loaded.source_text,
                executable_source_text,
            })
        }
        Err(message) => {
            log::debug!(
                "[startup_runtime] init module transpile failed: path={}, error={}",
                loaded.path.display(),
                message
            );
            StartupModulePrepareResult::TranspileFailed {
                path: loaded.path,
                message,
            }
        }
    }
}
