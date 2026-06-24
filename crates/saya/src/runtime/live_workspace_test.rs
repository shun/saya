use std::path::PathBuf;

use super::find_workspace_root_path;
use super::live_test_support::unique_path;

#[test]
fn workspace_root_detection_returns_absolute_root_for_relative_buffer_paths() {
    let _lock = crate::app::test_support::launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root = unique_path("relative-workspace-root");
    let nested = root.join("tmp");
    std::fs::create_dir_all(&nested).expect("nested workspace dir");
    std::fs::create_dir(root.join(".git")).expect("root marker");
    std::fs::write(nested.join("main.go"), "package main\n").expect("source file");
    let expected_root = std::fs::canonicalize(&root).expect("canonical root");
    let previous_dir = std::env::current_dir().expect("current dir");
    std::env::set_current_dir(&root).expect("enter workspace root");

    let detected = find_workspace_root_path(PathBuf::from("tmp/main.go"), &[".git".into()]);

    std::env::set_current_dir(previous_dir).expect("restore current dir");
    std::fs::remove_dir_all(&root).expect("cleanup workspace root");
    assert_eq!(detected, Some(expected_root));
}
