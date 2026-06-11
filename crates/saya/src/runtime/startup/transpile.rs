//! startup モジュールグラフの収集と TypeScript から JS へのトランスパイル。

use super::*;

#[derive(Debug, Clone)]
pub(super) struct StartupModuleGraph {
    pub(super) entry_id: String,
    pub(super) modules: Vec<StartupGraphModule>,
    pub(super) input_bytes: usize,
}

#[derive(Debug, Clone)]
pub(super) struct StartupGraphModule {
    pub(super) id: String,
    pub(super) path: PathBuf,
    pub(super) source_text: String,
    pub(super) source_hash: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct StartupTranspileCacheMetadata {
    pub(super) schema_version: u32,
    pub(super) cache_key: String,
    pub(super) entry_init_path: String,
    pub(super) files: Vec<StartupTranspileCacheFile>,
    pub(super) transpile_option_version: String,
    pub(super) created_at_unix_ms: u128,
    pub(super) input_bytes: usize,
    pub(super) output_bytes: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct StartupTranspileCacheFile {
    pub(super) path: String,
    pub(super) hash: String,
}

pub(super) fn transpile_typescript_module(
    module: &StartupModuleSource,
    prepare_started: Instant,
) -> Result<String, String> {
    log::debug!(
        "[startup_runtime] transpile init module source: path={}, len={}",
        module.path.display(),
        module.source_text.len()
    );
    let graph_started = Instant::now();
    let graph = collect_startup_module_graph(module)?;
    let graph_ms = graph_started.elapsed().as_millis();
    let cache_key = startup_transpile_cache_key(&graph);

    let cache_read_started = Instant::now();
    if let Some(executable_source_text) = read_startup_transpile_cache(&cache_key, &graph) {
        log::debug!(
            "[startup_runtime] startup transpile cache hit: key={}, modules={}, cache_read_ms={}, total_prepare_ms={}, input_bytes={}, output_bytes={}",
            cache_key,
            graph.modules.len(),
            cache_read_started.elapsed().as_millis(),
            prepare_started.elapsed().as_millis(),
            graph.input_bytes,
            executable_source_text.len()
        );
        return Ok(executable_source_text);
    }
    let cache_read_ms = cache_read_started.elapsed().as_millis();

    let transpile_started = Instant::now();
    let executable_source_text = transpile_startup_module_graph(&graph)?;
    validate_executable_module(&module.path, &executable_source_text)?;
    let transpile_ms = transpile_started.elapsed().as_millis();

    let cache_write_started = Instant::now();
    write_startup_transpile_cache(&cache_key, &graph, &executable_source_text);
    let cache_write_ms = cache_write_started.elapsed().as_millis();

    log::debug!(
        "[startup_runtime] startup transpile cache miss: key={}, modules={}, graph_ms={}, transpile_ms={}, cache_read_ms={}, cache_write_ms={}, total_prepare_ms={}, input_bytes={}, output_bytes={}",
        cache_key,
        graph.modules.len(),
        graph_ms,
        transpile_ms,
        cache_read_ms,
        cache_write_ms,
        prepare_started.elapsed().as_millis(),
        graph.input_bytes,
        executable_source_text.len()
    );
    Ok(executable_source_text)
}

pub(super) fn collect_startup_module_graph(
    module: &StartupModuleSource,
) -> Result<StartupModuleGraph, String> {
    let mut modules = Vec::new();
    let mut visited = HashSet::new();
    let mut stack = Vec::new();
    collect_startup_module_graph_from_source(
        &module.path,
        &module.source_text,
        &mut modules,
        &mut visited,
        &mut stack,
    )?;
    let entry_id = canonical_startup_module_id(&module.path);
    let input_bytes = modules
        .iter()
        .map(|module| module.source_text.len())
        .sum::<usize>();
    Ok(StartupModuleGraph {
        entry_id,
        modules,
        input_bytes,
    })
}

pub(super) fn collect_startup_module_graph_from_source(
    path: &Path,
    source_text: &str,
    modules: &mut Vec<StartupGraphModule>,
    visited: &mut HashSet<String>,
    stack: &mut Vec<String>,
) -> Result<(), String> {
    let module_id = canonical_startup_module_id(path);
    if stack.contains(&module_id) {
        return Err(format!(
            "startup module import cycle detected: {}",
            path.display()
        ));
    }
    if visited.contains(&module_id) {
        return Ok(());
    }

    stack.push(module_id.clone());
    let import_specifiers = startup_static_import_specifiers(source_text);
    for specifier in import_specifiers {
        let imported_path = resolve_local_startup_import(path, &specifier)?;
        let imported_source = fs::read_to_string(&imported_path).map_err(|error| {
            format!(
                "failed to read startup import {} from {}: {}",
                specifier,
                path.display(),
                error
            )
        })?;
        collect_startup_module_graph_from_source(
            &imported_path,
            &imported_source,
            modules,
            visited,
            stack,
        )?;
    }
    stack.pop();

    let source_hash = sha256_hex(source_text.as_bytes());
    modules.push(StartupGraphModule {
        id: module_id.clone(),
        path: path.to_path_buf(),
        source_text: source_text.to_string(),
        source_hash,
    });
    visited.insert(module_id);
    Ok(())
}

pub(super) fn startup_static_import_specifiers(source_text: &str) -> Vec<String> {
    let mut specifiers = Vec::new();
    let lines: Vec<&str> = source_text.lines().collect();
    let mut index = 0usize;
    while index < lines.len() {
        let (statement, consumed_lines) = collect_static_import_statement(&lines, index);
        if let Some(specifier) = parse_static_import_specifier(&statement)
            .or_else(|| parse_static_re_export_specifier(&statement))
        {
            specifiers.push(specifier.to_string());
        }
        index += consumed_lines;
    }
    specifiers
}

pub(super) fn transpile_startup_module_graph(graph: &StartupModuleGraph) -> Result<String, String> {
    let mut output = String::new();
    for module in &graph.modules {
        let transpiled = transpile_startup_module_to_js(module)?;
        output.push_str(&strip_es_module_syntax_from_transpiled_js(&transpiled));
        output.push('\n');
    }
    Ok(output)
}

pub(super) fn transpile_startup_module_to_js(
    module: &StartupGraphModule,
) -> Result<String, String> {
    let media_type = MediaType::from_path(&module.path);
    let specifier = deno_core::resolve_url_or_path(&module.path.to_string_lossy(), Path::new("."))
        .map_err(|error| error.to_string())?;
    let parsed = parse_module(ParseParams {
        specifier,
        text: Arc::from(module.source_text.as_str()),
        media_type,
        capture_tokens: false,
        scope_analysis: true,
        maybe_syntax: None,
    })
    .map_err(|error| error.to_string())?;
    let emitted = parsed
        .transpile(
            &TranspileOptions::default(),
            &TranspileModuleOptions::default(),
            &EmitOptions {
                source_map: SourceMapOption::None,
                source_map_base: None,
                source_map_file: None,
                inline_sources: false,
                remove_comments: false,
            },
        )
        .map_err(|error| error.to_string())?
        .into_source();
    Ok(emitted.text)
}

pub(super) fn strip_es_module_syntax_from_transpiled_js(source_text: &str) -> String {
    let mut output = String::with_capacity(source_text.len());
    let lines: Vec<&str> = source_text.lines().collect();
    let mut index = 0usize;
    while index < lines.len() {
        let line = lines[index];
        let (statement, consumed_lines) = collect_static_import_statement(&lines, index);
        if parse_static_import_specifier(&statement).is_some()
            || parse_static_re_export_specifier(&statement).is_some()
        {
            index += consumed_lines;
            continue;
        }

        let trimmed = line.trim_start();
        let indent_len = line.len() - trimmed.len();
        if trimmed.starts_with("export async function ")
            || trimmed.starts_with("export function ")
            || trimmed.starts_with("export const ")
            || trimmed.starts_with("export let ")
            || trimmed.starts_with("export class ")
        {
            output.push_str(&line[..indent_len]);
            output.push_str(&trimmed["export ".len()..]);
            output.push('\n');
        } else if trimmed == "export {};"
            || (trimmed.starts_with("export {") && trimmed.ends_with(';'))
        {
            output.push('\n');
        } else {
            output.push_str(line);
            output.push('\n');
        }
        index += 1;
    }
    output
}
