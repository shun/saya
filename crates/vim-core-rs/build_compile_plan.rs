use super::build_allowlist::Allowlist;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpstreamMetadata {
    pub tag: String,
    pub commit: String,
}

impl UpstreamMetadata {
    pub fn load(path: &Path) -> Result<Self, String> {
        let raw = fs::read_to_string(path)
            .map_err(|error| format!("failed to read metadata {}: {error}", path.display()))?;
        let metadata = serde_json::from_str::<Self>(&raw)
            .map_err(|error| format!("failed to parse metadata {}: {error}", path.display()))?;
        metadata.validate()?;
        Ok(metadata)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !is_valid_vim_tag(&self.tag) {
            return Err(format!("metadata tag {} is not a valid Vim tag", self.tag));
        }

        if self.commit.len() < 7 || self.commit.len() > 40 {
            return Err(format!(
                "metadata commit {} must be 7-40 hex characters",
                self.commit
            ));
        }

        if !self.commit.chars().all(|character| character.is_ascii_hexdigit()) {
            return Err(format!(
                "metadata commit {} must contain only hex characters",
                self.commit
            ));
        }

        Ok(())
    }
}

fn is_valid_vim_tag(tag: &str) -> bool {
    let Some(version) = tag.strip_prefix('v') else {
        return false;
    };

    let segments = version.split('.').collect::<Vec<_>>();
    if segments.len() < 2 {
        return false;
    }

    segments
        .iter()
        .all(|segment| !segment.is_empty() && segment.chars().all(|character| character.is_ascii_digit()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilePlan {
    pub native_sources: Vec<PathBuf>,
    pub vendor_sources: Vec<PathBuf>,
}

#[derive(Debug, Serialize)]
struct CompileProof<'a> {
    tag: &'a str,
    commit: &'a str,
    native_sources: Vec<String>,
    vendor_sources: Vec<String>,
}

pub fn collect_native_sources(repo_root: &Path, native_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut native_sources = WalkDir::new(native_dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("c"))
        .map(|entry| {
            entry
                .path()
                .strip_prefix(repo_root)
                .map(PathBuf::from)
                .map_err(|error| {
                    format!(
                        "failed to derive repo-relative path for native source {}: {error}",
                        entry.path().display()
                    )
                })
        })
        .collect::<Result<Vec<_>, _>>()?;

    native_sources.sort();

    if native_sources.is_empty() {
        return Err(format!(
            "native source directory {} does not contain .c files",
            native_dir.display()
        ));
    }

    Ok(native_sources)
}

pub fn parse_build_manifest(
    repo_root: &Path,
    manifest_path: &Path,
    allowlist: &Allowlist,
) -> Result<Vec<PathBuf>, String> {
    let raw = fs::read_to_string(manifest_path)
        .map_err(|error| format!("failed to read manifest {}: {error}", manifest_path.display()))?;

    let mut seen = BTreeSet::new();
    let mut vendor_sources = Vec::new();

    for line in raw.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if !allowlist.is_allowed(line) {
            return Err(format!(
                "manifest entry {line} is not covered by the allowlist"
            ));
        }

        if !seen.insert(line.to_string()) {
            return Err(format!("manifest entry {line} is duplicated"));
        }

        let absolute_path = repo_root.join(line);
        if !absolute_path.exists() {
            return Err(format!("manifest entry {line} does not exist"));
        }

        vendor_sources.push(PathBuf::from(line));
    }

    if vendor_sources.is_empty() {
        return Err(format!(
            "manifest {} does not contain vendor sources",
            manifest_path.display()
        ));
    }

    Ok(vendor_sources)
}

pub fn create_compile_plan(
    repo_root: &Path,
    native_dir: &Path,
    manifest_path: &Path,
    allowlist: &Allowlist,
) -> Result<CompilePlan, String> {
    Ok(CompilePlan {
        native_sources: collect_native_sources(repo_root, native_dir)?,
        vendor_sources: parse_build_manifest(repo_root, manifest_path, allowlist)?,
    })
}

pub fn write_compile_proof(
    out_dir: &Path,
    metadata: &UpstreamMetadata,
    plan: &CompilePlan,
) -> Result<PathBuf, String> {
    let proof = CompileProof {
        tag: &metadata.tag,
        commit: &metadata.commit,
        native_sources: plan
            .native_sources
            .iter()
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .collect(),
        vendor_sources: plan
            .vendor_sources
            .iter()
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .collect(),
    };

    let proof_path = out_dir.join("upstream_build_fingerprint.json");
    let serialized = serde_json::to_string_pretty(&proof)
        .map_err(|error| format!("failed to serialize compile proof: {error}"))?;
    fs::write(&proof_path, serialized).map_err(|error| {
        format!(
            "failed to write compile proof {}: {error}",
            proof_path.display()
        )
    })?;

    Ok(proof_path)
}

#[cfg(test)]
mod tests {
    use super::{create_compile_plan, parse_build_manifest, write_compile_proof, UpstreamMetadata};
    use crate::build_allowlist::Allowlist;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn rejects_invalid_vim_tag() {
        let dir = tempdir().expect("tempdir should be created");
        let metadata_path = dir.path().join("upstream-metadata.json");
        fs::write(
            &metadata_path,
            r#"{"tag":"vim-9.1","commit":"0123abcd"}"#,
        )
        .expect("metadata should be written");

        let error = UpstreamMetadata::load(&metadata_path).expect_err("metadata should fail");
        assert!(error.contains("valid Vim tag"));
    }

    #[test]
    fn accepts_valid_metadata() {
        let metadata = UpstreamMetadata {
            tag: "v9.1.0000".to_string(),
            commit: "0123abcd".to_string(),
        };

        metadata.validate().expect("metadata should validate");
    }

    #[test]
    fn rejects_duplicate_manifest_entries() {
        let dir = tempdir().expect("tempdir should be created");
        fs::create_dir_all(dir.path().join("vendor/vim_src")).expect("vendor dir should exist");
        fs::write(
            dir.path().join("vim-source-allowlist.txt"),
            "vendor/vim_src/**/*.c\n",
        )
        .expect("allowlist should be written");
        fs::write(
            dir.path().join("vim-source-build-manifest.txt"),
            "vendor/vim_src/headless_placeholder.c\nvendor/vim_src/headless_placeholder.c\n",
        )
        .expect("manifest should be written");
        fs::write(
            dir.path().join("vendor/vim_src/headless_placeholder.c"),
            "int placeholder(void) { return 0; }\n",
        )
        .expect("vendor source should be written");

        let allowlist = Allowlist::load(&dir.path().join("vim-source-allowlist.txt"))
            .expect("allowlist should load");
        let error = parse_build_manifest(
            dir.path(),
            &dir.path().join("vim-source-build-manifest.txt"),
            &allowlist,
        )
        .expect_err("manifest should reject duplicates");

        assert!(error.contains("duplicated"));
    }

    #[test]
    fn writes_compile_proof_with_native_and_vendor_sources() {
        let dir = tempdir().expect("tempdir should be created");
        fs::create_dir_all(dir.path().join("native")).expect("native dir should exist");
        fs::create_dir_all(dir.path().join("vendor/vim_src")).expect("vendor dir should exist");
        fs::write(
            dir.path().join("vim-source-allowlist.txt"),
            "vendor/vim_src/**/*.c\n",
        )
        .expect("allowlist should be written");
        fs::write(
            dir.path().join("vim-source-build-manifest.txt"),
            "vendor/vim_src/headless_placeholder.c\n",
        )
        .expect("manifest should be written");
        fs::write(dir.path().join("native/vim_bridge.c"), "int bridge(void) { return 0; }\n")
            .expect("native source should be written");
        fs::write(
            dir.path().join("vendor/vim_src/headless_placeholder.c"),
            "int placeholder(void) { return 0; }\n",
        )
        .expect("vendor source should be written");

        let allowlist = Allowlist::load(&dir.path().join("vim-source-allowlist.txt"))
            .expect("allowlist should load");
        let plan = create_compile_plan(
            dir.path(),
            &dir.path().join("native"),
            &dir.path().join("vim-source-build-manifest.txt"),
            &allowlist,
        )
        .expect("compile plan should load");
        let metadata = UpstreamMetadata {
            tag: "v9.1.0000".to_string(),
            commit: "0123abcd".to_string(),
        };

        let proof_path = write_compile_proof(dir.path(), &metadata, &plan)
            .expect("compile proof should be written");
        let proof = fs::read_to_string(proof_path).expect("proof should be readable");

        assert!(proof.contains("native/vim_bridge.c"));
        assert!(proof.contains("vendor/vim_src/headless_placeholder.c"));
        assert!(proof.contains("v9.1.0000"));
    }
}
