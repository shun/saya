//! startup トランスパイル結果のディスクキャッシュ層。

use super::*;

pub(super) fn startup_transpile_cache_key(graph: &StartupModuleGraph) -> String {
    let mut hasher = Sha256::new();
    hasher.update(
        STARTUP_TRANSPILE_CACHE_SCHEMA_VERSION
            .to_string()
            .as_bytes(),
    );
    hasher.update([0]);
    hasher.update(STARTUP_TRANSPILE_OPTION_VERSION.as_bytes());
    hasher.update([0]);
    hasher.update(graph.entry_id.as_bytes());
    for module in &graph.modules {
        hasher.update([0]);
        hasher.update(module.id.as_bytes());
        hasher.update([0]);
        hasher.update(module.source_hash.as_bytes());
    }
    bytes_to_hex(&hasher.finalize())
}

pub(super) fn read_startup_transpile_cache(
    cache_key: &str,
    graph: &StartupModuleGraph,
) -> Option<String> {
    let cache_dir = startup_transpile_cache_dir()?;
    let js_path = cache_dir.join(format!("{cache_key}.js"));
    let metadata_path = cache_dir.join(format!("{cache_key}.json"));
    let metadata_text = fs::read_to_string(&metadata_path).ok()?;
    let metadata: StartupTranspileCacheMetadata = serde_json::from_str(&metadata_text).ok()?;
    if !startup_transpile_cache_metadata_matches(&metadata, cache_key, graph) {
        log::debug!(
            "[startup_runtime] startup transpile cache metadata mismatch: key={}",
            cache_key
        );
        return None;
    }
    fs::read_to_string(&js_path).ok()
}

pub(super) fn write_startup_transpile_cache(
    cache_key: &str,
    graph: &StartupModuleGraph,
    executable_source_text: &str,
) {
    let Some(cache_dir) = startup_transpile_cache_dir() else {
        log::debug!("[startup_runtime] startup transpile cache unavailable: cache_dir missing");
        return;
    };
    if let Err(error) = fs::create_dir_all(&cache_dir) {
        log::debug!(
            "[startup_runtime] startup transpile cache directory create failed: path={}, error={}",
            cache_dir.display(),
            error
        );
        return;
    }

    let metadata = StartupTranspileCacheMetadata {
        schema_version: STARTUP_TRANSPILE_CACHE_SCHEMA_VERSION,
        cache_key: cache_key.to_string(),
        entry_init_path: graph.entry_id.clone(),
        files: graph
            .modules
            .iter()
            .map(|module| StartupTranspileCacheFile {
                path: module.id.clone(),
                hash: module.source_hash.clone(),
            })
            .collect(),
        transpile_option_version: STARTUP_TRANSPILE_OPTION_VERSION.to_string(),
        created_at_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default(),
        input_bytes: graph.input_bytes,
        output_bytes: executable_source_text.len(),
    };
    let js_path = cache_dir.join(format!("{cache_key}.js"));
    let metadata_path = cache_dir.join(format!("{cache_key}.json"));
    if let Err(error) = fs::write(&js_path, executable_source_text) {
        log::debug!(
            "[startup_runtime] startup transpile cache write failed: path={}, error={}",
            js_path.display(),
            error
        );
        return;
    }
    let Ok(metadata_text) = serde_json::to_string_pretty(&metadata) else {
        log::debug!(
            "[startup_runtime] startup transpile cache metadata serialization failed: key={}",
            cache_key
        );
        return;
    };
    if let Err(error) = fs::write(&metadata_path, metadata_text) {
        log::debug!(
            "[startup_runtime] startup transpile cache metadata write failed: path={}, error={}",
            metadata_path.display(),
            error
        );
    }
}

pub(super) fn startup_transpile_cache_metadata_matches(
    metadata: &StartupTranspileCacheMetadata,
    cache_key: &str,
    graph: &StartupModuleGraph,
) -> bool {
    metadata.schema_version == STARTUP_TRANSPILE_CACHE_SCHEMA_VERSION
        && metadata.cache_key == cache_key
        && metadata.entry_init_path == graph.entry_id
        && metadata.transpile_option_version == STARTUP_TRANSPILE_OPTION_VERSION
        && metadata.input_bytes == graph.input_bytes
        && metadata.files.len() == graph.modules.len()
        && metadata
            .files
            .iter()
            .zip(graph.modules.iter())
            .all(|(file, module)| file.path == module.id && file.hash == module.source_hash)
}

pub(super) fn startup_transpile_cache_dir() -> Option<PathBuf> {
    paths::cache_dir().map(|dir| dir.join(STARTUP_TRANSPILE_CACHE_DIR_NAME))
}
