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

    interface SayaTextStyle {
        fg?: string;
        bg?: string;
        bold?: boolean;
        italic?: boolean;
        underline?: boolean;
        strikethrough?: boolean;
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
        syntax: Partial<Record<
            | "comment"
            | "string"
            | "constant"
            | "statement"
            | "identifier"
            | "type"
            | "function"
            | "punctuation"
            | "markup"
            | "default",
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
