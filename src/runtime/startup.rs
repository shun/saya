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

const STARTUP_PUBLIC_SURFACE_PATHS: &[&str] = &[
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

const STARTUP_SAYA_BOOTSTRAP: &str = r#"
const {
    op_collect_startup_tabstop,
    op_collect_startup_line_numbers,
    op_collect_startup_number_width,
    op_collect_startup_bool_option,
    op_collect_startup_number_option,
    op_collect_startup_string_option,
    op_collect_startup_keymap,
    op_collect_startup_command,
    op_collect_startup_event,
    op_collect_startup_ftplugin_enabled,
    op_collect_startup_ftplugin_definition,
    op_collect_startup_ftplugin_disable,
    op_collect_startup_statusline,
    op_collect_startup_theme_palette,
    op_collect_startup_theme_ui,
    op_collect_startup_theme_syntax,
    op_collect_startup_theme_languages,
    op_collect_startup_theme_filer,
    op_collect_startup_theme_markdown,
    op_collect_startup_log_file,
    op_collect_startup_log_level,
    op_collect_startup_plugin_use,
    op_collect_startup_plugin_lazy,
    op_collect_startup_warning,
} = Deno.core.ops;

globalThis.saya = {
    options: {
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
        number: false,
        relativenumber: false,
        cursorline: false,
        numberwidth: 4,
        laststatus: 2,
        cmdheight: 5,
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
    statusline: {
        set(config) {
            if (config === null || typeof config !== "object" || Array.isArray(config)) {
                throw new TypeError("statusline config must be an object");
            }
            op_collect_startup_statusline(JSON.stringify(config));
        },
    },
    ftplugin: {
        set(filetype, definition) {
            if (typeof filetype !== "string" || filetype.trim().length === 0) {
                throw new TypeError("ftplugin filetype must be a non-empty string");
            }
            if (definition === null || typeof definition !== "object" || Array.isArray(definition)) {
                throw new TypeError("ftplugin definition must be an object");
            }
            op_collect_startup_ftplugin_definition(filetype, JSON.stringify(definition));
        },
        disable(filetype) {
            if (typeof filetype !== "string" || filetype.trim().length === 0) {
                throw new TypeError("ftplugin filetype must be a non-empty string");
            }
            op_collect_startup_ftplugin_disable(filetype);
        },
    },
    theme: {},
    log: {},
    plugins: {},
};

function normalizePluginDeclaration(spec, lazy) {
    if (spec === null || typeof spec !== "object" || Array.isArray(spec)) {
        throw new TypeError("plugin spec must be an object");
    }
    const hasLocal = typeof spec.local === "string" && spec.local.trim().length > 0;
    const hasGithub = typeof spec.github === "string" && spec.github.trim().length > 0;
    if (hasLocal === hasGithub) {
        throw new TypeError("plugin spec must set exactly one of local or github");
    }
    if (hasGithub && !/^[^/\s]+\/[^/\s]+$/.test(spec.github)) {
        throw new TypeError("plugin github source must use owner/repository form");
    }
    const sourcePath = hasLocal ? spec.local.trim() : spec.github.trim();
    const inferredName = sourcePath
        .replace(/\/+$/, "")
        .split("/")
        .pop()
        .replace(/\.git$/, "");
    const name = typeof spec.name === "string" && spec.name.trim().length > 0
        ? spec.name.trim()
        : inferredName;
    if (!name) {
        throw new TypeError("plugin name could not be inferred");
    }
    const declaration = {
        name,
        module: typeof spec.module === "string" && spec.module.trim().length > 0
            ? spec.module.trim()
            : "mod.ts",
        setup: typeof spec.setup === "string" && spec.setup.trim().length > 0
            ? spec.setup.trim()
            : "setup",
        commands: lazy ? [...(spec.commands ?? [])] : [],
        events: lazy ? [...(spec.events ?? [])] : [],
        options: Object.prototype.hasOwnProperty.call(spec, "options") ? spec.options : null,
    };
    if (hasLocal) {
        declaration.source = { kind: "local", path: spec.local.trim() };
    } else {
        declaration.source = {
            kind: "github",
            repo: spec.github.trim(),
            rev: typeof spec.rev === "string" && spec.rev.trim().length > 0
                ? spec.rev.trim()
                : null,
        };
    }
    for (const command of declaration.commands) {
        if (typeof command !== "string" || command.trim().length === 0) {
            throw new TypeError("lazy plugin commands must be non-empty strings");
        }
    }
    for (const event of declaration.events) {
        if (typeof event !== "string" || event.trim().length === 0) {
            throw new TypeError("lazy plugin events must be non-empty strings");
        }
    }
    return declaration;
}

function collectPluginDeclarations(specs, lazy) {
    if (!Array.isArray(specs)) {
        throw new TypeError("plugin declarations must be an array");
    }
    const declarations = specs.map((spec) => normalizePluginDeclaration(spec, lazy));
    const encoded = JSON.stringify(declarations);
    if (lazy) {
        op_collect_startup_plugin_lazy(encoded);
    } else {
        op_collect_startup_plugin_use(encoded);
    }
}

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

Object.defineProperty(globalThis.saya.theme, "ui", {
    configurable: true,
    enumerable: true,
    get() {
        return {};
    },
    set(value) {
        op_collect_startup_theme_ui(JSON.stringify(value ?? {}));
    },
});

Object.defineProperty(globalThis.saya.theme, "syntax", {
    configurable: true,
    enumerable: true,
    get() {
        return {};
    },
    set(value) {
        op_collect_startup_theme_syntax(JSON.stringify(value ?? {}));
    },
});

Object.defineProperty(globalThis.saya.theme, "languages", {
    configurable: true,
    enumerable: true,
    get() {
        return {};
    },
    set(value) {
        op_collect_startup_theme_languages(JSON.stringify(value ?? {}));
    },
});

Object.defineProperty(globalThis.saya.theme, "filer", {
    configurable: true,
    enumerable: true,
    get() {
        return {};
    },
    set(value) {
        op_collect_startup_theme_filer(JSON.stringify(value ?? {}));
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

Object.defineProperty(globalThis.saya.log, "level", {
    configurable: true,
    enumerable: true,
    get() {
        return undefined;
    },
    set(value) {
        if (typeof value !== "string") {
            throw new TypeError("log.level must be a string");
        }
        op_collect_startup_log_level(value);
    },
});

Object.defineProperty(globalThis.saya.plugins, "use", {
    configurable: true,
    enumerable: true,
    value(specs) {
        collectPluginDeclarations(specs, false);
    },
});

Object.defineProperty(globalThis.saya.plugins, "lazy", {
    configurable: true,
    enumerable: true,
    value(specs) {
        collectPluginDeclarations(specs, true);
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

Object.defineProperty(globalThis.saya.options, "tabstop", {
    configurable: true,
    enumerable: true,
    get() {
        return 8;
    },
    set(value) {
        op_collect_startup_tabstop(value);
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
defineNumberOption("cmdheight", "cmdheight", 5);
defineNumberOption("ch", "cmdheight", 5);
defineBoolOption("list", "list", false);
defineStringOption("listchars", "listchars", "tab:>-,trail:-");
defineStringOption("lcs", "listchars", "tab:>-,trail:-");
defineBoolOption("mermaidpreview", "mermaidpreview", true);
defineBoolOption("mmdpreview", "mermaidpreview", true);
defineStringOption("mermaidpreviewbackground", "mermaidpreviewbackground", "transparent");
defineStringOption("mmdpreviewbackground", "mermaidpreviewbackground", "transparent");
defineNumberOption("mermaidpreviewwidth", "mermaidpreviewwidth", 55);
defineNumberOption("mmdpreviewwidth", "mermaidpreviewwidth", 55);
defineNumberOption("mermaidpreviewheight", "mermaidpreviewheight", 55);
defineNumberOption("mmdpreviewheight", "mermaidpreviewheight", 55);
defineStringOption("foldmethod", "foldmethod", "manual");
defineStringOption("fdm", "foldmethod", "manual");
defineNumberOption("foldlevel", "foldlevel", 0);
defineNumberOption("fdl", "foldlevel", 0);

Object.defineProperty(globalThis.saya.ftplugin, "enabled", {
    configurable: true,
    enumerable: true,
    get() {
        return true;
    },
    set(value) {
        if (typeof value !== "boolean") {
            throw new TypeError("ftplugin.enabled must be a boolean");
        }
        op_collect_startup_ftplugin_enabled(value);
    },
});

globalThis.saya.options = new Proxy(globalThis.saya.options, {
    set(target, propertyName, value, receiver) {
        if (typeof propertyName === "string" && Reflect.has(target, propertyName)) {
            return Reflect.set(target, propertyName, value, receiver);
        }
        op_collect_startup_warning(`unsupported startup option: saya.options.${String(propertyName)}`);
        return true;
    },
});

Object.freeze(globalThis.saya.keymap);
Object.freeze(globalThis.saya.commands);
Object.freeze(globalThis.saya.events);
Object.freeze(globalThis.saya.statusline);
Object.freeze(globalThis.saya.ftplugin);
Object.freeze(globalThis.saya.theme);
Object.freeze(globalThis.saya.log);
Object.freeze(globalThis.saya.plugins);
Object.freeze(globalThis.saya);
"#;

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

pub const STARTUP_SAYA_TYPE_DECLARATION: &str = r#"
declare global {
    interface SayaReadonlyBufferSnapshot {
        id: number;
        path: string | null;
        lineCount: number;
        cursorRow: number;
        cursorCol: number;
        currentLine: string;
        text: string;
    }

    interface SayaBufferEventPayload {
        buffer: SayaReadonlyBufferSnapshot;
    }

    interface SayaStartupOptionsSurface {
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
        number: boolean;
        relativenumber: boolean;
        cursorline: boolean;
        numberwidth: number;
        laststatus: number;
        cmdheight: number;
        list: boolean;
        listchars: string;
        mermaidpreview: boolean;
        mermaidpreviewbackground: string;
        mermaidpreviewwidth: number;
        mermaidpreviewheight: number;
        foldmethod: string;
        foldlevel: number;
    }

    interface SayaStartupKeymapSurface {
        set(
            mode: "normal" | "insert" | "visual",
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
            name: "bufferOpen" | "bufferChanged" | "bufferWritePost" | "bufferClosed",
            callback: (payload: SayaBufferEventPayload) => unknown,
        ): void;
    }

    type SayaFtPluginOptions = Partial<SayaStartupOptionsSurface>;

    interface SayaFtPluginDefinition {
        extensions?: string[];
        options?: SayaFtPluginOptions;
        enabled?: boolean;
    }

    interface SayaStartupFtPluginSurface {
        enabled: boolean;
        set(filetype: string, definition: SayaFtPluginDefinition): void;
        disable(filetype: string): void;
    }

    type SayaStatusLineSegment = "fileName" | "mode" | "filetype" | "modified";

    interface SayaStatusLineConfig {
        left?: SayaStatusLineSegment[];
        right?: SayaStatusLineSegment[];
    }

    interface SayaStartupStatusLineSurface {
        set(config: SayaStatusLineConfig): void;
    }

    interface SayaTextStyle {
        fg?: string;
        bg?: string;
        bold?: boolean;
        italic?: boolean;
        underline?: boolean;
        strikethrough?: boolean;
    }

    type SayaSyntaxStyleKey =
        | "comment"
        | "string"
        | "constant"
        | "statement"
        | "identifier"
        | "type"
        | "function"
        | "punctuation"
        | "markup"
        | "default";

    interface SayaLanguageTheme {
        syntax?: Partial<Record<SayaSyntaxStyleKey, SayaTextStyle>>;
    }

    interface SayaStartupThemeSurface {
        palette: Record<string, string>;
        ui: Partial<Record<
            | "text"
            | "gutter"
            | "statusActive"
            | "statusInactive"
            | "message"
            | "warningMsg"
            | "prompt",
            SayaTextStyle
        >>;
        syntax: Partial<Record<SayaSyntaxStyleKey, SayaTextStyle>>;
        languages: Record<string, SayaLanguageTheme>;
        filer: Partial<Record<
            | "directory"
            | "file"
            | "symlink"
            | "other"
            | "marked",
            SayaTextStyle
        >>;
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
        level?: "error" | "warn" | "info" | "debug" | "trace";
    }

    interface SayaPluginUseSpec {
        name?: string;
        local?: string;
        github?: `${string}/${string}`;
        rev?: string;
        module?: string;
        setup?: string;
        options?: unknown;
    }

    interface SayaPluginLazySpec extends SayaPluginUseSpec {
        commands?: string[];
        events?: Array<"bufferOpen" | "bufferChanged" | "bufferWritePost" | "bufferClosed" | string>;
    }

    interface SayaStartupPluginsSurface {
        use(specs: SayaPluginUseSpec[]): void;
        lazy(specs: SayaPluginLazySpec[]): void;
    }

    interface SayaStartupSurface {
        options: SayaStartupOptionsSurface;
        keymap: SayaStartupKeymapSurface;
        commands: SayaStartupCommandsSurface;
        events: SayaStartupEventsSurface;
        ftplugin: SayaStartupFtPluginSurface;
        statusline: SayaStartupStatusLineSurface;
        theme: SayaStartupThemeSurface;
        log: SayaStartupLogSurface;
        plugins: SayaStartupPluginsSurface;
    }

    var saya: SayaStartupSurface;
}

export {};
"#;

#[op2(fast)]
fn op_collect_startup_tabstop(state: &mut OpState, #[number] value: i64) -> Result<(), JsErrorBox> {
    log::debug!(
        "[startup_runtime] collect startup tabstop option from runtime: value={}",
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
        "[startup_runtime] collect startup number option from runtime: value={}",
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
        "[startup_runtime] collect startup numberwidth option from runtime: value={}",
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
    let definition = crate::runtime::options::SayaOptionRegistry::resolve(name)
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
fn op_collect_startup_ftplugin_enabled(
    state: &mut OpState,
    enabled: bool,
) -> Result<(), JsErrorBox> {
    log::debug!(
        "[startup_runtime] collect startup ftplugin enabled flag: enabled={}",
        enabled
    );
    state
        .borrow_mut::<StartupRegistry>()
        .push(StartupRegistryEntry::FtPlugin {
            action: FtPluginStartupAction::SetEnabled(enabled),
        });
    Ok(())
}

#[op2(fast)]
fn op_collect_startup_ftplugin_definition(
    state: &mut OpState,
    #[string] filetype: String,
    #[string] definition_json: String,
) -> Result<(), JsErrorBox> {
    let filetype = normalize_ftplugin_filetype(&filetype)?;
    let wire: StartupFtPluginDefinitionWire = serde_json::from_str(&definition_json)
        .map_err(|error| JsErrorBox::generic(format!("invalid ftplugin definition: {error}")))?;
    let definition = FtPluginDefinition {
        filetype: filetype.clone(),
        extensions: normalize_ftplugin_extensions(&filetype, wire.extensions)?,
        options: parse_ftplugin_options(wire.options)?,
        enabled: wire.enabled.unwrap_or(true),
    };
    log::debug!(
        "[startup_runtime] collect startup ftplugin definition: filetype={}, extensions={}, options={}, enabled={}",
        definition.filetype,
        definition.extensions.len(),
        definition.options.len(),
        definition.enabled
    );
    state
        .borrow_mut::<StartupRegistry>()
        .push(StartupRegistryEntry::FtPlugin {
            action: FtPluginStartupAction::SetDefinition(definition),
        });
    Ok(())
}

#[op2(fast)]
fn op_collect_startup_ftplugin_disable(
    state: &mut OpState,
    #[string] filetype: String,
) -> Result<(), JsErrorBox> {
    let filetype = normalize_ftplugin_filetype(&filetype)?;
    log::debug!(
        "[startup_runtime] collect startup ftplugin filetype disable: filetype={}",
        filetype
    );
    state
        .borrow_mut::<StartupRegistry>()
        .push(StartupRegistryEntry::FtPlugin {
            action: FtPluginStartupAction::DisableFileType { filetype },
        });
    Ok(())
}

#[op2(fast)]
fn op_collect_startup_statusline(
    state: &mut OpState,
    #[string] config_json: String,
) -> Result<(), JsErrorBox> {
    let wire: StartupStatusLineConfigWire = serde_json::from_str(&config_json)
        .map_err(|error| JsErrorBox::generic(format!("invalid statusline config: {error}")))?;
    let config = StatusLineConfig {
        left: parse_statusline_segments("left", wire.left)?,
        right: parse_statusline_segments("right", wire.right)?,
    };
    log::debug!(
        "[startup_runtime] collect startup statusline: left_segments={}, right_segments={}",
        config.left.len(),
        config.right.len()
    );
    state
        .borrow_mut::<StartupRegistry>()
        .push(StartupRegistryEntry::StatusLine { config });
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
        let style = parse_theme_text_style("markdown theme style", &name, value)?;
        registry.push(StartupRegistryEntry::ThemeMarkdownStyle { key, style });
    }
    Ok(())
}

#[op2(fast)]
fn op_collect_startup_theme_ui(
    state: &mut OpState,
    #[string] ui_json: String,
) -> Result<(), JsErrorBox> {
    let ui = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&ui_json)
        .map_err(|error| JsErrorBox::generic(format!("invalid ui theme: {error}")))?;
    log::debug!(
        "[startup_runtime] collect startup ui theme: style_count={}",
        ui.len()
    );
    let registry = state.borrow_mut::<StartupRegistry>();
    for (name, value) in ui {
        let key = UiStyleKey::parse(&name)
            .ok_or_else(|| JsErrorBox::generic(format!("unsupported ui theme key: {name}")))?;
        let style = parse_theme_text_style("ui theme style", &name, value)?;
        registry.push(StartupRegistryEntry::ThemeUiStyle { key, style });
    }
    Ok(())
}

#[op2(fast)]
fn op_collect_startup_theme_syntax(
    state: &mut OpState,
    #[string] syntax_json: String,
) -> Result<(), JsErrorBox> {
    let syntax = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&syntax_json)
        .map_err(|error| JsErrorBox::generic(format!("invalid syntax theme: {error}")))?;
    log::debug!(
        "[startup_runtime] collect startup syntax theme: style_count={}",
        syntax.len()
    );
    let registry = state.borrow_mut::<StartupRegistry>();
    for (name, value) in syntax {
        let key = SyntaxSemanticStyleKey::parse(&name)
            .ok_or_else(|| JsErrorBox::generic(format!("unsupported syntax theme key: {name}")))?;
        let style = parse_theme_text_style("syntax theme style", &name, value)?;
        registry.push(StartupRegistryEntry::ThemeSyntaxStyle { key, style });
    }
    Ok(())
}

#[op2(fast)]
fn op_collect_startup_theme_languages(
    state: &mut OpState,
    #[string] languages_json: String,
) -> Result<(), JsErrorBox> {
    let languages =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&languages_json)
            .map_err(|error| JsErrorBox::generic(format!("invalid languages theme: {error}")))?;
    log::debug!(
        "[startup_runtime] collect startup language theme: language_count={}",
        languages.len()
    );
    let registry = state.borrow_mut::<StartupRegistry>();
    for (language, value) in languages {
        let serde_json::Value::Object(language_object) = value else {
            return Err(JsErrorBox::generic(format!(
                "language theme must be an object: {language}"
            )));
        };
        let Some(syntax_value) = language_object.get("syntax") else {
            continue;
        };
        let serde_json::Value::Object(syntax) = syntax_value else {
            return Err(JsErrorBox::generic(format!(
                "language syntax theme must be an object: {language}.syntax"
            )));
        };
        for (name, value) in syntax {
            let key = SyntaxSemanticStyleKey::parse(name).ok_or_else(|| {
                JsErrorBox::generic(format!(
                    "unsupported language syntax theme key: {language}.{name}"
                ))
            })?;
            let style = parse_theme_text_style(
                "language syntax theme style",
                &format!("{language}.syntax.{name}"),
                value.clone(),
            )?;
            registry.push(StartupRegistryEntry::ThemeLanguageSyntaxStyle {
                language: language.clone(),
                key,
                style,
            });
        }
    }
    Ok(())
}

#[op2(fast)]
fn op_collect_startup_theme_filer(
    state: &mut OpState,
    #[string] filer_json: String,
) -> Result<(), JsErrorBox> {
    let filer = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&filer_json)
        .map_err(|error| JsErrorBox::generic(format!("invalid filer theme: {error}")))?;
    log::debug!(
        "[startup_runtime] collect startup filer theme: style_count={}",
        filer.len()
    );
    let registry = state.borrow_mut::<StartupRegistry>();
    for (name, value) in filer {
        let key = FilerSemanticStyleKey::parse(&name)
            .ok_or_else(|| JsErrorBox::generic(format!("unsupported filer theme key: {name}")))?;
        let style = parse_theme_text_style("filer theme style", &name, value)?;
        registry.push(StartupRegistryEntry::ThemeFilerStyle { key, style });
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

#[op2(fast)]
fn op_collect_startup_log_level(
    state: &mut OpState,
    #[string] level: String,
) -> Result<(), JsErrorBox> {
    let Some(level_filter) = parse_startup_log_level(&level) else {
        return Err(JsErrorBox::generic(format!(
            "unsupported log.level: {level}"
        )));
    };
    log::debug!(
        "[startup_runtime] collect startup log level: level={}",
        level_filter
    );
    state
        .borrow_mut::<StartupRegistry>()
        .push(StartupRegistryEntry::LogLevel {
            level: level_filter,
        });
    Ok(())
}

#[op2(fast)]
fn op_collect_startup_plugin_use(
    state: &mut OpState,
    #[string] declarations_json: String,
) -> Result<(), JsErrorBox> {
    collect_startup_plugin_declarations(state, &declarations_json, false)
}

#[op2(fast)]
fn op_collect_startup_plugin_lazy(
    state: &mut OpState,
    #[string] declarations_json: String,
) -> Result<(), JsErrorBox> {
    collect_startup_plugin_declarations(state, &declarations_json, true)
}

#[op2(fast)]
fn op_collect_startup_warning(
    state: &mut OpState,
    #[string] message: String,
) -> Result<(), JsErrorBox> {
    log::debug!("[startup_runtime] collect startup warning: {}", message);

    state
        .borrow_mut::<StartupRegistry>()
        .push(StartupRegistryEntry::Warning { message });

    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartupFtPluginDefinitionWire {
    extensions: Option<Vec<String>>,
    options: Option<serde_json::Map<String, serde_json::Value>>,
    enabled: Option<bool>,
}

fn normalize_ftplugin_filetype(filetype: &str) -> Result<String, JsErrorBox> {
    let filetype = filetype.trim().to_ascii_lowercase();
    if filetype.is_empty() {
        return Err(JsErrorBox::generic(
            "ftplugin filetype must be a non-empty string",
        ));
    }
    Ok(filetype)
}

fn normalize_ftplugin_extensions(
    filetype: &str,
    extensions: Option<Vec<String>>,
) -> Result<Vec<String>, JsErrorBox> {
    let mut normalized = extensions.unwrap_or_else(|| vec![filetype.to_string()]);
    for extension in &mut normalized {
        *extension = extension
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase();
        if extension.is_empty() {
            return Err(JsErrorBox::generic(
                "ftplugin extension must be a non-empty string",
            ));
        }
    }
    Ok(normalized)
}

fn parse_ftplugin_options(
    options: Option<serde_json::Map<String, serde_json::Value>>,
) -> Result<Vec<FtPluginOption>, JsErrorBox> {
    let mut parsed = Vec::new();
    for (name, value) in options.unwrap_or_default() {
        let definition = SayaOptionRegistry::resolve(&name)
            .filter(|definition| definition.startup_public)
            .ok_or_else(|| JsErrorBox::generic(format!("unsupported ftplugin option: {name}")))?;
        let value = match definition.value_type {
            SayaOptionType::Boolean => value.as_bool().map(SayaOptionValue::Boolean),
            SayaOptionType::Number => value.as_i64().map(SayaOptionValue::Number),
            SayaOptionType::String => value
                .as_str()
                .map(|value| SayaOptionValue::String(value.to_string())),
        }
        .ok_or_else(|| {
            JsErrorBox::generic(format!(
                "ftplugin option type mismatch: option={}, expected={:?}",
                definition.name, definition.value_type
            ))
        })?;
        parsed.push(FtPluginOption {
            name: definition.name,
            value,
        });
    }
    Ok(parsed)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartupStatusLineConfigWire {
    left: Option<Vec<String>>,
    right: Option<Vec<String>>,
}

fn parse_statusline_segments(
    side: &str,
    segments: Option<Vec<String>>,
) -> Result<Vec<StatusLineSegment>, JsErrorBox> {
    segments
        .unwrap_or_default()
        .into_iter()
        .map(|segment| match segment.as_str() {
            "fileName" => Ok(StatusLineSegment::FileName),
            "mode" => Ok(StatusLineSegment::Mode),
            "filetype" => Ok(StatusLineSegment::FileType),
            "modified" => Ok(StatusLineSegment::Modified),
            other => Err(JsErrorBox::generic(format!(
                "unsupported statusline segment in {side}: {other}"
            ))),
        })
        .collect()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartupPluginDeclarationWire {
    name: String,
    source: StartupPluginSourceWire,
    module: String,
    setup: String,
    #[serde(default)]
    commands: Vec<String>,
    #[serde(default)]
    events: Vec<String>,
    #[serde(default)]
    options: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum StartupPluginSourceWire {
    Local { path: String },
    Github { repo: String, rev: Option<String> },
}

fn collect_startup_plugin_declarations(
    state: &mut OpState,
    declarations_json: &str,
    lazy: bool,
) -> Result<(), JsErrorBox> {
    let declarations = serde_json::from_str::<Vec<StartupPluginDeclarationWire>>(declarations_json)
        .map_err(|error| JsErrorBox::generic(format!("invalid plugin declarations: {error}")))?;
    log::debug!(
        "[startup_runtime] collect startup plugin declarations: lazy={}, count={}",
        lazy,
        declarations.len()
    );
    let registry = state.borrow_mut::<StartupRegistry>();
    for declaration in declarations {
        let declaration = normalize_startup_plugin_declaration(declaration)?;
        let entry = if lazy {
            StartupRegistryEntry::PluginLazy { declaration }
        } else {
            StartupRegistryEntry::PluginUse { declaration }
        };
        registry.push(entry);
    }
    Ok(())
}

fn normalize_startup_plugin_declaration(
    declaration: StartupPluginDeclarationWire,
) -> Result<StartupPluginDeclaration, JsErrorBox> {
    let name = non_empty_plugin_field("plugin name", declaration.name)?;
    let module = non_empty_plugin_field("plugin module", declaration.module)?;
    let setup = non_empty_plugin_field("plugin setup", declaration.setup)?;
    let commands = declaration
        .commands
        .into_iter()
        .map(|command| non_empty_plugin_field("plugin command", command))
        .collect::<Result<Vec<_>, _>>()?;
    let events = declaration
        .events
        .into_iter()
        .map(|event| non_empty_plugin_field("plugin event", event))
        .collect::<Result<Vec<_>, _>>()?;
    let source = match declaration.source {
        StartupPluginSourceWire::Local { path } => StartupPluginSource::Local {
            path: non_empty_plugin_field("plugin local path", path)?,
        },
        StartupPluginSourceWire::Github { repo, rev } => {
            let repo = non_empty_plugin_field("plugin github repo", repo)?;
            if repo.split('/').count() != 2 {
                return Err(JsErrorBox::generic(
                    "plugin github repo must use owner/repository form",
                ));
            }
            StartupPluginSource::Github {
                repo,
                rev: rev.and_then(|value| {
                    let trimmed = value.trim().to_string();
                    (!trimmed.is_empty()).then_some(trimmed)
                }),
            }
        }
    };
    Ok(StartupPluginDeclaration {
        name,
        source,
        module,
        setup,
        commands,
        events,
        options: declaration.options,
    })
}

fn non_empty_plugin_field(label: &str, value: String) -> Result<String, JsErrorBox> {
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        return Err(JsErrorBox::generic(format!("{label} must be non-empty")));
    }
    Ok(trimmed)
}

fn parse_startup_log_level(level: &str) -> Option<LevelFilter> {
    match level.trim().to_ascii_lowercase().as_str() {
        "error" => Some(LevelFilter::Error),
        "warn" => Some(LevelFilter::Warn),
        "info" => Some(LevelFilter::Info),
        "debug" => Some(LevelFilter::Debug),
        "trace" => Some(LevelFilter::Trace),
        _ => None,
    }
}

fn parse_theme_text_style(
    label: &str,
    name: &str,
    value: serde_json::Value,
) -> Result<ThemeTextStyleDeclaration, JsErrorBox> {
    let serde_json::Value::Object(object) = value else {
        return Err(JsErrorBox::generic(format!(
            "{label} must be an object: {name}"
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
                    "unsupported {label} property: {name}.{other}"
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
        op_collect_startup_tabstop,
        op_collect_startup_line_numbers,
        op_collect_startup_number_width,
        op_collect_startup_bool_option,
        op_collect_startup_number_option,
        op_collect_startup_string_option,
        op_collect_startup_keymap,
        op_collect_startup_command,
        op_collect_startup_event,
        op_collect_startup_ftplugin_enabled,
        op_collect_startup_ftplugin_definition,
        op_collect_startup_ftplugin_disable,
        op_collect_startup_statusline,
        op_collect_startup_theme_palette,
        op_collect_startup_theme_ui,
        op_collect_startup_theme_syntax,
        op_collect_startup_theme_languages,
        op_collect_startup_theme_filer,
        op_collect_startup_theme_markdown,
        op_collect_startup_log_file,
        op_collect_startup_log_level,
        op_collect_startup_plugin_use,
        op_collect_startup_plugin_lazy,
        op_collect_startup_warning
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
    let prepare_started = Instant::now();

    let loaded = match load_init_module(path, current_dir) {
        StartupModuleLoadResult::Success(module) => module,
        StartupModuleLoadResult::ReadFailed { path, message } => {
            return StartupModulePrepareResult::ReadFailed { path, message };
        }
    };

    match transpile_typescript_module(&loaded, prepare_started) {
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

#[derive(Debug, Clone)]
struct StartupModuleGraph {
    entry_id: String,
    modules: Vec<StartupGraphModule>,
    input_bytes: usize,
}

#[derive(Debug, Clone)]
struct StartupGraphModule {
    id: String,
    path: PathBuf,
    source_text: String,
    source_hash: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct StartupTranspileCacheMetadata {
    schema_version: u32,
    cache_key: String,
    entry_init_path: String,
    files: Vec<StartupTranspileCacheFile>,
    transpile_option_version: String,
    created_at_unix_ms: u128,
    input_bytes: usize,
    output_bytes: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct StartupTranspileCacheFile {
    path: String,
    hash: String,
}

fn transpile_typescript_module(
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

fn collect_startup_module_graph(
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

fn collect_startup_module_graph_from_source(
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

fn startup_static_import_specifiers(source_text: &str) -> Vec<String> {
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

fn transpile_startup_module_graph(graph: &StartupModuleGraph) -> Result<String, String> {
    let mut output = String::new();
    for module in &graph.modules {
        let transpiled = transpile_startup_module_to_js(module)?;
        output.push_str(&strip_es_module_syntax_from_transpiled_js(&transpiled));
        output.push('\n');
    }
    Ok(output)
}

fn transpile_startup_module_to_js(module: &StartupGraphModule) -> Result<String, String> {
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

fn strip_es_module_syntax_from_transpiled_js(source_text: &str) -> String {
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

fn startup_transpile_cache_key(graph: &StartupModuleGraph) -> String {
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

fn read_startup_transpile_cache(cache_key: &str, graph: &StartupModuleGraph) -> Option<String> {
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

fn write_startup_transpile_cache(
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

fn startup_transpile_cache_metadata_matches(
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

fn startup_transpile_cache_dir() -> Option<PathBuf> {
    paths::cache_dir().map(|dir| dir.join(STARTUP_TRANSPILE_CACHE_DIR_NAME))
}

fn canonical_startup_module_id(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn sha256_hex(bytes: &[u8]) -> String {
    bytes_to_hex(&Sha256::digest(bytes))
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

fn collect_static_import_statement(lines: &[&str], start_index: usize) -> (String, usize) {
    let first_line = lines[start_index];
    let trimmed = first_line.trim_start();
    if !trimmed.starts_with("import ") && !trimmed.starts_with("export {") {
        return (first_line.to_string(), 1);
    }

    let mut statement = first_line.to_string();
    let mut consumed_lines = 1usize;
    while !statement.trim_end().ends_with(';') && start_index + consumed_lines < lines.len() {
        statement.push('\n');
        statement.push_str(lines[start_index + consumed_lines]);
        consumed_lines += 1;
    }
    (statement, consumed_lines)
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

fn parse_static_re_export_specifier(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if !trimmed.starts_with("export ") {
        return None;
    }
    let (_, specifier) = trimmed.split_once(" from ")?;
    parse_quoted_module_specifier(specifier.trim_end_matches(';').trim())
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
    } else if let Some(path) = specifier.strip_prefix("~/") {
        resolve_home_relative_startup_import(specifier, path, std::env::var_os("HOME"))?
    } else if let Some((name, path)) = parse_env_relative_startup_import(specifier) {
        resolve_env_relative_startup_import(specifier, name, path, std::env::var_os(name))?
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

fn resolve_home_relative_startup_import(
    specifier: &str,
    path: &str,
    home: Option<OsString>,
) -> Result<PathBuf, String> {
    let home = home.filter(|value| !value.is_empty()).ok_or_else(|| {
        format!("unsupported startup import specifier: {specifier} (HOME is not set)")
    })?;
    Ok(PathBuf::from(home).join(path))
}

fn parse_env_relative_startup_import(specifier: &str) -> Option<(&str, &str)> {
    let rest = specifier.strip_prefix('$')?;
    if let Some(rest) = rest.strip_prefix('{') {
        let (name, path) = rest.split_once("}/")?;
        if is_valid_env_startup_import_name(name) {
            return Some((name, path));
        }
        return None;
    }

    let (name, path) = rest.split_once('/')?;
    if is_valid_env_startup_import_name(name) {
        Some((name, path))
    } else {
        None
    }
}

fn is_valid_env_startup_import_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if first != '_' && !first.is_ascii_alphabetic() {
        return false;
    }
    chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn resolve_env_relative_startup_import(
    specifier: &str,
    name: &str,
    path: &str,
    value: Option<OsString>,
) -> Result<PathBuf, String> {
    let value = value.filter(|value| !value.is_empty()).ok_or_else(|| {
        format!("unsupported startup import specifier: {specifier} ({name} is not set)")
    })?;
    Ok(PathBuf::from(value).join(path))
}

#[cfg(test)]
mod startup_import_path_tests {
    use super::*;

    #[test]
    fn home_relative_startup_import_resolves_against_home_directory() {
        let path = resolve_home_relative_startup_import(
            "~/saya-plugins/number.ts",
            "saya-plugins/number.ts",
            Some(OsString::from("/tmp/saya-home")),
        )
        .expect("home-relative import");

        assert_eq!(
            path,
            PathBuf::from("/tmp/saya-home")
                .join("saya-plugins")
                .join("number.ts")
        );
    }

    #[test]
    fn home_relative_startup_import_requires_home_directory() {
        let result = resolve_home_relative_startup_import(
            "~/saya-plugins/number.ts",
            "saya-plugins/number.ts",
            None,
        );

        assert_eq!(
            result,
            Err(
                "unsupported startup import specifier: ~/saya-plugins/number.ts (HOME is not set)"
                    .to_string()
            )
        );
    }

    #[test]
    fn env_relative_startup_import_parses_plain_environment_prefix() {
        assert_eq!(
            parse_env_relative_startup_import("$SAYA_HOME/runtime/plugins/dired/index.ts"),
            Some(("SAYA_HOME", "runtime/plugins/dired/index.ts"))
        );
    }

    #[test]
    fn env_relative_startup_import_parses_braced_environment_prefix() {
        assert_eq!(
            parse_env_relative_startup_import("${SAYA_HOME}/runtime/plugins/dired/index.ts"),
            Some(("SAYA_HOME", "runtime/plugins/dired/index.ts"))
        );
    }

    #[test]
    fn env_relative_startup_import_rejects_invalid_environment_prefix() {
        assert_eq!(parse_env_relative_startup_import("$1_BAD/plugin.ts"), None);
        assert_eq!(
            parse_env_relative_startup_import("${SAYA_HOME/plugin.ts"),
            None
        );
    }

    #[test]
    fn env_relative_startup_import_resolves_against_environment_value() {
        let path = resolve_env_relative_startup_import(
            "$SAYA_HOME/runtime/plugins/dired/index.ts",
            "SAYA_HOME",
            "runtime/plugins/dired/index.ts",
            Some(OsString::from("/tmp/saya-home")),
        )
        .expect("env-relative import");

        assert_eq!(
            path,
            PathBuf::from("/tmp/saya-home")
                .join("runtime")
                .join("plugins")
                .join("dired")
                .join("index.ts")
        );
    }

    #[test]
    fn env_relative_startup_import_requires_environment_value() {
        let result = resolve_env_relative_startup_import(
            "$SAYA_HOME/runtime/plugins/dired/index.ts",
            "SAYA_HOME",
            "runtime/plugins/dired/index.ts",
            None,
        );

        assert_eq!(
            result,
            Err(
                "unsupported startup import specifier: $SAYA_HOME/runtime/plugins/dired/index.ts (SAYA_HOME is not set)"
                    .to_string()
            )
        );
    }
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
