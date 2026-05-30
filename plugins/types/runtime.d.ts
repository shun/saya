// deno-fmt-ignore-file

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
