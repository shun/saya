// deno-fmt-ignore-file

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
