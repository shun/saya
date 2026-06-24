/// ホスト側の永続化処理を担当するモジュール。
///
/// ファイルシステムへの書き込みと保存結果の報告を行う。
/// CoreBridge や EditorSession とは独立して、純粋な I/O 責務を持つ。
use std::path::PathBuf;

/// 保存要求。buffer 内容と対象パスを保持する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveRequest {
    pub path: PathBuf,
    pub contents: String,
}

/// 保存結果。成功または失敗を表す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveResult {
    /// 保存成功
    Saved,
    /// 保存失敗（メッセージ付き）
    Failed { message: String },
}

/// ホスト I/O の永続化処理を実行する。
pub fn write_to_path(request: &SaveRequest) -> SaveResult {
    log::debug!(
        "[host_io] write requested: path={}, contents_len={}",
        request.path.display(),
        request.contents.len()
    );

    match std::fs::write(&request.path, &request.contents) {
        Ok(()) => {
            log::debug!("[host_io] write succeeded: path={}", request.path.display());
            SaveResult::Saved
        }
        Err(error) => {
            log::debug!(
                "[host_io] write failed: path={}, error={}",
                request.path.display(),
                error
            );
            SaveResult::Failed {
                message: error.to_string(),
            }
        }
    }
}

#[cfg(test)]
#[path = "host_io_test.rs"]
mod tests;
