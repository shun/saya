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
use crate::theme::{MarkdownSemanticStyleKey, ThemeTextStyleDeclaration};

const STARTUP_PUBLIC_SURFACE_PATHS: &[&str] = &[
    "saya.options.tabSize",
    "saya.options.tabstop",
    "saya.options.expandtab",
    "saya.options.shiftwidth",
    "saya.options.softtabstop",
    "saya.options.autoindent",
    "saya.options.smartindent",
    "saya.options.ignorecase",
    "saya.options.smartcase",
    "saya.options.syntax",
    "saya.options.scrolloff",
    "saya.options.sidescrolloff",
    "saya.options.wrap",
    "saya.options.lineNumbers",
    "saya.options.number",
    "saya.options.relativenumber",
    "saya.options.cursorline",
    "saya.options.numberWidth",
    "saya.options.numberwidth",
    "saya.options.laststatus",
    "saya.options.messageheight",
    "saya.options.messageHeight",
    "saya.options.list",
    "saya.options.listchars",
    "saya.options.foldmethod",
    "saya.options.foldlevel",
    "saya.keymap.set",
    "saya.commands.register",
    "saya.commands.execute",
    "saya.events.on",
    "saya.theme.palette",
    "saya.theme.markdown",
    "saya.log.file",
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
    op_collect_startup_bool_option,
    op_collect_startup_number_option,
    op_collect_startup_string_option,
    op_collect_startup_keymap,
    op_collect_startup_command,
    op_collect_startup_event,
    op_collect_startup_theme_palette,
    op_collect_startup_theme_markdown,
    op_collect_startup_log_file,
} = Deno.core.ops;

globalThis.saya = {
    options: {
        tabSize: 8,
        tabstop: 8,
        expandtab: false,
        shiftwidth: 8,
        softtabstop: 0,
        autoindent: false,
        smartindent: false,
        ignorecase: false,
        smartcase: false,
        syntax: false,
        scrolloff: 0,
        sidescrolloff: 0,
        wrap: true,
        lineNumbers: false,
        number: false,
        relativenumber: false,
        cursorline: false,
        numberWidth: 4,
        numberwidth: 4,
        laststatus: 2,
        messageheight: 5,
        messageHeight: 5,
        list: false,
        listchars: "tab:>-,trail:-",
        foldmethod: "manual",
        foldlevel: 0,
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
    theme: {},
    log: {},
};

Object.defineProperty(globalThis.saya.theme, "palette", {
    configurable: true,
    enumerable: true,
    get() {
        return {};
    },
    set(value) {
        op_collect_startup_theme_palette(JSON.stringify(value ?? {}));
    },
});

Object.defineProperty(globalThis.saya.theme, "markdown", {
    configurable: true,
    enumerable: true,
    get() {
        return {};
    },
    set(value) {
        op_collect_startup_theme_markdown(JSON.stringify(value ?? {}));
    },
});

Object.defineProperty(globalThis.saya.log, "file", {
    configurable: true,
    enumerable: true,
    get() {
        return undefined;
    },
    set(value) {
        if (typeof value !== "string") {
            throw new TypeError("log.file must be a string");
        }
        op_collect_startup_log_file(value);
    },
});

function defineNumberOption(propertyName, runtimeName, defaultValue) {
    Object.defineProperty(globalThis.saya.options, propertyName, {
        configurable: true,
        enumerable: true,
        get() {
            return defaultValue;
        },
        set(value) {
            op_collect_startup_number_option(runtimeName, value);
        },
    });
}

function defineBoolOption(propertyName, runtimeName, defaultValue) {
    Object.defineProperty(globalThis.saya.options, propertyName, {
        configurable: true,
        enumerable: true,
        get() {
            return defaultValue;
        },
        set(value) {
            op_collect_startup_bool_option(runtimeName, Boolean(value));
        },
    });
}

function defineStringOption(propertyName, runtimeName, defaultValue) {
    Object.defineProperty(globalThis.saya.options, propertyName, {
        configurable: true,
        enumerable: true,
        get() {
            return defaultValue;
        },
        set(value) {
            op_collect_startup_string_option(runtimeName, String(value));
        },
    });
}

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

defineBoolOption("expandtab", "expandtab", false);
defineBoolOption("et", "expandtab", false);
defineNumberOption("shiftwidth", "shiftwidth", 8);
defineNumberOption("sw", "shiftwidth", 8);
defineNumberOption("softtabstop", "softtabstop", 0);
defineNumberOption("sts", "softtabstop", 0);
defineBoolOption("autoindent", "autoindent", false);
defineBoolOption("ai", "autoindent", false);
defineBoolOption("smartindent", "smartindent", false);
defineBoolOption("si", "smartindent", false);
defineBoolOption("ignorecase", "ignorecase", false);
defineBoolOption("ic", "ignorecase", false);
defineBoolOption("smartcase", "smartcase", false);
defineBoolOption("scs", "smartcase", false);
defineBoolOption("syntax", "syntax", false);
defineNumberOption("scrolloff", "scrolloff", 0);
defineNumberOption("so", "scrolloff", 0);
defineNumberOption("sidescrolloff", "sidescrolloff", 0);
defineNumberOption("siso", "sidescrolloff", 0);
defineBoolOption("wrap", "wrap", true);
defineBoolOption("relativenumber", "relativenumber", false);
defineBoolOption("rnu", "relativenumber", false);
defineBoolOption("cursorline", "cursorline", false);
defineBoolOption("cul", "cursorline", false);
defineNumberOption("laststatus", "laststatus", 2);
defineNumberOption("ls", "laststatus", 2);
defineNumberOption("messageheight", "messageheight", 5);
defineNumberOption("messageHeight", "messageheight", 5);
defineNumberOption("mh", "messageheight", 5);
defineBoolOption("list", "list", false);
defineStringOption("listchars", "listchars", "tab:>-,trail:-");
defineStringOption("lcs", "listchars", "tab:>-,trail:-");
defineStringOption("foldmethod", "foldmethod", "manual");
defineStringOption("fdm", "foldmethod", "manual");
defineNumberOption("foldlevel", "foldlevel", 0);
defineNumberOption("fdl", "foldlevel", 0);

Object.freeze(globalThis.saya.options);
Object.freeze(globalThis.saya.keymap);
Object.freeze(globalThis.saya.commands);
Object.freeze(globalThis.saya.events);
Object.freeze(globalThis.saya.theme);
Object.freeze(globalThis.saya.log);
Object.freeze(globalThis.saya);
"#;

const STARTUP_PUBLIC_SURFACE_NAMES: &[&str] =
    &["options", "keymap", "commands", "events", "theme", "log"];
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
        tabstop: number;
        expandtab: boolean;
        shiftwidth: number;
        softtabstop: number;
        autoindent: boolean;
        smartindent: boolean;
        ignorecase: boolean;
        smartcase: boolean;
        syntax: boolean;
        scrolloff: number;
        sidescrolloff: number;
        wrap: boolean;
        lineNumbers: boolean;
        number: boolean;
        relativenumber: boolean;
        cursorline: boolean;
        numberWidth: number;
        numberwidth: number;
        laststatus: number;
        messageheight: number;
        messageHeight: number;
        list: boolean;
        listchars: string;
        foldmethod: string;
        foldlevel: number;
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

    type SayaThemeColor = string;

    interface SayaTextStyle {
        fg?: SayaThemeColor;
        bg?: SayaThemeColor;
        bold?: boolean;
        italic?: boolean;
        underline?: boolean;
        strikethrough?: boolean;
    }

    interface SayaStartupThemeSurface {
        palette: Record<string, SayaThemeColor>;
        markdown: Partial<Record<
            | "heading"
            | "heading1"
            | "heading2"
            | "heading3"
            | "heading4"
            | "heading5"
            | "heading6"
            | "inlineCode"
            | "link"
            | "listMarker"
            | "checkboxChecked"
            | "checkboxUnchecked"
            | "table"
            | "fencedCodeBlock",
            SayaTextStyle
        >>;
    }

    interface SayaStartupLogSurface {
        file?: string;
    }

    interface SayaStartupSurface {
        options: SayaStartupOptionsSurface;
        keymap: SayaStartupKeymapSurface;
        commands: SayaStartupCommandsSurface;
        events: SayaStartupEventsSurface;
        theme: SayaStartupThemeSurface;
        log: SayaStartupLogSurface;
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
fn op_collect_startup_bool_option(
    state: &mut OpState,
    #[string] name: String,
    value: bool,
) -> Result<(), JsErrorBox> {
    collect_startup_option(
        state,
        &name,
        SayaOptionValue::Boolean(value),
        "boolean startup option",
    )
}

#[op2(fast)]
fn op_collect_startup_number_option(
    state: &mut OpState,
    #[string] name: String,
    #[number] value: i64,
) -> Result<(), JsErrorBox> {
    collect_startup_option(
        state,
        &name,
        SayaOptionValue::Number(value),
        "number startup option",
    )
}

#[op2(fast)]
fn op_collect_startup_string_option(
    state: &mut OpState,
    #[string] name: String,
    #[string] value: String,
) -> Result<(), JsErrorBox> {
    collect_startup_option(
        state,
        &name,
        SayaOptionValue::String(value),
        "string startup option",
    )
}

fn collect_startup_option(
    state: &mut OpState,
    name: &str,
    value: SayaOptionValue,
    label: &str,
) -> Result<(), JsErrorBox> {
    let definition = crate::option_registry::SayaOptionRegistry::resolve(name)
        .filter(|definition| definition.startup_public)
        .ok_or_else(|| JsErrorBox::generic(format!("unsupported startup option: {name}")))?;
    if definition.value_type != value.option_type() {
        return Err(JsErrorBox::generic(format!(
            "startup option type mismatch: option={}, expected={:?}, actual={:?}",
            definition.name,
            definition.value_type,
            value.option_type()
        )));
    }
    log::debug!(
        "[startup_runtime] collect {label}: name={}, value={:?}",
        definition.name,
        value
    );
    state
        .borrow_mut::<StartupRegistry>()
        .push(StartupRegistryEntry::Option {
            name: definition.name,
            value,
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
            callback_source,
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
            callback_source,
        });

    Ok(())
}

#[op2(fast)]
fn op_collect_startup_theme_palette(
    state: &mut OpState,
    #[string] palette_json: String,
) -> Result<(), JsErrorBox> {
    let palette = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&palette_json)
        .map_err(|error| JsErrorBox::generic(format!("invalid theme palette: {error}")))?;
    log::debug!(
        "[startup_runtime] collect startup theme palette: token_count={}",
        palette.len()
    );
    let registry = state.borrow_mut::<StartupRegistry>();
    for (name, value) in palette {
        let Some(value) = value.as_str() else {
            return Err(JsErrorBox::generic(format!(
                "theme palette value must be a string: {name}"
            )));
        };
        registry.push(StartupRegistryEntry::ThemePalette {
            name,
            value: value.to_string(),
        });
    }
    Ok(())
}

#[op2(fast)]
fn op_collect_startup_theme_markdown(
    state: &mut OpState,
    #[string] markdown_json: String,
) -> Result<(), JsErrorBox> {
    let markdown =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&markdown_json)
            .map_err(|error| JsErrorBox::generic(format!("invalid markdown theme: {error}")))?;
    log::debug!(
        "[startup_runtime] collect startup markdown theme: style_count={}",
        markdown.len()
    );
    let registry = state.borrow_mut::<StartupRegistry>();
    for (name, value) in markdown {
        let key = MarkdownSemanticStyleKey::parse(&name).ok_or_else(|| {
            JsErrorBox::generic(format!("unsupported markdown theme key: {name}"))
        })?;
        let style = parse_theme_text_style(&name, value)?;
        registry.push(StartupRegistryEntry::ThemeMarkdownStyle { key, style });
    }
    Ok(())
}

#[op2(fast)]
fn op_collect_startup_log_file(
    state: &mut OpState,
    #[string] path: String,
) -> Result<(), JsErrorBox> {
    log::debug!("[startup_runtime] collect startup log file: path={}", path);
    state
        .borrow_mut::<StartupRegistry>()
        .push(StartupRegistryEntry::LogFile { path });
    Ok(())
}

fn parse_theme_text_style(
    name: &str,
    value: serde_json::Value,
) -> Result<ThemeTextStyleDeclaration, JsErrorBox> {
    let serde_json::Value::Object(object) = value else {
        return Err(JsErrorBox::generic(format!(
            "markdown theme style must be an object: {name}"
        )));
    };
    let mut style = ThemeTextStyleDeclaration::default();
    for (property, value) in object {
        match property.as_str() {
            "fg" => style.fg = Some(theme_string_property(name, &property, value)?),
            "bg" => style.bg = Some(theme_string_property(name, &property, value)?),
            "bold" => style.bold = Some(theme_bool_property(name, &property, value)?),
            "italic" => style.italic = Some(theme_bool_property(name, &property, value)?),
            "underline" => style.underline = Some(theme_bool_property(name, &property, value)?),
            "strikethrough" => {
                style.strikethrough = Some(theme_bool_property(name, &property, value)?)
            }
            other => {
                return Err(JsErrorBox::generic(format!(
                    "unsupported markdown theme style property: {name}.{other}"
                )));
            }
        }
    }
    Ok(style)
}

fn theme_string_property(
    style_name: &str,
    property: &str,
    value: serde_json::Value,
) -> Result<String, JsErrorBox> {
    value.as_str().map(ToString::to_string).ok_or_else(|| {
        JsErrorBox::generic(format!(
            "markdown theme style property must be a string: {style_name}.{property}"
        ))
    })
}

fn theme_bool_property(
    style_name: &str,
    property: &str,
    value: serde_json::Value,
) -> Result<bool, JsErrorBox> {
    value.as_bool().ok_or_else(|| {
        JsErrorBox::generic(format!(
            "markdown theme style property must be a boolean: {style_name}.{property}"
        ))
    })
}

deno_core::extension!(
    startup_saya_extension,
    ops = [
        op_collect_startup_tab_size,
        op_collect_startup_line_numbers,
        op_collect_startup_number_width,
        op_collect_startup_bool_option,
        op_collect_startup_number_option,
        op_collect_startup_string_option,
        op_collect_startup_keymap,
        op_collect_startup_command,
        op_collect_startup_event,
        op_collect_startup_theme_palette,
        op_collect_startup_theme_markdown,
        op_collect_startup_log_file
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
    let expanded_source_text = expand_local_startup_imports(module)?;
    let without_type_declarations = strip_type_declarations(&expanded_source_text);
    let without_export_modifiers = strip_export_modifiers(&without_type_declarations);
    let executable_source_text =
        normalize_assignment_spacing(&strip_type_annotations(&without_export_modifiers));
    validate_executable_module(&module.path, &executable_source_text)?;
    Ok(executable_source_text)
}

fn expand_local_startup_imports(module: &StartupModuleSource) -> Result<String, String> {
    let mut stack = Vec::new();
    expand_local_startup_imports_from_path(&module.path, &module.source_text, &mut stack)
}

fn expand_local_startup_imports_from_path(
    path: &Path,
    source_text: &str,
    stack: &mut Vec<PathBuf>,
) -> Result<String, String> {
    let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if stack.contains(&canonical_path) {
        return Err(format!(
            "startup module import cycle detected: {}",
            canonical_path.display()
        ));
    }
    stack.push(canonical_path);

    let mut output = String::with_capacity(source_text.len());
    for line in source_text.lines() {
        let Some(specifier) = parse_static_import_specifier(line) else {
            output.push_str(line);
            output.push('\n');
            continue;
        };

        let imported_path = resolve_local_startup_import(path, specifier)?;
        log::debug!(
            "[startup_runtime] inline local startup import: importer={}, specifier={}, resolved={}",
            path.display(),
            specifier,
            imported_path.display()
        );
        let imported_source = fs::read_to_string(&imported_path).map_err(|error| {
            format!(
                "failed to read startup import {} from {}: {}",
                specifier,
                path.display(),
                error
            )
        })?;
        let expanded_import =
            expand_local_startup_imports_from_path(&imported_path, &imported_source, stack)?;
        output.push_str(&expanded_import);
        output.push('\n');
    }

    stack.pop();
    Ok(output)
}

fn parse_static_import_specifier(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if !trimmed.starts_with("import ") {
        return None;
    }
    let after_from = trimmed
        .split_once(" from ")
        .map(|(_, specifier)| specifier.trim())
        .unwrap_or_else(|| trimmed.trim_start_matches("import").trim());
    parse_quoted_module_specifier(after_from.trim_end_matches(';').trim())
}

fn parse_quoted_module_specifier(value: &str) -> Option<&str> {
    let quote = value.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &value[quote.len_utf8()..];
    let end = rest.find(quote)?;
    Some(&rest[..end])
}

fn resolve_local_startup_import(importer: &Path, specifier: &str) -> Result<PathBuf, String> {
    let path = if let Some(path) = specifier.strip_prefix("file://") {
        PathBuf::from(path)
    } else if specifier.starts_with("./") || specifier.starts_with("../") {
        importer
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(specifier)
    } else if specifier.starts_with('/') {
        PathBuf::from(specifier)
    } else {
        return Err(format!(
            "unsupported startup import specifier: {} (only local file imports are supported)",
            specifier
        ));
    };

    Ok(path)
}

fn strip_type_declarations(source_text: &str) -> String {
    let mut output = String::with_capacity(source_text.len());
    let mut skipping_type_block = false;
    let mut brace_depth = 0isize;

    for line in source_text.lines() {
        let trimmed = line.trim_start();
        if !skipping_type_block
            && (trimmed.starts_with("interface ") || trimmed.starts_with("export interface "))
        {
            skipping_type_block = true;
            brace_depth += line.matches('{').count() as isize;
            brace_depth -= line.matches('}').count() as isize;
            if brace_depth <= 0 && line.contains('}') {
                skipping_type_block = false;
                brace_depth = 0;
            }
            continue;
        }

        if skipping_type_block {
            brace_depth += line.matches('{').count() as isize;
            brace_depth -= line.matches('}').count() as isize;
            if brace_depth <= 0 {
                skipping_type_block = false;
                brace_depth = 0;
            }
            continue;
        }

        output.push_str(line);
        output.push('\n');
    }

    output
}

fn strip_export_modifiers(source_text: &str) -> String {
    let mut output = String::with_capacity(source_text.len());
    for line in source_text.lines() {
        let trimmed = line.trim_start();
        let indent_len = line.len() - trimmed.len();
        let replacement = if trimmed.starts_with("export async function ")
            || trimmed.starts_with("export function ")
            || trimmed.starts_with("export const ")
            || trimmed.starts_with("export let ")
            || trimmed.starts_with("export class ")
        {
            Some(format!(
                "{}{}",
                &line[..indent_len],
                &trimmed["export ".len()..]
            ))
        } else if trimmed == "export {};" {
            Some(String::new())
        } else {
            None
        };

        match replacement {
            Some(line) => output.push_str(&line),
            None => output.push_str(line),
        }
        output.push('\n');
    }
    output
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
                if looks_like_ternary_separator(&chars, index) {
                    output.push(ch);
                    index += 1;
                    continue;
                }
                let mut lookahead = index + 1;
                while lookahead < chars.len() && chars[lookahead].is_whitespace() {
                    lookahead += 1;
                }
                if looks_like_object_literal_value(&chars, lookahead) {
                    output.push(ch);
                    index += 1;
                    continue;
                }
                while lookahead < chars.len() {
                    let next = chars[lookahead];
                    if next == '='
                        || next == ','
                        || next == ')'
                        || next == ';'
                        || next == '{'
                        || next == '\n'
                    {
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

fn looks_like_ternary_separator(chars: &[char], colon_index: usize) -> bool {
    if previous_non_whitespace(chars, colon_index) == Some('?') {
        return false;
    }

    let mut unresolved_questions = 0usize;
    let mut index = 0usize;
    let mut in_string: Option<char> = None;
    let mut escape = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;

    while index < colon_index {
        let ch = chars[index];
        let next = chars.get(index + 1).copied();

        if in_line_comment {
            if ch == '\n' {
                in_line_comment = false;
            }
            index += 1;
            continue;
        }

        if in_block_comment {
            if ch == '*' && next == Some('/') {
                in_block_comment = false;
                index += 2;
            } else {
                index += 1;
            }
            continue;
        }

        if let Some(quote) = in_string {
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
            '\'' | '"' | '`' => in_string = Some(ch),
            '/' if next == Some('/') => {
                in_line_comment = true;
                index += 1;
            }
            '/' if next == Some('*') => {
                in_block_comment = true;
                index += 1;
            }
            ';' | '{' | '}' => unresolved_questions = 0,
            '?' if next != Some('?') && next != Some('.') => unresolved_questions += 1,
            ':' if unresolved_questions > 0 => unresolved_questions -= 1,
            _ => {}
        }

        index += 1;
    }

    unresolved_questions > 0
}

fn previous_non_whitespace(chars: &[char], index: usize) -> Option<char> {
    chars
        .get(..index)?
        .iter()
        .rev()
        .find(|ch| !ch.is_whitespace())
        .copied()
}

fn looks_like_object_literal_value(chars: &[char], index: usize) -> bool {
    let Some(ch) = chars.get(index).copied() else {
        return false;
    };
    if matches!(ch, '"' | '\'' | '`' | '{' | '[' | '-' | '0'..='9') {
        return true;
    }
    let tail = chars[index..].iter().collect::<String>();
    tail.starts_with("true")
        || tail.starts_with("false")
        || tail.starts_with("null")
        || tail.starts_with("undefined")
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

fn parse_startup_keymap_action(action: &str) -> SayaKeymapAction {
    if let Some(command_name) = action.strip_prefix(STARTUP_COMMAND_REFERENCE_PREFIX) {
        return SayaKeymapAction::RegisteredCommand(command_name.to_string());
    }

    SayaKeymapAction::Literal(action.to_string())
}
