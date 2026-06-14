use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Mutex as StdMutex};
use std::thread;

use deno_core::{JsBuffer, JsRuntime, OpState, RuntimeOptions, op2};
use deno_error::JsErrorBox;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::process::Command as TokioCommand;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::features::completion::session::CompletionShowRequest;
use crate::features::lsp::runtime_bridge::{LspRuntimeBridgeRequest, LspRuntimeBridgeResponse};
use crate::features::selector::runtime::{
    RuntimeSelectorControlRequest, RuntimeSelectorError, RuntimeSelectorOpenRequest,
    RuntimeSelectorSessions, RuntimeSelectorSnapshot, RuntimeSelectorSourceRequest,
    RuntimeSelectorUpdateRequest, SelectorViewBackend, parse_rg_vimgrep_output,
};
use crate::runtime::callback_registry_seed::CallbackRegistrySeed;
use crate::runtime::lsp_session::{
    ManagedLspConnectRequest, ManagedLspConnectResponse, ManagedLspNotifyResponse,
    ManagedLspRequestResponse, ManagedLspSessionError, ManagedLspSessionPool,
};
use crate::runtime::process_pool::{ProcessPool, ProcessPoolError, ProcessSpec, StdioMode};
#[cfg(test)]
use crate::runtime::startup::{
    PreparedStartupModule, StartupModulePrepareResult, prepare_init_module,
};

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

const RUNTIME_COMMAND_ERROR_PREFIX: &str = "__SAYA_RUNTIME_COMMAND_ERROR__";
const RUNTIME_CALLBACK_ERROR_PREFIX: &str = "__SAYA_RUNTIME_CALLBACK_ERROR__";
const RUNTIME_PUBLIC_SURFACE_PATHS: &[&str] = &[
    "saya.commands.execute",
    "saya.buffer.current",
    "saya.buffer.currentPath",
    "saya.buffer.selection",
    "saya.window.current",
    "saya.window.openFloat",
    "saya.window.close",
    "saya.window.focus",
    "saya.window.floats",
    "saya.panel.open",
    "saya.panel.focus",
    "saya.panel.unfocus",
    "saya.panel.close",
    "saya.panel.list",
    "saya.panel.send",
    "saya.editor.current",
    "saya.editor.mode",
    "saya.workspace.findRoot",
    "saya.filer.list",
    "saya.filer.currentEntry",
    "saya.filer.createFile",
    "saya.filer.createDirectory",
    "saya.filer.copy",
    "saya.filer.move",
    "saya.filer.rename",
    "saya.filer.delete",
    "saya.filer.mark",
    "saya.filer.unmark",
    "saya.filer.clearMarks",
    "saya.filer.bulkDeletePreview",
    "saya.filer.bulkDelete",
    "saya.lsp.connect",
    "saya.lsif.request",
    "saya.input.prompt",
    "saya.selector.open",
    "saya.selector.update",
    "saya.selector.current",
    "saya.selector.control",
    "saya.selector.cancel",
    "saya.selector.dispose",
    "saya.completion.show",
    "saya.completion.close",
    "saya.process.spawn",
    "saya.plugins.loadLazy",
];

/// Formal runtime surface は read-only/command 実行に限定し、compat 文字列 DSL は含めない。
pub fn runtime_public_surface_paths() -> &'static [&'static str] {
    RUNTIME_PUBLIC_SURFACE_PATHS
}

const LIVE_RUNTIME_BOOTSTRAP: &str = r#"
const commandErrorPrefix = "__SAYA_RUNTIME_COMMAND_ERROR__";
const callbackErrorPrefix = "__SAYA_RUNTIME_CALLBACK_ERROR__";

function runtimeErrorMessage(error) {
    if (error instanceof Error) {
        return error.message;
    }
    return String(error);
}

globalThis.__sayaRuntime = {
    commands: new Map(),
    events: new Map(),
    commandStack: [],
    registerCommand(name, callback) {
        this.commands.set(String(name), callback);
    },
    registerEvent(name, callback) {
        const normalized = String(name);
        const handlers = this.events.get(normalized) ?? [];
        handlers.push(callback);
        this.events.set(normalized, handlers);
    },
    async executeCommand(name) {
        const normalized = String(name);
        if (this.commandStack.includes(normalized)) {
            throw `${commandErrorPrefix}${JSON.stringify({ "CircularCommand": { name: normalized } })}`;
        }

        this.commandStack.push(normalized);
        try {
            const callback = this.commands.get(normalized);
            if (callback) {
                return await callback();
            }
            return await Deno.core.ops.op_runtime_execute_host_command(normalized);
        } finally {
            this.commandStack.pop();
        }
    },
    async dispatchEvent(name, payload) {
        const handlers = this.events.get(String(name)) ?? [];
        for (let index = 0; index < handlers.length; index += 1) {
            try {
                await handlers[index](payload);
            } catch (error) {
                throw `${callbackErrorPrefix}${JSON.stringify({
                    handlerIndex: index,
                    error: runtimeErrorMessage(error),
                })}`;
            }
        }
    },
};

globalThis.saya = {
    commands: {
        execute(name) {
            return globalThis.__sayaRuntime.executeCommand(String(name));
        },
    },
    buffer: {
        current() {
            return Deno.core.ops.op_runtime_current_buffer();
        },
        currentPath() {
            return Deno.core.ops.op_runtime_current_buffer_path();
        },
        selection() {
            return Deno.core.ops.op_runtime_current_selection();
        },
    },
    window: {
        current() {
            return Deno.core.ops.op_runtime_current_window();
        },
        openFloat(options) {
            return Deno.core.ops.op_runtime_window_open_float(
                JSON.stringify(options ?? {}),
            );
        },
        close(id) {
            return Deno.core.ops.op_runtime_window_close_float(String(id));
        },
        focus(id) {
            return Deno.core.ops.op_runtime_window_focus_float(String(id));
        },
        floats() {
            return Deno.core.ops.op_runtime_window_floats();
        },
    },
    panel: {
        open(options) {
            const content = options?.content ?? {};
            const normalized = {
                id: String(options?.id ?? ""),
                position: String(options?.position ?? ""),
                size: String(options?.size ?? ""),
                content: {
                    ...content,
                    kind: String(content?.kind ?? ""),
                    command: Array.isArray(content?.command) ? content.command.map((arg) => String(arg)) : [],
                    lines: Array.isArray(content?.lines) ? content.lines.map((line) => String(line)) : [],
                    nodes: Array.isArray(content?.nodes)
                        ? content.nodes.map((node) => ({
                            type: String(node?.type ?? ""),
                            text: node?.text === undefined || node?.text === null ? null : String(node.text),
                            label: node?.label === undefined || node?.label === null ? null : String(node.label),
                            src: node?.src === undefined || node?.src === null ? null : String(node.src),
                            alt: node?.alt === undefined || node?.alt === null ? null : String(node.alt),
                            value: Number.isFinite(Number(node?.value)) ? Number(node.value) : null,
                        }))
                        : [],
                    closeBehavior: content?.closeBehavior === undefined || content?.closeBehavior === null
                        ? null
                        : String(content.closeBehavior),
                },
                focus: Boolean(options?.focus),
            };
            return Deno.core.ops.op_runtime_panel_open(JSON.stringify(normalized));
        },
        focus(id) {
            return Deno.core.ops.op_runtime_panel_focus(String(id));
        },
        unfocus() {
            return Deno.core.ops.op_runtime_panel_unfocus();
        },
        close(id) {
            return Deno.core.ops.op_runtime_panel_close(String(id));
        },
        list() {
            return Deno.core.ops.op_runtime_panel_list();
        },
        send(id, text) {
            return Deno.core.ops.op_runtime_panel_send(String(id), String(text));
        },
    },
    editor: {
        current() {
            return Deno.core.ops.op_runtime_current_editor();
        },
        async mode() {
            const editor = await Deno.core.ops.op_runtime_current_editor();
            return editor.mode;
        },
    },
    workspace: {
        findRoot(path, markers = []) {
            return Deno.core.ops.op_runtime_workspace_find_root(
                String(path),
                JSON.stringify(markers ?? []),
            );
        },
    },
    fs: {
        readDir(path = ".", options = {}) {
            return Deno.core.ops.op_runtime_fs_read_dir(String(path), JSON.stringify(options ?? {}));
        },
    },
    filer: {
        list(path = ".", options = {}) {
            return Deno.core.ops.op_runtime_filer_list(String(path), JSON.stringify(options ?? {}));
        },
        currentEntry() {
            return Deno.core.ops.op_runtime_filer_current_entry();
        },
        createFile(path) {
            return Deno.core.ops.op_runtime_filer_create_file(String(path));
        },
        createDirectory(path) {
            return Deno.core.ops.op_runtime_filer_create_directory(String(path));
        },
        copy(from, to) {
            return Deno.core.ops.op_runtime_filer_copy(String(from), String(to));
        },
        move(from, to) {
            return Deno.core.ops.op_runtime_filer_move(String(from), String(to));
        },
        rename(from, to) {
            return Deno.core.ops.op_runtime_filer_rename(String(from), String(to));
        },
        delete(path, options = {}) {
            return Deno.core.ops.op_runtime_filer_delete(
                String(path),
                Boolean(options?.confirm),
                Boolean(options?.recursive),
                Boolean(options?.trash),
            );
        },
        mark(path) {
            return Deno.core.ops.op_runtime_filer_mark(String(path));
        },
        unmark(path) {
            return Deno.core.ops.op_runtime_filer_unmark(String(path));
        },
        clearMarks() {
            return Deno.core.ops.op_runtime_filer_clear_marks();
        },
        bulkDeletePreview() {
            return Deno.core.ops.op_runtime_filer_bulk_delete_preview();
        },
        bulkDelete(options = {}) {
            return Deno.core.ops.op_runtime_filer_bulk_delete(
                String(options?.previewId ?? ""),
                Boolean(options?.confirm),
            );
        },
    },
    lsif: {
        request(payload) {
            return Deno.core.ops.op_runtime_lsif_request(JSON.stringify(payload ?? {}));
        },
    },
    lsp: {
        async connect(options) {
            const server = options?.server ?? {};
            const request = {
                server: {
                    name: String(server?.name ?? ""),
                    command: String(server?.command ?? ""),
                    args: Array.isArray(server?.args) ? server.args.map((arg) => String(arg)) : [],
                    env: server?.env && typeof server.env === "object" ? server.env : {},
                    cwd: server?.cwd === undefined || server?.cwd === null ? null : String(server.cwd),
                    rootMarkers: Array.isArray(server?.rootMarkers) ? server.rootMarkers.map((marker) => String(marker)) : [],
                    initializationOptions: server?.initializationOptions ?? null,
                },
                initializeParams: options?.initializeParams ?? {},
            };
            console.info(`[saya.lsp] connect server=${request.server.name} command=${request.server.command}`);
            const connected = await Deno.core.ops.op_runtime_lsp_connect(JSON.stringify(request));
            return makeLspClient(connected);
        },
    },
    input: {
        async prompt(options) {
            const request = {
                title: String(options?.title ?? ""),
                placeholder: options?.placeholder === undefined || options?.placeholder === null
                    ? null
                    : String(options.placeholder),
            };
            console.info(`[saya.input.prompt] start title=${request.title}`);
            const response = await Deno.core.ops.op_runtime_input_prompt(JSON.stringify(request));
            if (response.status === "submitted") {
                console.info(`[saya.input.prompt] resolved title=${request.title} value_len=${response.value.length}`);
                return response.value;
            }
            console.info(`[saya.input.prompt] cancelled title=${request.title}`);
            return null;
        },
    },
    selector: {
        open(options) {
            return Deno.core.ops.op_runtime_selector_open(JSON.stringify(options ?? {}));
        },
        update(id, options) {
            return Deno.core.ops.op_runtime_selector_update(String(id), JSON.stringify(options ?? {}));
        },
        current(id) {
            return Deno.core.ops.op_runtime_selector_current(String(id));
        },
        control(id, options) {
            return Deno.core.ops.op_runtime_selector_control(String(id), JSON.stringify(options ?? {}));
        },
        cancel(id) {
            return Deno.core.ops.op_runtime_selector_cancel(String(id));
        },
        dispose(id) {
            return Deno.core.ops.op_runtime_selector_dispose(String(id));
        },
    },
    completion: {
            show(request) {
                const normalized = {
                    sessionId: String(request?.sessionId ?? ""),
                    requestId: Number.isFinite(Number(request?.requestId)) ? Number(request.requestId) : 0,
                replaceRange: request?.replaceRange ?? null,
                candidates: Array.isArray(request?.candidates) ? request.candidates : [],
                selectedIndex: Number.isFinite(Number(request?.selectedIndex)) ? Number(request.selectedIndex) : 0,
                maxVisibleItems: Number.isFinite(Number(request?.maxVisibleItems)) ? Number(request.maxVisibleItems) : 8,
                documentationMaxWidth: Number.isFinite(Number(request?.documentationMaxWidth)) ? Number(request.documentationMaxWidth) : 72,
                documentationMaxHeight: Number.isFinite(Number(request?.documentationMaxHeight)) ? Number(request.documentationMaxHeight) : 12,
                keys: request?.keys && typeof request.keys === "object" ? request.keys : undefined,
            };
                console.info(`[saya.completion] show session=${normalized.sessionId} request=${normalized.requestId} candidates=${normalized.candidates.length}`);
                return Deno.core.ops.op_runtime_completion_show(JSON.stringify(normalized));
            },
            close() {
                return Deno.core.ops.op_runtime_completion_close();
            },
        },
    // Phase A.2: 汎用プロセス I/O。Rust 側の op_process_* を Object.freeze
    // で凍結したラッパ越しに公開する。LSP / DAP / linter / formatter 等
    // のプラグインから利用される基盤。
    process: {
        async spawn(spec) {
            const normalized = {
                command: String(spec?.command ?? ""),
                args: Array.isArray(spec?.args) ? spec.args.map((arg) => String(arg)) : [],
                env: spec?.env ?? {},
                cwd: spec?.cwd === undefined ? null : (spec.cwd === null ? null : String(spec.cwd)),
                stdin: typeof spec?.stdin === "string" ? spec.stdin : "null",
                stdout: typeof spec?.stdout === "string" ? spec.stdout : "null",
                stderr: typeof spec?.stderr === "string" ? spec.stderr : "null",
            };
            const handle = await Deno.core.ops.op_process_spawn(JSON.stringify(normalized));
            return makeProcessHandle(handle);
        },
    },
    plugins: {
        async loadLazy(request) {
            const payload = {
                kind: String(request?.kind ?? ""),
                name: String(request?.name ?? ""),
                plugin: String(request?.plugin ?? ""),
                module: String(request?.module ?? ""),
                exportName: String(request?.exportName ?? ""),
            };
            console.info(`[saya-plugin-host][lazy] bridge request kind=${payload.kind} name=${payload.name} plugin=${payload.plugin}`);
            return await Deno.core.ops.op_runtime_plugin_load_lazy(JSON.stringify(payload));
        },
    },
};

function makeProcessHandle(id) {
    // 0 byte 読み = EOF を `null` に正規化するヘルパ。Rust 側 op は
    // fast path を維持するため `0` を返す（read(2) 相当の慣例）。
    async function readInto(buf, op) {
        if (!(buf instanceof Uint8Array)) {
            throw new TypeError("read buffer must be Uint8Array");
        }
        const n = await op(id, buf);
        return n === 0 ? null : n;
    }
    const handle = {
        id,
        stdin: Object.freeze({
            async write(buf) {
                if (!(buf instanceof Uint8Array)) {
                    throw new TypeError("stdin write buffer must be Uint8Array");
                }
                return await Deno.core.ops.op_process_write_stdin(id, buf);
            },
        }),
        stdout: Object.freeze({
            read(buf) {
                return readInto(buf, Deno.core.ops.op_process_read_stdout);
            },
        }),
        stderr: Object.freeze({
            read(buf) {
                return readInto(buf, Deno.core.ops.op_process_read_stderr);
            },
        }),
        async kill() {
            await Deno.core.ops.op_process_kill(id);
        },
        async wait() {
            return await Deno.core.ops.op_process_wait(id);
        },
    };
    return Object.freeze(handle);
}

function makeLspClient(connected) {
    const id = Number(connected?.sessionId ?? 0);
    const client = {
        id,
        initializeResult: connected?.initializeResult ?? null,
        takeNotifications() {
            const notifications = Array.isArray(connected?.notifications) ? connected.notifications : [];
            connected.notifications = [];
            return notifications;
        },
        async request(method, params) {
            const response = await Deno.core.ops.op_runtime_lsp_request(id, String(method), JSON.stringify(params ?? null));
            if (Array.isArray(response?.notifications) && response.notifications.length > 0) {
                connected.notifications = (connected.notifications ?? []).concat(response.notifications);
            }
            return response?.result ?? null;
        },
        async notify(method, params) {
            const response = await Deno.core.ops.op_runtime_lsp_notify(id, String(method), JSON.stringify(params ?? null));
            if (Array.isArray(response?.notifications) && response.notifications.length > 0) {
                connected.notifications = (connected.notifications ?? []).concat(response.notifications);
            }
        },
        async close() {
            await Deno.core.ops.op_runtime_lsp_close(id);
        },
    };
    return Object.freeze(client);
}

Object.freeze(globalThis.saya.commands);
Object.freeze(globalThis.saya.buffer);
Object.freeze(globalThis.saya.window);
Object.freeze(globalThis.saya.editor);
Object.freeze(globalThis.saya.filer);
Object.freeze(globalThis.saya.lsp);
Object.freeze(globalThis.saya.lsif);
Object.freeze(globalThis.saya.input);
Object.freeze(globalThis.saya.selector);
Object.freeze(globalThis.saya.completion);
Object.freeze(globalThis.saya.process);
Object.freeze(globalThis.saya.plugins);
Object.freeze(globalThis.saya);

// プラグインの console.* をすべて diagnostic logger に流す。
// raw-mode の TUI で stdout に書くと画面が破壊されるため、
// console.log を呼ぶプラグインがあっても安全になるようここで防ぐ。
(function setupSayaRuntimeConsole() {
    function stringifyArg(value) {
        if (typeof value === "string") {
            return value;
        }
        if (value instanceof Error) {
            return value.stack ?? value.message ?? String(value);
        }
        try {
            return JSON.stringify(value);
        } catch (_error) {
            return String(value);
        }
    }
    function emit(level, args) {
        const message = Array.from(args).map(stringifyArg).join(" ");
        try {
            Deno.core.ops.op_runtime_console_log(level, message);
        } catch (_error) {
            // logger 不在時は静かに捨てる（端末に書かない）。
        }
    }
    const consoleProxy = {
        log: function () { emit("log", arguments); },
        info: function () { emit("info", arguments); },
        debug: function () { emit("debug", arguments); },
        warn: function () { emit("warn", arguments); },
        error: function () { emit("error", arguments); },
        trace: function () { emit("debug", arguments); },
        dir: function () { emit("debug", arguments); },
        group: function () {},
        groupCollapsed: function () {},
        groupEnd: function () {},
        time: function () {},
        timeEnd: function () {},
        assert: function () {},
        count: function () {},
        countReset: function () {},
        clear: function () {},
        table: function (value) { emit("debug", [value]); },
    };
    globalThis.console = consoleProxy;
})();
"#;

const RUNTIME_PUBLIC_SURFACE_NAMES: &[&str] = &[
    "commands",
    "buffer",
    "window",
    "panel",
    "editor",
    "filer",
    "lsp",
    "lsif",
    "input",
    "selector",
    "completion",
    "process",
    "plugins",
];
const RUNTIME_FORBIDDEN_SURFACE_NAMES: &[&str] = &["filesystem", "network"];

pub const RUNTIME_SAYA_TYPE_DECLARATION: &str = r#"
declare global {
    type SayaRuntimeMode = "Normal" | "Insert" | "Visual";

    interface SayaReadonlyBufferSnapshot {
        id: number;
        path: string | null;
        lineCount: number;
        cursorRow: number;
        cursorCol: number;
        currentLine: string;
        text: string;
    }

    interface SayaReadonlyWindowSnapshot {
        id: number;
    }

    type SayaRuntimeFloatBorder = "none" | "single" | "rounded";
    type SayaRuntimeFloatZIndex =
        | "hover"
        | "user"
        | "completion"
        | "completionDocumentation"
        | "blockingPrompt"
        | number;
    type SayaRuntimeFloatLifecycle =
        | "manual"
        | "closeOnCursorMove"
        | "closeOnInsert"
        | "closeOnBufferChange";

    interface SayaRuntimeLinesFloatContent {
        kind: "lines";
        lines: string[];
    }

    interface SayaRuntimeBufferFloatContent {
        kind: "buffer";
        bufferId?: number | null;
        windowId?: number | null;
    }

    interface SayaRuntimeTerminalFloatContent {
        kind: "terminal";
        command: string[];
        closeBehavior?: "kill" | "detach" | "killOnClose" | "detachOnClose";
    }

    type SayaRuntimeFloatContent =
        | SayaRuntimeLinesFloatContent
        | SayaRuntimeBufferFloatContent
        | SayaRuntimeTerminalFloatContent;

    interface SayaRuntimeEditorFloatPlacement {
        kind: "editor";
    }

    interface SayaRuntimeCursorFloatPlacement {
        kind: "cursor";
        windowId?: number | null;
    }

    interface SayaRuntimeWindowFloatPlacement {
        kind: "window";
        windowId?: number | null;
    }

    interface SayaRuntimeBufferPositionFloatPlacement {
        kind: "bufferPosition";
        windowId?: number | null;
        line: number;
        column: number;
    }

    type SayaRuntimeFloatPlacement =
        | SayaRuntimeEditorFloatPlacement
        | SayaRuntimeCursorFloatPlacement
        | SayaRuntimeWindowFloatPlacement
        | SayaRuntimeBufferPositionFloatPlacement;

    interface SayaRuntimeOpenFloatOptions {
        content: SayaRuntimeFloatContent;
        relativeTo?: SayaRuntimeFloatPlacement;
        width?: number;
        height?: number;
        row?: number;
        col?: number;
        anchor?: "nw" | "ne" | "sw" | "se";
        focusable?: boolean;
        border?: SayaRuntimeFloatBorder;
        zIndex?: SayaRuntimeFloatZIndex;
        lifecycle?: SayaRuntimeFloatLifecycle;
        group?: string | null;
    }

    interface SayaReadonlyFloatSnapshot {
        id: number;
        kind: string;
        focused: boolean;
        focusable: boolean;
        width: number;
        height: number;
        row: number;
        col: number;
        border: string;
        zIndex: number;
        lifecycle: string;
        replacementGroup?: string | null;
    }

    interface SayaReadonlyEditorSnapshot {
        mode: SayaRuntimeMode;
    }

    interface SayaRuntimeCommandsSurface {
        execute(name: string): Promise<unknown>;
    }

    interface SayaRuntimeBufferSurface {
        current(): Promise<SayaReadonlyBufferSnapshot>;
        currentPath(): Promise<string | null>;
        selection(): Promise<SayaReadonlySelectionSnapshot | null>;
    }

    interface SayaReadonlySelectionSnapshot {
        mode: "visual" | "visualLine" | "visualBlock";
        startLine: number;
        startColumn: number;
        endLine: number;
        endColumn: number;
        text: string;
    }

    interface SayaRuntimeWindowSurface {
        current(): Promise<SayaReadonlyWindowSnapshot>;
        openFloat(options: SayaRuntimeOpenFloatOptions): Promise<SayaReadonlyFloatSnapshot>;
        close(id: number): Promise<boolean>;
        focus(id: number): Promise<boolean>;
        floats(): Promise<SayaReadonlyFloatSnapshot[]>;
    }

    interface SayaRuntimeEditorSurface {
        current(): Promise<SayaReadonlyEditorSnapshot>;
        mode(): Promise<SayaRuntimeMode>;
    }

    interface SayaRuntimeWorkspaceSurface {
        findRoot(path: string, markers: string[]): Promise<string | null>;
    }

    type SayaLsifRuntimeSource = "lsif";
    type SayaLsifPositionEncoding = "utf-16" | "utf-8" | "utf-32";

    interface SayaLsifTextDocumentIdentifier {
        uri: string;
    }

    interface SayaLsifPosition {
        line: number;
        character: number;
    }

    interface SayaLsifRuntimeBridgeRequest {
        source: SayaLsifRuntimeSource;
        lspVersion: string;
        method: string;
        clientName: string;
        rootUri?: string | null;
        languageId: string;
        positionEncoding: SayaLsifPositionEncoding;
        dumpPath: string;
        textDocument?: SayaLsifTextDocumentIdentifier | null;
        position: SayaLsifPosition;
        params?: unknown;
        buffer: SayaReadonlyBufferSnapshot;
        editor: SayaReadonlyEditorSnapshot;
        event?: unknown;
    }

    interface SayaLsifRuntimeBridgeResponse {
        source: SayaLsifRuntimeSource;
        method: string;
        result: unknown;
    }

    interface SayaRuntimeLsifSurface {
        request(payload: SayaLsifRuntimeBridgeRequest): Promise<SayaLsifRuntimeBridgeResponse>;
    }

    interface SayaLspServerDefinition {
        name: string;
        command: string;
        args?: string[];
        env?: Record<string, string>;
        cwd?: string | null;
        rootMarkers?: string[];
        initializationOptions?: unknown;
    }

    interface SayaLspConnectOptions {
        server: SayaLspServerDefinition;
        initializeParams: unknown;
    }

    interface SayaRuntimeLspClient {
        readonly id: number;
        readonly initializeResult: unknown;
        takeNotifications(): unknown[];
        request(method: string, params?: unknown): Promise<unknown>;
        notify(method: string, params?: unknown): Promise<void>;
        close(): Promise<void>;
    }

    interface SayaRuntimeLspSurface {
        connect(options: SayaLspConnectOptions): Promise<SayaRuntimeLspClient>;
    }

    interface SayaInputPromptOptions {
        title: string;
        placeholder?: string | null;
    }

    interface SayaRuntimeInputSurface {
        prompt(options: SayaInputPromptOptions): Promise<string | null>;
    }

    type SayaSelectorMatcherName = "prefixAnd" | "substringAnd" | "suffixAnd";
    type SayaSelectorWorkState =
        | "idle"
        | "running"
        | "completed"
        | "cancelled"
        | "failed";
    type SayaSelectorStorageMode = "memory" | "tempFile";

    interface SayaSelectorItem<TDetail = unknown> {
        id: string;
        value: string;
        kind: string;
        detail: TDetail;
    }

    interface SayaStaticSelectorSource<TDetail = unknown> {
        kind: "static";
        items: SayaSelectorItem<TDetail>[];
    }

    interface SayaRgSelectorSource {
        kind: "rg";
        root?: string;
        pattern: string;
    }

    interface SayaSelectorLimits {
        maxRenderedItems?: number;
    }

    type SayaSelectorWindowSizeValue = number | `${number}%`;

    interface SayaSelectorWindowUiOptions {
        width?: SayaSelectorWindowSizeValue;
        height?: SayaSelectorWindowSizeValue;
    }

    interface SayaSelectorUiOptions {
        window?: SayaSelectorWindowUiOptions;
    }

    interface SayaSelectorOpenOptions<TDetail = unknown> {
        source: SayaStaticSelectorSource<TDetail> | SayaRgSelectorSource;
        matcher?: SayaSelectorMatcherName;
        query?: string;
        limits?: SayaSelectorLimits;
        ui?: SayaSelectorUiOptions;
    }

    interface SayaSelectorUpdateOptions {
        query: string;
    }

    type SayaSelectorControllerCommand =
        | "cursorNext"
        | "cursorPrevious"
        | "cursorFirst"
        | "cursorLast"
        | "pageDown"
        | "pageUp"
        | "show"
        | "hide"
        | "cancel";

    interface SayaSelectorControlOptions {
        command: SayaSelectorControllerCommand;
    }

    interface SayaSelectorHighlight {
        column: number;
        width: number;
        kind: "match" | "selection" | "diagnostic";
    }

    interface SayaRenderedSelectorItem {
        id: string;
        label: string;
        kind: string;
        detail: unknown;
        highlights: SayaSelectorHighlight[];
    }

    interface SayaSelectorViewState {
        cursor: number;
        offset: number;
        renderedItemsLen: number;
        hidden: boolean;
        cancelled: boolean;
    }

    interface SayaSelectorCollectStatus {
        state: SayaSelectorWorkState;
        totalSeen: number;
        totalStored: number;
        storage: SayaSelectorStorageMode;
        errorMessage?: string | null;
    }

    interface SayaSelectorMatchStatus {
        state: SayaSelectorWorkState;
        totalMatched: number;
        totalRendered: number;
        errorMessage?: string | null;
    }

    interface SayaSelectorStoreStatus {
        storage: SayaSelectorStorageMode;
        totalStored: number;
        estimatedBytes?: number | null;
        tempFilePath?: string | null;
    }

    interface SayaSelectorStatus {
        collect: SayaSelectorCollectStatus;
        match: SayaSelectorMatchStatus;
        store: SayaSelectorStoreStatus;
    }

    interface SayaSelectorSnapshot {
        id: number;
        query: string;
        renderedItems: SayaRenderedSelectorItem[];
        selectedItem?: SayaRenderedSelectorItem | null;
        view: SayaSelectorViewState;
        status: SayaSelectorStatus;
        ui: SayaSelectorUiOptions;
    }

    interface SayaRuntimeSelectorSurface {
        open<TDetail = unknown>(
            options: SayaSelectorOpenOptions<TDetail>,
        ): Promise<SayaSelectorSnapshot>;
        update(id: number, options: SayaSelectorUpdateOptions): Promise<SayaSelectorSnapshot>;
        current(id: number): Promise<SayaSelectorSnapshot>;
        control(id: number, options: SayaSelectorControlOptions): Promise<SayaSelectorSnapshot>;
        cancel(id: number): Promise<SayaSelectorSnapshot>;
        dispose(id: number): Promise<boolean>;
    }

    interface SayaCompletionPosition {
        line: number;
        character: number;
    }

    interface SayaCompletionRange {
        start: SayaCompletionPosition;
        end: SayaCompletionPosition;
    }

    interface SayaCompletionCandidate {
        label: string;
        insertText?: string | null;
        kind?: string | null;
        detail?: string | null;
        documentation?: string[];
        source?: string | null;
    }

    interface SayaCompletionKeyBindings {
        confirm?: string[];
        close?: string[];
        next?: string[];
        previous?: string[];
        pageNext?: string[];
        pagePrevious?: string[];
    }

    interface SayaCompletionShowRequest {
        sessionId: string;
        requestId: number;
        replaceRange: SayaCompletionRange;
        candidates: SayaCompletionCandidate[];
        selectedIndex?: number;
        maxVisibleItems?: number;
        documentationMaxWidth?: number;
        documentationMaxHeight?: number;
        keys?: SayaCompletionKeyBindings;
    }

    interface SayaRuntimeCompletionSurface {
        show(request: SayaCompletionShowRequest): Promise<boolean>;
        close(): Promise<boolean>;
    }

    type SayaProcessStdioMode = "inherit" | "null" | "piped";

    interface SayaProcessSpec {
        command: string;
        args?: string[];
        env?: Record<string, string>;
        cwd?: string | null;
        stdin?: SayaProcessStdioMode;
        stdout?: SayaProcessStdioMode;
        stderr?: SayaProcessStdioMode;
    }

    interface SayaProcessReader {
        read(buf: Uint8Array): Promise<number | null>;
    }

    interface SayaProcessWriter {
        write(buf: Uint8Array): Promise<number>;
    }

    interface SayaProcessHandle {
        readonly id: number;
        readonly stdin: SayaProcessWriter;
        readonly stdout: SayaProcessReader;
        readonly stderr: SayaProcessReader;
        kill(): Promise<void>;
        wait(): Promise<number>;
    }

    interface SayaRuntimeProcessSurface {
        spawn(spec: SayaProcessSpec): Promise<SayaProcessHandle>;
    }

    type SayaLazyPluginTriggerKind = "command" | "event";

    interface SayaLazyPluginLoadRequest {
        kind: SayaLazyPluginTriggerKind;
        name: string;
        plugin: string;
        module: string;
        exportName: string;
    }

    interface SayaRuntimePluginsSurface {
        loadLazy(request: SayaLazyPluginLoadRequest): Promise<void>;
    }

    type SayaPanelPosition = "left" | "right" | "top" | "bottom";
    type SayaPanelCloseBehavior = "kill" | "detach";

    type SayaPanelNode =
        | { type: "text"; text: string }
        | { type: "heading"; text: string }
        | { type: "divider" }
        | { type: "image"; src: string; alt?: string }
        | { type: "badge"; label: string }
        | { type: "progress"; value: number; label?: string }
        | { type: "button"; label: string };

    type SayaPanelContent =
        | { kind: "terminal"; command: string[]; closeBehavior?: SayaPanelCloseBehavior }
        | { kind: "lines"; lines: string[] }
        | { kind: "view"; nodes: SayaPanelNode[] };

    interface SayaPanelOpenOptions {
        id: string;
        position: SayaPanelPosition;
        size: number | `${number}%` | string;
        content: SayaPanelContent;
        focus?: boolean;
    }

    interface SayaPanelSnapshot {
        id: string;
        numericId: number;
        position: SayaPanelPosition;
        size: string;
        kind: "terminal" | "lines" | "view";
        focused: boolean;
    }

    interface SayaRuntimePanelSurface {
        open(options: SayaPanelOpenOptions): Promise<SayaPanelSnapshot>;
        focus(id: string): Promise<boolean>;
        unfocus(): Promise<boolean>;
        close(id: string): Promise<boolean>;
        list(): Promise<SayaPanelSnapshot[]>;
        send(id: string, text: string): Promise<boolean>;
    }

    type SayaFilerEntryKind = "directory" | "file" | "symlink" | "other";

    type SayaFilerSortKey = "name" | "kind" | "modifiedTime" | "size";

    interface SayaFilerListOptions {
        showHidden?: boolean;
        sortBy?: SayaFilerSortKey;
        filter?: string | null;
    }

    interface SayaFilerEntry {
        name: string;
        path: string;
        kind: SayaFilerEntryKind;
        displayText: string;
        size?: number | null;
        modifiedTimeMs?: number | null;
    }

    interface SayaCurrentFilerEntry extends SayaFilerEntry {
        id: number;
        rootPath: string;
        displayText: string;
    }

    interface SayaRuntimeFilerSurface {
        list(path?: string, options?: SayaFilerListOptions): Promise<SayaFilerEntry[]>;
        currentEntry(): Promise<SayaCurrentFilerEntry | null>;
        createFile(path: string): Promise<SayaFilerOperationReport>;
        createDirectory(path: string): Promise<SayaFilerOperationReport>;
        copy(from: string, to: string): Promise<SayaFilerOperationReport>;
        move(from: string, to: string): Promise<SayaFilerOperationReport>;
        rename(from: string, to: string): Promise<SayaFilerOperationReport>;
        delete(path: string, options?: SayaFilerDeleteOptions): Promise<SayaFilerOperationReport>;
        mark(path: string): Promise<SayaFilerOperationReport>;
        unmark(path: string): Promise<SayaFilerOperationReport>;
        clearMarks(): Promise<SayaFilerOperationReport>;
        bulkDeletePreview(): Promise<SayaFilerOperationReport>;
        bulkDelete(options?: SayaFilerBulkDeleteOptions): Promise<SayaFilerOperationReport>;
    }

    interface SayaFilerDeleteOptions {
        confirm?: boolean;
        recursive?: boolean;
        trash?: boolean;
    }

    interface SayaFilerBulkDeleteOptions {
        confirm?: boolean;
        previewId?: string;
    }

    type SayaFilerOperationKind =
        | "createFile"
        | "createDirectory"
        | "copy"
        | "move"
        | "rename"
        | "delete"
        | "mark"
        | "unmark"
        | "clearMarks"
        | "bulkDeletePreview"
        | "bulkDelete";

    interface SayaFilerOperationReport {
        operation: SayaFilerOperationKind;
        path: string;
        targetPath?: string | null;
        entries: SayaCurrentFilerEntry[];
        previewId?: string | null;
    }

    type SayaDirectoryBufferOperationKind =
        | "createFile"
        | "createDirectory"
        | "rename"
        | "delete";

    type SayaDirectoryBufferOperationRisk = "low" | "high";

    interface SayaDirectoryBufferPreviewOperation {
        kind: SayaDirectoryBufferOperationKind;
        sourcePath?: string | null;
        targetPath?: string | null;
        risk: SayaDirectoryBufferOperationRisk;
    }

    interface SayaDirectoryBufferOperationPreview {
        id: string;
        rootPath: string;
        operationCount: number;
        highRiskCount: number;
        operations: SayaDirectoryBufferPreviewOperation[];
    }

    interface SayaDirectoryBufferOperationPrompt {
        previewId: string;
        statusLine: string;
        detailLines: string[];
        confirmCommand: "OK";
        cancelCommand: "Cancel";
        recoveryHint: string;
    }

    interface SayaDirectoryBufferApplyReport {
        rootPath: string;
        operationCount: number;
        successfulSteps: number;
        failedSteps: number;
        rollbackSucceeded: number;
        rollbackFailed: number;
        manualRecoveryRequired: boolean;
    }

    interface SayaRuntimeFsSurface {
        readDir(path?: string, options?: SayaFilerListOptions): Promise<SayaFilerEntry[]>;
    }

    interface SayaRuntimeSurface {
        commands: SayaRuntimeCommandsSurface;
        buffer: SayaRuntimeBufferSurface;
        window: SayaRuntimeWindowSurface;
        panel: SayaRuntimePanelSurface;
        editor: SayaRuntimeEditorSurface;
        workspace: SayaRuntimeWorkspaceSurface;
        fs: SayaRuntimeFsSurface;
        filer: SayaRuntimeFilerSurface;
        lsp: SayaRuntimeLspSurface;
        lsif: SayaRuntimeLsifSurface;
        input: SayaRuntimeInputSurface;
        selector: SayaRuntimeSelectorSurface;
        completion: SayaRuntimeCompletionSurface;
        process: SayaRuntimeProcessSurface;
        plugins: SayaRuntimePluginsSurface;
    }

    var saya: SayaRuntimeSurface;
}

export {};
"#;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadonlyBufferSnapshot {
    pub id: u64,
    pub path: Option<PathBuf>,
    pub line_count: usize,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub current_line: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadonlySelectionSnapshot {
    pub mode: String,
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadonlyWindowSnapshot {
    pub id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeFloatOpenRequest {
    pub content: RuntimeFloatContentRequest,
    pub relative_to: Option<RuntimeFloatRelativeToRequest>,
    pub width: Option<u16>,
    pub height: Option<u16>,
    pub row: Option<i16>,
    pub col: Option<i16>,
    pub anchor: Option<String>,
    pub focusable: Option<bool>,
    pub border: Option<String>,
    pub z_index: Option<RuntimeFloatZIndexRequest>,
    pub lifecycle: Option<String>,
    pub group: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RuntimeFloatContentRequest {
    Lines {
        lines: Vec<String>,
    },
    Buffer {
        #[serde(default)]
        buffer_id: Option<u64>,
        #[serde(default)]
        window_id: Option<u64>,
    },
    Terminal {
        command: Vec<String>,
        #[serde(default)]
        close_behavior: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RuntimeFloatRelativeToRequest {
    Editor,
    Cursor {
        #[serde(default)]
        window_id: Option<u64>,
    },
    Window {
        #[serde(default)]
        window_id: Option<u64>,
    },
    BufferPosition {
        #[serde(default)]
        window_id: Option<u64>,
        line: usize,
        column: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RuntimeFloatZIndexRequest {
    Named(String),
    Custom(i32),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeFloatSnapshot {
    pub id: u64,
    pub kind: String,
    pub focused: bool,
    pub focusable: bool,
    pub width: u16,
    pub height: u16,
    pub row: i16,
    pub col: i16,
    pub border: String,
    pub z_index: i32,
    pub lifecycle: String,
    pub replacement_group: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimePanelOpenRequest {
    pub id: String,
    pub position: String,
    pub size: String,
    pub content: RuntimePanelContentRequest,
    #[serde(default)]
    pub focus: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimePanelContentRequest {
    pub kind: String,
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default)]
    pub lines: Vec<String>,
    #[serde(default)]
    pub nodes: Vec<RuntimePanelNodeRequest>,
    #[serde(default)]
    pub close_behavior: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimePanelNodeRequest {
    #[serde(rename = "type")]
    pub node_type: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub src: Option<String>,
    #[serde(default)]
    pub alt: Option<String>,
    #[serde(default)]
    pub value: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimePanelSnapshot {
    pub id: String,
    pub numeric_id: u64,
    pub position: String,
    pub size: String,
    pub kind: String,
    pub focused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadonlyEditorSnapshot {
    pub mode: RuntimeMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeMode {
    Normal,
    Insert,
    Visual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BufferEventPayload {
    pub buffer: ReadonlyBufferSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeFilerEntry {
    pub name: String,
    pub path: String,
    pub kind: RuntimeFilerEntryKind,
    pub display_text: String,
    pub size: Option<u64>,
    pub modified_time_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeFilerCurrentEntry {
    pub id: u64,
    pub name: String,
    pub path: String,
    pub kind: RuntimeFilerEntryKind,
    pub root_path: String,
    pub display_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeFilerEntryKind {
    Directory,
    File,
    Symlink,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeFilerSortKey {
    Name,
    Kind,
    ModifiedTime,
    Size,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeFilerListOptions {
    #[serde(default = "runtime_filer_show_hidden_default")]
    pub show_hidden: bool,
    #[serde(default)]
    pub sort_by: RuntimeFilerSortKey,
    #[serde(default)]
    pub filter: Option<String>,
}

impl Default for RuntimeFilerListOptions {
    fn default() -> Self {
        Self {
            show_hidden: runtime_filer_show_hidden_default(),
            sort_by: RuntimeFilerSortKey::Kind,
            filter: None,
        }
    }
}

impl Default for RuntimeFilerSortKey {
    fn default() -> Self {
        Self::Kind
    }
}

fn runtime_filer_show_hidden_default() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeFilerOperationKind {
    CreateFile,
    CreateDirectory,
    Copy,
    Move,
    Rename,
    Delete,
    Mark,
    Unmark,
    ClearMarks,
    BulkDeletePreview,
    BulkDelete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeFilerErrorKind {
    AlreadyExists,
    ConfirmationRequired,
    Io,
    NotFound,
    PermissionDenied,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeFilerOperationReport {
    pub operation: RuntimeFilerOperationKind,
    pub path: String,
    pub target_path: Option<String>,
    pub entries: Vec<RuntimeFilerCurrentEntry>,
    pub preview_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeFilerOperation {
    CreateFile {
        path: PathBuf,
    },
    CreateDirectory {
        path: PathBuf,
    },
    Copy {
        from: PathBuf,
        to: PathBuf,
    },
    Move {
        from: PathBuf,
        to: PathBuf,
    },
    Rename {
        from: PathBuf,
        to: PathBuf,
    },
    Delete {
        path: PathBuf,
        confirm: bool,
        recursive: bool,
        trash: bool,
    },
    Mark {
        path: PathBuf,
    },
    Unmark {
        path: PathBuf,
    },
    ClearMarks,
    BulkDeletePreview,
    BulkDelete {
        preview_id: String,
        confirm: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeFilerError {
    ReadFailed {
        path: PathBuf,
        message: String,
    },
    OperationFailed {
        operation: RuntimeFilerOperationKind,
        path: PathBuf,
        target_path: Option<PathBuf>,
        kind: RuntimeFilerErrorKind,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuntimeEventName {
    BufferOpen,
    BufferChanged,
    BufferWritePost,
    BufferClosed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeEventPayload {
    BufferOpen(BufferEventPayload),
    BufferChanged(BufferEventPayload),
    BufferWritePost(BufferEventPayload),
    BufferClosed(BufferEventPayload),
}

impl RuntimeEventPayload {
    pub fn event_name(&self) -> RuntimeEventName {
        match self {
            Self::BufferOpen(_) => RuntimeEventName::BufferOpen,
            Self::BufferChanged(_) => RuntimeEventName::BufferChanged,
            Self::BufferWritePost(_) => RuntimeEventName::BufferWritePost,
            Self::BufferClosed(_) => RuntimeEventName::BufferClosed,
        }
    }

    pub fn buffer_payload(&self) -> &BufferEventPayload {
        match self {
            Self::BufferOpen(payload)
            | Self::BufferChanged(payload)
            | Self::BufferWritePost(payload)
            | Self::BufferClosed(payload) => payload,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeCommandError {
    UnknownCommand { name: String },
    CommandFailed { name: String, message: String },
    CircularCommand { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeCallbackError {
    Command(RuntimeCommandError),
    ScriptFailed { message: String },
}

impl From<RuntimeCommandError> for RuntimeCallbackError {
    fn from(value: RuntimeCommandError) -> Self {
        Self::Command(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeDispatchError {
    QueueClosed,
    WorkerStopped,
    CallbackFailed {
        event: RuntimeEventName,
        handler_index: usize,
        error: RuntimeCallbackError,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeDispatchReport {
    pub event: RuntimeEventName,
    pub handler_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeInitError {
    WorkerStartFailed { message: String },
    UnsupportedEvent { name: String },
    BootstrapFailed { message: String },
}

pub trait HostCapabilityBridge: Send + Sync + 'static {
    fn execute_host_command(&self, name: &str) -> BoxFuture<Result<(), RuntimeCommandError>>;
    fn request_input_prompt(
        &self,
        request: RuntimeInputPromptRequest,
    ) -> BoxFuture<Result<RuntimeInputPromptResponse, RuntimeCommandError>> {
        Box::pin(async move {
            log::debug!(
                "[saya_live_runtime][input] typed prompt unavailable: title={}, placeholder_present={}",
                request.title,
                request.placeholder.is_some()
            );
            Err(RuntimeCommandError::UnknownCommand {
                name: "input.prompt".to_string(),
            })
        })
    }
    fn execute_lsif_request(
        &self,
        request: LspRuntimeBridgeRequest,
    ) -> BoxFuture<Result<LspRuntimeBridgeResponse, RuntimeCommandError>> {
        Box::pin(async move {
            log::debug!(
                "[saya_live_runtime][lsif] typed LSIF bridge unavailable: method={}",
                request.method
            );
            Err(RuntimeCommandError::UnknownCommand {
                name: "lsif.request".to_string(),
            })
        })
    }
    fn show_completion(
        &self,
        request: CompletionShowRequest,
    ) -> BoxFuture<Result<bool, RuntimeCommandError>> {
        Box::pin(async move {
            log::debug!(
                "[saya_live_runtime][completion] typed show unavailable: session_id={}, request_id={}, candidates={}",
                request.session_id,
                request.request_id,
                request.candidates.len()
            );
            Err(RuntimeCommandError::UnknownCommand {
                name: "completion.show".to_string(),
            })
        })
    }
    fn close_completion(&self) -> BoxFuture<Result<bool, RuntimeCommandError>> {
        Box::pin(async move {
            log::debug!("[saya_live_runtime][completion] typed close unavailable");
            Err(RuntimeCommandError::UnknownCommand {
                name: "completion.close".to_string(),
            })
        })
    }
    fn current_buffer(&self) -> BoxFuture<ReadonlyBufferSnapshot>;
    fn current_buffer_path(&self) -> BoxFuture<Option<PathBuf>> {
        let buffer = self.current_buffer();
        Box::pin(async move { buffer.await.path })
    }
    fn current_selection(&self) -> BoxFuture<Option<ReadonlySelectionSnapshot>> {
        Box::pin(async move { None })
    }
    fn current_window(&self) -> BoxFuture<ReadonlyWindowSnapshot>;
    fn open_float(
        &self,
        request: RuntimeFloatOpenRequest,
    ) -> BoxFuture<Result<RuntimeFloatSnapshot, RuntimeCommandError>> {
        Box::pin(async move {
            log::debug!(
                "[saya_live_runtime][window] typed openFloat unavailable: content={:?}",
                request.content
            );
            Err(RuntimeCommandError::UnknownCommand {
                name: "window.openFloat".to_string(),
            })
        })
    }
    fn close_float(&self, id: u64) -> BoxFuture<Result<bool, RuntimeCommandError>> {
        Box::pin(async move {
            log::debug!(
                "[saya_live_runtime][window] typed close unavailable: id={}",
                id
            );
            Err(RuntimeCommandError::UnknownCommand {
                name: "window.close".to_string(),
            })
        })
    }
    fn focus_float(&self, id: u64) -> BoxFuture<Result<bool, RuntimeCommandError>> {
        Box::pin(async move {
            log::debug!(
                "[saya_live_runtime][window] typed focus unavailable: id={}",
                id
            );
            Err(RuntimeCommandError::UnknownCommand {
                name: "window.focus".to_string(),
            })
        })
    }
    fn list_float_snapshots(
        &self,
    ) -> BoxFuture<Result<Vec<RuntimeFloatSnapshot>, RuntimeCommandError>> {
        Box::pin(async move {
            log::debug!("[saya_live_runtime][window] typed float snapshots unavailable");
            Err(RuntimeCommandError::UnknownCommand {
                name: "window.floats".to_string(),
            })
        })
    }
    fn open_panel(
        &self,
        request: RuntimePanelOpenRequest,
    ) -> BoxFuture<Result<RuntimePanelSnapshot, RuntimeCommandError>> {
        Box::pin(async move {
            log::debug!(
                "[saya_live_runtime][panel] typed open unavailable: id={}, content={:?}",
                request.id,
                request.content
            );
            Err(RuntimeCommandError::UnknownCommand {
                name: "panel.open".to_string(),
            })
        })
    }
    fn focus_panel(&self, id: String) -> BoxFuture<Result<bool, RuntimeCommandError>> {
        Box::pin(async move {
            log::debug!("[saya_live_runtime][panel] focus unavailable: id={}", id);
            Err(RuntimeCommandError::UnknownCommand {
                name: "panel.focus".to_string(),
            })
        })
    }
    fn unfocus_panel(&self) -> BoxFuture<Result<bool, RuntimeCommandError>> {
        Box::pin(async move {
            log::debug!("[saya_live_runtime][panel] unfocus unavailable");
            Err(RuntimeCommandError::UnknownCommand {
                name: "panel.unfocus".to_string(),
            })
        })
    }
    fn close_panel(&self, id: String) -> BoxFuture<Result<bool, RuntimeCommandError>> {
        Box::pin(async move {
            log::debug!("[saya_live_runtime][panel] close unavailable: id={}", id);
            Err(RuntimeCommandError::UnknownCommand {
                name: "panel.close".to_string(),
            })
        })
    }
    fn list_panel_snapshots(
        &self,
    ) -> BoxFuture<Result<Vec<RuntimePanelSnapshot>, RuntimeCommandError>> {
        Box::pin(async move {
            log::debug!("[saya_live_runtime][panel] list unavailable");
            Err(RuntimeCommandError::UnknownCommand {
                name: "panel.list".to_string(),
            })
        })
    }
    fn send_panel_text(
        &self,
        id: String,
        text: String,
    ) -> BoxFuture<Result<bool, RuntimeCommandError>> {
        Box::pin(async move {
            log::debug!(
                "[saya_live_runtime][panel] send unavailable: id={}, bytes={}",
                id,
                text.len()
            );
            Err(RuntimeCommandError::UnknownCommand {
                name: "panel.send".to_string(),
            })
        })
    }
    fn current_editor(&self) -> BoxFuture<ReadonlyEditorSnapshot>;
    fn selector_view_backend(&self) -> Option<Arc<dyn SelectorViewBackend>> {
        None
    }
    fn find_workspace_root(&self, path: String, markers: Vec<String>) -> BoxFuture<Option<String>> {
        Box::pin(async move {
            let root = find_workspace_root_path(PathBuf::from(path), &markers)?;
            Some(root.to_string_lossy().into_owned())
        })
    }
    fn list_filer_entries(
        &self,
        path: PathBuf,
        options: RuntimeFilerListOptions,
    ) -> BoxFuture<Result<Vec<RuntimeFilerEntry>, RuntimeFilerError>> {
        Box::pin(async move { list_local_filer_entries(path, options) })
    }
    fn current_filer_entry(
        &self,
    ) -> BoxFuture<Result<Option<RuntimeFilerCurrentEntry>, RuntimeFilerError>> {
        Box::pin(async { Ok(None) })
    }
    fn execute_filer_operation(
        &self,
        operation: RuntimeFilerOperation,
    ) -> BoxFuture<Result<RuntimeFilerOperationReport, RuntimeFilerError>> {
        Box::pin(async move { execute_local_filer_operation(operation) })
    }
}

pub fn find_workspace_root_path(path: PathBuf, markers: &[String]) -> Option<PathBuf> {
    if markers.is_empty() {
        return None;
    }
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    let mut current = if path.is_file() || path.extension().is_some() {
        path.parent().map(std::path::Path::to_path_buf)?
    } else {
        path
    };
    loop {
        for marker in markers {
            if marker.trim().is_empty() {
                continue;
            }
            if current.join(marker).exists() {
                log::debug!(
                    "[saya_live_runtime][workspace] root marker matched: root={}, marker={}",
                    current.display(),
                    marker
                );
                return Some(current);
            }
        }
        if !current.pop() {
            log::debug!(
                "[saya_live_runtime][workspace] no root marker matched: markers={markers:?}"
            );
            return None;
        }
    }
}

/// runtime phase の正式な `saya` 公開面を返す。
pub fn runtime_public_surface_names() -> &'static [&'static str] {
    RUNTIME_PUBLIC_SURFACE_NAMES
}

/// MVP から除外する危険な capability 名を返す。
pub fn runtime_forbidden_surface_names() -> &'static [&'static str] {
    RUNTIME_FORBIDDEN_SURFACE_NAMES
}

type CommandCallback =
    Arc<dyn Fn(RuntimeContext) -> BoxFuture<Result<(), RuntimeCommandError>> + Send + Sync>;
type EventCallback = Arc<
    dyn Fn(RuntimeContext, BufferEventPayload) -> BoxFuture<Result<(), RuntimeCallbackError>>
        + Send
        + Sync,
>;

#[derive(Clone, Default)]
pub struct CallbackRegistryBuilder {
    commands: Vec<(String, CommandCallback)>,
    buffer_open_handlers: Vec<EventCallback>,
    buffer_changed_handlers: Vec<EventCallback>,
    buffer_write_post_handlers: Vec<EventCallback>,
    buffer_closed_handlers: Vec<EventCallback>,
}

impl CallbackRegistryBuilder {
    pub fn register_command<F>(&mut self, name: &str, callback: F) -> &mut Self
    where
        F: Fn(RuntimeContext) -> BoxFuture<Result<(), RuntimeCommandError>> + Send + Sync + 'static,
    {
        log::debug!("[saya_live_runtime] register command: {}", name);
        self.commands.push((name.to_string(), Arc::new(callback)));
        self
    }

    pub fn on_buffer_open<F>(&mut self, callback: F) -> &mut Self
    where
        F: Fn(RuntimeContext, BufferEventPayload) -> BoxFuture<Result<(), RuntimeCallbackError>>
            + Send
            + Sync
            + 'static,
    {
        log::debug!("[saya_live_runtime] register bufferOpen handler");
        self.buffer_open_handlers.push(Arc::new(callback));
        self
    }

    pub fn on_buffer_write_post<F>(&mut self, callback: F) -> &mut Self
    where
        F: Fn(RuntimeContext, BufferEventPayload) -> BoxFuture<Result<(), RuntimeCallbackError>>
            + Send
            + Sync
            + 'static,
    {
        log::debug!("[saya_live_runtime] register bufferWritePost handler");
        self.buffer_write_post_handlers.push(Arc::new(callback));
        self
    }

    pub fn on_buffer_changed<F>(&mut self, callback: F) -> &mut Self
    where
        F: Fn(RuntimeContext, BufferEventPayload) -> BoxFuture<Result<(), RuntimeCallbackError>>
            + Send
            + Sync
            + 'static,
    {
        log::debug!("[saya_live_runtime] register bufferChanged handler");
        self.buffer_changed_handlers.push(Arc::new(callback));
        self
    }

    pub fn on_buffer_closed<F>(&mut self, callback: F) -> &mut Self
    where
        F: Fn(RuntimeContext, BufferEventPayload) -> BoxFuture<Result<(), RuntimeCallbackError>>
            + Send
            + Sync
            + 'static,
    {
        log::debug!("[saya_live_runtime] register bufferClosed handler");
        self.buffer_closed_handlers.push(Arc::new(callback));
        self
    }

    pub fn build(&self) -> CallbackRegistry {
        log::debug!(
            "[saya_live_runtime] build callback registry: commands={}, buffer_open_handlers={}, buffer_changed_handlers={}, buffer_write_post_handlers={}, buffer_closed_handlers={}",
            self.commands.len(),
            self.buffer_open_handlers.len(),
            self.buffer_changed_handlers.len(),
            self.buffer_write_post_handlers.len(),
            self.buffer_closed_handlers.len()
        );

        let mut commands = HashMap::new();
        for (name, callback) in &self.commands {
            commands.insert(name.clone(), callback.clone());
        }

        CallbackRegistry {
            commands,
            handlers: HashMap::from([
                (
                    RuntimeEventName::BufferOpen,
                    self.buffer_open_handlers.clone(),
                ),
                (
                    RuntimeEventName::BufferChanged,
                    self.buffer_changed_handlers.clone(),
                ),
                (
                    RuntimeEventName::BufferWritePost,
                    self.buffer_write_post_handlers.clone(),
                ),
                (
                    RuntimeEventName::BufferClosed,
                    self.buffer_closed_handlers.clone(),
                ),
            ]),
        }
    }
}

#[derive(Clone)]
pub struct CallbackRegistry {
    commands: HashMap<String, CommandCallback>,
    handlers: HashMap<RuntimeEventName, Vec<EventCallback>>,
}

impl CallbackRegistry {
    fn command(&self, name: &str) -> Option<CommandCallback> {
        self.commands.get(name).cloned()
    }

    fn handlers_for(&self, event: RuntimeEventName) -> Vec<EventCallback> {
        self.handlers.get(&event).cloned().unwrap_or_default()
    }
}

#[derive(Clone)]
struct LiveRuntimeOpState {
    bridge: Arc<dyn HostCapabilityBridge>,
    selector_sessions: Arc<StdMutex<RuntimeSelectorSessions>>,
    /// Phase A.2: 汎用プロセス I/O (`saya.process.*`) op の共有プール。
    ///
    /// Bootstrap script の `saya.process.spawn(...)` ラッパおよびテスト
    /// 用 `Deno.core.ops.op_process_*` から参照される。
    process_pool: Arc<crate::runtime::process_pool::ProcessPool>,
    /// LSP 専用の managed session capability。
    ///
    /// TypeScript plugin は server selection と request construction を
    /// 維持しつつ、process lifecycle と JSON-RPC transport はここへ委譲する。
    lsp_session_pool: Arc<ManagedLspSessionPool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeLazyPluginLoadRequest {
    pub kind: String,
    pub name: String,
    pub plugin: String,
    pub module: String,
    pub export_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInputPromptRequest {
    pub title: String,
    pub placeholder: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum RuntimeInputPromptResponse {
    Submitted { value: String },
    Cancelled,
}

#[derive(Debug, Default)]
struct SeedRuntimeMetadata {
    handler_counts: HashMap<RuntimeEventName, usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EncodedCallbackFailure {
    handler_index: usize,
    error: String,
}

#[op2(async(deferred), fast)]
async fn op_runtime_execute_host_command(
    state: Rc<RefCell<OpState>>,
    #[string] name: String,
) -> Result<(), JsErrorBox> {
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    log::debug!(
        "[saya_live_runtime] runtime op execute host command: name={}",
        name
    );
    bridge
        .execute_host_command(&name)
        .await
        .map_err(runtime_command_error_to_js_error)
}

#[op2(async(deferred), fast)]
async fn op_runtime_plugin_load_lazy(#[string] request_json: String) -> Result<(), JsErrorBox> {
    let request = serde_json::from_str::<RuntimeLazyPluginLoadRequest>(&request_json)
        .map_err(|error| JsErrorBox::generic(format!("invalid lazy plugin request: {error}")))?;
    if request.kind != "command" && request.kind != "event" {
        log::debug!(
            "[saya-plugin-host][lazy] lazy bridge rejected unsupported trigger kind: kind={}, name={}, plugin={}",
            request.kind,
            request.name,
            request.plugin
        );
        return Err(JsErrorBox::generic(format!(
            "unsupported lazy plugin trigger kind: {}",
            request.kind
        )));
    }
    if request.plugin.trim().is_empty()
        || request.module.trim().is_empty()
        || request.export_name.trim().is_empty()
    {
        log::debug!(
            "[saya-plugin-host][lazy] lazy bridge rejected incomplete target: kind={}, name={}, plugin={}, module={}, export={}",
            request.kind,
            request.name,
            request.plugin,
            request.module,
            request.export_name
        );
        return Err(JsErrorBox::generic("incomplete lazy plugin target"));
    }
    log::info!(
        "[saya-plugin-host][lazy] lazy bridge accepted: kind={}, name={}, plugin={}, module={}, export={}",
        request.kind,
        request.name,
        request.plugin,
        request.module,
        request.export_name
    );
    Ok(())
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_lsif_request(
    state: Rc<RefCell<OpState>>,
    #[string] request_json: String,
) -> Result<LspRuntimeBridgeResponse, JsErrorBox> {
    let request = serde_json::from_str::<LspRuntimeBridgeRequest>(&request_json)
        .map_err(|error| JsErrorBox::generic(format!("invalid LSIF bridge request: {error}")))?;
    if let Err(error) = request.validate() {
        log::debug!(
            "[saya_live_runtime][lsif] invalid typed LSIF request rejected: error={:?}",
            error
        );
        return Err(JsErrorBox::generic(format!(
            "invalid LSIF bridge request: {error:?}"
        )));
    }
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    log::debug!(
        "[saya_live_runtime][lsif] runtime op typed LSIF request: method={}, client={}, document={}",
        request.method,
        request.client_name,
        request
            .text_document
            .as_ref()
            .map(|document| document.uri.as_str())
            .unwrap_or("<none>")
    );
    match bridge.execute_lsif_request(request).await {
        Ok(response) => Ok(response),
        Err(error) => {
            log::debug!(
                "[saya_live_runtime][lsif] typed LSIF request failed: error={:?}",
                error
            );
            Err(runtime_command_error_to_js_error(error))
        }
    }
}

#[op2(async(deferred), fast)]
async fn op_runtime_completion_show(
    state: Rc<RefCell<OpState>>,
    #[string] request_json: String,
) -> Result<bool, JsErrorBox> {
    let request =
        serde_json::from_str::<CompletionShowRequest>(&request_json).map_err(|error| {
            JsErrorBox::generic(format!("invalid completion show request: {error}"))
        })?;
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    log::debug!(
        "[saya_live_runtime][completion] runtime op show: session_id={}, request_id={}, candidates={}",
        request.session_id,
        request.request_id,
        request.candidates.len()
    );
    bridge
        .show_completion(request)
        .await
        .map_err(runtime_command_error_to_js_error)
}

#[op2(async(deferred), fast)]
async fn op_runtime_completion_close(state: Rc<RefCell<OpState>>) -> Result<bool, JsErrorBox> {
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    log::debug!("[saya_live_runtime][completion] runtime op close");
    bridge
        .close_completion()
        .await
        .map_err(runtime_command_error_to_js_error)
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_input_prompt(
    state: Rc<RefCell<OpState>>,
    #[string] request_json: String,
) -> Result<RuntimeInputPromptResponse, JsErrorBox> {
    let request = serde_json::from_str::<RuntimeInputPromptRequest>(&request_json)
        .map_err(|error| JsErrorBox::generic(format!("invalid input.prompt request: {error}")))?;
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    log::info!(
        "[saya_live_runtime][input] runtime prompt requested: title={}, placeholder_present={}",
        request.title,
        request.placeholder.is_some()
    );
    match bridge.request_input_prompt(request).await {
        Ok(RuntimeInputPromptResponse::Submitted { value }) => {
            log::info!(
                "[saya_live_runtime][input] runtime prompt resolved: value_len={}",
                value.len()
            );
            Ok(RuntimeInputPromptResponse::Submitted { value })
        }
        Ok(RuntimeInputPromptResponse::Cancelled) => {
            log::info!("[saya_live_runtime][input] runtime prompt cancelled");
            Ok(RuntimeInputPromptResponse::Cancelled)
        }
        Err(error) => {
            log::debug!(
                "[saya_live_runtime][input] runtime prompt failed: error={:?}",
                error
            );
            Err(runtime_command_error_to_js_error(error))
        }
    }
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_selector_open(
    state: Rc<RefCell<OpState>>,
    #[string] request_json: String,
) -> Result<RuntimeSelectorSnapshot, JsErrorBox> {
    let request = serde_json::from_str::<RuntimeSelectorOpenRequest>(&request_json)
        .map_err(|error| JsErrorBox::generic(format!("invalid selector.open request: {error}")))?;
    let sessions = state
        .borrow()
        .borrow::<LiveRuntimeOpState>()
        .selector_sessions
        .clone();
    log::debug!("[saya_live_runtime][selector] runtime op open");
    let request = resolve_runtime_selector_source(request)
        .await
        .map_err(|error| JsErrorBox::generic(format!("rg selector source failed: {error}")))?;
    sessions
        .lock()
        .expect("selector sessions poisoned")
        .open(request)
        .map_err(|error| JsErrorBox::generic(error.to_string()))
}

async fn resolve_runtime_selector_source(
    mut request: RuntimeSelectorOpenRequest,
) -> Result<RuntimeSelectorOpenRequest, RuntimeSelectorError> {
    let RuntimeSelectorSourceRequest::Rg { root, pattern } = &request.source else {
        return Ok(request);
    };
    if pattern.is_empty() {
        return Err(RuntimeSelectorError::InvalidRequest(
            "rg selector source requires a non-empty pattern".to_string(),
        ));
    }
    let root = root
        .as_deref()
        .filter(|root| !root.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let pattern = pattern.clone();
    let items = collect_rg_selector_items(root, pattern).await?;
    request.source = RuntimeSelectorSourceRequest::Static { items };
    Ok(request)
}

async fn collect_rg_selector_items(
    root: PathBuf,
    pattern: String,
) -> Result<Vec<crate::features::selector::runtime::RuntimeSelectorItem>, RuntimeSelectorError> {
    log::debug!(
        "[saya_live_runtime][selector][rg] collect source: root={}, pattern_len={}",
        root.display(),
        pattern.len()
    );
    let mut command = TokioCommand::new("rg");
    command
        .arg("--vimgrep")
        .arg("--")
        .arg(&pattern)
        .arg(&root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let output = command.output().await.map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            RuntimeSelectorError::SourceUnavailable("rg executable was not found".to_string())
        } else {
            RuntimeSelectorError::SourceFailed(format!("failed to execute rg: {error}"))
        }
    })?;

    let status_code = output.status.code();
    if output.status.success() {
        let stdout = String::from_utf8(output.stdout).map_err(|error| {
            RuntimeSelectorError::SourceFailed(format!("rg stdout was not UTF-8: {error}"))
        })?;
        let items = parse_rg_vimgrep_output(&stdout)?;
        log::debug!(
            "[saya_live_runtime][selector][rg] collect completed: root={}, items={}",
            root.display(),
            items.len()
        );
        return Ok(items);
    }

    if status_code == Some(1) {
        log::debug!(
            "[saya_live_runtime][selector][rg] collect completed with no matches: root={}",
            root.display()
        );
        return Ok(Vec::new());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let message = stderr.lines().next().unwrap_or("rg failed").to_string();
    log::debug!(
        "[saya_live_runtime][selector][rg] collect failed: root={}, status={:?}, stderr_first_line={:?}",
        root.display(),
        status_code,
        message
    );
    Err(RuntimeSelectorError::SourceFailed(format!(
        "rg exited with status {:?}: {}",
        status_code, message
    )))
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_selector_update(
    state: Rc<RefCell<OpState>>,
    #[string] id: String,
    #[string] request_json: String,
) -> Result<RuntimeSelectorSnapshot, JsErrorBox> {
    let id = id
        .parse::<u64>()
        .map_err(|error| JsErrorBox::generic(format!("invalid selector.update id: {error}")))?;
    let request =
        serde_json::from_str::<RuntimeSelectorUpdateRequest>(&request_json).map_err(|error| {
            JsErrorBox::generic(format!("invalid selector.update request: {error}"))
        })?;
    let sessions = state
        .borrow()
        .borrow::<LiveRuntimeOpState>()
        .selector_sessions
        .clone();
    log::debug!("[saya_live_runtime][selector] runtime op update: id={id}");
    sessions
        .lock()
        .expect("selector sessions poisoned")
        .update(id, request)
        .map_err(|error| JsErrorBox::generic(error.to_string()))
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_selector_current(
    state: Rc<RefCell<OpState>>,
    #[string] id: String,
) -> Result<RuntimeSelectorSnapshot, JsErrorBox> {
    let id = id
        .parse::<u64>()
        .map_err(|error| JsErrorBox::generic(format!("invalid selector.current id: {error}")))?;
    let sessions = state
        .borrow()
        .borrow::<LiveRuntimeOpState>()
        .selector_sessions
        .clone();
    log::debug!("[saya_live_runtime][selector] runtime op current: id={id}");
    sessions
        .lock()
        .expect("selector sessions poisoned")
        .current(id)
        .map_err(|error| JsErrorBox::generic(error.to_string()))
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_selector_control(
    state: Rc<RefCell<OpState>>,
    #[string] id: String,
    #[string] request_json: String,
) -> Result<RuntimeSelectorSnapshot, JsErrorBox> {
    let id = id
        .parse::<u64>()
        .map_err(|error| JsErrorBox::generic(format!("invalid selector.control id: {error}")))?;
    let request =
        serde_json::from_str::<RuntimeSelectorControlRequest>(&request_json).map_err(|error| {
            JsErrorBox::generic(format!("invalid selector.control request: {error}"))
        })?;
    let sessions = state
        .borrow()
        .borrow::<LiveRuntimeOpState>()
        .selector_sessions
        .clone();
    log::debug!(
        "[saya_live_runtime][selector] runtime op control: id={id}, command={:?}",
        request.command
    );
    sessions
        .lock()
        .expect("selector sessions poisoned")
        .control(id, request)
        .map_err(|error| JsErrorBox::generic(error.to_string()))
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_selector_cancel(
    state: Rc<RefCell<OpState>>,
    #[string] id: String,
) -> Result<RuntimeSelectorSnapshot, JsErrorBox> {
    let id = id
        .parse::<u64>()
        .map_err(|error| JsErrorBox::generic(format!("invalid selector.cancel id: {error}")))?;
    let sessions = state
        .borrow()
        .borrow::<LiveRuntimeOpState>()
        .selector_sessions
        .clone();
    log::debug!("[saya_live_runtime][selector] runtime op cancel: id={id}");
    sessions
        .lock()
        .expect("selector sessions poisoned")
        .cancel(id)
        .map_err(|error| JsErrorBox::generic(error.to_string()))
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_selector_dispose(
    state: Rc<RefCell<OpState>>,
    #[string] id: String,
) -> Result<serde_json::Value, JsErrorBox> {
    let id = id
        .parse::<u64>()
        .map_err(|error| JsErrorBox::generic(format!("invalid selector.dispose id: {error}")))?;
    let sessions = state
        .borrow()
        .borrow::<LiveRuntimeOpState>()
        .selector_sessions
        .clone();
    log::debug!("[saya_live_runtime][selector] runtime op dispose: id={id}");
    let disposed = sessions
        .lock()
        .expect("selector sessions poisoned")
        .dispose(id)
        .map_err(|error| JsErrorBox::generic(error.to_string()))?;
    Ok(serde_json::Value::Bool(disposed))
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_workspace_find_root(
    state: Rc<RefCell<OpState>>,
    #[string] path: String,
    #[string] markers_json: String,
) -> Result<serde_json::Value, JsErrorBox> {
    let markers = serde_json::from_str::<Vec<String>>(&markers_json)
        .map_err(|error| JsErrorBox::generic(format!("invalid workspace root markers: {error}")))?;
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    log::debug!(
        "[saya_live_runtime][workspace] runtime op findRoot: path={}, markers={:?}",
        path,
        markers
    );
    Ok(bridge
        .find_workspace_root(path, markers)
        .await
        .map(serde_json::Value::String)
        .unwrap_or(serde_json::Value::Null))
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_current_buffer(
    state: Rc<RefCell<OpState>>,
) -> Result<ReadonlyBufferSnapshot, JsErrorBox> {
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    log::debug!("[saya_live_runtime] runtime op current_buffer");
    Ok(bridge.current_buffer().await)
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_current_buffer_path(
    state: Rc<RefCell<OpState>>,
) -> Result<Option<PathBuf>, JsErrorBox> {
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    log::debug!("[saya_live_runtime] runtime op current_buffer_path");
    Ok(bridge.current_buffer_path().await)
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_current_selection(
    state: Rc<RefCell<OpState>>,
) -> Result<Option<ReadonlySelectionSnapshot>, JsErrorBox> {
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    log::debug!("[saya_live_runtime] runtime op current_selection");
    Ok(bridge.current_selection().await)
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_current_window(
    state: Rc<RefCell<OpState>>,
) -> Result<ReadonlyWindowSnapshot, JsErrorBox> {
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    log::debug!("[saya_live_runtime] runtime op current_window");
    Ok(bridge.current_window().await)
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_window_open_float(
    state: Rc<RefCell<OpState>>,
    #[string] request_json: String,
) -> Result<RuntimeFloatSnapshot, JsErrorBox> {
    let request =
        serde_json::from_str::<RuntimeFloatOpenRequest>(&request_json).map_err(|error| {
            JsErrorBox::generic(format!("invalid window.openFloat options: {error}"))
        })?;
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    log::debug!(
        "[saya_live_runtime][window] runtime op openFloat: content={:?}, size=({:?},{:?}), group={:?}",
        request.content,
        request.width,
        request.height,
        request.group
    );
    bridge
        .open_float(request)
        .await
        .map_err(runtime_command_error_to_js_error)
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_window_close_float(
    state: Rc<RefCell<OpState>>,
    #[string] id: String,
) -> Result<serde_json::Value, JsErrorBox> {
    let id = id
        .parse::<u64>()
        .map_err(|error| JsErrorBox::generic(format!("invalid window.close id: {error}")))?;
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    log::debug!("[saya_live_runtime][window] runtime op close: id={}", id);
    let closed = bridge
        .close_float(id)
        .await
        .map_err(runtime_command_error_to_js_error)?;
    Ok(serde_json::Value::Bool(closed))
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_window_focus_float(
    state: Rc<RefCell<OpState>>,
    #[string] id: String,
) -> Result<serde_json::Value, JsErrorBox> {
    let id = id
        .parse::<u64>()
        .map_err(|error| JsErrorBox::generic(format!("invalid window.focus id: {error}")))?;
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    log::debug!("[saya_live_runtime][window] runtime op focus: id={}", id);
    let focused = bridge
        .focus_float(id)
        .await
        .map_err(runtime_command_error_to_js_error)?;
    Ok(serde_json::Value::Bool(focused))
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_window_floats(
    state: Rc<RefCell<OpState>>,
) -> Result<Vec<RuntimeFloatSnapshot>, JsErrorBox> {
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    log::debug!("[saya_live_runtime][window] runtime op floats");
    bridge
        .list_float_snapshots()
        .await
        .map_err(runtime_command_error_to_js_error)
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_panel_open(
    state: Rc<RefCell<OpState>>,
    #[string] request_json: String,
) -> Result<RuntimePanelSnapshot, JsErrorBox> {
    let request = serde_json::from_str::<RuntimePanelOpenRequest>(&request_json)
        .map_err(|error| JsErrorBox::generic(format!("invalid panel.open options: {error}")))?;
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    log::debug!(
        "[saya_live_runtime][panel] runtime op open: id={}, position={}, size={}, content={:?}, focus={}",
        request.id,
        request.position,
        request.size,
        request.content,
        request.focus
    );
    bridge
        .open_panel(request)
        .await
        .map_err(runtime_command_error_to_js_error)
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_panel_focus(
    state: Rc<RefCell<OpState>>,
    #[string] id: String,
) -> Result<serde_json::Value, JsErrorBox> {
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    log::debug!("[saya_live_runtime][panel] runtime op focus: id={}", id);
    let focused = bridge
        .focus_panel(id)
        .await
        .map_err(runtime_command_error_to_js_error)?;
    Ok(serde_json::Value::Bool(focused))
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_panel_unfocus(
    state: Rc<RefCell<OpState>>,
) -> Result<serde_json::Value, JsErrorBox> {
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    log::debug!("[saya_live_runtime][panel] runtime op unfocus");
    let unfocused = bridge
        .unfocus_panel()
        .await
        .map_err(runtime_command_error_to_js_error)?;
    Ok(serde_json::Value::Bool(unfocused))
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_panel_close(
    state: Rc<RefCell<OpState>>,
    #[string] id: String,
) -> Result<serde_json::Value, JsErrorBox> {
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    log::debug!("[saya_live_runtime][panel] runtime op close: id={}", id);
    let closed = bridge
        .close_panel(id)
        .await
        .map_err(runtime_command_error_to_js_error)?;
    Ok(serde_json::Value::Bool(closed))
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_panel_list(
    state: Rc<RefCell<OpState>>,
) -> Result<Vec<RuntimePanelSnapshot>, JsErrorBox> {
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    log::debug!("[saya_live_runtime][panel] runtime op list");
    bridge
        .list_panel_snapshots()
        .await
        .map_err(runtime_command_error_to_js_error)
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_panel_send(
    state: Rc<RefCell<OpState>>,
    #[string] id: String,
    #[string] text: String,
) -> Result<serde_json::Value, JsErrorBox> {
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    log::debug!(
        "[saya_live_runtime][panel] runtime op send: id={}, bytes={}",
        id,
        text.len()
    );
    let sent = bridge
        .send_panel_text(id, text)
        .await
        .map_err(runtime_command_error_to_js_error)?;
    Ok(serde_json::Value::Bool(sent))
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_current_editor(
    state: Rc<RefCell<OpState>>,
) -> Result<ReadonlyEditorSnapshot, JsErrorBox> {
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    log::debug!("[saya_live_runtime] runtime op current_editor");
    Ok(bridge.current_editor().await)
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_filer_list(
    state: Rc<RefCell<OpState>>,
    #[string] path: String,
    #[string] options_json: String,
) -> Result<Vec<RuntimeFilerEntry>, JsErrorBox> {
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    let path = PathBuf::from(path);
    let options = serde_json::from_str::<RuntimeFilerListOptions>(&options_json)
        .map_err(|error| JsErrorBox::generic(format!("invalid filer list options: {error}")))?;
    log::debug!(
        "[saya_live_runtime] runtime op filer list: path={}, show_hidden={}, sort_by={:?}, filter={:?}",
        path.display(),
        options.show_hidden,
        options.sort_by,
        options.filter
    );
    bridge
        .list_filer_entries(path, options)
        .await
        .map_err(runtime_filer_error_to_js_error)
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_fs_read_dir(
    #[string] path: String,
    #[string] options_json: String,
) -> Result<Vec<RuntimeFilerEntry>, JsErrorBox> {
    let path = PathBuf::from(path);
    let options = serde_json::from_str::<RuntimeFilerListOptions>(&options_json)
        .map_err(|error| JsErrorBox::generic(format!("invalid fs readDir options: {error}")))?;
    log::debug!(
        "[saya_live_runtime] runtime op fs readDir: path={}, show_hidden={}, sort_by={:?}, filter={:?}",
        path.display(),
        options.show_hidden,
        options.sort_by,
        options.filter
    );
    list_local_filer_entries(path, options).map_err(runtime_filer_error_to_js_error)
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_filer_current_entry(
    state: Rc<RefCell<OpState>>,
) -> Result<Option<RuntimeFilerCurrentEntry>, JsErrorBox> {
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    log::debug!("[saya_live_runtime] runtime op filer currentEntry");
    bridge
        .current_filer_entry()
        .await
        .map_err(runtime_filer_error_to_js_error)
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_filer_create_file(
    state: Rc<RefCell<OpState>>,
    #[string] path: String,
) -> Result<RuntimeFilerOperationReport, JsErrorBox> {
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    let path = PathBuf::from(path);
    log::debug!(
        "[saya_live_runtime] runtime op filer createFile: path={}",
        path.display()
    );
    bridge
        .execute_filer_operation(RuntimeFilerOperation::CreateFile { path })
        .await
        .map_err(runtime_filer_error_to_js_error)
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_filer_create_directory(
    state: Rc<RefCell<OpState>>,
    #[string] path: String,
) -> Result<RuntimeFilerOperationReport, JsErrorBox> {
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    let path = PathBuf::from(path);
    log::debug!(
        "[saya_live_runtime] runtime op filer createDirectory: path={}",
        path.display()
    );
    bridge
        .execute_filer_operation(RuntimeFilerOperation::CreateDirectory { path })
        .await
        .map_err(runtime_filer_error_to_js_error)
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_filer_rename(
    state: Rc<RefCell<OpState>>,
    #[string] from: String,
    #[string] to: String,
) -> Result<RuntimeFilerOperationReport, JsErrorBox> {
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    let from = PathBuf::from(from);
    let to = PathBuf::from(to);
    log::debug!(
        "[saya_live_runtime] runtime op filer rename: from={}, to={}",
        from.display(),
        to.display()
    );
    bridge
        .execute_filer_operation(RuntimeFilerOperation::Rename { from, to })
        .await
        .map_err(runtime_filer_error_to_js_error)
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_filer_copy(
    state: Rc<RefCell<OpState>>,
    #[string] from: String,
    #[string] to: String,
) -> Result<RuntimeFilerOperationReport, JsErrorBox> {
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    let from = PathBuf::from(from);
    let to = PathBuf::from(to);
    log::debug!(
        "[saya_live_runtime] runtime op filer copy: from={}, to={}",
        from.display(),
        to.display()
    );
    bridge
        .execute_filer_operation(RuntimeFilerOperation::Copy { from, to })
        .await
        .map_err(runtime_filer_error_to_js_error)
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_filer_move(
    state: Rc<RefCell<OpState>>,
    #[string] from: String,
    #[string] to: String,
) -> Result<RuntimeFilerOperationReport, JsErrorBox> {
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    let from = PathBuf::from(from);
    let to = PathBuf::from(to);
    log::debug!(
        "[saya_live_runtime] runtime op filer move: from={}, to={}",
        from.display(),
        to.display()
    );
    bridge
        .execute_filer_operation(RuntimeFilerOperation::Move { from, to })
        .await
        .map_err(runtime_filer_error_to_js_error)
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_filer_delete(
    state: Rc<RefCell<OpState>>,
    #[string] path: String,
    confirm: bool,
    recursive: bool,
    trash: bool,
) -> Result<RuntimeFilerOperationReport, JsErrorBox> {
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    let path = PathBuf::from(path);
    log::debug!(
        "[saya_live_runtime] runtime op filer delete: path={}, confirm={}, recursive={}, trash={}",
        path.display(),
        confirm,
        recursive,
        trash
    );
    bridge
        .execute_filer_operation(RuntimeFilerOperation::Delete {
            path,
            confirm,
            recursive,
            trash,
        })
        .await
        .map_err(runtime_filer_error_to_js_error)
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_filer_mark(
    state: Rc<RefCell<OpState>>,
    #[string] path: String,
) -> Result<RuntimeFilerOperationReport, JsErrorBox> {
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    let path = PathBuf::from(path);
    log::debug!(
        "[saya_live_runtime] runtime op filer mark: path={}",
        path.display()
    );
    bridge
        .execute_filer_operation(RuntimeFilerOperation::Mark { path })
        .await
        .map_err(runtime_filer_error_to_js_error)
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_filer_unmark(
    state: Rc<RefCell<OpState>>,
    #[string] path: String,
) -> Result<RuntimeFilerOperationReport, JsErrorBox> {
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    let path = PathBuf::from(path);
    log::debug!(
        "[saya_live_runtime] runtime op filer unmark: path={}",
        path.display()
    );
    bridge
        .execute_filer_operation(RuntimeFilerOperation::Unmark { path })
        .await
        .map_err(runtime_filer_error_to_js_error)
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_filer_clear_marks(
    state: Rc<RefCell<OpState>>,
) -> Result<RuntimeFilerOperationReport, JsErrorBox> {
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    log::debug!("[saya_live_runtime] runtime op filer clearMarks");
    bridge
        .execute_filer_operation(RuntimeFilerOperation::ClearMarks)
        .await
        .map_err(runtime_filer_error_to_js_error)
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_filer_bulk_delete_preview(
    state: Rc<RefCell<OpState>>,
) -> Result<RuntimeFilerOperationReport, JsErrorBox> {
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    log::debug!("[saya_live_runtime] runtime op filer bulkDeletePreview");
    bridge
        .execute_filer_operation(RuntimeFilerOperation::BulkDeletePreview)
        .await
        .map_err(runtime_filer_error_to_js_error)
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_filer_bulk_delete(
    state: Rc<RefCell<OpState>>,
    #[string] preview_id: String,
    confirm: bool,
) -> Result<RuntimeFilerOperationReport, JsErrorBox> {
    let bridge = state.borrow().borrow::<LiveRuntimeOpState>().bridge.clone();
    log::debug!(
        "[saya_live_runtime] runtime op filer bulkDelete: preview_id={}, confirm={}",
        preview_id,
        confirm
    );
    bridge
        .execute_filer_operation(RuntimeFilerOperation::BulkDelete {
            preview_id,
            confirm,
        })
        .await
        .map_err(runtime_filer_error_to_js_error)
}

/// Plugin から呼ばれた `console.log` / `console.warn` / `console.error` 等を
/// stdout にそのまま吐かせると raw-mode の TUI 画面が破壊されるため、
/// すべて diagnostic logger 経由（log::debug! / log::warn! / log::error!）に
/// 流す。レベルは `level` 引数で受ける（"log"/"info"/"debug"/"warn"/"error"）。
#[op2(fast)]
fn op_runtime_console_log(#[string] level: &str, #[string] message: &str) {
    match level {
        "error" => log::error!("[saya-runtime-console] {message}"),
        "warn" => log::warn!("[saya-runtime-console] {message}"),
        "info" | "log" => log::info!("[saya-runtime-console] {message}"),
        _ => log::debug!("[saya-runtime-console] {message}"),
    }
}

/// `op_process_*` で TS から渡される `ProcessSpec` の JSON 表現。
///
/// `saya.process.spawn(...)` ラッパが Object を `JSON.stringify` して
/// 渡すため、ここでは serde で受けて内部の `ProcessSpec` に変換する。
/// `command` 以外のフィールドはオプショナルで、未指定時は安全側
/// (`StdioMode::Null`) にフォールバックする。
#[derive(Debug, Deserialize)]
struct RuntimeProcessSpec {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    stdin: Option<RuntimeStdioMode>,
    #[serde(default)]
    stdout: Option<RuntimeStdioMode>,
    #[serde(default)]
    stderr: Option<RuntimeStdioMode>,
}

/// TS 側 `"inherit" | "null" | "piped"` を `StdioMode` に対応付ける。
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RuntimeStdioMode {
    Inherit,
    Null,
    Piped,
}

impl From<RuntimeStdioMode> for StdioMode {
    fn from(value: RuntimeStdioMode) -> Self {
        match value {
            RuntimeStdioMode::Inherit => StdioMode::Inherit,
            RuntimeStdioMode::Null => StdioMode::Null,
            RuntimeStdioMode::Piped => StdioMode::Piped,
        }
    }
}

impl RuntimeProcessSpec {
    fn into_process_spec(self) -> ProcessSpec {
        ProcessSpec {
            command: self.command,
            args: self.args,
            env: self.env,
            cwd: self.cwd.map(PathBuf::from),
            stdin: self.stdin.map(StdioMode::from).unwrap_or(StdioMode::Null),
            stdout: self.stdout.map(StdioMode::from).unwrap_or(StdioMode::Null),
            stderr: self.stderr.map(StdioMode::from).unwrap_or(StdioMode::Null),
        }
    }
}

/// `ProcessPoolError` を `JsErrorBox` へ正規化するヘルパ。
///
/// op 戻り値の `Err(JsErrorBox)` は JS 側で `Error.message` として観測
/// できる文字列のみを保持できるため、enum バリアントの情報を `Display`
/// 経由で文字列化する。LSP プラグイン (Phase B) は `error.message` の
/// 先頭プレフィクスでバリアントを判定する設計を取れるよう、`Display`
/// 実装側でラベルを揃えている。
fn process_pool_error_to_js_error(error: ProcessPoolError) -> JsErrorBox {
    JsErrorBox::generic(error.to_string())
}

fn managed_lsp_error_to_js_error(error: ManagedLspSessionError) -> JsErrorBox {
    JsErrorBox::generic(error.to_string())
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_lsp_connect(
    state: Rc<RefCell<OpState>>,
    #[string] request_json: String,
) -> Result<ManagedLspConnectResponse, JsErrorBox> {
    let request =
        serde_json::from_str::<ManagedLspConnectRequest>(&request_json).map_err(|error| {
            log::debug!("[saya_live_runtime][lsp] connect rejected: invalid request json: {error}");
            JsErrorBox::generic(format!("invalid LSP connect request: {error}"))
        })?;
    let pool = state
        .borrow()
        .borrow::<LiveRuntimeOpState>()
        .lsp_session_pool
        .clone();
    pool.connect(request)
        .await
        .map_err(managed_lsp_error_to_js_error)
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_lsp_request(
    state: Rc<RefCell<OpState>>,
    #[smi] session_id: u32,
    #[string] method: String,
    #[string] params_json: String,
) -> Result<ManagedLspRequestResponse, JsErrorBox> {
    let params = serde_json::from_str::<Value>(&params_json)
        .map_err(|error| JsErrorBox::generic(format!("invalid LSP request params: {error}")))?;
    let pool = state
        .borrow()
        .borrow::<LiveRuntimeOpState>()
        .lsp_session_pool
        .clone();
    pool.request(session_id, method, params)
        .await
        .map_err(managed_lsp_error_to_js_error)
}

#[op2(async(deferred), fast)]
#[serde]
async fn op_runtime_lsp_notify(
    state: Rc<RefCell<OpState>>,
    #[smi] session_id: u32,
    #[string] method: String,
    #[string] params_json: String,
) -> Result<ManagedLspNotifyResponse, JsErrorBox> {
    let params = serde_json::from_str::<Value>(&params_json).map_err(|error| {
        JsErrorBox::generic(format!("invalid LSP notification params: {error}"))
    })?;
    let pool = state
        .borrow()
        .borrow::<LiveRuntimeOpState>()
        .lsp_session_pool
        .clone();
    pool.notify(session_id, method, params)
        .await
        .map_err(managed_lsp_error_to_js_error)
}

#[op2(async(deferred), fast)]
async fn op_runtime_lsp_close(
    state: Rc<RefCell<OpState>>,
    #[smi] session_id: u32,
) -> Result<(), JsErrorBox> {
    let pool = state
        .borrow()
        .borrow::<LiveRuntimeOpState>()
        .lsp_session_pool
        .clone();
    pool.close(session_id)
        .await
        .map_err(managed_lsp_error_to_js_error)
}

/// Phase A.2: 子プロセスを spawn し、ハンドル ID を返す。
///
/// `spec_json` は `RuntimeProcessSpec` の JSON 表現。`stdin`/`stdout`/
/// `stderr` の各フィールドは `"inherit" | "null" | "piped"` のいずれか
/// を取り、未指定時は `null` (= `/dev/null`) にフォールバックする。
///
/// 戻り値は `u32` のハンドル ID（1 始まり、`AtomicU32` 連番）。失敗時
/// は `ProcessPoolError` を `Display` 経由で文字列化した `JsErrorBox`
/// を返す。
#[op2(async(deferred), fast)]
#[smi]
async fn op_process_spawn(
    state: Rc<RefCell<OpState>>,
    #[string] spec_json: String,
) -> Result<u32, JsErrorBox> {
    log::debug!(
        "[saya_live_runtime][process] op_process_spawn: spec_len={}",
        spec_json.len()
    );
    let runtime_spec = serde_json::from_str::<RuntimeProcessSpec>(&spec_json).map_err(|error| {
        log::debug!(
            "[saya_live_runtime][process] op_process_spawn rejected: invalid spec json: {error}"
        );
        JsErrorBox::generic(format!("invalid process spec: {error}"))
    })?;
    let pool = state
        .borrow()
        .borrow::<LiveRuntimeOpState>()
        .process_pool
        .clone();
    let process_spec = runtime_spec.into_process_spec();
    log::debug!(
        "[saya_live_runtime][process] op_process_spawn dispatching: command={:?}, args_len={}",
        process_spec.command,
        process_spec.args.len()
    );
    let handle = pool
        .spawn(process_spec)
        .await
        .map_err(process_pool_error_to_js_error)?;
    log::debug!("[saya_live_runtime][process] op_process_spawn ok: handle={handle}");
    Ok(handle)
}

/// Phase A.2: stdin に `buf` を書き込む（zero-copy）。
///
/// `JsBuffer` は V8 ArrayBuffer の `V8Slice` を保持しており、`as_ref()`
/// で `&[u8]` を取り出して `pool.write_stdin` に渡すことで kernel への
/// 単一コピーで到達する（中間 `Vec<u8>` を作らない）。戻り値は書き込ん
/// だバイト数（常に `buf.len()` と一致）。
///
/// `JsBuffer` を引数に取る async op は deno_core の fast path に乗らな
/// いため、`fast` 属性は付けない（deferred なら通常 path で十分）。
#[op2(async(deferred))]
#[smi]
async fn op_process_write_stdin(
    state: Rc<RefCell<OpState>>,
    #[smi] handle: u32,
    #[buffer] buf: JsBuffer,
) -> Result<u32, JsErrorBox> {
    let bytes = buf.as_ref();
    log::trace!(
        "[saya_live_runtime][process] op_process_write_stdin: handle={handle}, bytes={}",
        bytes.len()
    );
    let pool = state
        .borrow()
        .borrow::<LiveRuntimeOpState>()
        .process_pool
        .clone();
    let written = pool
        .write_stdin(handle, bytes)
        .await
        .map_err(process_pool_error_to_js_error)?;
    log::trace!(
        "[saya_live_runtime][process] op_process_write_stdin ok: handle={handle}, written={written}"
    );
    Ok(written as u32)
}

/// Phase A.2: stdout から `buf` に最大 `buf.len()` バイト読み出す（zero-copy）。
///
/// `JsBuffer` の `as_mut()` 経由で V8 ArrayBuffer の backing store に
/// 直接書き込む。戻り値は読み込みバイト数で、`0` は EOF を表す（TS
/// 側 `saya.process` ラッパで `null` に正規化される）。
#[op2(async(deferred))]
#[smi]
async fn op_process_read_stdout(
    state: Rc<RefCell<OpState>>,
    #[smi] handle: u32,
    #[buffer] mut buf: JsBuffer,
) -> Result<u32, JsErrorBox> {
    let capacity = buf.as_ref().len();
    log::trace!(
        "[saya_live_runtime][process] op_process_read_stdout: handle={handle}, capacity={capacity}"
    );
    let pool = state
        .borrow()
        .borrow::<LiveRuntimeOpState>()
        .process_pool
        .clone();
    let outcome = pool
        .read_stdout(handle, buf.as_mut())
        .await
        .map_err(process_pool_error_to_js_error)?;
    let n = outcome.unwrap_or(0);
    log::trace!(
        "[saya_live_runtime][process] op_process_read_stdout ok: handle={handle}, bytes={n}"
    );
    Ok(n as u32)
}

/// Phase A.2: stderr から `buf` に最大 `buf.len()` バイト読み出す。
///
/// 動作仕様は `op_process_read_stdout` と同等。`0` は EOF を表す。
#[op2(async(deferred))]
#[smi]
async fn op_process_read_stderr(
    state: Rc<RefCell<OpState>>,
    #[smi] handle: u32,
    #[buffer] mut buf: JsBuffer,
) -> Result<u32, JsErrorBox> {
    let capacity = buf.as_ref().len();
    log::trace!(
        "[saya_live_runtime][process] op_process_read_stderr: handle={handle}, capacity={capacity}"
    );
    let pool = state
        .borrow()
        .borrow::<LiveRuntimeOpState>()
        .process_pool
        .clone();
    let outcome = pool
        .read_stderr(handle, buf.as_mut())
        .await
        .map_err(process_pool_error_to_js_error)?;
    let n = outcome.unwrap_or(0);
    log::trace!(
        "[saya_live_runtime][process] op_process_read_stderr ok: handle={handle}, bytes={n}"
    );
    Ok(n as u32)
}

/// Phase A.2: `handle` のプロセスに kill シグナルを送る。
#[op2(async(deferred), fast)]
async fn op_process_kill(
    state: Rc<RefCell<OpState>>,
    #[smi] handle: u32,
) -> Result<(), JsErrorBox> {
    log::debug!("[saya_live_runtime][process] op_process_kill: handle={handle}");
    let pool = state
        .borrow()
        .borrow::<LiveRuntimeOpState>()
        .process_pool
        .clone();
    pool.kill(handle)
        .await
        .map_err(process_pool_error_to_js_error)?;
    log::debug!("[saya_live_runtime][process] op_process_kill ok: handle={handle}");
    Ok(())
}

/// Phase A.2: `handle` のプロセスの終了を待ち、exit code を返す。
///
/// シグナルで終了した場合は `128 + signal` (Unix 慣例)。
#[op2(async(deferred), fast)]
async fn op_process_wait(
    state: Rc<RefCell<OpState>>,
    #[smi] handle: u32,
) -> Result<i32, JsErrorBox> {
    log::debug!("[saya_live_runtime][process] op_process_wait: handle={handle}");
    let pool = state
        .borrow()
        .borrow::<LiveRuntimeOpState>()
        .process_pool
        .clone();
    let code = pool
        .wait(handle)
        .await
        .map_err(process_pool_error_to_js_error)?;
    log::debug!("[saya_live_runtime][process] op_process_wait ok: handle={handle}, code={code}");
    Ok(code)
}

deno_core::extension!(
    live_saya_extension,
    ops = [
        op_runtime_execute_host_command,
        op_runtime_plugin_load_lazy,
        op_runtime_lsif_request,
        op_runtime_lsp_connect,
        op_runtime_lsp_request,
        op_runtime_lsp_notify,
        op_runtime_lsp_close,
        op_runtime_completion_show,
        op_runtime_completion_close,
        op_runtime_input_prompt,
        op_runtime_selector_open,
        op_runtime_selector_update,
        op_runtime_selector_current,
        op_runtime_selector_control,
        op_runtime_selector_cancel,
        op_runtime_selector_dispose,
        op_runtime_workspace_find_root,
        op_runtime_fs_read_dir,
        op_runtime_current_buffer,
        op_runtime_current_buffer_path,
        op_runtime_current_selection,
        op_runtime_current_window,
        op_runtime_window_open_float,
        op_runtime_window_close_float,
        op_runtime_window_focus_float,
        op_runtime_window_floats,
        op_runtime_panel_open,
        op_runtime_panel_focus,
        op_runtime_panel_unfocus,
        op_runtime_panel_close,
        op_runtime_panel_list,
        op_runtime_panel_send,
        op_runtime_current_editor,
        op_runtime_filer_list,
        op_runtime_filer_current_entry,
        op_runtime_filer_create_file,
        op_runtime_filer_create_directory,
        op_runtime_filer_copy,
        op_runtime_filer_move,
        op_runtime_filer_rename,
        op_runtime_filer_delete,
        op_runtime_filer_mark,
        op_runtime_filer_unmark,
        op_runtime_filer_clear_marks,
        op_runtime_filer_bulk_delete_preview,
        op_runtime_filer_bulk_delete,
        op_runtime_console_log,
        op_process_spawn,
        op_process_write_stdin,
        op_process_read_stdout,
        op_process_read_stderr,
        op_process_kill,
        op_process_wait
    ],
    options = {
        bridge: Arc<dyn HostCapabilityBridge>,
        selector_sessions: Arc<StdMutex<RuntimeSelectorSessions>>,
        process_pool: Arc<ProcessPool>,
        lsp_session_pool: Arc<ManagedLspSessionPool>,
    },
    state = |state, options| {
        state.put(LiveRuntimeOpState {
            bridge: options.bridge,
            selector_sessions: options.selector_sessions,
            process_pool: options.process_pool,
            lsp_session_pool: options.lsp_session_pool,
        });
    }
);

fn runtime_command_error_to_js_error(error: RuntimeCommandError) -> JsErrorBox {
    let encoded = serde_json::to_string(&error).expect("runtime command error should serialize");
    JsErrorBox::generic(format!("{RUNTIME_COMMAND_ERROR_PREFIX}{encoded}"))
}

fn runtime_filer_error_to_js_error(error: RuntimeFilerError) -> JsErrorBox {
    match error {
        RuntimeFilerError::ReadFailed { path, message } => JsErrorBox::generic(format!(
            "failed to list filer path {}: {}",
            path.display(),
            message
        )),
        RuntimeFilerError::OperationFailed { .. } => {
            let encoded = serde_json::to_string(&error)
                .expect("runtime filer operation error should serialize");
            JsErrorBox::generic(format!("filer operation failed: {encoded}"))
        }
    }
}

pub fn execute_local_filer_operation(
    operation: RuntimeFilerOperation,
) -> Result<RuntimeFilerOperationReport, RuntimeFilerError> {
    match operation {
        RuntimeFilerOperation::CreateFile { path } => {
            log::info!(
                "[saya_live_runtime][filer] creating file through host operation: path={}",
                path.display()
            );
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|error| {
                    runtime_filer_io_error(
                        RuntimeFilerOperationKind::CreateFile,
                        &path,
                        None,
                        error,
                    )
                })?;
            Ok(RuntimeFilerOperationReport {
                operation: RuntimeFilerOperationKind::CreateFile,
                path: path.to_string_lossy().into_owned(),
                target_path: None,
                entries: Vec::new(),
                preview_id: None,
            })
        }
        RuntimeFilerOperation::CreateDirectory { path } => {
            log::info!(
                "[saya_live_runtime][filer] creating directory through host operation: path={}",
                path.display()
            );
            std::fs::create_dir(&path).map_err(|error| {
                runtime_filer_io_error(
                    RuntimeFilerOperationKind::CreateDirectory,
                    &path,
                    None,
                    error,
                )
            })?;
            Ok(RuntimeFilerOperationReport {
                operation: RuntimeFilerOperationKind::CreateDirectory,
                path: path.to_string_lossy().into_owned(),
                target_path: None,
                entries: Vec::new(),
                preview_id: None,
            })
        }
        RuntimeFilerOperation::Rename { from, to } => {
            log::info!(
                "[saya_live_runtime][filer] renaming through host operation: from={}, to={}",
                from.display(),
                to.display()
            );
            std::fs::rename(&from, &to).map_err(|error| {
                runtime_filer_io_error(RuntimeFilerOperationKind::Rename, &from, Some(&to), error)
            })?;
            Ok(RuntimeFilerOperationReport {
                operation: RuntimeFilerOperationKind::Rename,
                path: from.to_string_lossy().into_owned(),
                target_path: Some(to.to_string_lossy().into_owned()),
                entries: Vec::new(),
                preview_id: None,
            })
        }
        RuntimeFilerOperation::Copy { from, to } => {
            let start = std::time::Instant::now();
            log::info!(
                "[saya_live_runtime][filer] copying through host operation: from={}, to={}",
                from.display(),
                to.display()
            );
            if from.is_dir() {
                return Err(RuntimeFilerError::OperationFailed {
                    operation: RuntimeFilerOperationKind::Copy,
                    path: from,
                    target_path: Some(to),
                    kind: RuntimeFilerErrorKind::Unsupported,
                    message: "directory copy is not supported yet; recursive copy requires an explicit future policy".to_string(),
                });
            }
            std::fs::copy(&from, &to).map_err(|error| {
                runtime_filer_io_error(RuntimeFilerOperationKind::Copy, &from, Some(&to), error)
            })?;
            log::info!(
                "[saya_live_runtime][filer] copy completed: from={}, to={}, duration_ms={}",
                from.display(),
                to.display(),
                start.elapsed().as_millis()
            );
            Ok(RuntimeFilerOperationReport {
                operation: RuntimeFilerOperationKind::Copy,
                path: from.to_string_lossy().into_owned(),
                target_path: Some(to.to_string_lossy().into_owned()),
                entries: Vec::new(),
                preview_id: None,
            })
        }
        RuntimeFilerOperation::Move { from, to } => {
            let start = std::time::Instant::now();
            log::info!(
                "[saya_live_runtime][filer] moving through host operation: from={}, to={}",
                from.display(),
                to.display()
            );
            std::fs::rename(&from, &to).map_err(|error| {
                runtime_filer_io_error(RuntimeFilerOperationKind::Move, &from, Some(&to), error)
            })?;
            log::info!(
                "[saya_live_runtime][filer] move completed: from={}, to={}, duration_ms={}",
                from.display(),
                to.display(),
                start.elapsed().as_millis()
            );
            Ok(RuntimeFilerOperationReport {
                operation: RuntimeFilerOperationKind::Move,
                path: from.to_string_lossy().into_owned(),
                target_path: Some(to.to_string_lossy().into_owned()),
                entries: Vec::new(),
                preview_id: None,
            })
        }
        RuntimeFilerOperation::Delete {
            path,
            confirm,
            recursive,
            trash,
        } => {
            let start = std::time::Instant::now();
            log::info!(
                "[saya_live_runtime][filer] deleting through host operation: path={}, confirm={}, recursive={}, trash={}",
                path.display(),
                confirm,
                recursive,
                trash
            );
            if !confirm {
                return Err(RuntimeFilerError::OperationFailed {
                    operation: RuntimeFilerOperationKind::Delete,
                    path,
                    target_path: None,
                    kind: RuntimeFilerErrorKind::ConfirmationRequired,
                    message: "delete requires explicit confirmation".to_string(),
                });
            }
            if trash {
                return Err(RuntimeFilerError::OperationFailed {
                    operation: RuntimeFilerOperationKind::Delete,
                    path,
                    target_path: None,
                    kind: RuntimeFilerErrorKind::Unsupported,
                    message: "trash backend is not configured for this platform; permanent delete policy remains separate".to_string(),
                });
            }
            if recursive {
                return Err(RuntimeFilerError::OperationFailed {
                    operation: RuntimeFilerOperationKind::Delete,
                    path,
                    target_path: None,
                    kind: RuntimeFilerErrorKind::Unsupported,
                    message:
                        "recursive delete is disabled; it requires an explicit future opt-in policy"
                            .to_string(),
                });
            }
            if path.is_dir() {
                std::fs::remove_dir(&path).map_err(|error| {
                    runtime_filer_io_error(RuntimeFilerOperationKind::Delete, &path, None, error)
                })?;
            } else {
                std::fs::remove_file(&path).map_err(|error| {
                    runtime_filer_io_error(RuntimeFilerOperationKind::Delete, &path, None, error)
                })?;
            }
            log::info!(
                "[saya_live_runtime][filer] delete completed: path={}, duration_ms={}",
                path.display(),
                start.elapsed().as_millis()
            );
            Ok(RuntimeFilerOperationReport {
                operation: RuntimeFilerOperationKind::Delete,
                path: path.to_string_lossy().into_owned(),
                target_path: None,
                entries: Vec::new(),
                preview_id: None,
            })
        }
        RuntimeFilerOperation::Mark { path } => Err(RuntimeFilerError::OperationFailed {
            operation: RuntimeFilerOperationKind::Mark,
            path,
            target_path: None,
            kind: RuntimeFilerErrorKind::Unsupported,
            message: "mark requires an active directory buffer".to_string(),
        }),
        RuntimeFilerOperation::Unmark { path } => Err(RuntimeFilerError::OperationFailed {
            operation: RuntimeFilerOperationKind::Unmark,
            path,
            target_path: None,
            kind: RuntimeFilerErrorKind::Unsupported,
            message: "unmark requires an active directory buffer".to_string(),
        }),
        RuntimeFilerOperation::ClearMarks => Err(RuntimeFilerError::OperationFailed {
            operation: RuntimeFilerOperationKind::ClearMarks,
            path: PathBuf::new(),
            target_path: None,
            kind: RuntimeFilerErrorKind::Unsupported,
            message: "clearMarks requires an active directory buffer".to_string(),
        }),
        RuntimeFilerOperation::BulkDeletePreview => Err(RuntimeFilerError::OperationFailed {
            operation: RuntimeFilerOperationKind::BulkDeletePreview,
            path: PathBuf::new(),
            target_path: None,
            kind: RuntimeFilerErrorKind::Unsupported,
            message: "bulkDeletePreview requires an active directory buffer".to_string(),
        }),
        RuntimeFilerOperation::BulkDelete {
            preview_id,
            confirm: _,
        } => Err(RuntimeFilerError::OperationFailed {
            operation: RuntimeFilerOperationKind::BulkDelete,
            path: PathBuf::from(preview_id),
            target_path: None,
            kind: RuntimeFilerErrorKind::Unsupported,
            message: "bulkDelete requires an active directory buffer".to_string(),
        }),
    }
}

fn runtime_filer_io_error(
    operation: RuntimeFilerOperationKind,
    path: &PathBuf,
    target_path: Option<&PathBuf>,
    error: std::io::Error,
) -> RuntimeFilerError {
    let kind = match error.kind() {
        std::io::ErrorKind::AlreadyExists => RuntimeFilerErrorKind::AlreadyExists,
        std::io::ErrorKind::NotFound => RuntimeFilerErrorKind::NotFound,
        std::io::ErrorKind::PermissionDenied => RuntimeFilerErrorKind::PermissionDenied,
        _ => RuntimeFilerErrorKind::Io,
    };
    log::debug!(
        "[saya_live_runtime][filer] operation failed: operation={:?}, path={}, target_path={:?}, kind={:?}, message={}",
        operation,
        path.display(),
        target_path.map(|path| path.display().to_string()),
        kind,
        error
    );
    RuntimeFilerError::OperationFailed {
        operation,
        path: path.clone(),
        target_path: target_path.cloned(),
        kind,
        message: error.to_string(),
    }
}

pub fn list_local_filer_entries(
    path: PathBuf,
    options: RuntimeFilerListOptions,
) -> Result<Vec<RuntimeFilerEntry>, RuntimeFilerError> {
    let started_at = std::time::Instant::now();
    log::debug!(
        "[saya_live_runtime] listing local filer entries: path={}, show_hidden={}, sort_by={:?}, filter={:?}",
        path.display(),
        options.show_hidden,
        options.sort_by,
        options.filter
    );
    let normalized_filter = options
        .filter
        .as_deref()
        .map(str::trim)
        .filter(|filter| !filter.is_empty())
        .map(|filter| filter.to_ascii_lowercase());
    let mut entries = std::fs::read_dir(&path)
        .map_err(|error| RuntimeFilerError::ReadFailed {
            path: path.clone(),
            message: error.to_string(),
        })?
        .map(|entry| {
            let entry = entry.map_err(|error| RuntimeFilerError::ReadFailed {
                path: path.clone(),
                message: error.to_string(),
            })?;
            let file_type = entry
                .file_type()
                .map_err(|error| RuntimeFilerError::ReadFailed {
                    path: entry.path(),
                    message: error.to_string(),
                })?;
            let kind = if file_type.is_dir() {
                RuntimeFilerEntryKind::Directory
            } else if file_type.is_file() {
                RuntimeFilerEntryKind::File
            } else if file_type.is_symlink() {
                RuntimeFilerEntryKind::Symlink
            } else {
                RuntimeFilerEntryKind::Other
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            if !options.show_hidden && name.starts_with('.') {
                return Ok(None);
            }
            if let Some(filter) = normalized_filter.as_deref() {
                let display_text = runtime_filer_display_text(&name, &kind);
                let normalized_name = name.to_ascii_lowercase();
                let normalized_display_text = display_text.to_ascii_lowercase();
                if !normalized_name.contains(filter) && !normalized_display_text.contains(filter) {
                    return Ok(None);
                }
            }
            let metadata = entry
                .metadata()
                .map_err(|error| RuntimeFilerError::ReadFailed {
                    path: entry.path(),
                    message: error.to_string(),
                })?;
            let modified_time_ms = metadata
                .modified()
                .ok()
                .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX));
            let display_text = runtime_filer_display_text(&name, &kind);
            Ok(Some(RuntimeFilerEntry {
                name,
                path: entry.path().to_string_lossy().into_owned(),
                kind,
                display_text,
                size: Some(metadata.len()),
                modified_time_ms,
            }))
        })
        .filter_map(|entry| match entry {
            Ok(Some(entry)) => Some(Ok(entry)),
            Ok(None) => None,
            Err(error) => Some(Err(error)),
        })
        .collect::<Result<Vec<_>, RuntimeFilerError>>()?;
    entries.sort_by(|left, right| runtime_filer_compare_entries(left, right, options.sort_by));
    if options.sort_by == RuntimeFilerSortKey::Kind {
        let directory_count = entries
            .iter()
            .filter(|entry| entry.kind == RuntimeFilerEntryKind::Directory)
            .count();
        log::debug!(
            "[saya_live_runtime] sorted local filer entries with eza-style directory grouping: path={}, directories={}, non_directories={}",
            path.display(),
            directory_count,
            entries.len().saturating_sub(directory_count)
        );
    }
    log::debug!(
        "[saya_live_runtime] listed local filer entries: path={}, count={}, duration_ms={}, show_hidden={}, sort_by={:?}, filter={:?}",
        path.display(),
        entries.len(),
        started_at.elapsed().as_millis(),
        options.show_hidden,
        options.sort_by,
        options.filter
    );
    Ok(entries)
}

fn runtime_filer_display_text(name: &str, kind: &RuntimeFilerEntryKind) -> String {
    match kind {
        RuntimeFilerEntryKind::Directory => format!("{name}/"),
        RuntimeFilerEntryKind::Symlink => format!("{name}@"),
        RuntimeFilerEntryKind::Other => format!("{name}?"),
        RuntimeFilerEntryKind::File => name.to_string(),
    }
}

fn runtime_filer_compare_entries(
    left: &RuntimeFilerEntry,
    right: &RuntimeFilerEntry,
    sort_by: RuntimeFilerSortKey,
) -> std::cmp::Ordering {
    match sort_by {
        RuntimeFilerSortKey::Name => left.display_text.cmp(&right.display_text),
        RuntimeFilerSortKey::Kind => filer_directory_group_rank(&left.kind)
            .cmp(&filer_directory_group_rank(&right.kind))
            .then_with(|| left.display_text.cmp(&right.display_text)),
        RuntimeFilerSortKey::ModifiedTime => left
            .modified_time_ms
            .cmp(&right.modified_time_ms)
            .then_with(|| left.display_text.cmp(&right.display_text)),
        RuntimeFilerSortKey::Size => left
            .size
            .cmp(&right.size)
            .then_with(|| left.display_text.cmp(&right.display_text)),
    }
}

fn filer_directory_group_rank(kind: &RuntimeFilerEntryKind) -> usize {
    match kind {
        RuntimeFilerEntryKind::Directory => 0,
        RuntimeFilerEntryKind::File
        | RuntimeFilerEntryKind::Symlink
        | RuntimeFilerEntryKind::Other => 1,
    }
}

fn runtime_callback_error_from_script_message(message: &str) -> RuntimeCallbackError {
    if let Some(payload) = extract_prefixed_json_payload(message, RUNTIME_COMMAND_ERROR_PREFIX) {
        if let Ok(error) = serde_json::from_str::<RuntimeCommandError>(payload) {
            return RuntimeCallbackError::Command(error);
        }
    }

    RuntimeCallbackError::ScriptFailed {
        message: message.to_string(),
    }
}

fn runtime_event_name_from_seed(name: &str) -> Result<RuntimeEventName, RuntimeInitError> {
    match name {
        "bufferOpen" => Ok(RuntimeEventName::BufferOpen),
        "bufferChanged" => Ok(RuntimeEventName::BufferChanged),
        "bufferWritePost" => Ok(RuntimeEventName::BufferWritePost),
        "bufferClosed" => Ok(RuntimeEventName::BufferClosed),
        other => Err(RuntimeInitError::UnsupportedEvent {
            name: other.to_string(),
        }),
    }
}

fn runtime_event_name_to_script(event: RuntimeEventName) -> &'static str {
    match event {
        RuntimeEventName::BufferOpen => "bufferOpen",
        RuntimeEventName::BufferChanged => "bufferChanged",
        RuntimeEventName::BufferWritePost => "bufferWritePost",
        RuntimeEventName::BufferClosed => "bufferClosed",
    }
}

fn callback_expression(source: &str, default_params: &str) -> String {
    let trimmed = source.trim().trim_end_matches(';').trim();
    let looks_like_function = trimmed.contains("=>")
        || trimmed.starts_with("function")
        || trimmed.starts_with("async function")
        || trimmed.starts_with("async (")
        || trimmed.starts_with('(');

    if looks_like_function {
        format!("({trimmed})")
    } else if default_params.is_empty() {
        format!("(async () => {{ {trimmed} }})")
    } else {
        format!("(async ({default_params}) => {{ {trimmed} }})")
    }
}

fn build_seed_registration_script(
    seed: &CallbackRegistrySeed,
) -> Result<(String, SeedRuntimeMetadata), RuntimeInitError> {
    let mut script = String::from("\"use strict\";\n");
    let mut metadata = SeedRuntimeMetadata::default();

    for command in seed.commands() {
        let name = serde_json::to_string(command.name()).expect("command name should serialize");
        let callback = callback_expression(command.callback_source(), "");
        script.push_str(&format!(
            "globalThis.__sayaRuntime.registerCommand({name}, {callback});\n"
        ));
    }

    for event in seed.events() {
        let event_name = runtime_event_name_from_seed(event.name())?;
        let name = serde_json::to_string(event.name()).expect("event name should serialize");
        let callback = callback_expression(event.callback_source(), "payload");
        script.push_str(&format!(
            "globalThis.__sayaRuntime.registerEvent({name}, {callback});\n"
        ));
        *metadata.handler_counts.entry(event_name).or_insert(0) += 1;
    }

    Ok((script, metadata))
}

fn create_seed_runtime(
    bridge: Arc<dyn HostCapabilityBridge>,
    seed: &CallbackRegistrySeed,
) -> Result<
    (
        JsRuntime,
        SeedRuntimeMetadata,
        Arc<ProcessPool>,
        Arc<ManagedLspSessionPool>,
        Arc<StdMutex<RuntimeSelectorSessions>>,
    ),
    RuntimeInitError,
> {
    log::debug!(
        "[saya_live_runtime] create deno_core live runtime from seed: commands={}, events={}",
        seed.commands().len(),
        seed.events().len()
    );
    // Phase A.2: 共有プロセスプールを生成し、extension にも、worker
    // ループ脱出時の sweeper にも参照を渡せるよう Arc を 2 部複製する。
    let process_pool = Arc::new(ProcessPool::new());
    let lsp_session_pool = Arc::new(ManagedLspSessionPool::new(process_pool.clone()));
    let selector_sessions = Arc::new(StdMutex::new(RuntimeSelectorSessions::new(
        bridge.selector_view_backend(),
    )));
    let mut runtime = JsRuntime::new(RuntimeOptions {
        extensions: vec![live_saya_extension::init(
            bridge,
            selector_sessions.clone(),
            process_pool.clone(),
            lsp_session_pool.clone(),
        )],
        ..Default::default()
    });

    runtime
        .execute_script("<saya-live-runtime-bootstrap>", LIVE_RUNTIME_BOOTSTRAP)
        .map_err(|error| RuntimeInitError::BootstrapFailed {
            message: error.to_string(),
        })?;

    let (registration_script, metadata) = build_seed_registration_script(seed)?;
    runtime
        .execute_script("<saya-live-runtime-seed>", registration_script)
        .map_err(|error| RuntimeInitError::BootstrapFailed {
            message: error.to_string(),
        })?;

    Ok((
        runtime,
        metadata,
        process_pool,
        lsp_session_pool,
        selector_sessions,
    ))
}

async fn dispatch_event_in_seed_runtime(
    runtime: &mut JsRuntime,
    event: RuntimeEventPayload,
) -> Result<(), RuntimeDispatchError> {
    let event_name = event.event_name();
    let payload_json = serde_json::to_string(event.buffer_payload())
        .expect("buffer event payload should serialize");
    let event_name_json = serde_json::to_string(runtime_event_name_to_script(event_name))
        .expect("runtime event name should serialize");
    let script = format!(
        "(async () => {{ await globalThis.__sayaRuntime.dispatchEvent({event_name_json}, {payload_json}); }})()"
    );

    log::debug!(
        "[saya_live_runtime] execute seed runtime dispatch script: event={:?}",
        event_name
    );
    let promise = runtime
        .execute_script("<saya-live-runtime-dispatch>", script)
        .map_err(|error| parse_runtime_dispatch_error(event_name, error.to_string()))?;
    #[allow(deprecated)]
    runtime
        .resolve_value(promise)
        .await
        .map_err(|error| parse_runtime_dispatch_error(event_name, error.to_string()))?;
    Ok(())
}

async fn execute_command_in_seed_runtime(
    runtime: &mut JsRuntime,
    name: &str,
) -> Result<(), RuntimeCommandError> {
    let name_json = serde_json::to_string(name).expect("runtime command name should serialize");
    let script = format!(
        "(async () => {{ await globalThis.__sayaRuntime.executeCommand({name_json}); }})()"
    );

    log::debug!(
        "[saya_live_runtime] execute seed runtime command script: name={}",
        name
    );
    let promise = runtime
        .execute_script("<saya-live-runtime-command>", script)
        .map_err(|error| runtime_command_error_from_script_message(name, &error.to_string()))?;
    #[allow(deprecated)]
    runtime
        .resolve_value(promise)
        .await
        .map_err(|error| runtime_command_error_from_script_message(name, &error.to_string()))?;
    Ok(())
}

fn runtime_command_error_from_script_message(name: &str, message: &str) -> RuntimeCommandError {
    if let Some(payload) = extract_prefixed_json_payload(message, RUNTIME_COMMAND_ERROR_PREFIX)
        && let Ok(error) = serde_json::from_str::<RuntimeCommandError>(payload)
    {
        return error;
    }

    RuntimeCommandError::CommandFailed {
        name: name.to_string(),
        message: message.to_string(),
    }
}

fn parse_runtime_dispatch_error(event: RuntimeEventName, message: String) -> RuntimeDispatchError {
    if let Some(payload) = extract_prefixed_json_payload(&message, RUNTIME_CALLBACK_ERROR_PREFIX) {
        if let Ok(encoded) = serde_json::from_str::<EncodedCallbackFailure>(payload) {
            return RuntimeDispatchError::CallbackFailed {
                event,
                handler_index: encoded.handler_index,
                error: runtime_callback_error_from_script_message(&encoded.error),
            };
        }
    }

    RuntimeDispatchError::CallbackFailed {
        event,
        handler_index: 0,
        error: RuntimeCallbackError::ScriptFailed { message },
    }
}

fn extract_prefixed_json_payload<'a>(message: &'a str, prefix: &str) -> Option<&'a str> {
    let payload = message.split_once(prefix).map(|(_, rhs)| rhs)?;
    let trimmed = payload.trim_start();
    let opening = trimmed.as_bytes().first().copied()?;
    let closing = match opening {
        b'{' => b'}',
        b'[' => b']',
        _ => return None,
    };

    let mut depth = 0usize;
    let mut in_string = false;
    let mut escape = false;

    for (index, byte) in trimmed.bytes().enumerate() {
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            match byte {
                b'\\' => escape = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }

        match byte {
            b'"' => in_string = true,
            value if value == opening => depth += 1,
            value if value == closing => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(&trimmed[..=index]);
                }
            }
            _ => {}
        }
    }

    None
}

pub trait SayaStartupPhaseEvaluator: Send + Sync + 'static {
    type Output: Send + 'static;
    type Error: Send + 'static;

    fn evaluate(&self) -> BoxFuture<Result<Self::Output, Self::Error>>;
}

#[cfg(test)]
mod startup_runtime_prepare_test_support {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(super) enum StartupRuntimePrepareError {
        ReadFailed { path: PathBuf, message: String },
        TranspileFailed { path: PathBuf, message: String },
    }

    pub(super) struct StartupRuntimePrepareEvaluator {
        config_path: PathBuf,
        current_dir: PathBuf,
    }

    impl SayaStartupPhaseEvaluator for StartupRuntimePrepareEvaluator {
        type Output = PreparedStartupModule;
        type Error = StartupRuntimePrepareError;

        fn evaluate(&self) -> BoxFuture<Result<Self::Output, Self::Error>> {
            let config_path = self.config_path.clone();
            let current_dir = self.current_dir.clone();
            Box::pin(async move {
                log::debug!(
                    "[saya_live_runtime] prepare startup runtime on worker: config_path={}, current_dir={}",
                    config_path.display(),
                    current_dir.display()
                );
                match prepare_init_module(&config_path, &current_dir) {
                    StartupModulePrepareResult::Success(module) => Ok(module),
                    StartupModulePrepareResult::ReadFailed { path, message } => {
                        Err(StartupRuntimePrepareError::ReadFailed { path, message })
                    }
                    StartupModulePrepareResult::TranspileFailed { path, message } => {
                        Err(StartupRuntimePrepareError::TranspileFailed { path, message })
                    }
                }
            })
        }
    }

    pub(super) fn spawn_startup_runtime_prepare_runner(
        config_path: PathBuf,
        current_dir: PathBuf,
    ) -> SayaStartupPhaseRunner<StartupRuntimePrepareEvaluator> {
        log::debug!(
            "[saya_live_runtime] spawn startup runtime prepare runner: config_path={}, current_dir={}",
            config_path.display(),
            current_dir.display()
        );
        SayaStartupPhaseRunner::new(Arc::new(StartupRuntimePrepareEvaluator {
            config_path,
            current_dir,
        }))
    }
}

#[cfg(test)]
use startup_runtime_prepare_test_support::spawn_startup_runtime_prepare_runner;

pub struct SayaStartupPhaseRunner<E>
where
    E: SayaStartupPhaseEvaluator,
{
    sender: mpsc::UnboundedSender<oneshot::Sender<Result<E::Output, E::Error>>>,
    _worker: JoinHandle<()>,
}

impl<E> SayaStartupPhaseRunner<E>
where
    E: SayaStartupPhaseEvaluator,
{
    pub fn new(evaluator: Arc<E>) -> Self {
        let (sender, mut receiver) =
            mpsc::unbounded_channel::<oneshot::Sender<Result<E::Output, E::Error>>>();

        let worker = tokio::spawn(async move {
            log::debug!("[saya_live_runtime] startup worker spawned");
            while let Some(reply) = receiver.recv().await {
                log::debug!("[saya_live_runtime] startup worker evaluating request");
                let result = evaluator.evaluate().await;
                let _ = reply.send(result);
            }
            log::debug!("[saya_live_runtime] startup worker stopped");
        });

        Self {
            sender,
            _worker: worker,
        }
    }

    pub fn begin(&self) -> Result<StartupPhaseReceipt<E::Output, E::Error>, StartupPhaseError> {
        log::debug!("[saya_live_runtime] queue startup evaluation");
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(sender)
            .map_err(|_| StartupPhaseError::QueueClosed)?;
        Ok(StartupPhaseReceipt { receiver })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupPhaseError {
    QueueClosed,
    WorkerStopped,
}

pub struct StartupPhaseReceipt<T, E> {
    receiver: oneshot::Receiver<Result<T, E>>,
}

impl<T, E> StartupPhaseReceipt<T, E> {
    pub async fn await_result(self) -> Result<T, StartupPhaseError> {
        self.receiver
            .await
            .map_err(|_| StartupPhaseError::WorkerStopped)?
            .map_err(|_| StartupPhaseError::WorkerStopped)
    }
}

#[derive(Clone)]
pub struct RuntimeContext {
    shared: Arc<RuntimeSharedState>,
}

impl RuntimeContext {
    pub fn commands(&self) -> RuntimeCommandsApi {
        RuntimeCommandsApi {
            shared: self.shared.clone(),
        }
    }

    pub fn buffer(&self) -> RuntimeBufferApi {
        RuntimeBufferApi {
            bridge: self.shared.bridge.clone(),
        }
    }

    pub fn window(&self) -> RuntimeWindowApi {
        RuntimeWindowApi {
            bridge: self.shared.bridge.clone(),
        }
    }

    pub fn editor(&self) -> RuntimeEditorApi {
        RuntimeEditorApi {
            bridge: self.shared.bridge.clone(),
        }
    }
}

pub struct RuntimeCommandsApi {
    shared: Arc<RuntimeSharedState>,
}

impl RuntimeCommandsApi {
    pub async fn execute(&self, name: &str) -> Result<(), RuntimeCommandError> {
        log::info!("[saya_live_runtime][command] execute requested: {}", name);

        {
            let mut stack = self.shared.command_stack.lock().await;
            if stack.iter().any(|entry| entry == name) {
                log::info!(
                    "[saya_live_runtime][command] circular command detected: {}",
                    name
                );
                return Err(RuntimeCommandError::CircularCommand {
                    name: name.to_string(),
                });
            }
            stack.push(name.to_string());
        }

        let result = if let Some(callback) = self.shared.registry.command(name) {
            log::info!(
                "[saya_live_runtime][command] execute registered command: {}",
                name
            );
            callback(RuntimeContext {
                shared: self.shared.clone(),
            })
            .await
        } else {
            log::info!(
                "[saya_live_runtime][host_command] execute host command fallback: {}",
                name
            );
            self.shared.bridge.execute_host_command(name).await
        };

        let mut stack = self.shared.command_stack.lock().await;
        if let Some(position) = stack.iter().rposition(|entry| entry == name) {
            stack.remove(position);
        }

        result
    }
}

pub struct RuntimeBufferApi {
    bridge: Arc<dyn HostCapabilityBridge>,
}

impl RuntimeBufferApi {
    pub async fn current(&self) -> ReadonlyBufferSnapshot {
        self.bridge.current_buffer().await
    }

    pub async fn current_path(&self) -> Option<PathBuf> {
        self.bridge.current_buffer_path().await
    }
}

pub struct RuntimeWindowApi {
    bridge: Arc<dyn HostCapabilityBridge>,
}

impl RuntimeWindowApi {
    pub async fn current(&self) -> ReadonlyWindowSnapshot {
        self.bridge.current_window().await
    }

    pub async fn open_float(
        &self,
        request: RuntimeFloatOpenRequest,
    ) -> Result<RuntimeFloatSnapshot, RuntimeCommandError> {
        self.bridge.open_float(request).await
    }

    pub async fn close(&self, id: u64) -> Result<bool, RuntimeCommandError> {
        self.bridge.close_float(id).await
    }

    pub async fn focus(&self, id: u64) -> Result<bool, RuntimeCommandError> {
        self.bridge.focus_float(id).await
    }

    pub async fn floats(&self) -> Result<Vec<RuntimeFloatSnapshot>, RuntimeCommandError> {
        self.bridge.list_float_snapshots().await
    }
}

pub struct RuntimeEditorApi {
    bridge: Arc<dyn HostCapabilityBridge>,
}

impl RuntimeEditorApi {
    pub async fn current(&self) -> ReadonlyEditorSnapshot {
        self.bridge.current_editor().await
    }

    pub async fn mode(&self) -> RuntimeMode {
        self.current().await.mode
    }
}

struct RuntimeSharedState {
    bridge: Arc<dyn HostCapabilityBridge>,
    registry: CallbackRegistry,
    selector_sessions: Arc<StdMutex<RuntimeSelectorSessions>>,
    command_stack: Mutex<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeWorkerKind {
    Tokio,
    Thread,
}

pub struct SayaLiveRuntime {
    sender: mpsc::UnboundedSender<RuntimeMessage>,
    worker_kind: RuntimeWorkerKind,
}

impl Drop for SayaLiveRuntime {
    fn drop(&mut self) {
        match self.worker_kind {
            RuntimeWorkerKind::Tokio => {
                log::debug!("[saya_live_runtime] drop live runtime backed by tokio worker");
            }
            RuntimeWorkerKind::Thread => {
                log::debug!("[saya_live_runtime] drop live runtime backed by dedicated thread");
            }
        }
    }
}

impl SayaLiveRuntime {
    pub fn spawn(bridge: Arc<dyn HostCapabilityBridge>, registry: CallbackRegistry) -> Self {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let selector_sessions = Arc::new(StdMutex::new(RuntimeSelectorSessions::new(
            bridge.selector_view_backend(),
        )));
        let shared = Arc::new(RuntimeSharedState {
            bridge,
            registry,
            selector_sessions,
            command_stack: Mutex::new(Vec::new()),
        });

        let worker_shared = shared.clone();
        tokio::spawn(async move {
            log::debug!("[saya_live_runtime] live runtime worker spawned");
            while let Some(message) = receiver.recv().await {
                match message {
                    RuntimeMessage::Dispatch { event, reply } => {
                        let result = dispatch_event(worker_shared.clone(), event).await;
                        let _ = reply.send(result);
                    }
                    RuntimeMessage::ExecuteCommand { name, reply } => {
                        let result = RuntimeContext {
                            shared: worker_shared.clone(),
                        }
                        .commands()
                        .execute(&name)
                        .await;
                        let _ = reply.send(result);
                    }
                    RuntimeMessage::ControlSelector { id, request, reply } => {
                        let result =
                            control_selector_session(&worker_shared.selector_sessions, id, request);
                        let _ = reply.send(result);
                    }
                    RuntimeMessage::UpdateSelector { id, request, reply } => {
                        let result =
                            update_selector_session(&worker_shared.selector_sessions, id, request);
                        let _ = reply.send(result);
                    }
                }
            }
            log::debug!("[saya_live_runtime] live runtime worker stopped");
        });

        Self {
            sender,
            worker_kind: RuntimeWorkerKind::Tokio,
        }
    }

    pub fn spawn_from_seed(
        bridge: Arc<dyn HostCapabilityBridge>,
        seed: CallbackRegistrySeed,
    ) -> Result<Self, RuntimeInitError> {
        let (sender, receiver) = mpsc::unbounded_channel();
        let (init_sender, init_receiver) = std::sync::mpsc::sync_channel(1);

        let worker = thread::Builder::new()
            .name("saya-live-runtime".to_string())
            .spawn(move || {
                log::debug!("[saya_live_runtime] spawn seed-backed live runtime thread");
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("seed runtime thread should create tokio runtime");

                runtime.block_on(async move {
                    let mut receiver = receiver;
                    let (mut js_runtime, metadata, process_pool, lsp_session_pool, selector_sessions) =
                        match create_seed_runtime(bridge, &seed) {
                            Ok(runtime) => runtime,
                            Err(error) => {
                                let _ = init_sender.send(Err(error));
                                return;
                            }
                        };
                    let _ = init_sender.send(Ok(()));
                    log::debug!("[saya_live_runtime] seed-backed live runtime initialized");

                    while let Some(message) = receiver.recv().await {
                        match message {
                            RuntimeMessage::Dispatch { event, reply } => {
                                let event_name = event.event_name();
                                log::debug!(
                                    "[saya_live_runtime] seed runtime received dispatch: event={:?}",
                                    event_name
                                );
                                let result =
                                    dispatch_event_in_seed_runtime(&mut js_runtime, event).await;
                                let report = result.map(|()| RuntimeDispatchReport {
                                    event: event_name,
                                    handler_count: metadata
                                        .handler_counts
                                        .get(&event_name)
                                        .copied()
                                        .unwrap_or_default(),
                                });
                                let _ = reply.send(report);
                            }
                            RuntimeMessage::ExecuteCommand { name, reply } => {
                                log::debug!(
                                    "[saya_live_runtime] seed runtime received command execution: name={}",
                                    name
                                );
                                let result =
                                    execute_command_in_seed_runtime(&mut js_runtime, &name).await;
                                let _ = reply.send(result);
                            }
                            RuntimeMessage::ControlSelector { id, request, reply } => {
                                log::debug!(
                                    "[saya_live_runtime][selector] seed runtime received host selector control: id={}, command={:?}",
                                    id,
                                    request.command
                                );
                                let result =
                                    control_selector_session(&selector_sessions, id, request);
                                let _ = reply.send(result);
                            }
                            RuntimeMessage::UpdateSelector { id, request, reply } => {
                                log::debug!(
                                    "[saya_live_runtime][selector] seed runtime received host selector update: id={}, query_len={}",
                                    id,
                                    request.query.len()
                                );
                                let result = update_selector_session(&selector_sessions, id, request);
                                let _ = reply.send(result);
                            }
                        }
                    }

                    // Phase A.2: receiver が close（SayaLiveRuntime drop）
                    // した時点で `saya.process` で起動した子プロセス群を
                    // 確実に kill する。タイムアウトを掛けて kill ハングを
                    // 検出する（見つかった場合は debug ログのみ残し、
                    // worker thread を最終的には抜ける）。
                    log::info!(
                        "[saya_live_runtime] worker loop ended; sweeping process pool"
                    );
                    if let Err(_elapsed) = tokio::time::timeout(
                        std::time::Duration::from_secs(2),
                        lsp_session_pool.shutdown_all(),
                    )
                    .await
                    {
                        log::debug!(
                            "[saya_live_runtime] managed LSP session shutdown timed out (some children may rely on kill_on_drop)"
                        );
                    }
                    if let Err(_elapsed) = tokio::time::timeout(
                        std::time::Duration::from_secs(2),
                        process_pool.shutdown_all(),
                    )
                    .await
                    {
                        log::debug!(
                            "[saya_live_runtime] process pool shutdown timed out (some children may rely on kill_on_drop)"
                        );
                    }
                    drop(js_runtime);
                    log::debug!("[saya_live_runtime] seed-backed live runtime stopped");
                });
            })
            .map_err(|error| RuntimeInitError::WorkerStartFailed {
                message: error.to_string(),
            })?;

        match init_receiver.recv() {
            Ok(Ok(())) => Ok(Self {
                sender,
                worker_kind: RuntimeWorkerKind::Thread,
            }),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(error) => {
                let _ = worker.join();
                Err(RuntimeInitError::WorkerStartFailed {
                    message: error.to_string(),
                })
            }
        }
    }

    pub fn dispatch_event(
        &self,
        event: RuntimeEventPayload,
    ) -> Result<RuntimeDispatchReceipt, RuntimeDispatchError> {
        log::debug!(
            "[saya_live_runtime] queue dispatch: event={:?}",
            event.event_name()
        );
        let (reply, receiver) = oneshot::channel();
        self.sender
            .send(RuntimeMessage::Dispatch { event, reply })
            .map_err(|_| RuntimeDispatchError::QueueClosed)?;
        Ok(RuntimeDispatchReceipt { receiver })
    }

    pub fn execute_command(
        &self,
        name: &str,
    ) -> Result<RuntimeCommandReceipt, RuntimeCommandError> {
        log::info!(
            "[saya_live_runtime][command] queue command execution: name={}",
            name
        );
        let (reply, receiver) = oneshot::channel();
        self.sender
            .send(RuntimeMessage::ExecuteCommand {
                name: name.to_string(),
                reply,
            })
            .map_err(|_| RuntimeCommandError::CommandFailed {
                name: name.to_string(),
                message: "runtime command queue closed".to_string(),
            })?;
        Ok(RuntimeCommandReceipt { receiver })
    }

    pub fn control_selector(
        &self,
        id: u64,
        request: RuntimeSelectorControlRequest,
    ) -> Result<RuntimeSelectorControlReceipt, RuntimeCommandError> {
        log::info!(
            "[saya_live_runtime][selector] queue host selector control: id={}, command={:?}",
            id,
            request.command
        );
        let (reply, receiver) = oneshot::channel();
        self.sender
            .send(RuntimeMessage::ControlSelector { id, request, reply })
            .map_err(|_| RuntimeCommandError::CommandFailed {
                name: "selector.control".to_string(),
                message: "runtime selector control queue closed".to_string(),
            })?;
        Ok(RuntimeSelectorControlReceipt { receiver })
    }

    pub fn update_selector(
        &self,
        id: u64,
        request: RuntimeSelectorUpdateRequest,
    ) -> Result<RuntimeSelectorUpdateReceipt, RuntimeCommandError> {
        log::info!(
            "[saya_live_runtime][selector] queue host selector update: id={}, query_len={}",
            id,
            request.query.len()
        );
        let (reply, receiver) = oneshot::channel();
        self.sender
            .send(RuntimeMessage::UpdateSelector { id, request, reply })
            .map_err(|_| RuntimeCommandError::CommandFailed {
                name: "selector.update".to_string(),
                message: "runtime selector update queue closed".to_string(),
            })?;
        Ok(RuntimeSelectorUpdateReceipt { receiver })
    }
}

pub struct RuntimeDispatchReceipt {
    receiver: oneshot::Receiver<Result<RuntimeDispatchReport, RuntimeDispatchError>>,
}

impl RuntimeDispatchReceipt {
    pub async fn await_result(self) -> Result<RuntimeDispatchReport, RuntimeDispatchError> {
        self.receiver
            .await
            .map_err(|_| RuntimeDispatchError::WorkerStopped)?
    }
}

pub struct RuntimeCommandReceipt {
    receiver: oneshot::Receiver<Result<(), RuntimeCommandError>>,
}

impl RuntimeCommandReceipt {
    pub(crate) fn into_receiver(self) -> oneshot::Receiver<Result<(), RuntimeCommandError>> {
        self.receiver
    }

    pub async fn await_result(self) -> Result<(), RuntimeCommandError> {
        self.receiver
            .await
            .map_err(|_| RuntimeCommandError::CommandFailed {
                name: "<runtime-command-reply>".to_string(),
                message: "runtime command worker stopped".to_string(),
            })?
    }
}

pub struct RuntimeSelectorControlReceipt {
    receiver: oneshot::Receiver<Result<RuntimeSelectorSnapshot, RuntimeCommandError>>,
}

impl RuntimeSelectorControlReceipt {
    pub async fn await_result(self) -> Result<RuntimeSelectorSnapshot, RuntimeCommandError> {
        self.receiver
            .await
            .map_err(|_| RuntimeCommandError::CommandFailed {
                name: "selector.control".to_string(),
                message: "runtime selector control worker stopped".to_string(),
            })?
    }
}

pub struct RuntimeSelectorUpdateReceipt {
    receiver: oneshot::Receiver<Result<RuntimeSelectorSnapshot, RuntimeCommandError>>,
}

impl RuntimeSelectorUpdateReceipt {
    pub async fn await_result(self) -> Result<RuntimeSelectorSnapshot, RuntimeCommandError> {
        self.receiver
            .await
            .map_err(|_| RuntimeCommandError::CommandFailed {
                name: "selector.update".to_string(),
                message: "runtime selector update worker stopped".to_string(),
            })?
    }
}

enum RuntimeMessage {
    Dispatch {
        event: RuntimeEventPayload,
        reply: oneshot::Sender<Result<RuntimeDispatchReport, RuntimeDispatchError>>,
    },
    ExecuteCommand {
        name: String,
        reply: oneshot::Sender<Result<(), RuntimeCommandError>>,
    },
    ControlSelector {
        id: u64,
        request: RuntimeSelectorControlRequest,
        reply: oneshot::Sender<Result<RuntimeSelectorSnapshot, RuntimeCommandError>>,
    },
    UpdateSelector {
        id: u64,
        request: RuntimeSelectorUpdateRequest,
        reply: oneshot::Sender<Result<RuntimeSelectorSnapshot, RuntimeCommandError>>,
    },
}

fn control_selector_session(
    selector_sessions: &Arc<StdMutex<RuntimeSelectorSessions>>,
    id: u64,
    request: RuntimeSelectorControlRequest,
) -> Result<RuntimeSelectorSnapshot, RuntimeCommandError> {
    let command = request.command;
    log::debug!(
        "[saya_live_runtime][selector] host selector control start: id={}, command={:?}",
        id,
        command
    );
    selector_sessions
        .lock()
        .expect("selector sessions poisoned")
        .control(id, request)
        .map_err(|error| RuntimeCommandError::CommandFailed {
            name: "selector.control".to_string(),
            message: error.to_string(),
        })
}

fn update_selector_session(
    selector_sessions: &Arc<StdMutex<RuntimeSelectorSessions>>,
    id: u64,
    request: RuntimeSelectorUpdateRequest,
) -> Result<RuntimeSelectorSnapshot, RuntimeCommandError> {
    log::debug!(
        "[saya_live_runtime][selector] host selector update start: id={}, query_len={}",
        id,
        request.query.len()
    );
    selector_sessions
        .lock()
        .expect("selector sessions poisoned")
        .update(id, request)
        .map_err(|error| RuntimeCommandError::CommandFailed {
            name: "selector.update".to_string(),
            message: error.to_string(),
        })
}

async fn dispatch_event(
    shared: Arc<RuntimeSharedState>,
    event: RuntimeEventPayload,
) -> Result<RuntimeDispatchReport, RuntimeDispatchError> {
    let event_name = event.event_name();
    let payload = event.buffer_payload().clone();
    let handlers = shared.registry.handlers_for(event_name);

    log::debug!(
        "[saya_live_runtime] dispatch start: event={:?}, handler_count={}",
        event_name,
        handlers.len()
    );

    for (index, handler) in handlers.iter().enumerate() {
        log::debug!(
            "[saya_live_runtime] dispatch handler: event={:?}, index={}",
            event_name,
            index
        );
        handler(
            RuntimeContext {
                shared: shared.clone(),
            },
            payload.clone(),
        )
        .await
        .map_err(|error| RuntimeDispatchError::CallbackFailed {
            event: event_name,
            handler_index: index,
            error,
        })?;
    }

    Ok(RuntimeDispatchReport {
        event: event_name,
        handler_count: handlers.len(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use crate::runtime::callback_registry_seed::CallbackRegistrySeed;
    use crate::runtime::config::StartupRegistryEntry;
    use crate::runtime::startup::PreparedStartupModule;

    use tokio::sync::Mutex;

    use super::{
        BufferEventPayload, CallbackRegistryBuilder, HostCapabilityBridge, ReadonlyBufferSnapshot,
        ReadonlyEditorSnapshot, ReadonlyWindowSnapshot, RuntimeCommandError, RuntimeEventPayload,
        RuntimeFilerError, RuntimeFilerErrorKind, RuntimeFilerOperationKind, RuntimeMode,
        SayaLiveRuntime, SayaStartupPhaseEvaluator, SayaStartupPhaseRunner,
        find_workspace_root_path, runtime_filer_io_error, spawn_startup_runtime_prepare_runner,
    };

    fn unique_path(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        std::env::temp_dir().join(format!("saya-live-runtime-{name}-{nanos}"))
    }

    #[test]
    fn workspace_root_detection_returns_absolute_root_for_relative_buffer_paths() {
        let _lock = crate::app::bootstrap::launch_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = unique_path("relative-workspace-root");
        let nested = root.join("tmp");
        std::fs::create_dir_all(&nested).expect("nested workspace dir");
        std::fs::create_dir(root.join(".git")).expect("root marker");
        std::fs::write(nested.join("main.go"), "package main\n").expect("source file");
        let expected_root = std::fs::canonicalize(&root).expect("canonical root");
        let previous_dir = std::env::current_dir().expect("current dir");
        std::env::set_current_dir(&root).expect("enter workspace root");

        let detected = find_workspace_root_path(PathBuf::from("tmp/main.go"), &[".git".into()]);

        std::env::set_current_dir(previous_dir).expect("restore current dir");
        std::fs::remove_dir_all(&root).expect("cleanup workspace root");
        assert_eq!(detected, Some(expected_root));
    }

    #[test]
    fn filer_io_error_maps_permission_denied_to_structured_error_kind() {
        let path = PathBuf::from("/tmp/permission-denied.txt");
        let error = runtime_filer_io_error(
            RuntimeFilerOperationKind::CreateFile,
            &path,
            None,
            std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        );

        assert_eq!(
            error,
            RuntimeFilerError::OperationFailed {
                operation: RuntimeFilerOperationKind::CreateFile,
                path,
                target_path: None,
                kind: RuntimeFilerErrorKind::PermissionDenied,
                message: "permission denied".to_string(),
            }
        );
    }

    #[test]
    fn filer_list_options_filter_hidden_and_sort_by_size_with_metadata() {
        let root_path = unique_path("filer-list-options");
        let small_path = root_path.join("small.txt");
        let large_path = root_path.join("large.txt");
        let hidden_path = root_path.join(".hidden.txt");
        std::fs::create_dir_all(&root_path).expect("root directory");
        std::fs::write(&small_path, "1").expect("small file");
        std::fs::write(&large_path, "12345").expect("large file");
        std::fs::write(&hidden_path, "hidden").expect("hidden file");

        let entries = super::list_local_filer_entries(
            root_path.clone(),
            super::RuntimeFilerListOptions {
                show_hidden: false,
                sort_by: super::RuntimeFilerSortKey::Size,
                filter: None,
            },
        )
        .expect("filer list should succeed");

        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec!["small.txt", "large.txt"]
        );
        assert_eq!(entries[0].size, Some(1));
        assert_eq!(entries[1].size, Some(5));
        assert!(
            entries.iter().all(|entry| entry.modified_time_ms.is_some()),
            "metadata should include modified_time_ms without removing existing fields"
        );

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }

    #[cfg(unix)]
    #[test]
    fn filer_kind_sort_groups_directories_then_sorts_other_entries_by_name() {
        let root_path = unique_path("filer-kind-sort");
        let directory_path = root_path.join("middle-dir");
        let file_path = root_path.join("z-file.txt");
        let target_path = root_path.join("target.md");
        let link_path = root_path.join("a-link.md");
        std::fs::create_dir_all(&directory_path).expect("nested directory");
        std::fs::write(&file_path, "file\n").expect("file entry");
        std::fs::write(&target_path, "target\n").expect("symlink target");
        std::os::unix::fs::symlink(&target_path, &link_path).expect("symlink");

        let entries = super::list_local_filer_entries(
            root_path.clone(),
            super::RuntimeFilerListOptions {
                show_hidden: true,
                sort_by: super::RuntimeFilerSortKey::Kind,
                filter: None,
            },
        )
        .expect("filer list should succeed");

        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.display_text.as_str())
                .collect::<Vec<_>>(),
            vec!["middle-dir/", "a-link.md@", "target.md", "z-file.txt"]
        );

        std::fs::remove_dir_all(root_path).expect("cleanup directory");
    }

    struct RecordingHostBridge {
        executed_commands: Arc<Mutex<Vec<String>>>,
        command_results: HashMap<String, RuntimeCommandError>,
    }

    impl RecordingHostBridge {
        fn new() -> Self {
            Self {
                executed_commands: Arc::new(Mutex::new(Vec::new())),
                command_results: HashMap::new(),
            }
        }

        fn with_command_error(name: &str, error: RuntimeCommandError) -> Self {
            let mut command_results = HashMap::new();
            command_results.insert(name.to_string(), error);
            Self {
                executed_commands: Arc::new(Mutex::new(Vec::new())),
                command_results,
            }
        }
    }

    impl HostCapabilityBridge for RecordingHostBridge {
        fn execute_host_command(
            &self,
            name: &str,
        ) -> super::BoxFuture<Result<(), RuntimeCommandError>> {
            let executed_commands = self.executed_commands.clone();
            let name = name.to_string();
            let result = self.command_results.get(&name).cloned();
            Box::pin(async move {
                if let Some(error) = result {
                    return Err(error);
                }
                executed_commands.lock().await.push(name);
                Ok(())
            })
        }

        fn current_buffer(&self) -> super::BoxFuture<ReadonlyBufferSnapshot> {
            Box::pin(async move {
                ReadonlyBufferSnapshot {
                    id: 7,
                    path: Some(PathBuf::from("notes.md")),
                    line_count: 3,
                    cursor_row: 0,
                    cursor_col: 0,
                    current_line: String::new(),
                    text: String::new(),
                }
            })
        }

        fn current_window(&self) -> super::BoxFuture<ReadonlyWindowSnapshot> {
            Box::pin(async move { ReadonlyWindowSnapshot { id: 9 } })
        }

        fn current_editor(&self) -> super::BoxFuture<ReadonlyEditorSnapshot> {
            Box::pin(async move {
                ReadonlyEditorSnapshot {
                    mode: RuntimeMode::Normal,
                }
            })
        }
    }

    struct SleepingStartupEvaluator;

    impl SayaStartupPhaseEvaluator for SleepingStartupEvaluator {
        type Output = &'static str;
        type Error = &'static str;

        fn evaluate(&self) -> super::BoxFuture<Result<Self::Output, Self::Error>> {
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(40)).await;
                Ok("startup-ready")
            })
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn startup_phase_runs_on_worker_boundary_without_blocking_caller() {
        let runner = SayaStartupPhaseRunner::new(Arc::new(SleepingStartupEvaluator));

        let started_at = Instant::now();
        let receipt = runner.begin().expect("startup evaluation should be queued");

        assert!(
            started_at.elapsed() < Duration::from_millis(20),
            "begin should return quickly without waiting for startup evaluation"
        );

        let result = receipt
            .await_result()
            .await
            .expect("startup evaluation result");
        assert_eq!(result, "startup-ready");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn startup_runtime_prepare_runs_on_worker_boundary_without_blocking_caller() {
        let current_dir = unique_path("startup-runtime-cwd");
        std::fs::create_dir_all(&current_dir).expect("current dir");
        let config_path = current_dir.join("init.ts");
        std::fs::write(
            &config_path,
            r#"
                const tabstop: number = 4;
                saya.options.tabstop = tabstop;
            "#,
        )
        .expect("config file");

        let runner = spawn_startup_runtime_prepare_runner(config_path.clone(), current_dir.clone());

        let started_at = Instant::now();
        let receipt = runner
            .begin()
            .expect("startup runtime evaluation should be queued");

        assert!(
            started_at.elapsed() < Duration::from_millis(20),
            "begin should return quickly without waiting for startup runtime preparation"
        );

        let result: PreparedStartupModule = receipt
            .await_result()
            .await
            .expect("startup runtime evaluation result");
        assert_eq!(result.path, config_path);
        assert_eq!(
            result.specifier.as_str(),
            format!("file://{}/init.ts", current_dir.to_string_lossy())
        );
        assert!(result.executable_source_text.contains("const tabstop = 4;"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runtime_dispatch_preserves_registration_order_without_blocking_sender() {
        let trace = Arc::new(Mutex::new(Vec::new()));
        let first_trace = trace.clone();
        let second_trace = trace.clone();

        let registry = CallbackRegistryBuilder::default()
            .on_buffer_open(move |_, payload| {
                let first_trace = first_trace.clone();
                Box::pin(async move {
                    first_trace
                        .lock()
                        .await
                        .push(format!("first:{:?}", payload.buffer.path));
                    tokio::time::sleep(Duration::from_millis(30)).await;
                    first_trace.lock().await.push("first:done".to_string());
                    Ok(())
                })
            })
            .on_buffer_open(move |_, payload| {
                let second_trace = second_trace.clone();
                Box::pin(async move {
                    second_trace
                        .lock()
                        .await
                        .push(format!("second:{:?}", payload.buffer.path));
                    Ok(())
                })
            })
            .build();

        let runtime = SayaLiveRuntime::spawn(Arc::new(RecordingHostBridge::new()), registry);
        let payload = RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: ReadonlyBufferSnapshot {
                id: 11,
                path: Some(PathBuf::from("article.md")),
                line_count: 8,
                cursor_row: 0,
                cursor_col: 0,
                current_line: String::new(),
                text: String::new(),
            },
        });

        let started_at = Instant::now();
        let receipt = runtime
            .dispatch_event(payload)
            .expect("dispatch should succeed");

        assert!(
            started_at.elapsed() < Duration::from_millis(20),
            "dispatch should queue work without waiting for handlers"
        );

        let report = receipt.await_result().await.expect("dispatch result");
        assert_eq!(report.handler_count, 2);

        let trace = trace.lock().await.clone();
        assert_eq!(
            trace,
            vec![
                "first:Some(\"article.md\")".to_string(),
                "first:done".to_string(),
                "second:Some(\"article.md\")".to_string(),
            ]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runtime_callback_can_execute_registered_command_and_read_typed_state() {
        let host_bridge = Arc::new(RecordingHostBridge::new());
        let observed = Arc::new(Mutex::new(Vec::new()));
        let observed_in_command = observed.clone();
        let observed_in_event = observed.clone();

        let registry = CallbackRegistryBuilder::default()
            .register_command("writeCurrent", move |ctx| {
                let observed_in_command = observed_in_command.clone();
                Box::pin(async move {
                    let buffer = ctx.buffer().current().await;
                    let editor_mode = ctx.editor().mode().await;
                    observed_in_command
                        .lock()
                        .await
                        .push(format!("command:{:?}:{:?}", buffer.path, editor_mode));
                    ctx.commands().execute("write").await
                })
            })
            .on_buffer_open(move |ctx, payload| {
                let observed_in_event = observed_in_event.clone();
                Box::pin(async move {
                    observed_in_event.lock().await.push(format!(
                        "event:{}:{:?}",
                        payload.buffer.id, payload.buffer.path
                    ));
                    ctx.commands().execute("writeCurrent").await?;
                    Ok(())
                })
            })
            .build();

        let runtime = SayaLiveRuntime::spawn(host_bridge.clone(), registry);
        let receipt = runtime
            .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
                buffer: ReadonlyBufferSnapshot {
                    id: 21,
                    path: Some(PathBuf::from("typed.md")),
                    line_count: 5,
                    cursor_row: 0,
                    cursor_col: 0,
                    current_line: String::new(),
                    text: String::new(),
                },
            }))
            .expect("dispatch should succeed");

        let report = receipt.await_result().await.expect("dispatch report");
        assert_eq!(report.handler_count, 1);

        assert_eq!(
            observed.lock().await.clone(),
            vec![
                "event:21:Some(\"typed.md\")".to_string(),
                "command:Some(\"notes.md\"):Normal".to_string(),
            ]
        );
        assert_eq!(
            host_bridge.executed_commands.lock().await.clone(),
            vec!["write".to_string()]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn seed_runtime_can_list_filer_entries_for_typescript_plugin() {
        let root = unique_path("filer-root");
        let dir_path = root.join("src");
        let file_path = root.join("README.md");
        let alpha_path = root.join("alpha.md");
        let hidden_path = root.join(".hidden.md");
        std::fs::create_dir_all(&dir_path).expect("test directory");
        std::fs::write(&file_path, "hello\n").expect("test file");
        std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
        std::fs::write(&hidden_path, "hidden\n").expect("hidden file");
        let root_json = serde_json::to_string(&root.to_string_lossy().to_string())
            .expect("path should serialize");
        let host_bridge = Arc::new(RecordingHostBridge::new());
        let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
            name: "bufferOpen".to_string(),
            callback_source: format!(
                r#"
                    async () => {{
                        if (!Object.isFrozen(saya.filer)) {{
                            throw new Error("runtime filer surface should be frozen");
                        }}
                        const entries = await saya.filer.list({root_json});
                        await saya.commands.execute(entries.map((entry) => `${{entry.kind}}:${{entry.name}}`).join(","));
                        const visibleByName = await saya.filer.list({root_json}, {{ showHidden: false, sortBy: "name" }});
                        await saya.commands.execute(visibleByName.map((entry) => `${{entry.displayText}}:${{entry.size != null}}:${{entry.modifiedTimeMs != null}}`).join(","));
                    }}
                "#
            ),
        }]);

        let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), seed)
            .expect("runtime should initialize");
        let receipt = runtime
            .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
                buffer: ReadonlyBufferSnapshot {
                    id: 22,
                    path: None,
                    line_count: 1,
                    cursor_row: 0,
                    cursor_col: 0,
                    current_line: String::new(),
                    text: String::new(),
                },
            }))
            .expect("dispatch should queue");
        let report = receipt.await_result().await.expect("dispatch report");

        assert_eq!(report.handler_count, 1);
        assert_eq!(
            host_bridge.executed_commands.lock().await.clone(),
            vec![
                "directory:src,file:.hidden.md,file:README.md,file:alpha.md".to_string(),
                "README.md:true:true,alpha.md:true:true,src/:true:true".to_string()
            ]
        );

        std::fs::remove_file(hidden_path).expect("cleanup hidden file");
        std::fs::remove_file(alpha_path).expect("cleanup alpha file");
        std::fs::remove_file(file_path).expect("cleanup file");
        std::fs::remove_dir(dir_path).expect("cleanup dir");
        std::fs::remove_dir(root).expect("cleanup root");
    }

    // NOTE: 遅延プラグインロード (`saya.plugins.loadLazy` ->
    // `op_runtime_plugin_load_lazy`) は現状 **未実装** であり、op は request の
    // validation とログ出力のみを行って `Ok(())` を返す no-op である
    // (live.rs:2083-2121 を参照)。モジュールの import / 評価や、遅延ロード対象が
    // 登録するコマンド・イベントの live registry への反映は一切行われない。
    //
    // 以前ここには `..._bridge_accepts_logged_lazy_command_trigger` という、
    // 「loadLazy が成功裏に解決する」ことだけを確認するテストが存在したが、
    // それは no-op が成功を返すことを追認するだけで「遅延プラグインが実際に
    // ロードされコマンドが登録される」というユーザー可視挙動を全く検証して
    // おらず、緑色が「遅延ロードが動作する」という誤解 (偽の安心感) を生んでいた。
    //
    // そのため下記テストは「遅延ロード対象モジュールの `setup` が実際に評価され、
    // そこで登録されたコマンドが loadLazy 後に live registry 経由で実行可能になる」
    // という本来保証すべき挙動を表明する aspirational test として書き換えたうえで
    // `#[ignore]` している。遅延ロードが実装されたら ignore を外すことで、実装の
    // 正しさを検証するテストとして即座に有効化できる。
    #[tokio::test(flavor = "current_thread")]
    #[ignore = "lazy plugin load is currently a no-op (op_runtime_plugin_load_lazy validates+logs only, does not evaluate the module); re-enable when real lazy loading is implemented"]
    async fn seed_runtime_lazy_plugin_load_registers_module_command() {
        // 遅延ロードのトリガとなるコマンド `GitStatus` を seed する。トリガが
        // 発火すると `loadLazy` 経由で遅延モジュール (`git-tools`) が評価され、
        // その `setup` が `GitStatusReal` という *seed には存在しない* 新規コマンドを
        // 登録する、というのが遅延ロード実装後に期待される挙動。
        let host_bridge = Arc::new(RecordingHostBridge::new());
        let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Command {
            name: "GitStatus".to_string(),
            callback_source: r#"
                async () => {
                    console.info("[saya-plugin-host][lazy] command trigger: name=GitStatus plugin=git-tools module=plugins/git-tools.ts");
                    await saya.plugins.loadLazy({
                        kind: "command",
                        name: "GitStatus",
                        plugin: "git-tools",
                        module: "plugins/git-tools.ts",
                        exportName: "setup",
                    });
                }
            "#
            .to_string(),
        }]);

        let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), seed)
            .expect("runtime should initialize");

        // 遅延ロードのトリガを実行する。実装後は、この時点で遅延モジュールが
        // 評価され `GitStatusReal` が registry に登録されている必要がある。
        runtime
            .execute_command("GitStatus")
            .expect("lazy command should queue")
            .await_result()
            .await
            .expect("lazy command trigger should resolve");

        // ユーザー可視の本質: 遅延ロードで登録されたコマンドが実際に実行可能で
        // あること。no-op 実装では `GitStatusReal` は registry に存在せず host
        // command fallback に落ちるため、この実行は遅延ロードによる登録を意味
        // しない。実装が入れば登録済みコールバックとして実行され、ここで観測できる。
        runtime
            .execute_command("GitStatusReal")
            .expect("lazily registered command should queue")
            .await_result()
            .await
            .expect("lazily registered command should be executable after lazy load");

        // 遅延ロードされたコマンドのコールバックが評価され副作用 (host command の
        // 発行) を起こしたことを確認する。これにより「ロードされた」ことが
        // registry 参照の有無に依存せず観測できる。
        assert!(
            host_bridge
                .executed_commands
                .lock()
                .await
                .iter()
                .any(|command| command == "git-tools:status"),
            "lazily loaded module setup should have run and emitted its side effect"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn seed_runtime_lazy_plugin_bridge_reports_invalid_target_as_command_failure() {
        let host_bridge = Arc::new(RecordingHostBridge::new());
        let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Command {
            name: "BrokenLazy".to_string(),
            callback_source: r#"
                async () => {
                    console.info("[saya-plugin-host][lazy] command trigger: name=BrokenLazy plugin= module=");
                    await saya.plugins.loadLazy({
                        kind: "command",
                        name: "BrokenLazy",
                        plugin: "",
                        module: "",
                        exportName: "setup",
                    });
                }
            "#
            .to_string(),
        }]);

        let runtime =
            SayaLiveRuntime::spawn_from_seed(host_bridge, seed).expect("runtime should initialize");
        let receipt = runtime
            .execute_command("BrokenLazy")
            .expect("lazy command should queue");
        let error = receipt
            .await_result()
            .await
            .expect_err("invalid lazy target should fail command");

        assert!(
            format!("{error:?}").contains("incomplete lazy plugin target"),
            "failure should expose lazy bridge message: {error:?}"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn seed_runtime_can_read_buffer_window_editor_state_and_typed_payload() {
        let host_bridge = Arc::new(RecordingHostBridge::new());
        let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
            name: "bufferOpen".to_string(),
            callback_source: r#"
                async (payload) => {
                    const buffer = await saya.buffer.current();
                    const window = await saya.window.current();
                    const mode = await saya.editor.mode();
                    await saya.commands.execute(`state:${payload.buffer.id}:${buffer.id}:${window.id}:${mode}`);
                }
            "#
            .to_string(),
        }]);

        let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), seed)
            .expect("seed runtime should initialize");

        let report = runtime
            .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
                buffer: ReadonlyBufferSnapshot {
                    id: 51,
                    path: Some(PathBuf::from("typed-payload.md")),
                    line_count: 12,
                    cursor_row: 0,
                    cursor_col: 0,
                    current_line: String::new(),
                    text: String::new(),
                },
            }))
            .expect("dispatch queued")
            .await_result()
            .await
            .expect("dispatch result");

        assert_eq!(report.handler_count, 1);
        assert_eq!(
            host_bridge.executed_commands.lock().await.clone(),
            vec!["state:51:7:9:Normal".to_string()]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn seed_runtime_can_dispatch_buffer_write_post_payload() {
        let host_bridge = Arc::new(RecordingHostBridge::new());
        let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
            name: "bufferWritePost".to_string(),
            callback_source:
                "(payload) => saya.commands.execute(`write-post:${payload.buffer.id}:${payload.buffer.lineCount}`)"
                    .to_string(),
        }]);

        let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), seed)
            .expect("seed runtime should initialize");

        let report = runtime
            .dispatch_event(RuntimeEventPayload::BufferWritePost(BufferEventPayload {
                buffer: ReadonlyBufferSnapshot {
                    id: 61,
                    path: Some(PathBuf::from("write-post.md")),
                    line_count: 14,
                    cursor_row: 0,
                    cursor_col: 0,
                    current_line: String::new(),
                    text: String::new(),
                },
            }))
            .expect("dispatch queued")
            .await_result()
            .await
            .expect("dispatch result");

        assert_eq!(report.handler_count, 1);
        assert_eq!(
            host_bridge.executed_commands.lock().await.clone(),
            vec!["write-post:61:14".to_string()]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn seed_runtime_reports_unknown_command_as_structured_error() {
        let host_bridge = Arc::new(RecordingHostBridge::with_command_error(
            "missing",
            RuntimeCommandError::UnknownCommand {
                name: "missing".to_string(),
            },
        ));
        let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
            name: "bufferOpen".to_string(),
            callback_source: "(payload) => saya.commands.execute(\"missing\")".to_string(),
        }]);

        let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge, seed)
            .expect("seed runtime should initialize");

        let error = runtime
            .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
                buffer: ReadonlyBufferSnapshot {
                    id: 71,
                    path: Some(PathBuf::from("unknown-command.md")),
                    line_count: 2,
                    cursor_row: 0,
                    cursor_col: 0,
                    current_line: String::new(),
                    text: String::new(),
                },
            }))
            .expect("dispatch queued")
            .await_result()
            .await
            .expect_err("unknown command should be surfaced");

        assert_eq!(
            error,
            super::RuntimeDispatchError::CallbackFailed {
                event: super::RuntimeEventName::BufferOpen,
                handler_index: 0,
                error: super::RuntimeCallbackError::Command(RuntimeCommandError::UnknownCommand {
                    name: "missing".to_string(),
                },),
            }
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn seed_runtime_reports_script_failure_as_structured_error() {
        let host_bridge = Arc::new(RecordingHostBridge::new());
        let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
            name: "bufferOpen".to_string(),
            callback_source: "(payload) => { throw new Error(\"boom\"); }".to_string(),
        }]);

        let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge, seed)
            .expect("seed runtime should initialize");

        let error = runtime
            .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
                buffer: ReadonlyBufferSnapshot {
                    id: 81,
                    path: Some(PathBuf::from("script-failure.md")),
                    line_count: 6,
                    cursor_row: 0,
                    cursor_col: 0,
                    current_line: String::new(),
                    text: String::new(),
                },
            }))
            .expect("dispatch queued")
            .await_result()
            .await
            .expect_err("script failure should be surfaced");

        assert!(
            matches!(
                error,
                super::RuntimeDispatchError::CallbackFailed {
                    event: super::RuntimeEventName::BufferOpen,
                    handler_index: 0,
                    error: super::RuntimeCallbackError::ScriptFailed { ref message },
                } if message.contains("boom")
            ),
            "script failure should stay structured: {error:?}"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn seed_runtime_dispatch_queues_without_blocking_sender() {
        let host_bridge = Arc::new(RecordingHostBridge::new());
        let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
            name: "bufferOpen".to_string(),
            callback_source: "(payload) => saya.commands.execute(\"write\")".to_string(),
        }]);

        let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge, seed)
            .expect("seed runtime should initialize");

        let started_at = Instant::now();
        let receipt = runtime
            .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
                buffer: ReadonlyBufferSnapshot {
                    id: 31,
                    path: Some(PathBuf::from("seed.md")),
                    line_count: 4,
                    cursor_row: 0,
                    cursor_col: 0,
                    current_line: String::new(),
                    text: String::new(),
                },
            }))
            .expect("dispatch should be queued");

        assert!(
            started_at.elapsed() < Duration::from_millis(20),
            "dispatch should return quickly even for seed runtime"
        );

        let report = receipt.await_result().await.expect("dispatch report");
        assert_eq!(report.handler_count, 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn seed_runtime_dispatches_event_handlers_in_registration_order() {
        let host_bridge = Arc::new(RecordingHostBridge::new());
        let seed = CallbackRegistrySeed::from_startup_entries(vec![
            StartupRegistryEntry::Command {
                name: "writeCurrent".to_string(),
                callback_source: "() => saya.commands.execute(\"write\")".to_string(),
            },
            StartupRegistryEntry::Event {
                name: "bufferOpen".to_string(),
                callback_source: "(payload) => saya.commands.execute(\"writeCurrent\")".to_string(),
            },
            StartupRegistryEntry::Event {
                name: "bufferOpen".to_string(),
                callback_source: "(payload) => saya.commands.execute(\"write!\")".to_string(),
            },
        ]);

        let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), seed)
            .expect("seed runtime should initialize");

        let report = runtime
            .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
                buffer: ReadonlyBufferSnapshot {
                    id: 41,
                    path: Some(PathBuf::from("ordered.md")),
                    line_count: 9,
                    cursor_row: 0,
                    cursor_col: 0,
                    current_line: String::new(),
                    text: String::new(),
                },
            }))
            .expect("dispatch queued")
            .await_result()
            .await
            .expect("dispatch result");

        assert_eq!(report.handler_count, 2);
        assert_eq!(
            host_bridge.executed_commands.lock().await.clone(),
            vec!["write".to_string(), "write!".to_string()]
        );
    }
}
