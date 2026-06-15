//! startup ランタイムへ注入する `saya` namespace の JS ペイロードと型宣言文字列。

pub(super) const STARTUP_SAYA_BOOTSTRAP: &str = r#"
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
defineBoolOption("hlsearch", "hlsearch", false);
defineBoolOption("hls", "hlsearch", false);
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
        hlsearch: boolean;
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
