use std::fs;
use std::path::{Path, PathBuf};

use deno_core::{OpState, RuntimeOptions, op2};
use deno_error::JsErrorBox;

pub use crate::config_runtime::{
    SayaKeyMode, SayaKeymapAction, SayaOptionName, SayaOptionValue, StartupRegistry,
    StartupRegistryEntry,
};
pub use crate::config_runtime::{
    SayaOptionName as StartupOptionName, SayaOptionValue as StartupOptionValue,
};

const STARTUP_PUBLIC_SURFACE_PATHS: &[&str] = &[
    "saya.options.tabSize",
    "saya.options.lineNumbers",
    "saya.options.numberWidth",
    "saya.keymap.set",
    "saya.commands.register",
    "saya.commands.execute",
    "saya.events.on",
];

const STARTUP_COMMAND_REFERENCE_PREFIX: &str = "__SAYA_STARTUP_COMMAND_REF__:";

/// Formal startup surface は TypeScript API に限定し、文字列 DSL は含めない。
pub fn startup_public_surface_paths() -> &'static [&'static str] {
    STARTUP_PUBLIC_SURFACE_PATHS
}

const STARTUP_SAYA_BOOTSTRAP: &str = r#"
const {
    op_collect_startup_tab_size,
    op_collect_startup_line_numbers,
    op_collect_startup_number_width,
    op_collect_startup_keymap,
    op_collect_startup_command,
    op_collect_startup_event,
} = Deno.core.ops;

globalThis.saya = {
    options: {
        tabSize: 8,
        lineNumbers: false,
        numberWidth: 4,
    },
    keymap: {
        set(mode, lhs, action) {
            if (typeof lhs !== "string") {
                throw new TypeError("keymap lhs must be a string");
            }
            if (typeof action !== "string") {
                throw new TypeError("keymap action must be a string");
            }
            op_collect_startup_keymap(String(mode), lhs, action);
        },
    },
    commands: {
        register(name, callback) {
            if (typeof name !== "string") {
                throw new TypeError("command name must be a string");
            }
            if (typeof callback !== "function") {
                throw new TypeError("command callback must be a function");
            }
            op_collect_startup_command(name, callback.toString());
        },
        execute(name) {
            if (typeof name !== "string") {
                throw new TypeError("command reference name must be a string");
            }
            return "__SAYA_STARTUP_COMMAND_REF__:" + name;
        },
    },
    events: {
        on(name, callback) {
            if (typeof name !== "string") {
                throw new TypeError("event name must be a string");
            }
            if (typeof callback !== "function") {
                throw new TypeError("event callback must be a function");
            }
            op_collect_startup_event(name, callback.toString());
        },
    },
};

Object.defineProperty(globalThis.saya.options, "tabSize", {
    configurable: true,
    enumerable: true,
    get() {
        return 8;
    },
    set(value) {
        op_collect_startup_tab_size(value);
    },
});

Object.defineProperty(globalThis.saya.options, "tabstop", {
    configurable: true,
    enumerable: true,
    get() {
        return 8;
    },
    set(value) {
        op_collect_startup_tab_size(value);
    },
});

Object.defineProperty(globalThis.saya.options, "lineNumbers", {
    configurable: true,
    enumerable: true,
    get() {
        return false;
    },
    set(value) {
        op_collect_startup_line_numbers(Boolean(value));
    },
});

Object.defineProperty(globalThis.saya.options, "number", {
    configurable: true,
    enumerable: true,
    get() {
        return false;
    },
    set(value) {
        op_collect_startup_line_numbers(Boolean(value));
    },
});

Object.defineProperty(globalThis.saya.options, "numberWidth", {
    configurable: true,
    enumerable: true,
    get() {
        return 4;
    },
    set(value) {
        op_collect_startup_number_width(value);
    },
});

Object.defineProperty(globalThis.saya.options, "numberwidth", {
    configurable: true,
    enumerable: true,
    get() {
        return 4;
    },
    set(value) {
        op_collect_startup_number_width(value);
    },
});

Object.defineProperty(globalThis.saya.options, "nuw", {
    configurable: true,
    enumerable: true,
    get() {
        return 4;
    },
    set(value) {
        op_collect_startup_number_width(value);
    },
});

Object.freeze(globalThis.saya.options);
Object.freeze(globalThis.saya.keymap);
Object.freeze(globalThis.saya.commands);
Object.freeze(globalThis.saya.events);
Object.freeze(globalThis.saya);
"#;

const STARTUP_PUBLIC_SURFACE_NAMES: &[&str] = &["options", "keymap", "commands", "events"];
const STARTUP_FORBIDDEN_SURFACE_NAMES: &[&str] = &["filesystem", "network"];

pub const STARTUP_SAYA_TYPE_DECLARATION: &str = r#"
declare global {
    type SayaStartupKeymapMode = "normal" | "insert" | "visual";

    interface SayaReadonlyBufferSnapshot {
        id: number;
        path: string | null;
        lineCount: number;
    }

    interface SayaBufferEventPayload {
        buffer: SayaReadonlyBufferSnapshot;
    }

    interface SayaStartupOptionsSurface {
        tabSize: number;
        lineNumbers: boolean;
        numberWidth: number;
    }

    interface SayaStartupKeymapSurface {
        set(
            mode: SayaStartupKeymapMode,
            lhs: string,
            action: string | SayaStartupCommandReference,
        ): void;
    }

    interface SayaStartupCommandReference {
        readonly __sayaStartupCommandReference: unique symbol;
    }

    interface SayaStartupCommandsSurface {
        register(name: string, callback: (...args: unknown[]) => unknown): void;
        execute(name: string): SayaStartupCommandReference;
    }

    interface SayaStartupEventsSurface {
        on(
            name: "bufferOpen" | "bufferWritePost",
            callback: (payload: SayaBufferEventPayload) => unknown,
        ): void;
    }

    interface SayaStartupSurface {
        options: SayaStartupOptionsSurface;
        keymap: SayaStartupKeymapSurface;
        commands: SayaStartupCommandsSurface;
        events: SayaStartupEventsSurface;
    }

    var saya: SayaStartupSurface;
}

export {};
"#;

#[op2(fast)]
fn op_collect_startup_tab_size(
    state: &mut OpState,
    #[number] value: i64,
) -> Result<(), JsErrorBox> {
    log::debug!(
        "[startup_runtime] collect startup tabSize option from runtime: value={}",
        value
    );

    state
        .borrow_mut::<StartupRegistry>()
        .push(StartupRegistryEntry::Option {
            name: SayaOptionName::TabSize,
            value: SayaOptionValue::Number(value),
        });

    Ok(())
}

#[op2(fast)]
fn op_collect_startup_line_numbers(state: &mut OpState, value: bool) -> Result<(), JsErrorBox> {
    log::debug!(
        "[startup_runtime] collect startup lineNumbers option from runtime: value={}",
        value
    );

    state
        .borrow_mut::<StartupRegistry>()
        .push(StartupRegistryEntry::Option {
            name: SayaOptionName::LineNumbers,
            value: SayaOptionValue::Boolean(value),
        });

    Ok(())
}

#[op2(fast)]
fn op_collect_startup_number_width(
    state: &mut OpState,
    #[number] value: i64,
) -> Result<(), JsErrorBox> {
    log::debug!(
        "[startup_runtime] collect startup numberWidth option from runtime: value={}",
        value
    );

    state
        .borrow_mut::<StartupRegistry>()
        .push(StartupRegistryEntry::Option {
            name: SayaOptionName::NumberWidth,
            value: SayaOptionValue::Number(value),
        });

    Ok(())
}

#[op2(fast)]
fn op_collect_startup_keymap(
    state: &mut OpState,
    #[string] mode: String,
    #[string] lhs: String,
    #[string] action: String,
) -> Result<(), JsErrorBox> {
    log::debug!(
        "[startup_runtime] collect startup keymap from runtime: mode={}, lhs={}, action={}",
        mode,
        lhs,
        action
    );

    let mode = match mode.as_str() {
        "normal" => SayaKeyMode::Normal,
        "insert" => SayaKeyMode::Insert,
        "visual" => SayaKeyMode::Visual,
        other => {
            return Err(JsErrorBox::generic(format!(
                "unsupported keymap mode: {}",
                other
            )));
        }
    };

    state
        .borrow_mut::<StartupRegistry>()
        .push(StartupRegistryEntry::Keymap {
            mode,
            lhs,
            action: parse_startup_keymap_action(&action),
        });

    Ok(())
}

#[op2(fast)]
fn op_collect_startup_command(
    state: &mut OpState,
    #[string] name: String,
    #[string] callback_source: String,
) -> Result<(), JsErrorBox> {
    log::debug!(
        "[startup_runtime] collect startup command from runtime: name={}",
        name
    );

    state
        .borrow_mut::<StartupRegistry>()
        .push(StartupRegistryEntry::Command {
            name,
            callback_source: collapse_whitespace(&callback_source),
        });

    Ok(())
}

#[op2(fast)]
fn op_collect_startup_event(
    state: &mut OpState,
    #[string] name: String,
    #[string] callback_source: String,
) -> Result<(), JsErrorBox> {
    log::debug!(
        "[startup_runtime] collect startup event from runtime: name={}",
        name
    );

    state
        .borrow_mut::<StartupRegistry>()
        .push(StartupRegistryEntry::Event {
            name,
            callback_source: collapse_whitespace(&callback_source),
        });

    Ok(())
}

deno_core::extension!(
    startup_saya_extension,
    ops = [
        op_collect_startup_tab_size,
        op_collect_startup_line_numbers,
        op_collect_startup_number_width,
        op_collect_startup_keymap,
        op_collect_startup_command,
        op_collect_startup_event
    ],
    state = |state| state.put(StartupRegistry::default())
);

/// `init.ts` を `deno_core` へ渡す前の最小解決ヘルパー。
pub fn resolve_init_module_specifier(
    specifier: &str,
    current_dir: &Path,
) -> Result<deno_core::url::Url, deno_core::anyhow::Error> {
    log::debug!(
        "[startup_runtime] resolve init module specifier: specifier={}, current_dir={}",
        specifier,
        current_dir.display()
    );
    deno_core::resolve_url_or_path(specifier, current_dir).map_err(Into::into)
}

/// `init.ts` を local file module として読み込んだ結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupModuleLoadResult {
    Success(StartupModuleSource),
    ReadFailed { path: PathBuf, message: String },
}

/// 読み込み成功時の startup module 情報。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupModuleSource {
    pub path: PathBuf,
    pub specifier: deno_core::url::Url,
    pub source_text: String,
}

/// `deno_core` に渡せる executable module の準備結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupModulePrepareResult {
    Success(PreparedStartupModule),
    ReadFailed { path: PathBuf, message: String },
    TranspileFailed { path: PathBuf, message: String },
}

/// transpile 後の startup module 情報。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedStartupModule {
    pub path: PathBuf,
    pub specifier: deno_core::url::Url,
    pub source_text: String,
    pub executable_source_text: String,
}

/// `init.ts` を読み込み、`deno_core` に渡せる local file module へ正規化する。
pub fn load_init_module(path: &Path, current_dir: &Path) -> StartupModuleLoadResult {
    log::debug!(
        "[startup_runtime] load init module: path={}, current_dir={}",
        path.display(),
        current_dir.display()
    );

    let specifier = match path.to_str() {
        Some(specifier) => match deno_core::resolve_url_or_path(specifier, current_dir) {
            Ok(specifier) => specifier,
            Err(error) => {
                log::debug!(
                    "[startup_runtime] init module specifier resolution failed: path={}, error={}",
                    path.display(),
                    error
                );
                return StartupModuleLoadResult::ReadFailed {
                    path: path.to_path_buf(),
                    message: error.to_string(),
                };
            }
        },
        None => {
            let message = "config path に有効な UTF-8 を含める必要があります".to_string();
            log::debug!(
                "[startup_runtime] init module path is not valid UTF-8: path={}",
                path.display()
            );
            return StartupModuleLoadResult::ReadFailed {
                path: path.to_path_buf(),
                message,
            };
        }
    };

    match fs::read_to_string(path) {
        Ok(source_text) => {
            log::debug!(
                "[startup_runtime] init module read success: path={}, len={}",
                path.display(),
                source_text.len()
            );
            StartupModuleLoadResult::Success(StartupModuleSource {
                path: path.to_path_buf(),
                specifier,
                source_text,
            })
        }
        Err(error) => {
            log::debug!(
                "[startup_runtime] init module read failed: path={}, error={}",
                path.display(),
                error
            );
            StartupModuleLoadResult::ReadFailed {
                path: path.to_path_buf(),
                message: error.to_string(),
            }
        }
    }
}

/// `init.ts` を runtime 評価可能な executable module へ変換する。
pub fn prepare_init_module(path: &Path, current_dir: &Path) -> StartupModulePrepareResult {
    log::debug!(
        "[startup_runtime] prepare init module: path={}, current_dir={}",
        path.display(),
        current_dir.display()
    );

    let loaded = match load_init_module(path, current_dir) {
        StartupModuleLoadResult::Success(module) => module,
        StartupModuleLoadResult::ReadFailed { path, message } => {
            return StartupModulePrepareResult::ReadFailed { path, message };
        }
    };

    match transpile_typescript_module(&loaded) {
        Ok(executable_source_text) => {
            log::debug!(
                "[startup_runtime] init module transpile success: path={}, output_len={}",
                loaded.path.display(),
                executable_source_text.len()
            );
            StartupModulePrepareResult::Success(PreparedStartupModule {
                path: loaded.path,
                specifier: loaded.specifier,
                source_text: loaded.source_text,
                executable_source_text,
            })
        }
        Err(message) => {
            log::debug!(
                "[startup_runtime] init module transpile failed: path={}, error={}",
                loaded.path.display(),
                message
            );
            StartupModulePrepareResult::TranspileFailed {
                path: loaded.path,
                message,
            }
        }
    }
}

fn transpile_typescript_module(module: &StartupModuleSource) -> Result<String, String> {
    log::debug!(
        "[startup_runtime] transpile init module source: path={}, len={}",
        module.path.display(),
        module.source_text.len()
    );
    let executable_source_text =
        normalize_assignment_spacing(&strip_type_annotations(&module.source_text));
    validate_executable_module(&module.path, &executable_source_text)?;
    Ok(executable_source_text)
}

fn strip_type_annotations(source_text: &str) -> String {
    let mut output = String::with_capacity(source_text.len());
    let chars: Vec<char> = source_text.chars().collect();
    let mut index = 0usize;
    let mut in_string: Option<char> = None;
    let mut escape = false;

    while index < chars.len() {
        let ch = chars[index];

        if let Some(quote) = in_string {
            output.push(ch);
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == quote {
                in_string = None;
            }
            index += 1;
            continue;
        }

        match ch {
            '\'' | '"' | '`' => {
                in_string = Some(ch);
                output.push(ch);
                index += 1;
            }
            ':' => {
                let mut lookahead = index + 1;
                while lookahead < chars.len() && chars[lookahead].is_whitespace() {
                    lookahead += 1;
                }
                while lookahead < chars.len() {
                    let next = chars[lookahead];
                    if next == '=' || next == ',' || next == ')' || next == ';' || next == '\n' {
                        break;
                    }
                    lookahead += 1;
                }
                index = lookahead;
            }
            _ => {
                output.push(ch);
                index += 1;
            }
        }
    }

    output
}

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

fn normalize_assignment_spacing(source_text: &str) -> String {
    let mut output = String::with_capacity(source_text.len());
    let chars: Vec<char> = source_text.chars().collect();
    let mut index = 0usize;
    let mut in_string: Option<char> = None;
    let mut escape = false;

    while index < chars.len() {
        let ch = chars[index];

        if let Some(quote) = in_string {
            output.push(ch);
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == quote {
                in_string = None;
            }
            index += 1;
            continue;
        }

        match ch {
            '\'' | '"' | '`' => {
                in_string = Some(ch);
                output.push(ch);
            }
            '=' => {
                let prev_is_operator = output.ends_with('=')
                    || output.ends_with('!')
                    || output.ends_with('<')
                    || output.ends_with('>')
                    || output.ends_with('-');
                let next = chars.get(index + 1).copied();
                let next_is_operator = matches!(next, Some('=') | Some('>'));

                if prev_is_operator || next_is_operator {
                    output.push('=');
                } else {
                    if !output.ends_with(' ') && !output.ends_with('\n') {
                        output.push(' ');
                    }
                    output.push('=');
                    if !matches!(next, Some(' ') | Some('\n')) {
                        output.push(' ');
                    }
                }
            }
            _ => output.push(ch),
        }

        index += 1;
    }

    output
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

fn collapse_whitespace(source: &str) -> String {
    source
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

fn parse_startup_keymap_action(action: &str) -> SayaKeymapAction {
    if let Some(command_name) = action.strip_prefix(STARTUP_COMMAND_REFERENCE_PREFIX) {
        return SayaKeymapAction::RegisteredCommand(command_name.to_string());
    }

    SayaKeymapAction::Literal(action.to_string())
}
