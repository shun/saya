use std::collections::HashSet;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use deno_ast::{
    EmitOptions, MediaType, ParseParams, SourceMapOption, TranspileModuleOptions, TranspileOptions,
    parse_module,
};
use deno_core::{OpState, RuntimeOptions, op2};
use deno_error::JsErrorBox;
use log::LevelFilter;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::presentation::theme::{
    FilerSemanticStyleKey, MarkdownSemanticStyleKey, SyntaxSemanticStyleKey,
    ThemeTextStyleDeclaration, UiStyleKey,
};
pub use crate::runtime::config::{
    FtPluginDefinition, FtPluginOption, FtPluginStartupAction, SayaKeyMode, SayaKeymapAction,
    SayaOptionName, SayaOptionValue, StartupPluginDeclaration, StartupPluginSource,
    StartupRegistry, StartupRegistryEntry, StatusLineConfig, StatusLineSegment,
};
pub use crate::runtime::config::{
    SayaOptionName as StartupOptionName, SayaOptionValue as StartupOptionValue,
};
use crate::runtime::options::{SayaOptionRegistry, SayaOptionType};
use crate::support::paths;

mod cache;
mod import_resolve;
mod loader;
mod ops;
mod saya_payload;
mod transpile;

use cache::*;
use import_resolve::*;
use ops::*;
use saya_payload::*;
use transpile::*;

pub use loader::{
    PreparedStartupModule, StartupModuleLoadResult, StartupModulePrepareResult,
    StartupModuleSource, load_init_module, prepare_init_module, resolve_init_module_specifier,
};
pub use saya_payload::STARTUP_SAYA_TYPE_DECLARATION;

const STARTUP_PUBLIC_SURFACE_PATHS: &[&str] = &[
    "saya.options.tabstop",
    "saya.options.expandtab",
    "saya.options.shiftwidth",
    "saya.options.softtabstop",
    "saya.options.autoindent",
    "saya.options.smartindent",
    "saya.options.ignorecase",
    "saya.options.smartcase",
    "saya.options.hlsearch",
    "saya.options.syntax",
    "saya.options.scrolloff",
    "saya.options.sidescrolloff",
    "saya.options.wrap",
    "saya.options.number",
    "saya.options.relativenumber",
    "saya.options.cursorline",
    "saya.options.numberwidth",
    "saya.options.laststatus",
    "saya.options.cmdheight",
    "saya.options.list",
    "saya.options.listchars",
    "saya.options.mermaidpreview",
    "saya.options.mermaidpreviewbackground",
    "saya.options.mermaidpreviewwidth",
    "saya.options.mermaidpreviewheight",
    "saya.options.foldmethod",
    "saya.options.foldlevel",
    "saya.keymap.set",
    "saya.commands.register",
    "saya.commands.execute",
    "saya.events.on",
    "saya.ftplugin.enabled",
    "saya.ftplugin.set",
    "saya.ftplugin.disable",
    "saya.statusline.set",
    "saya.theme.palette",
    "saya.theme.ui",
    "saya.theme.syntax",
    "saya.theme.languages",
    "saya.theme.filer",
    "saya.theme.markdown",
    "saya.log.file",
    "saya.log.level",
    "saya.plugins.use",
    "saya.plugins.lazy",
];

const STARTUP_COMMAND_REFERENCE_PREFIX: &str = "__SAYA_STARTUP_COMMAND_REF__:";
const STARTUP_TRANSPILE_CACHE_SCHEMA_VERSION: u32 = 1;
const STARTUP_TRANSPILE_CACHE_DIR_NAME: &str = "startup-transpile";
const STARTUP_TRANSPILE_OPTION_VERSION: &str = "deno_ast=0.53.2,module=EsmBundled,source_map=None";

/// Formal startup surface は TypeScript API に限定し、文字列 DSL は含めない。
pub fn startup_public_surface_paths() -> &'static [&'static str] {
    STARTUP_PUBLIC_SURFACE_PATHS
}

const STARTUP_PUBLIC_SURFACE_NAMES: &[&str] = &[
    "options",
    "keymap",
    "commands",
    "events",
    "ftplugin",
    "statusline",
    "theme",
    "log",
    "plugins",
];
const STARTUP_FORBIDDEN_SURFACE_NAMES: &[&str] = &["filesystem", "network"];

fn validate_executable_module(path: &Path, executable_source_text: &str) -> Result<(), String> {
    let normalized = executable_source_text
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ");

    if normalized.contains("= ;") || normalized.contains("=;") {
        return Err(format!(
            "transpile 後も無効な代入式が残っています: {}",
            path.display()
        ));
    }

    Ok(())
}

/// startup-only `saya` namespace を注入した `deno_core` runtime を生成する。
pub fn create_startup_runtime() -> deno_core::JsRuntime {
    log::debug!("[startup_runtime] create startup runtime with saya namespace");
    let mut runtime = deno_core::JsRuntime::new(RuntimeOptions {
        extensions: vec![startup_saya_extension::init()],
        ..Default::default()
    });
    runtime
        .execute_script("<saya-startup-bootstrap>", STARTUP_SAYA_BOOTSTRAP)
        .expect("startup saya bootstrap should evaluate");
    runtime
}

/// startup phase の正式な `saya` 公開面を返す。
pub fn startup_public_surface_names() -> &'static [&'static str] {
    STARTUP_PUBLIC_SURFACE_NAMES
}

/// MVP から除外する危険な capability 名を返す。
pub fn startup_forbidden_surface_names() -> &'static [&'static str] {
    STARTUP_FORBIDDEN_SURFACE_NAMES
}

/// startup-only namespace を使って `init.ts` 相当の module を評価する。
pub async fn evaluate_startup_module(source_text: &str) -> Result<(), String> {
    collect_startup_registry(source_text).await.map(|_| ())
}

/// startup-only namespace を使って `init.ts` 相当の module を評価し registry を返す。
pub async fn collect_startup_registry(source_text: &str) -> Result<StartupRegistry, String> {
    log::debug!(
        "[startup_runtime] evaluate startup module with saya namespace: len={}",
        source_text.len()
    );

    let current_dir = std::env::current_dir().map_err(|error| error.to_string())?;
    let specifier = resolve_init_module_specifier("init.ts", &current_dir)
        .map_err(|error| error.to_string())?;
    let mut runtime = create_startup_runtime();
    let module_id = runtime
        .load_main_es_module_from_code(&specifier, source_text.to_string())
        .await
        .map_err(|error| error.to_string())?;
    let evaluation = runtime.mod_evaluate(module_id);
    runtime
        .run_event_loop(Default::default())
        .await
        .map_err(|error| error.to_string())?;
    evaluation.await.map_err(|error| error.to_string())?;
    let op_state = runtime.op_state();
    Ok(op_state.borrow().borrow::<StartupRegistry>().clone())
}

fn parse_startup_keymap_action(action: &str) -> SayaKeymapAction {
    if let Some(command_name) = action.strip_prefix(STARTUP_COMMAND_REFERENCE_PREFIX) {
        return SayaKeymapAction::RegisteredCommand(command_name.to_string());
    }

    SayaKeymapAction::Literal(action.to_string())
}
