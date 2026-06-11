//! 設定ソースの読み込み（I/O 層）。

use super::*;

/// 設定ファイルを読み取り、文字列入力として取得する。
///
/// 起動時に設定ファイルを見つけ、文字列入力として取得する。
/// 設定未指定時は既定値扱いにする。
pub fn read_config_source(input: &ConfigInput) -> ConfigSourceResult {
    log::debug!("[config_runtime] reading config source: {:?}", input);
    match input {
        ConfigInput::None => {
            log::debug!("[config_runtime] no config input, using defaults");
            ConfigSourceResult::Default
        }
        ConfigInput::FilePath(path) => {
            log::debug!("[config_runtime] reading config file: {}", path.display());
            match std::fs::read_to_string(path) {
                Ok(source) => {
                    log::debug!(
                        "[config_runtime] config file read success: path={}, len={}",
                        path.display(),
                        source.len()
                    );
                    ConfigSourceResult::Loaded {
                        path: path.clone(),
                        source,
                    }
                }
                Err(error) => {
                    log::debug!(
                        "[config_runtime] config file read failed: path={}, error={}",
                        path.display(),
                        error
                    );
                    ConfigSourceResult::ReadFailed {
                        path: path.clone(),
                        message: error.to_string(),
                    }
                }
            }
        }
    }
}

/// 設定ファイル読み取りの結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSourceResult {
    /// 既定値を使用
    Default,
    /// ファイルから読み込み成功
    Loaded { path: PathBuf, source: String },
    /// ファイル読み込み失敗
    ReadFailed { path: PathBuf, message: String },
}
