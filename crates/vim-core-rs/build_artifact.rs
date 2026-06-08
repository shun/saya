use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use tar::Archive;

pub const VIM_CORE_FROM_SOURCE_ENV: &str = "VIM_CORE_FROM_SOURCE";
pub const VIM_CORE_ARTIFACT_BASE_URL_ENV: &str = "VIM_CORE_ARTIFACT_BASE_URL";
pub const VIM_CORE_ARTIFACT_DIR_ENV: &str = "VIM_CORE_ARTIFACT_DIR";
pub const VIM_CORE_VERBOSE_BUILD_ENV: &str = "VIM_CORE_VERBOSE_BUILD";
pub const ARTIFACT_MANIFEST_FILE: &str = "artifact-manifest.json";
pub const PREBUILT_ABI_VERSION: u32 = 1;
pub const SUPPORTED_TARGETS: [&str; 2] = ["aarch64-apple-darwin", "x86_64-unknown-linux-gnu"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactConfig {
    pub crate_version: String,
    pub target_triple: String,
    pub artifact_base_url: String,
    pub artifact_cache_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedArtifact {
    pub cache_key: String,
    pub cache_dir: PathBuf,
    pub manifest: ArtifactManifest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactManifest {
    pub crate_version: String,
    pub target_triple: String,
    pub artifact_profile: String,
    pub abi_version: u32,
    pub upstream_vim_tag: String,
    pub upstream_vim_commit: String,
    pub generated_at_utc: String,
    pub files: BTreeMap<String, String>,
}

pub fn emit_artifact_rerun_if_env_changed() {
    for key in [
        VIM_CORE_FROM_SOURCE_ENV,
        VIM_CORE_ARTIFACT_BASE_URL_ENV,
        VIM_CORE_ARTIFACT_DIR_ENV,
        VIM_CORE_VERBOSE_BUILD_ENV,
        "TARGET",
        "PROFILE",
    ] {
        println!("cargo:rerun-if-env-changed={key}");
    }
}

pub fn source_build_requested() -> bool {
    match env::var(VIM_CORE_FROM_SOURCE_ENV) {
        Ok(value) => matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"),
        Err(_) => repository_checkout_detected(),
    }
}

fn repository_checkout_detected() -> bool {
    let Ok(manifest_dir) = env::var("CARGO_MANIFEST_DIR") else {
        return false;
    };

    Path::new(&manifest_dir).join(".git").exists()
}

pub fn verbose_build_requested() -> bool {
    match env::var(VIM_CORE_VERBOSE_BUILD_ENV) {
        Ok(value) => matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"),
        Err(_) => false,
    }
}

pub fn unsupported_target_error(target: &str) -> String {
    format!(
        "prebuilt vim-core-rs artifacts do not support target `{target}`; \
set {VIM_CORE_FROM_SOURCE_ENV}=1 to build from source"
    )
}

pub fn missing_artifact_error(url: &str) -> String {
    format!(
        "prebuilt vim-core-rs artifact was not found at {url}; \
set {VIM_CORE_FROM_SOURCE_ENV}=1 to build from source"
    )
}

pub fn artifact_asset_name(crate_version: &str, target_triple: &str) -> String {
    format!("vim-core-rs-{crate_version}-{target_triple}.tar.gz")
}

pub fn artifact_release_tag(crate_version: &str) -> String {
    format!("v{crate_version}")
}

pub fn default_artifact_base_url(crate_version: &str) -> String {
    format!(
        "https://github.com/shun/vim-core-rs/releases/download/{}",
        artifact_release_tag(crate_version)
    )
}

pub fn default_artifact_cache_dir() -> Result<PathBuf, String> {
    if let Ok(path) = env::var(VIM_CORE_ARTIFACT_DIR_ENV) {
        return Ok(PathBuf::from(path));
    }

    if cfg!(target_os = "macos") {
        let home = env::var("HOME").map_err(|error| format!("missing HOME: {error}"))?;
        return Ok(PathBuf::from(home)
            .join("Library")
            .join("Caches")
            .join("vim-core-rs")
            .join("artifacts"));
    }

    if let Ok(path) = env::var("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(path).join("vim-core-rs").join("artifacts"));
    }

    let home = env::var("HOME").map_err(|error| format!("missing HOME: {error}"))?;
    Ok(PathBuf::from(home)
        .join(".cache")
        .join("vim-core-rs")
        .join("artifacts"))
}

pub fn resolve_artifact_config_from_env() -> Result<ArtifactConfig, String> {
    let crate_version = env::var("CARGO_PKG_VERSION")
        .map_err(|error| format!("missing CARGO_PKG_VERSION: {error}"))?;
    let target_triple = env::var("TARGET").map_err(|error| format!("missing TARGET: {error}"))?;
    let artifact_base_url = env::var(VIM_CORE_ARTIFACT_BASE_URL_ENV)
        .unwrap_or_else(|_| default_artifact_base_url(&crate_version));
    let artifact_cache_dir = default_artifact_cache_dir()?;

    Ok(ArtifactConfig {
        crate_version,
        target_triple,
        artifact_base_url,
        artifact_cache_dir,
    })
}

pub fn install_prebuilt_artifact(
    config: &ArtifactConfig,
    out_dir: &Path,
) -> Result<PreparedArtifact, String> {
    let asset_name = artifact_asset_name(&config.crate_version, &config.target_triple);
    let artifact_url = artifact_url(&config.artifact_base_url, &asset_name);

    if !SUPPORTED_TARGETS.contains(&config.target_triple.as_str()) {
        return Err(unsupported_target_error(&config.target_triple));
    }

    fs::create_dir_all(&config.artifact_cache_dir).map_err(|error| {
        format!(
            "failed to create artifact cache directory {}: {error}",
            config.artifact_cache_dir.display()
        )
    })?;

    let downloads_dir = config.artifact_cache_dir.join("downloads");
    fs::create_dir_all(&downloads_dir).map_err(|error| {
        format!(
            "failed to create artifact download directory {}: {error}",
            downloads_dir.display()
        )
    })?;

    let archive_path = downloads_dir.join(&asset_name);
    if !archive_path.exists() {
        fetch_artifact_archive(&artifact_url, &archive_path)?;
    }

    let staging_dir = config.artifact_cache_dir.join("staging").join(format!(
        "{}-{}",
        std::process::id(),
        current_nanos()?
    ));
    fs::create_dir_all(&staging_dir).map_err(|error| {
        format!(
            "failed to create artifact staging directory {}: {error}",
            staging_dir.display()
        )
    })?;
    unpack_archive(&archive_path, &staging_dir)?;

    let manifest_path = staging_dir.join(ARTIFACT_MANIFEST_FILE);
    if !manifest_path.exists() {
        return Err(format!(
            "prebuilt vim-core-rs artifact {} does not contain {}",
            archive_path.display(),
            ARTIFACT_MANIFEST_FILE
        ));
    }

    let manifest = read_manifest(&manifest_path)?;
    verify_manifest_identity(&manifest, config)?;
    verify_manifest_files(&staging_dir, &manifest)?;

    let cache_key = sha256_file(&manifest_path)?;
    let finalized_dir = config
        .artifact_cache_dir
        .join("artifacts")
        .join(&config.crate_version)
        .join(&config.target_triple)
        .join(&cache_key);

    if !finalized_dir.exists() {
        if let Some(parent) = finalized_dir.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "failed to create artifact cache parent {}: {error}",
                    parent.display()
                )
            })?;
        }
        copy_tree(&staging_dir, &finalized_dir)?;
    }

    if out_dir.exists() {
        copy_tree(&finalized_dir, out_dir)?;
    } else {
        fs::create_dir_all(out_dir).map_err(|error| {
            format!(
                "failed to create build output directory {}: {error}",
                out_dir.display()
            )
        })?;
        copy_tree(&finalized_dir, out_dir)?;
    }

    let _ = fs::remove_dir_all(&staging_dir);

    Ok(PreparedArtifact {
        cache_key,
        cache_dir: finalized_dir,
        manifest,
    })
}

fn current_nanos() -> Result<u128, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| format!("system clock is before UNIX_EPOCH: {error}"))?;
    Ok(now.as_nanos())
}

fn artifact_url(base: &str, asset_name: &str) -> String {
    let normalized = base.trim_end_matches('/');
    format!("{normalized}/{asset_name}")
}

fn fetch_artifact_archive(url: &str, destination: &Path) -> Result<(), String> {
    let tmp_path = destination.with_extension("tmp");

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "failed to create artifact destination directory {}: {error}",
                parent.display()
            )
        })?;
    }

    if let Some(local_path) = local_path_from_url(url) {
        let source = if local_path.is_dir() {
            local_path.join(
                destination
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| {
                        format!(
                            "artifact destination {} does not have a file name",
                            destination.display()
                        )
                    })?,
            )
        } else {
            local_path
        };

        if !source.exists() {
            return Err(missing_artifact_error(url));
        }

        fs::copy(&source, &tmp_path).map_err(|error| {
            format!(
                "failed to copy local artifact from {} to {}: {error}",
                source.display(),
                tmp_path.display()
            )
        })?;
    } else {
        let output = Command::new("curl")
            .arg("--fail")
            .arg("--location")
            .arg("--silent")
            .arg("--show-error")
            .arg("--output")
            .arg(&tmp_path)
            .arg(url)
            .output()
            .map_err(|error| format!("failed to execute curl for {url}: {error}"))?;

        if !output.status.success() {
            if output.status.code() == Some(22) {
                return Err(missing_artifact_error(url));
            }

            return Err(format!(
                "failed to download prebuilt vim-core-rs artifact from {url}: status {:?}\nstdout:\n{}\nstderr:\n{}",
                output.status.code(),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }

    fs::rename(&tmp_path, destination).map_err(|error| {
        format!(
            "failed to move downloaded artifact into place {}: {error}",
            destination.display()
        )
    })?;

    Ok(())
}

fn local_path_from_url(url: &str) -> Option<PathBuf> {
    if let Some(path) = url.strip_prefix("file://") {
        return Some(PathBuf::from(path));
    }

    if url.starts_with("http://") || url.starts_with("https://") {
        return None;
    }

    Some(PathBuf::from(url))
}

fn unpack_archive(archive_path: &Path, destination: &Path) -> Result<(), String> {
    let archive_file = fs::File::open(archive_path).map_err(|error| {
        format!(
            "failed to open prebuilt artifact archive {}: {error}",
            archive_path.display()
        )
    })?;
    let decoder = GzDecoder::new(archive_file);
    let mut archive = Archive::new(decoder);
    archive.unpack(destination).map_err(|error| {
        format!(
            "failed to unpack prebuilt artifact archive {} into {}: {error}",
            archive_path.display(),
            destination.display()
        )
    })
}

fn read_manifest(path: &Path) -> Result<ArtifactManifest, String> {
    let content = fs::read_to_string(path).map_err(|error| {
        format!(
            "failed to read artifact manifest {}: {error}",
            path.display()
        )
    })?;
    serde_json::from_str(&content).map_err(|error| {
        format!(
            "failed to parse artifact manifest {}: {error}",
            path.display()
        )
    })
}

fn verify_manifest_identity(
    manifest: &ArtifactManifest,
    config: &ArtifactConfig,
) -> Result<(), String> {
    if manifest.crate_version != config.crate_version {
        return Err(format!(
            "prebuilt artifact crate version mismatch: expected {}, got {}",
            config.crate_version, manifest.crate_version
        ));
    }

    if manifest.target_triple != config.target_triple {
        return Err(format!(
            "prebuilt artifact target mismatch: expected {}, got {}",
            config.target_triple, manifest.target_triple
        ));
    }

    if manifest.abi_version != PREBUILT_ABI_VERSION {
        return Err(format!(
            "prebuilt artifact ABI version mismatch: expected {}, got {}",
            PREBUILT_ABI_VERSION, manifest.abi_version
        ));
    }

    Ok(())
}

fn verify_manifest_files(root: &Path, manifest: &ArtifactManifest) -> Result<(), String> {
    for (relative_path, expected_sha) in &manifest.files {
        let path = root.join(relative_path);
        if !path.exists() {
            return Err(format!(
                "prebuilt artifact is missing required file {}",
                path.display()
            ));
        }

        let actual_sha = sha256_file(&path)?;
        if &actual_sha != expected_sha {
            return Err(format!(
                "prebuilt artifact checksum mismatch for {}: expected {}, got {}",
                relative_path, expected_sha, actual_sha
            ));
        }
    }

    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    for entry in walkdir::WalkDir::new(source) {
        let entry = entry.map_err(|error| {
            format!(
                "failed to read artifact cache entry under {}: {error}",
                source.display()
            )
        })?;
        let path = entry.path();
        let relative = path.strip_prefix(source).map_err(|error| {
            format!(
                "failed to compute artifact relative path for {}: {error}",
                path.display()
            )
        })?;

        if relative.as_os_str().is_empty() {
            continue;
        }

        let target = destination.join(relative);
        if entry.file_name().to_string_lossy().starts_with("._") {
            continue;
        }
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target).map_err(|error| {
                format!(
                    "failed to create artifact output dir {}: {error}",
                    target.display()
                )
            })?;
            continue;
        }

        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "failed to create artifact output dir {}: {error}",
                    parent.display()
                )
            })?;
        }

        fs::copy(path, &target).map_err(|error| {
            format!(
                "failed to copy artifact file {} to {}: {error}",
                path.display(),
                target.display()
            )
        })?;
    }

    Ok(())
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path)
        .map_err(|error| format!("failed to open {} for hashing: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0_u8; 16 * 1024];

    loop {
        let read = file
            .read(&mut buf)
            .map_err(|error| format!("failed to read {} for hashing: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::Write;
    use tar::Builder;
    use tempfile::TempDir;

    fn write_file(root: &Path, relative: &str, content: &[u8]) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("should create parent dir");
        }
        fs::write(path, content).expect("should write file");
    }

    fn build_manifest(root: &Path, crate_version: &str, target: &str) -> ArtifactManifest {
        let mut files = BTreeMap::new();
        for relative in [
            "libvimcore.a",
            "bindings.rs",
            "native-source-audit-report.txt",
            "archive-member-audit-report.txt",
            "normal-delegation-proof.txt",
            "ex-delegation-proof.txt",
            "upstream_build_fingerprint.json",
            "upstream_vim_tests.rs",
            "vim_build/auto/config.h",
            "vim_build/auto/osdef.h",
            "vim_build/auto/pathdef.c",
        ] {
            files.insert(
                relative.to_string(),
                sha256_file(&root.join(relative)).expect("should hash file"),
            );
        }

        ArtifactManifest {
            crate_version: crate_version.to_string(),
            target_triple: target.to_string(),
            artifact_profile: "release".to_string(),
            abi_version: PREBUILT_ABI_VERSION,
            upstream_vim_tag: "v9.2.0437".to_string(),
            upstream_vim_commit: "9d3019104c37bad56ff5bdc7614b26cb3fea7ce4".to_string(),
            generated_at_utc: "2026-03-24T00:00:00Z".to_string(),
            files,
        }
    }

    fn create_local_artifact(
        dir: &TempDir,
        crate_version: &str,
        target: &str,
    ) -> (PathBuf, ArtifactManifest) {
        let artifact_root = dir.path().join("artifact-root");
        fs::create_dir_all(&artifact_root).expect("should create artifact root");
        write_file(&artifact_root, "libvimcore.a", b"fake archive");
        write_file(&artifact_root, "bindings.rs", b"pub const VALUE: u32 = 7;");
        write_file(
            &artifact_root,
            "native-source-audit-report.txt",
            b"native source audit report\nstatus: passed\n",
        );
        write_file(
            &artifact_root,
            "archive-member-audit-report.txt",
            b"archive member audit report\nstatus: passed\n",
        );
        write_file(
            &artifact_root,
            "normal-delegation-proof.txt",
            b"normal delegation proof report\nstatus: passed\n",
        );
        write_file(
            &artifact_root,
            "ex-delegation-proof.txt",
            b"ex delegation proof report\nstatus: passed\n",
        );
        write_file(
            &artifact_root,
            "upstream_build_fingerprint.json",
            br#"{"tag":"v9.2.0437","commit":"9d3019104c37bad56ff5bdc7614b26cb3fea7ce4","native_sources":[],"vendor_sources":[]}"#,
        );
        write_file(&artifact_root, "upstream_vim_tests.rs", b"");
        write_file(
            &artifact_root,
            "vim_build/auto/config.h",
            b"#define MODIFIED_BY \"vim-core-rs\"\n",
        );
        write_file(&artifact_root, "vim_build/auto/osdef.h", b"");
        write_file(
            &artifact_root,
            "vim_build/auto/pathdef.c",
            b"char_u *default_vim_dir = (char_u *)\"/tmp/vim\";\nchar_u *default_vimruntime_dir = (char_u *)\"/tmp/vim/vim92\";\n",
        );

        let manifest = build_manifest(&artifact_root, crate_version, target);
        fs::write(
            artifact_root.join(ARTIFACT_MANIFEST_FILE),
            serde_json::to_vec_pretty(&manifest).expect("should serialize manifest"),
        )
        .expect("should write manifest");

        let archive_path = dir.path().join(artifact_asset_name(crate_version, target));
        let archive_file = fs::File::create(&archive_path).expect("should create archive");
        let encoder = GzEncoder::new(archive_file, Compression::default());
        let mut builder = Builder::new(encoder);
        builder
            .append_dir_all(".", &artifact_root)
            .expect("should append artifact dir");
        let encoder = builder.into_inner().expect("should finish tar builder");
        let mut archive_file = encoder.finish().expect("should finish encoder");
        archive_file.flush().expect("should flush archive file");

        (archive_path, manifest)
    }

    #[test]
    fn default_base_url_uses_github_releases() {
        assert_eq!(
            default_artifact_base_url("1.2.3"),
            "https://github.com/shun/vim-core-rs/releases/download/v1.2.3"
        );
    }

    #[test]
    fn installs_prebuilt_artifact_from_local_directory() {
        let dir = TempDir::new().expect("should create temp dir");
        let crate_version = "0.1.0";
        let target = "aarch64-apple-darwin";
        let (_archive_path, manifest) = create_local_artifact(&dir, crate_version, target);
        let cache_dir = dir.path().join("cache");
        let out_dir = dir.path().join("out");

        let prepared = install_prebuilt_artifact(
            &ArtifactConfig {
                crate_version: crate_version.to_string(),
                target_triple: target.to_string(),
                artifact_base_url: format!("file://{}", dir.path().display()),
                artifact_cache_dir: cache_dir.clone(),
            },
            &out_dir,
        )
        .expect("should install prebuilt artifact");

        assert_eq!(prepared.manifest, manifest);
        assert!(out_dir.join("bindings.rs").exists());
        assert!(out_dir.join("libvimcore.a").exists());
        assert!(out_dir.join("vim_build/auto/config.h").exists());
    }

    #[test]
    fn checksum_mismatch_is_rejected() {
        let dir = TempDir::new().expect("should create temp dir");
        let crate_version = "0.1.0";
        let target = "aarch64-apple-darwin";
        let (archive_path, mut manifest) = create_local_artifact(&dir, crate_version, target);
        manifest
            .files
            .insert("bindings.rs".to_string(), "deadbeef".to_string());

        let tampered_root = dir.path().join("tampered-root");
        fs::create_dir_all(&tampered_root).expect("should create tampered root");
        let archive_file = fs::File::open(&archive_path).expect("should open archive");
        let decoder = GzDecoder::new(archive_file);
        let mut archive = Archive::new(decoder);
        archive
            .unpack(&tampered_root)
            .expect("should unpack existing archive");
        fs::write(
            tampered_root.join(ARTIFACT_MANIFEST_FILE),
            serde_json::to_vec_pretty(&manifest).expect("should serialize tampered manifest"),
        )
        .expect("should overwrite manifest");

        let rebuilt_archive = dir.path().join("tampered-artifact.tar.gz");
        let rebuilt_file = fs::File::create(&rebuilt_archive).expect("should create rebuilt file");
        let encoder = GzEncoder::new(rebuilt_file, Compression::default());
        let mut builder = Builder::new(encoder);
        builder
            .append_dir_all(".", &tampered_root)
            .expect("should append tampered dir");
        let encoder = builder.into_inner().expect("should finish tar");
        encoder.finish().expect("should finish gzip");

        let asset_name = artifact_asset_name(crate_version, target);
        fs::rename(&rebuilt_archive, dir.path().join(&asset_name)).expect("should rename asset");

        let result = install_prebuilt_artifact(
            &ArtifactConfig {
                crate_version: crate_version.to_string(),
                target_triple: target.to_string(),
                artifact_base_url: format!("file://{}", dir.path().display()),
                artifact_cache_dir: dir.path().join("cache"),
            },
            &dir.path().join("out"),
        );

        assert!(result.is_err(), "checksum mismatch should fail");
        let error = result.expect_err("result should contain an error");
        assert!(
            error.contains("checksum mismatch"),
            "checksum error should mention mismatch: {error}"
        );
    }
}
