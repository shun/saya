use super::*;

#[test]
fn swapfile_path_prefixes_visible_file_names_with_dot() {
    let target = Path::new("/tmp/sample.txt");

    let actual = swapfile_path_for_target(target);

    assert_eq!(actual, Some(PathBuf::from("/tmp/.sample.txt.swp")));
}

#[test]
fn swapfile_path_keeps_hidden_file_name_shape() {
    let target = Path::new("/tmp/.hidden.md");

    let actual = swapfile_path_for_target(target);

    assert_eq!(actual, Some(PathBuf::from("/tmp/.hidden.md.swp")));
}
