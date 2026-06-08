use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct SwapfileCleanupGuard {
    target_path: Option<PathBuf>,
}

impl SwapfileCleanupGuard {
    pub fn new(target_path: Option<PathBuf>) -> Self {
        Self { target_path }
    }
}

pub fn swapfile_path_for_target(target_path: &Path) -> Option<PathBuf> {
    let file_name = target_path.file_name()?.to_string_lossy();
    let swap_name = if file_name.starts_with('.') {
        format!("{file_name}.swp")
    } else {
        format!(".{file_name}.swp")
    };
    Some(target_path.with_file_name(swap_name))
}

pub fn cleanup_swapfile_for_target(target_path: &Path) {
    let Some(swap_path) = swapfile_path_for_target(target_path) else {
        log::debug!(
            "[swapfile] skipping cleanup because target path has no file name: {}",
            target_path.display()
        );
        return;
    };

    match std::fs::remove_file(&swap_path) {
        Ok(()) => {
            log::debug!(
                "[swapfile] removed swapfile during shutdown: {}",
                swap_path.display()
            );
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            log::debug!(
                "[swapfile] no swapfile present during shutdown: {}",
                swap_path.display()
            );
        }
        Err(error) => {
            log::debug!(
                "[swapfile] failed to remove swapfile during shutdown: path={}, error={}",
                swap_path.display(),
                error
            );
        }
    }
}

impl Drop for SwapfileCleanupGuard {
    fn drop(&mut self) {
        if let Some(target_path) = self.target_path.as_ref() {
            cleanup_swapfile_for_target(target_path);
        }
    }
}

#[cfg(test)]
mod tests {
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
}
