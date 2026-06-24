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
