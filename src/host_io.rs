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
            log::debug!(
                "[host_io] write succeeded: path={}",
                request.path.display()
            );
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
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn unique_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        std::env::temp_dir().join(format!("saya-host-io-{name}-{nanos}.txt"))
    }

    // ---- タスク 5.1: 保存要求の変換テスト ----

    #[test]
    fn save_request_holds_path_and_contents() {
        let request = SaveRequest {
            path: PathBuf::from("/tmp/test.txt"),
            contents: "hello\n".to_string(),
        };

        assert_eq!(request.path, PathBuf::from("/tmp/test.txt"));
        assert_eq!(request.contents, "hello\n");
    }

    #[test]
    fn write_to_path_succeeds_for_writable_location() {
        let save_path = unique_path("write-ok");
        let request = SaveRequest {
            path: save_path.clone(),
            contents: "saved content\n".to_string(),
        };

        let result = write_to_path(&request);

        assert_eq!(result, SaveResult::Saved);
        let written = std::fs::read_to_string(&save_path).expect("read back saved file");
        assert_eq!(written, "saved content\n");

        std::fs::remove_file(save_path).expect("cleanup");
    }

    #[test]
    fn write_to_path_returns_failure_for_nonexistent_directory() {
        let bad_path = PathBuf::from("/nonexistent/dir/file.txt");
        let request = SaveRequest {
            path: bad_path,
            contents: "data".to_string(),
        };

        let result = write_to_path(&request);

        assert!(
            matches!(result, SaveResult::Failed { message } if !message.is_empty()),
            "書き込み不能パスでは Failed が返ること"
        );
    }
}
