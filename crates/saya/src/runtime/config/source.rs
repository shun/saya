//! 設定ソースの読み込み結果型。

use super::*;

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
