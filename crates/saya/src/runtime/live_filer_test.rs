use std::path::PathBuf;

use super::live_test_support::unique_path;
use super::{
    RuntimeFilerError, RuntimeFilerErrorKind, RuntimeFilerOperationKind, runtime_filer_io_error,
};

#[test]
fn filer_io_error_maps_permission_denied_to_structured_error_kind() {
    let path = PathBuf::from("/tmp/permission-denied.txt");
    let error = runtime_filer_io_error(
        RuntimeFilerOperationKind::CreateFile,
        &path,
        None,
        std::io::Error::from(std::io::ErrorKind::PermissionDenied),
    );

    assert_eq!(
        error,
        RuntimeFilerError::OperationFailed {
            operation: RuntimeFilerOperationKind::CreateFile,
            path,
            target_path: None,
            kind: RuntimeFilerErrorKind::PermissionDenied,
            message: "permission denied".to_string(),
        }
    );
}

#[test]
fn filer_list_options_filter_hidden_and_sort_by_size_with_metadata() {
    let root_path = unique_path("filer-list-options");
    let small_path = root_path.join("small.txt");
    let large_path = root_path.join("large.txt");
    let hidden_path = root_path.join(".hidden.txt");
    std::fs::create_dir_all(&root_path).expect("root directory");
    std::fs::write(&small_path, "1").expect("small file");
    std::fs::write(&large_path, "12345").expect("large file");
    std::fs::write(&hidden_path, "hidden").expect("hidden file");

    let entries = super::list_local_filer_entries(
        root_path.clone(),
        super::RuntimeFilerListOptions {
            show_hidden: false,
            sort_by: super::RuntimeFilerSortKey::Size,
            filter: None,
        },
    )
    .expect("filer list should succeed");

    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        vec!["small.txt", "large.txt"]
    );
    assert_eq!(entries[0].size, Some(1));
    assert_eq!(entries[1].size, Some(5));
    assert!(
        entries.iter().all(|entry| entry.modified_time_ms.is_some()),
        "metadata should include modified_time_ms without removing existing fields"
    );

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[cfg(unix)]
#[test]
fn filer_kind_sort_groups_directories_then_sorts_other_entries_by_name() {
    let root_path = unique_path("filer-kind-sort");
    let directory_path = root_path.join("middle-dir");
    let file_path = root_path.join("z-file.txt");
    let target_path = root_path.join("target.md");
    let link_path = root_path.join("a-link.md");
    std::fs::create_dir_all(&directory_path).expect("nested directory");
    std::fs::write(&file_path, "file\n").expect("file entry");
    std::fs::write(&target_path, "target\n").expect("symlink target");
    std::os::unix::fs::symlink(&target_path, &link_path).expect("symlink");

    let entries = super::list_local_filer_entries(
        root_path.clone(),
        super::RuntimeFilerListOptions {
            show_hidden: true,
            sort_by: super::RuntimeFilerSortKey::Kind,
            filter: None,
        },
    )
    .expect("filer list should succeed");

    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.display_text.as_str())
            .collect::<Vec<_>>(),
        vec!["middle-dir/", "a-link.md@", "target.md", "z-file.txt"]
    );

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}
