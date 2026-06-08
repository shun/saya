use globset::{Glob, GlobSet, GlobSetBuilder};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug)]
pub struct Allowlist {
    #[cfg_attr(not(test), allow(dead_code))]
    patterns: Vec<String>,
    matcher: GlobSet,
}

impl Allowlist {
    pub fn load(path: &Path) -> Result<Self, String> {
        let raw = fs::read_to_string(path)
            .map_err(|error| format!("failed to read allowlist {}: {error}", path.display()))?;

        let patterns = raw
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();

        if patterns.is_empty() {
            return Err(format!("allowlist {} does not contain patterns", path.display()));
        }

        let mut builder = GlobSetBuilder::new();
        for pattern in &patterns {
            let glob = Glob::new(pattern)
                .map_err(|error| format!("invalid allowlist pattern {pattern}: {error}"))?;
            builder.add(glob);
        }

        let matcher = builder
            .build()
            .map_err(|error| format!("failed to build allowlist matcher: {error}"))?;

        Ok(Self { patterns, matcher })
    }

    pub fn is_allowed(&self, relative_path: &str) -> bool {
        self.matcher.is_match(relative_path)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }
}

pub fn verify_bridge_header(path: &Path) -> Result<(), String> {
    let raw = fs::read_to_string(path)
        .map_err(|error| format!("failed to read bridge header {}: {error}", path.display()))?;

    for (index, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("#include \"") {
            return Err(format!(
                "bridge header {} contains forbidden local include on line {}: {}",
                path.display(),
                index + 1,
                trimmed
            ));
        }
    }

    Ok(())
}

pub fn validate_allowlist(
    repo_root: &Path,
    vendor_dir: &Path,
    allowlist: &Allowlist,
) -> Result<Vec<PathBuf>, String> {
    if !vendor_dir.exists() {
        return Err(format!(
            "vendor directory {} does not exist",
            vendor_dir.display()
        ));
    }

    let mut allowed_files = Vec::new();

    for entry in WalkDir::new(vendor_dir).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }

        let relative_path = entry
            .path()
            .strip_prefix(repo_root)
            .map_err(|error| {
                format!(
                    "failed to derive repo-relative path for {}: {error}",
                    entry.path().display()
                )
            })?
            .to_string_lossy()
            .replace('\\', "/");

        if !allowlist.is_allowed(&relative_path) {
            return Err(format!(
                "vendor file {relative_path} is not covered by the allowlist"
            ));
        }

        allowed_files.push(PathBuf::from(relative_path));
    }

    allowed_files.sort();
    Ok(allowed_files)
}

#[cfg(test)]
mod tests {
    use super::{validate_allowlist, verify_bridge_header, Allowlist};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn ignores_comments_and_blank_lines() {
        let dir = tempdir().expect("tempdir should be created");
        let allowlist_path = dir.path().join("vim-source-allowlist.txt");
        fs::write(
            &allowlist_path,
            "\n# comment\nvendor/vim_src/**/*.c\n\nvendor/vim_src/**/*.h\n",
        )
        .expect("allowlist should be written");

        let allowlist = Allowlist::load(&allowlist_path).expect("allowlist should load");

        assert_eq!(
            allowlist.patterns(),
            &[
                "vendor/vim_src/**/*.c".to_string(),
                "vendor/vim_src/**/*.h".to_string()
            ]
        );
        assert!(allowlist.is_allowed("vendor/vim_src/core/main.c"));
        assert!(!allowlist.is_allowed("vendor/other/rogue.c"));
    }

    #[test]
    fn rejects_local_includes_in_bridge_header() {
        let dir = tempdir().expect("tempdir should be created");
        let header_path = dir.path().join("vim_bridge.h");
        fs::write(
            &header_path,
            "#include <stddef.h>\n#include \"vim.h\"\n",
        )
        .expect("header should be written");

        let error = verify_bridge_header(&header_path).expect_err("header should be rejected");
        assert!(error.contains("forbidden local include"));
    }

    #[test]
    fn rejects_vendor_file_outside_allowlist() {
        let dir = tempdir().expect("tempdir should be created");
        let vendor_dir = dir.path().join("vendor/vim_src");
        fs::create_dir_all(&vendor_dir).expect("vendor dir should be created");

        let allowlist_path = dir.path().join("vim-source-allowlist.txt");
        fs::write(&allowlist_path, "vendor/vim_src/**/*.c\n")
            .expect("allowlist should be written");
        fs::write(vendor_dir.join("rogue.txt"), "nope").expect("rogue file should be written");

        let allowlist = Allowlist::load(&allowlist_path).expect("allowlist should load");
        let error = validate_allowlist(dir.path(), &vendor_dir, &allowlist)
            .expect_err("rogue file should be rejected");

        assert!(error.contains("rogue.txt"));
    }
}
