// Phase B: plugins/saya-lsp/ 配下のサブモジュールを inline 展開する。
// import 行はローダ (expand_local_startup_imports_from_path) によって
// 対象ファイルの内容に置換され、const __lspXxx = { ... }; という
// top-level namespace 宣言が現在の評価スコープに持ち込まれる。
import {} from "./utf8.ts";
import {} from "./json-rpc.ts";
import {} from "./transport.ts";
import {} from "./lifecycle.ts";
import {} from "./session.ts";

export interface SayaLspCommandNames {
  initialize?: string;
  initialized?: string;
  hover?: string;
  definition?: string;
  references?: string;
  documentSymbol?: string;
  completion?: string;
  completionResolve?: string;
  signatureHelp?: string;
  formatting?: string;
  rangeFormatting?: string;
  rename?: string;
  codeAction?: string;
  codeActionResolve?: string;
  nextDiagnostic?: string;
  previousDiagnostic?: string;
  shutdown?: string;
  lsifHover?: string;
  lsifDefinition?: string;
}

export interface SayaLspKeymap {
  hover?: string;
  definition?: string;
  references?: string;
  documentSymbol?: string;
  completion?: string;
  signatureHelp?: string;
  formatting?: string;
  rangeFormatting?: string;
  rename?: string;
  codeAction?: string;
  nextDiagnostic?: string;
  previousDiagnostic?: string;
  lsifHover?: string;
  lsifDefinition?: string;
}

export interface SayaLspClientOptions {
  commands?: SayaLspCommandNames;
  keymap?: SayaLspKeymap;
  bridgeCommand?: string;
  clientName?: string;
  rootUri?: string | null;
  languageId?: string;
  languageIdByExtension?: Record<string, string>;
  servers?: Record<string, SayaLspLanguageServerOptions> | SayaLspLanguageServerOptions[];
  trace?: "off" | "messages" | "verbose";
  positionEncoding?: "utf-16" | "utf-8" | "utf-32";
  enableBufferEvents?: boolean;
  completionTriggerCharacters?: string[];
  formattingOptions?: SayaLspFormattingOptions;
  renameNewName?: string;
  codeActionKinds?: string[];
  lsif?: SayaLsifClientOptions;
}

export interface SayaLspLanguageServerOptions {
  name?: string;
  command: string;
  args?: string[];
  env?: Record<string, string>;
  cwd?: string;
  languages?: string[];
  filePatterns?: string[];
  rootMarkers?: string[];
  rootUri?: string | null;
  initializationOptions?: unknown;
  trace?: "off" | "messages" | "verbose";
  positionEncoding?: "utf-16" | "utf-8" | "utf-32";
}

export interface SayaLsifClientOptions {
  enabled?: boolean;
  bridgeCommand?: string;
  dumpPath?: string;
}

export interface SayaLspFormattingOptions {
  tabSize?: number;
  insertSpaces?: boolean;
  trimTrailingWhitespace?: boolean;
  insertFinalNewline?: boolean;
  trimFinalNewlines?: boolean;
}

export interface SayaLspJsonRpcRequest {
  jsonrpc: "2.0";
  id: number | string;
  method: string;
  params?: unknown;
}

export interface SayaLspJsonRpcNotification {
  jsonrpc: "2.0";
  method: string;
  params?: unknown;
}

export interface SayaLspJsonRpcResponse {
  jsonrpc: "2.0";
  id: number | string | null;
  result?: unknown;
  error?: {
    code: number;
    message: string;
    data?: unknown;
  };
}

export interface SayaLspTransport {
  send(message: string): void | Promise<void>;
}

export interface SayaLspMessageParser {
  accept(chunk: string | Uint8Array): void;
}

export interface SayaLsifEntry {
  id: number | string;
  type: "vertex" | "edge";
  label: string;
  [key: string]: unknown;
}

function quoteRuntimeValue(value) {
  return JSON.stringify(value);
}

function lspMethodForCommand(kind) {
  if (kind === "initialize") {
    return "initialize";
  }
  if (kind === "initialized") {
    return "initialized";
  }
  if (kind === "hover") {
    return "textDocument/hover";
  }
  if (kind === "definition") {
    return "textDocument/definition";
  }
  if (kind === "references") {
    return "textDocument/references";
  }
  if (kind === "documentSymbol") {
    return "textDocument/documentSymbol";
  }
  if (kind === "completion") {
    return "textDocument/completion";
  }
  if (kind === "completionResolve") {
    return "completionItem/resolve";
  }
  if (kind === "signatureHelp") {
    return "textDocument/signatureHelp";
  }
  if (kind === "formatting") {
    return "textDocument/formatting";
  }
  if (kind === "rangeFormatting") {
    return "textDocument/rangeFormatting";
  }
  if (kind === "rename") {
    return "textDocument/rename";
  }
  if (kind === "codeAction") {
    return "textDocument/codeAction";
  }
  if (kind === "codeActionResolve") {
    return "codeAction/resolve";
  }
  return "shutdown";
}

function defaultCommandNames(commands = {}) {
  const names = {};
  names.initialize = commands.initialize ?? "lsp.initialize";
  names.initialized = commands.initialized ?? "lsp.initialized";
  names.hover = commands.hover ?? "lsp.hover";
  names.definition = commands.definition ?? "lsp.definition";
  names.references = commands.references ?? "lsp.references";
  names.documentSymbol = commands.documentSymbol ?? "lsp.documentSymbol";
  names.completion = commands.completion ?? "lsp.completion";
  names.completionResolve = commands.completionResolve ?? "lsp.completionResolve";
  names.signatureHelp = commands.signatureHelp ?? "lsp.signatureHelp";
  names.formatting = commands.formatting ?? "lsp.formatting";
  names.rangeFormatting = commands.rangeFormatting ?? "lsp.rangeFormatting";
  names.rename = commands.rename ?? "lsp.rename";
  names.codeAction = commands.codeAction ?? "lsp.codeAction";
  names.codeActionResolve = commands.codeActionResolve ?? "lsp.codeActionResolve";
  names.nextDiagnostic = commands.nextDiagnostic ?? "lsp.nextDiagnostic";
  names.previousDiagnostic = commands.previousDiagnostic ?? "lsp.previousDiagnostic";
  names.shutdown = commands.shutdown ?? "lsp.shutdown";
  names.lsifHover = commands.lsifHover ?? "lsif.hover";
  names.lsifDefinition = commands.lsifDefinition ?? "lsif.definition";
  return names;
}

function assertNonEmptyString(value, field) {
  if (typeof value !== "string" || value.trim() === "") {
    throw new Error(`invalid LSP configuration: ${field} must be a non-empty string`);
  }
}

function validateRootUri(rootUri, field) {
  if (rootUri == null) {
    return;
  }
  if (typeof rootUri !== "string" || !rootUri.startsWith("file://")) {
    throw new Error(`invalid LSP configuration: ${field} must be a file:// URI`);
  }
}

function validateStringArray(values, field) {
  if (values == null) {
    return [];
  }
  if (!Array.isArray(values)) {
    throw new Error(`invalid LSP configuration: ${field} must be an array`);
  }
  return values.map((value, index) => {
    assertNonEmptyString(value, `${field}[${index}]`);
    return value;
  });
}

function validateCommandNames(commandNames) {
  for (const [kind, name] of Object.entries(commandNames)) {
    assertNonEmptyString(name, `commands.${kind}`);
  }
}

function normalizeLspServers(options, commandNames) {
  const source = options.servers;
  validateCommandNames(commandNames);
  assertNonEmptyString(options.bridgeCommand ?? "lsp.request", "bridgeCommand");
  assertNonEmptyString(options.clientName ?? "saya", "clientName");
  assertNonEmptyString(options.languageId ?? "plaintext", "languageId");
  validateRootUri(options.rootUri ?? null, "rootUri");

  if (source == null) {
    const name = options.clientName ?? "saya";
    const languages = [options.languageId ?? "plaintext"];
    const rootUri = options.rootUri ?? null;
    const trace = options.trace ?? "off";
    const positionEncoding = options.positionEncoding ?? "utf-16";
    return [
      {
        name,
        command: "",
        args: [],
        env: {},
        cwd: null,
        languages,
        filePatterns: [],
        rootMarkers: [],
        rootUri,
        initializationOptions: null,
        trace,
        positionEncoding,
        hasCommand: false,
      },
    ];
  }

  const entries = Array.isArray(source)
    ? source.map((server, index) => [server.name ?? `server-${index + 1}`, server])
    : Object.entries(source);
  if (entries.length === 0) {
    throw new Error("invalid LSP configuration: servers must not be empty");
  }

  return entries.map(([key, server], index) => {
    if (!server || typeof server !== "object") {
      throw new Error(`invalid LSP configuration: servers[${index}] must be an object`);
    }
    const name = server.name ?? key;
    assertNonEmptyString(name, `servers[${index}].name`);
    assertNonEmptyString(server.command, `servers[${index}].command`);
    validateRootUri(server.rootUri ?? options.rootUri ?? null, `servers[${index}].rootUri`);
    const languages = validateStringArray(server.languages ?? [key], `servers[${index}].languages`);
    const filePatterns = validateStringArray(server.filePatterns ?? [], `servers[${index}].filePatterns`);
    const rootMarkers = validateStringArray(server.rootMarkers ?? [], `servers[${index}].rootMarkers`);
    const args = validateStringArray(server.args ?? [], `servers[${index}].args`);
    const command = server.command;
    const cwd = server.cwd ?? null;
    const rootUri = server.rootUri ?? options.rootUri ?? null;
    const initializationOptions = server.initializationOptions ?? null;
    const trace = server.trace ?? options.trace ?? "off";
    const positionEncoding = server.positionEncoding ?? options.positionEncoding ?? "utf-16";
    const env = server.env ?? {};
    if (env == null || typeof env !== "object" || Array.isArray(env)) {
      throw new Error(`invalid LSP configuration: servers[${index}].env must be an object`);
    }
    for (const [envName, envValue] of Object.entries(env)) {
      assertNonEmptyString(envName, `servers[${index}].env key`);
      assertNonEmptyString(envValue, `servers[${index}].env.${envName}`);
    }
    return {
      name,
      command,
      args,
      env,
      cwd,
      languages,
      filePatterns,
      rootMarkers,
      rootUri,
      initializationOptions,
      trace,
      positionEncoding,
      hasCommand: true,
    };
  });
}

function fileUri(path) {
  if (!path) {
    return null;
  }
  if (path.startsWith("file://")) {
    return path;
  }
  const normalized = path.startsWith("/") ? path : `/${path}`;
  return `file://${normalized
    .split("/")
    .map((part) => encodeURIComponent(part))
    .join("/")}`;
}

function utf8ByteLength(text) {
  let length = 0;
  for (let index = 0; index < text.length; index = index + 1) {
    const codePoint = text.codePointAt(index);
    if (codePoint > 0xffff) {
      index = index + 1;
    }
    if (codePoint <= 0x7f) {
      length = length + 1;
    } else if (codePoint <= 0x7ff) {
      length = length + 2;
    } else if (codePoint <= 0xffff) {
      length = length + 3;
    } else {
      length = length + 4;
    }
  }
  return length;
}

function utf8PrefixCharLength(text, byteLength) {
  let consumedBytes = 0;
  let index = 0;
  while (index < text.length && consumedBytes < byteLength) {
    const codePoint = text.codePointAt(index);
    let charByteLength = 4;
    let charLength = 1;
    if (codePoint > 0xffff) {
      charLength = 2;
    }
    if (codePoint <= 0x7f) {
      charByteLength = 1;
    } else if (codePoint <= 0x7ff) {
      charByteLength = 2;
    } else if (codePoint <= 0xffff) {
      charByteLength = 3;
    }
    if (consumedBytes + charByteLength > byteLength) {
      break;
    }
    consumedBytes = consumedBytes + charByteLength;
    index = index + charLength;
  }
  return index;
}

function normalizeUtf8ByteOffset(text, byteOffset) {
  const target = Math.max(0, Math.min(Number(byteOffset) || 0, utf8ByteLength(text)));
  let consumedBytes = 0;
  let index = 0;
  while (index < text.length && consumedBytes < target) {
    const codePoint = text.codePointAt(index);
    let charByteLength = 4;
    let charLength = 1;
    if (codePoint > 0xffff) {
      charLength = 2;
    }
    if (codePoint <= 0x7f) {
      charByteLength = 1;
    } else if (codePoint <= 0x7ff) {
      charByteLength = 2;
    } else if (codePoint <= 0xffff) {
      charByteLength = 3;
    }
    if (consumedBytes + charByteLength > target) {
      break;
    }
    consumedBytes = consumedBytes + charByteLength;
    index = index + charLength;
  }
  return consumedBytes;
}

function lspCharacterFromSayaByteColumn(text, byteOffset, positionEncoding = "utf-16") {
  const safeText = String(text ?? "");
  const safeByteOffset = normalizeUtf8ByteOffset(safeText, byteOffset);
  if (positionEncoding === "utf-8") {
    return safeByteOffset;
  }
  const prefix = safeText.slice(0, utf8PrefixCharLength(safeText, safeByteOffset));
  if (positionEncoding === "utf-32") {
    let character = 0;
    for (let index = 0; index < prefix.length; index = index + 1) {
      const codePoint = prefix.codePointAt(index);
      if (codePoint > 0xffff) {
        index = index + 1;
      }
      character = character + 1;
    }
    return character;
  }
  return prefix.length;
}

export function lspPositionFromSayaCursor(
  lineText,
  cursorRow,
  cursorCol,
  positionEncoding = "utf-16",
) {
  const line = Math.max(0, Number(cursorRow) || 0);
  const character = lspCharacterFromSayaByteColumn(lineText, cursorCol, positionEncoding);
  return {
    line,
    character,
  };
}

function lineAt(documentText, line) {
  const lines = String(documentText ?? "").split(/\r\n|\r|\n/);
  return lines[Math.max(0, Number(line) || 0)] ?? "";
}

export function lspRangeFromSayaRange(documentText, range, positionEncoding = "utf-16") {
  const startLine = Math.max(0, Number(range?.start?.line) || 0);
  const endLine = Math.max(0, Number(range?.end?.line) || 0);
  const start = lspPositionFromSayaCursor(
    lineAt(documentText, startLine),
    startLine,
    range?.start?.character ?? 0,
    positionEncoding,
  );
  const end = lspPositionFromSayaCursor(
    lineAt(documentText, endLine),
    endLine,
    range?.end?.character ?? 0,
    positionEncoding,
  );
  return {
    start,
    end,
  };
}

function decodeLspChunk(chunk) {
  if (typeof chunk === "string") {
    return chunk;
  }
  if (typeof TextDecoder !== "undefined") {
    return new TextDecoder().decode(chunk, { stream: true });
  }
  let text = "";
  for (let index = 0; index < chunk.length; index = index + 1) {
    text = text + String.fromCharCode(chunk[index]);
  }
  return text;
}

// =============================================================================
// Phase B: __lspManager 初期化ソース組み立て。
//
// startup runtime 上で各 namespace オブジェクトのメソッド source を
// `Function.prototype.toString()` 経由で抽出し、object literal の文字列
// として再構築する。callback の body にこのソースを inline し、最初の
// 起動時に `new Function(initSource)()` で LIVE runtime 側の globalThis に
// セッションマネージャをセットアップする。
//
// strip_type_annotations の罠を避けるため、namespace の本体ソースは
// すべて method shorthand 形式で持ち、key: prefix を付けないこと。
// =============================================================================

function namespaceToSource(varName, ns) {
  const lines = [];
  lines.push("const " + varName + " = {");
  const keys = Object.keys(ns);
  for (let i = 0; i < keys.length; i = i + 1) {
    const key = keys[i];
    const value = ns[key];
    if (typeof value === "function") {
      lines.push(value.toString() + ",");
    }
  }
  lines.push("};");
  return lines.join("\n");
}

// `__lspManager` は LSP セッションを per-server で管理し、`dispatch(request)`
// で existing callback flow へ統一インタフェースを提供する。
//
// dispatch ルール:
// - source !== "lsp" の場合: legacy bridge command にフォールバック呼び出し
// - request.server が無い / command が無い: bridge fallback
// - method === "initialize": getOrStart 経由で session を起動し、capability を返す
// - method === "initialized" / "exit": 既に start / shutdown 内で送出済みなので no-op
// - method === "shutdown": session.shutdown() を呼ぶ
// - method が "textDocument/did*" で response 不要: notify
// - その他: request して result を返す
const __lspManagerSourceTemplate =
  "const __lspManager = (function () {\n" +
  "  const sessions = new Map();\n" +
  "  const initializeResults = new Map();\n" +
  "  const pendingNotifications = [];\n" +
  "  // hover / definition / references / completion などの「結果を待つ」\n" +
  "  // request 系メソッドについて、`${server.name}:${method}` を key に\n" +
  "  // 直前 inflight を覚えておく。次の同 key request が来たら前 token を\n" +
  "  // cancel して $/cancelRequest を送出する。これで K 連打や gd 連打で\n" +
  "  // 古いリクエストの response が UI に飛ばないことを保証する。\n" +
  "  const inflightCancels = new Map();\n" +
  "  function spawnSession(serverConfig, initializeParams) {\n" +
  "    const promise = (async function () {\n" +
  "      const spec = {};\n" +
  "      spec.command = serverConfig.command;\n" +
  "      spec.args = serverConfig.args || [];\n" +
  "      spec.env = serverConfig.env || {};\n" +
  "      spec.cwd = serverConfig.cwd || null;\n" +
  "      spec.stdin = 'piped';\n" +
  "      spec.stdout = 'piped';\n" +
  "      spec.stderr = 'piped';\n" +
  "      const child = await saya.process.spawn(spec);\n" +
  "      const transport = __lspTransport.createFromProcess(child);\n" +
  "      const session = __lspSession.create(transport, {});\n" +
  "      session.onNotification(function (message) {\n" +
  "        pendingNotifications.push({ source: 'lsp', method: message && message.method, params: message && message.params, result: message });\n" +
  "      });\n" +
  "      const initializeResult = await session.start(initializeParams);\n" +
  "      initializeResults.set(serverConfig.name, initializeResult);\n" +
  "      return session;\n" +
  "    })();\n" +
  "    promise.catch(function (err) {\n" +
  "      // 失敗時は cache から外して次回再試行できるようにする\n" +
  "      sessions.delete(serverConfig.name);\n" +
  "      initializeResults.delete(serverConfig.name);\n" +
  "      console.log('[saya-lsp] session spawn failed: ' + (err && err.message ? err.message : String(err)));\n" +
  "    });\n" +
  "    return promise;\n" +
  "  }\n" +
  "  function getOrStart(serverConfig, initializeParams) {\n" +
  "    const name = serverConfig.name;\n" +
  "    if (!sessions.has(name)) {\n" +
  "      sessions.set(name, spawnSession(serverConfig, initializeParams));\n" +
  "    }\n" +
  "    return sessions.get(name);\n" +
  "  }\n" +
  "  function isCancellableMethod(method) {\n" +
  "    if (method === 'initialize') return false;\n" +
  "    if (method === 'initialized') return false;\n" +
  "    if (method === 'exit') return false;\n" +
  "    if (method === 'shutdown') return false;\n" +
  "    if (method === 'textDocument/didOpen') return false;\n" +
  "    if (method === 'textDocument/didChange') return false;\n" +
  "    if (method === 'textDocument/didSave') return false;\n" +
  "    if (method === 'textDocument/didClose') return false;\n" +
  "    return true;\n" +
  "  }\n" +
  "  function supersedePreviousInflight(cancelKey) {\n" +
  "    const previous = inflightCancels.get(cancelKey);\n" +
  "    if (previous) {\n" +
  "      inflightCancels.delete(cancelKey);\n" +
  "      try {\n" +
  "        previous.cancel(new Error('superseded by newer ' + cancelKey + ' request'));\n" +
  "        console.log('[saya-lsp] cancelled previous inflight request: ' + cancelKey);\n" +
  "      } catch (err) {\n" +
  "        console.log('[saya-lsp] previous-inflight cancel threw: ' + (err && err.message ? err.message : String(err)));\n" +
  "      }\n" +
  "    }\n" +
  "  }\n" +
  "  function dispatch(request) {\n" +
  "    return (async function () {\n" +
  "      if (request.source !== 'lsp') {\n" +
  "        throw new Error('manager only handles lsp source, got ' + request.source);\n" +
  "      }\n" +
  "      if (!request.server || !request.server.command) {\n" +
  "        throw new Error('no server configured for ' + request.method);\n" +
  "      }\n" +
  "      const method = request.method;\n" +
  "      if (method === 'initialize') {\n" +
  "        const initializeParams = request.params || {};\n" +
  "        const session = await getOrStart(request.server, initializeParams);\n" +
  "        const cached = initializeResults.get(request.server.name);\n" +
  "        const response = {};\n" +
  "        response.source = 'lsp';\n" +
  "        response.method = method;\n" +
  "        response.result = cached || {};\n" +
  "        return response;\n" +
  "      }\n" +
  "      if (method === 'initialized' || method === 'exit') {\n" +
  "        // initialized は session.start 内で送出済み。exit は session.shutdown 内。\n" +
  "        const noop = {};\n" +
  "        noop.source = 'lsp';\n" +
  "        noop.method = method;\n" +
  "        noop.result = null;\n" +
  "        return noop;\n" +
  "      }\n" +
  "      let sessionPromise = sessions.get(request.server.name);\n" +
  "      if (!sessionPromise) {\n" +
  "        if (method === 'shutdown') {\n" +
  "          // session 未起動の shutdown は no-op として扱う（idempotent）。\n" +
  "          const r = {};\n" +
  "          r.source = 'lsp';\n" +
  "          r.method = method;\n" +
  "          r.result = null;\n" +
  "          return r;\n" +
  "        }\n" +
  "        // textDocument/didOpen のような buffer event が initialize より先に\n" +
  "        // 到達することがあるので、request.initializeParams で session を\n" +
  "        // auto-start する（K キーや明示 lsp.initialize と同じ start 経路）。\n" +
  "        const autoInitializeParams = request.initializeParams || {};\n" +
  "        console.log('[saya-lsp] auto-start session for ' + request.server.name + ' triggered by ' + method);\n" +
  "        sessionPromise = spawnSession(request.server, autoInitializeParams);\n" +
  "        sessions.set(request.server.name, sessionPromise);\n" +
  "      }\n" +
  "      const session = await sessionPromise;\n" +
  "      if (method === 'shutdown') {\n" +
  "        // shutdown 前にこの server に紐づく全 inflight を cancel しておく\n" +
  "        const prefix = request.server.name + ':';\n" +
  "        const staleKeys = [];\n" +
  "        for (const key of inflightCancels.keys()) {\n" +
  "          if (key.indexOf(prefix) === 0) staleKeys.push(key);\n" +
  "        }\n" +
  "        for (let i = 0; i < staleKeys.length; i = i + 1) {\n" +
  "          supersedePreviousInflight(staleKeys[i]);\n" +
  "        }\n" +
  "        await session.shutdown();\n" +
  "        sessions.delete(request.server.name);\n" +
  "        initializeResults.delete(request.server.name);\n" +
  "        const r = {};\n" +
  "        r.source = 'lsp';\n" +
  "        r.method = method;\n" +
  "        r.result = null;\n" +
  "        return r;\n" +
  "      }\n" +
  "      const isNotification = method === 'textDocument/didOpen'\n" +
  "        || method === 'textDocument/didChange'\n" +
  "        || method === 'textDocument/didSave'\n" +
  "        || method === 'textDocument/didClose';\n" +
  "      if (isNotification) {\n" +
  "        await session.notify(method, request.params);\n" +
  "        const r = {};\n" +
  "        r.source = 'lsp';\n" +
  "        r.method = method;\n" +
  "        r.result = null;\n" +
  "        return r;\n" +
  "      }\n" +
  "      const cancellable = isCancellableMethod(method);\n" +
  "      const cancelKey = request.server.name + ':' + method;\n" +
  "      let token = null;\n" +
  "      let requestOptions;\n" +
  "      if (cancellable) {\n" +
  "        supersedePreviousInflight(cancelKey);\n" +
  "        token = __lspJsonRpc.createCancelToken();\n" +
  "        inflightCancels.set(cancelKey, token);\n" +
  "        requestOptions = { signal: token.signal };\n" +
  "      }\n" +
  "      try {\n" +
  "        const result = await session.request(method, request.params, requestOptions);\n" +
  "        if (cancellable && inflightCancels.get(cancelKey) === token) {\n" +
  "          inflightCancels.delete(cancelKey);\n" +
  "        }\n" +
  "        const r = {};\n" +
  "        r.source = 'lsp';\n" +
  "        r.method = method;\n" +
  "        r.result = result;\n" +
  "        return r;\n" +
  "      } catch (err) {\n" +
  "        if (cancellable && inflightCancels.get(cancelKey) === token) {\n" +
  "          inflightCancels.delete(cancelKey);\n" +
  "        }\n" +
  "        if (cancellable && token && token.signal && token.signal.aborted) {\n" +
  "          // supersede による cancel。古い結果として静かに drop。\n" +
  "          console.log('[saya-lsp] dropped stale request response: ' + cancelKey);\n" +
  "          const supersededError = new Error('LSP request superseded: ' + cancelKey);\n" +
  "          supersededError.lspSuperseded = true;\n" +
  "          throw supersededError;\n" +
  "        }\n" +
  "        throw err;\n" +
  "      }\n" +
  "    })();\n" +
  "  }\n" +
  "  const api = {};\n" +
  "  api.dispatch = dispatch;\n" +
  "  api.drainNotifications = function () {\n" +
  "    return pendingNotifications.splice(0, pendingNotifications.length);\n" +
  "  };\n" +
  "  api.shutdownAll = async function () {\n" +
  "    for (const key of Array.from(inflightCancels.keys())) {\n" +
  "      supersedePreviousInflight(key);\n" +
  "    }\n" +
  "    const all = Array.from(sessions.values());\n" +
  "    sessions.clear();\n" +
  "    for (let i = 0; i < all.length; i = i + 1) {\n" +
  "      try {\n" +
  "        const s = await all[i];\n" +
  "        await s.shutdown();\n" +
  "      } catch (_err) {\n" +
  "        // best-effort\n" +
  "      }\n" +
  "    }\n" +
  "    initializeResults.clear();\n" +
  "  };\n" +
  "  return api;\n" +
  "})();\n" +
  "return __lspManager;";

function buildLspModuleInitSource() {
  // 各 namespace のメソッドソースを object literal として直列化し、
  // 最後に __lspManager を組み立てる。namespace 同士の依存関係は
  // 宣言順で解決される（utf8 → json-rpc → transport → lifecycle → session）。
  const parts = [];
  parts.push(namespaceToSource("__lspUtf8", __lspUtf8));
  parts.push(namespaceToSource("__lspJsonRpc", __lspJsonRpc));
  parts.push(namespaceToSource("__lspTransport", __lspTransport));
  parts.push(namespaceToSource("__lspLifecycle", __lspLifecycle));
  parts.push(namespaceToSource("__lspSession", __lspSession));
  parts.push(__lspManagerSourceTemplate);
  return parts.join("\n");
}

// モジュール評価時に一度だけ構築する。後続の各 callback 内に
// JSON 文字列リテラルとして embed される。
const __lspModuleInitSourceText = buildLspModuleInitSource();
const __lspModuleInitSourceLiteral = JSON.stringify(__lspModuleInitSourceText);

function createRuntimeBridgeCallbackSource(
  source,
  bridgeCommand,
  method,
  clientName,
  rootUri,
  languageId,
  languageIdByExtension,
  servers,
  trace,
  positionEncoding,
  completionTriggerCharacters,
  formattingOptions,
  renameNewName,
  codeActionKinds,
  dumpPath,
) {
  return new Function(
    "return async (payload) => {\n" +
      "  const toFileUri = (path, rootUri = null) => {\n" +
      "    if (!path) return null;\n" +
      "    if (String(path).startsWith('file://')) return String(path);\n" +
      "    const rawPath = String(path);\n" +
      "    if (!rawPath.startsWith('/') && rootUri && String(rootUri).startsWith('file://')) {\n" +
      "      const parts = [];\n" +
      "      for (const part of rawPath.replace(/\\\\/g, '/').split('/')) {\n" +
      "        if (!part || part === '.') continue;\n" +
      "        if (part === '..') parts.pop(); else parts.push(part);\n" +
      "      }\n" +
      "      return String(rootUri).replace(/\\/+$/, '') + '/' + parts.map((part) => encodeURIComponent(part)).join('/');\n" +
      "    }\n" +
      "    const raw = rawPath.startsWith('/') ? rawPath : '/' + rawPath;\n" +
      "    const parts = [];\n" +
      "    for (const part of raw.split('/')) {\n" +
      "      if (!part || part === '.') continue;\n" +
      "      if (part === '..') parts.pop(); else parts.push(part);\n" +
      "    }\n" +
      "    const normalized = '/' + parts.join('/');\n" +
      "    return 'file://' + normalized.split('/').map((part) => encodeURIComponent(part)).join('/');\n" +
      "  };\n" +
      "  const extensionOf = (path) => {\n" +
      "    const base = String(path ?? '').split('/').pop() ?? '';\n" +
      "    const dot = base.lastIndexOf('.');\n" +
      "    return dot >= 0 ? base.slice(dot + 1).toLowerCase() : '';\n" +
      "  };\n" +
      "  const documentText = (buffer) => typeof buffer.text === 'string' ? buffer.text : (buffer.currentLine ?? '');\n" +
      "  const utf8ByteLength = (text) => {\n" +
      "    let length = 0;\n" +
      "    for (let index = 0; index < text.length; index = index + 1) {\n" +
      "      const codePoint = text.codePointAt(index);\n" +
      "      if (codePoint > 0xffff) index = index + 1;\n" +
      "      if (codePoint <= 0x7f) length = length + 1;\n" +
      "      else if (codePoint <= 0x7ff) length = length + 2;\n" +
      "      else if (codePoint <= 0xffff) length = length + 3;\n" +
      "      else length = length + 4;\n" +
      "    }\n" +
      "    return length;\n" +
      "  };\n" +
      "  const utf8PrefixCharLength = (text, byteLength) => {\n" +
      "    let consumedBytes = 0;\n" +
      "    let index = 0;\n" +
      "    while (index < text.length && consumedBytes < byteLength) {\n" +
      "      const codePoint = text.codePointAt(index);\n" +
      "      let charByteLength = 4;\n" +
      "      let charLength = 1;\n" +
      "      if (codePoint > 0xffff) charLength = 2;\n" +
      "      if (codePoint <= 0x7f) charByteLength = 1;\n" +
      "      else if (codePoint <= 0x7ff) charByteLength = 2;\n" +
      "      else if (codePoint <= 0xffff) charByteLength = 3;\n" +
      "      if (consumedBytes + charByteLength > byteLength) break;\n" +
      "      consumedBytes = consumedBytes + charByteLength;\n" +
      "      index = index + charLength;\n" +
      "    }\n" +
      "    return index;\n" +
      "  };\n" +
      "  const normalizeUtf8ByteOffset = (text, byteOffset) => {\n" +
      "    const target = Math.max(0, Math.min(Number(byteOffset) || 0, utf8ByteLength(text)));\n" +
      "    let consumedBytes = 0;\n" +
      "    let index = 0;\n" +
      "    while (index < text.length && consumedBytes < target) {\n" +
      "      const codePoint = text.codePointAt(index);\n" +
      "      let charByteLength = 4;\n" +
      "      let charLength = 1;\n" +
      "      if (codePoint > 0xffff) charLength = 2;\n" +
      "      if (codePoint <= 0x7f) charByteLength = 1;\n" +
      "      else if (codePoint <= 0x7ff) charByteLength = 2;\n" +
      "      else if (codePoint <= 0xffff) charByteLength = 3;\n" +
      "      if (consumedBytes + charByteLength > target) break;\n" +
      "      consumedBytes = consumedBytes + charByteLength;\n" +
      "      index = index + charLength;\n" +
      "    }\n" +
      "    return consumedBytes;\n" +
      "  };\n" +
      "  const lspCharacterFromSayaByteColumn = (text, byteOffset, positionEncoding) => {\n" +
      "    const safeText = String(text ?? '');\n" +
      "    const safeByteOffset = normalizeUtf8ByteOffset(safeText, byteOffset);\n" +
      "    if (positionEncoding === 'utf-8') return safeByteOffset;\n" +
      "    const prefix = safeText.slice(0, utf8PrefixCharLength(safeText, safeByteOffset));\n" +
      "    if (positionEncoding === 'utf-32') {\n" +
      "      let character = 0;\n" +
      "      for (let index = 0; index < prefix.length; index = index + 1) {\n" +
      "        const codePoint = prefix.codePointAt(index);\n" +
      "        if (codePoint > 0xffff) index = index + 1;\n" +
      "        character = character + 1;\n" +
      "      }\n" +
      "      return character;\n" +
      "    }\n" +
      "    return prefix.length;\n" +
      "  };\n" +
      "  const lspPositionFromSayaCursor = (lineText, cursorRow, cursorCol, positionEncoding) => ({\n" +
      "    line: Math.max(0, Number(cursorRow) || 0),\n" +
      "    character: lspCharacterFromSayaByteColumn(lineText, cursorCol, positionEncoding),\n" +
      "  });\n" +
      "  const documentState = () => {\n" +
      "    const key = '__sayaLspDocumentSyncState';\n" +
      "    if (!globalThis[key]) globalThis[key] = { documents: {} };\n" +
      "    return globalThis[key].documents;\n" +
      "  };\n" +
      "  const serverState = () => {\n" +
      "    const key = '__sayaLspServerState';\n" +
      "    if (!globalThis[key]) globalThis[key] = { servers: {} };\n" +
      "    return globalThis[key].servers;\n" +
      "  };\n" +
      "  const matchFilePattern = (path, pattern) => {\n" +
      "    const normalizedPath = String(path ?? '').replace(/\\\\/g, '/');\n" +
      "    const normalizedPattern = String(pattern ?? '').replace(/\\\\/g, '/');\n" +
      "    if (normalizedPattern.startsWith('**/*.')) return normalizedPath.endsWith(normalizedPattern.slice(4));\n" +
      "    if (normalizedPattern.startsWith('*.')) return normalizedPath.split('/').pop().endsWith(normalizedPattern.slice(1));\n" +
      "    if (normalizedPattern.endsWith('/**')) return normalizedPath.startsWith(normalizedPattern.slice(0, -3));\n" +
      "    return normalizedPath === normalizedPattern || normalizedPath.endsWith('/' + normalizedPattern);\n" +
      "  };\n" +
      "  const selectServer = (path, languageId, servers) => {\n" +
      "    const byPattern = servers.find((server) => (server.filePatterns ?? []).some((pattern) => matchFilePattern(path, pattern)));\n" +
      "    if (byPattern) return byPattern;\n" +
      "    const byLanguage = servers.find((server) => (server.languages ?? []).includes(languageId));\n" +
      "    return byLanguage ?? servers[0];\n" +
      "  };\n" +
      "  const negotiatedPositionEncoding = (server) => serverState()[server.name]?.positionEncoding ?? server.positionEncoding;\n" +
      "  const rememberServerCapabilities = (server, response) => {\n" +
      "    if (method !== 'initialize') return;\n" +
      "    const capabilities = response?.result?.capabilities ?? response?.capabilities ?? null;\n" +
      "    if (!capabilities) return;\n" +
      "    const next = { ...(serverState()[server.name] ?? {}), capabilities };\n" +
      "    if (capabilities.positionEncoding === 'utf-8' || capabilities.positionEncoding === 'utf-16' || capabilities.positionEncoding === 'utf-32') {\n" +
      "      next.positionEncoding = capabilities.positionEncoding;\n" +
      "    }\n" +
      "    serverState()[server.name] = next;\n" +
      "  };\n" +
      "  const resolveRootUri = async (buffer, server, fallbackRootUri) => {\n" +
      "    if (server.rootUri) return server.rootUri;\n" +
      "    if (fallbackRootUri) return fallbackRootUri;\n" +
      "    if (saya.workspace && typeof saya.workspace.findRoot === 'function' && buffer.path && server.rootMarkers && server.rootMarkers.length > 0) {\n" +
      "      const root = await saya.workspace.findRoot(String(buffer.path), server.rootMarkers);\n" +
      "      if (root != null) return toFileUri(root);\n" +
      "    }\n" +
      "    return null;\n" +
      "  };\n" +
      "  const buildDocumentParams = (method, uri, buffer, languageId, rootUri, server, positionEncoding) => {\n" +
      "    if (method === 'initialize') {\n" +
      "      return { processId: null, rootUri, capabilities: { general: { positionEncodings: ['utf-16', 'utf-8', 'utf-32'] }, textDocument: { hover: {}, definition: {}, references: {}, documentSymbol: {}, completion: { completionItem: { documentationFormat: ['markdown', 'plaintext'], resolveSupport: { properties: ['documentation', 'detail', 'additionalTextEdits'] } } }, signatureHelp: { signatureInformation: { documentationFormat: ['markdown', 'plaintext'], parameterInformation: { labelOffsetSupport: true } } }, formatting: {}, rangeFormatting: {}, rename: { prepareSupport: true }, codeAction: { codeActionLiteralSupport: { codeActionKind: { valueSet: ['quickfix', 'refactor', 'source', 'source.organizeImports'] } }, resolveSupport: { properties: ['edit', 'command'] } }, publishDiagnostics: {} } }, initializationOptions: server.initializationOptions ?? null };\n" +
      "    }\n" +
      "    if (method === 'initialized' || method === 'shutdown') return null;\n" +
      "    if (!uri) return null;\n" +
      "    const documents = documentState();\n" +
      "    const current = documents[uri] ?? { version: 0, open: false };\n" +
      "    if (method === 'textDocument/didOpen') {\n" +
      "      const version = current.version > 0 ? current.version : 1;\n" +
      "      documents[uri] = { version, open: true };\n" +
      "      return { textDocument: { uri, languageId, version, text: documentText(buffer) } };\n" +
      "    }\n" +
      "    if (method === 'textDocument/didChange') {\n" +
      "      const version = current.version + 1;\n" +
      "      documents[uri] = { version, open: true };\n" +
      "      return { textDocument: { uri, version }, contentChanges: [{ text: documentText(buffer) }] };\n" +
      "    }\n" +
      "    if (method === 'textDocument/didSave') {\n" +
      "      return { textDocument: { uri }, text: documentText(buffer) };\n" +
      "    }\n" +
      "    if (method === 'textDocument/didClose') {\n" +
      "      documents[uri] = { version: current.version, open: false };\n" +
      "      return { textDocument: { uri } };\n" +
      "    }\n" +
      "    if (method === 'textDocument/completion') {\n" +
      "      const triggerCharacters = " +
      quoteRuntimeValue(completionTriggerCharacters ?? [".", ":", ">", "/"]) +
      ";\n" +
      "      const previousCharacter = String(buffer.currentLine ?? '').slice(0, Number(buffer.cursorCol) || 0).slice(-1);\n" +
      "      const context = triggerCharacters.includes(previousCharacter) ? { triggerKind: 2, triggerCharacter: previousCharacter } : { triggerKind: 1 };\n" +
      "      return { textDocument: { uri }, position, context };\n" +
      "    }\n" +
      "    if (method === 'textDocument/hover' || method === 'textDocument/definition' || method === 'textDocument/references' || method === 'textDocument/signatureHelp') {\n" +
      "      return { textDocument: { uri }, position };\n" +
      "    }\n" +
      "    if (method === 'textDocument/documentSymbol') {\n" +
      "      return { textDocument: { uri } };\n" +
      "    }\n" +
      "    if (method === 'textDocument/formatting') {\n" +
      "      return { textDocument: { uri }, options: " +
      quoteRuntimeValue(formattingOptions ?? {}) +
      " };\n" +
      "    }\n" +
      "    if (method === 'textDocument/rangeFormatting') {\n" +
      "      const lineCount = Math.max(1, Number(buffer.lineCount) || String(documentText(buffer)).split(/\\r\\n|\\r|\\n/).length);\n" +
      "      const range = eventPayload && eventPayload.range ? eventPayload.range : { start: position, end: { line: lineCount - 1, character: 0 } };\n" +
      "      return { textDocument: { uri }, range, options: " +
      quoteRuntimeValue(formattingOptions ?? {}) +
      " };\n" +
      "    }\n" +
      "    if (method === 'textDocument/rename') {\n" +
      "      const newName = eventPayload && typeof eventPayload.newName === 'string' ? eventPayload.newName : " +
      quoteRuntimeValue(renameNewName ?? "") +
      ";\n" +
      "      return { textDocument: { uri }, position, newName };\n" +
      "    }\n" +
      "    if (method === 'textDocument/codeAction') {\n" +
      "      const diagnostics = eventPayload && Array.isArray(eventPayload.diagnostics) ? eventPayload.diagnostics : [];\n" +
      "      const only = eventPayload && Array.isArray(eventPayload.only) ? eventPayload.only : " +
      quoteRuntimeValue(codeActionKinds ?? ["quickfix", "refactor", "source.organizeImports"]) +
      ";\n" +
      "      const range = eventPayload && eventPayload.range ? eventPayload.range : { start: position, end: position };\n" +
      "      return { textDocument: { uri }, range, context: { diagnostics, only } };\n" +
      "    }\n" +
      "    if (method === 'completionItem/resolve' || method === 'codeAction/resolve') {\n" +
      "      return eventPayload && eventPayload.item ? eventPayload.item : (eventPayload ?? {});\n" +
      "    }\n" +
      "    return null;\n" +
      "  };\n" +
      "  const executeUiCommand = async (name, payload) => {\n" +
      "    await saya.commands.execute(`${name} ${JSON.stringify(payload)}`);\n" +
      "  };\n" +
      "  const routeFeatureResponse = async (response) => {\n" +
      "    const responseMethod = response && response.method ? response.method : method;\n" +
      "    if (responseMethod === 'textDocument/hover') {\n" +
      "      await executeUiCommand('lsp.floatHover', { response });\n" +
      "    } else if (responseMethod === 'textDocument/definition') {\n" +
      "      await executeUiCommand('lsp.gotoDefinition', { response });\n" +
      "    } else if (responseMethod === 'textDocument/references') {\n" +
      "      await executeUiCommand('lsp.floatLocations', { title: 'References', response });\n" +
      "    } else if (responseMethod === 'textDocument/documentSymbol') {\n" +
      "      await executeUiCommand('lsp.floatSymbols', { response });\n" +
      "    } else if (responseMethod === 'textDocument/completion') {\n" +
      "      const result = response?.result ?? response;\n" +
      "      const items = Array.isArray(result) ? result : (Array.isArray(result?.items) ? result.items : []);\n" +
      "      await executeUiCommand('completion.floatMenu', { candidates: items, selectedIndex: 0 });\n" +
      "    } else if (responseMethod === 'completionItem/resolve') {\n" +
      "      await executeUiCommand('completion.floatMenu', { candidates: [response?.result ?? response], selectedIndex: 0 });\n" +
      "    } else if (responseMethod === 'textDocument/signatureHelp') {\n" +
      "      const result = response?.result ?? response;\n" +
      "      const signatures = Array.isArray(result?.signatures) ? result.signatures : [];\n" +
      "      const active = Math.max(0, Math.min(Number(result?.activeSignature) || 0, Math.max(0, signatures.length - 1)));\n" +
      "      const signature = signatures[active];\n" +
      "      const label = signature?.label ?? '';\n" +
      "      const doc = typeof signature?.documentation === 'string' ? signature.documentation : (signature?.documentation?.value ?? '');\n" +
      "      await executeUiCommand('lsp.floatHover', { response: { result: { contents: [label, doc].filter(Boolean).join('\\n') } } });\n" +
      "    } else if (responseMethod === 'textDocument/formatting' || responseMethod === 'textDocument/rangeFormatting') {\n" +
      "      await executeUiCommand('lsp.previewWorkspaceEdit', { title: 'Formatting preview', response });\n" +
      "    } else if (responseMethod === 'textDocument/rename') {\n" +
      "      await executeUiCommand('lsp.previewWorkspaceEdit', { title: 'Rename preview', response });\n" +
      "    } else if (responseMethod === 'textDocument/codeAction' || responseMethod === 'codeAction/resolve') {\n" +
      "      await executeUiCommand('lsp.floatCodeActions', { response });\n" +
      "    } else if (responseMethod === 'textDocument/publishDiagnostics') {\n" +
      "      await executeUiCommand('lsp.publishDiagnostics', response.result ?? response);\n" +
      "    }\n" +
      "  };\n" +
      "  const bridgeCommand = " +
      quoteRuntimeValue(bridgeCommand) +
      ";\n" +
      "  const method = " +
      quoteRuntimeValue(method) +
      ";\n" +
      "  const source = " +
      quoteRuntimeValue(source) +
      ";\n" +
      "  const lspVersion = " +
      quoteRuntimeValue(source === "lsp" ? "3.17" : "0.6.0") +
      ";\n" +
      "  const eventPayload = payload ?? null;\n" +
      "  const buffer = eventPayload && eventPayload.buffer ? eventPayload.buffer : await saya.buffer.current();\n" +
      "  const editor = await saya.editor.current();\n" +
      "  const languageByExtension = " +
      quoteRuntimeValue(languageIdByExtension ?? {}) +
      ";\n" +
      "  const selectedLanguageId = languageByExtension[extensionOf(buffer.path)] ?? " +
      quoteRuntimeValue(languageId) +
      ";\n" +
      "  const servers = " +
      quoteRuntimeValue(servers ?? []) +
      ";\n" +
      "  const selectedServer = source === 'lsp' ? selectServer(buffer.path, selectedLanguageId, servers) : null;\n" +
      "  const effectivePositionEncoding = selectedServer ? negotiatedPositionEncoding(selectedServer) : " +
      quoteRuntimeValue(positionEncoding) +
      ";\n" +
      "  const resolvedRootUri = selectedServer ? await resolveRootUri(buffer, selectedServer, " +
      quoteRuntimeValue(rootUri) +
      ") : " +
      quoteRuntimeValue(rootUri) +
      ";\n" +
      "  const uri = toFileUri(buffer.path, resolvedRootUri);\n" +
      "  const effectiveLanguageId = selectedServer && selectedServer.languages && selectedServer.languages.length > 0 && !languageByExtension[extensionOf(buffer.path)] ? selectedServer.languages[0] : selectedLanguageId;\n" +
      "  const position = lspPositionFromSayaCursor(buffer.currentLine ?? '', buffer.cursorRow, buffer.cursorCol ?? 0, effectivePositionEncoding);\n" +
      "  const params = buildDocumentParams(method, uri, buffer, effectiveLanguageId, resolvedRootUri, selectedServer ?? {}, effectivePositionEncoding);\n" +
      // session が未起動でも textDocument/did* 系がそのまま動くよう、initialize 用の
      // params を毎回同時に組み立てて request に同梱する。__lspManager.dispatch が
      // 必要なら request.initializeParams を使って session を auto-start する。
      "  const initializeParams = buildDocumentParams('initialize', null, buffer, effectiveLanguageId, resolvedRootUri, selectedServer ?? {}, effectivePositionEncoding);\n" +
      "  const request = {\n" +
      "    source,\n" +
      "    lspVersion,\n" +
      "    method,\n" +
      "    clientName: selectedServer?.name ?? " +
      quoteRuntimeValue(clientName) +
      ",\n" +
      "    rootUri: resolvedRootUri,\n" +
      "    languageId: effectiveLanguageId,\n" +
      "    trace: selectedServer?.trace ?? " +
      quoteRuntimeValue(trace) +
      ",\n" +
      "    positionEncoding: effectivePositionEncoding,\n" +
      "    dumpPath: " +
      quoteRuntimeValue(dumpPath) +
      ",\n" +
      "    textDocument: uri ? { uri } : null,\n" +
      "    server: selectedServer && selectedServer.hasCommand ? { name: selectedServer.name, command: selectedServer.command, args: selectedServer.args ?? [], env: selectedServer.env ?? {}, cwd: selectedServer.cwd ?? null, rootMarkers: selectedServer.rootMarkers ?? [], initializationOptions: selectedServer.initializationOptions ?? null } : null,\n" +
      "    position,\n" +
      "    params,\n" +
      "    initializeParams,\n" +
      "    buffer,\n" +
      "    editor,\n" +
      "    event: eventPayload,\n" +
      "  };\n" +
      "  console.log(`[saya-lsp] dispatch ${source}:${method} for ${buffer.path ?? '<scratch>'}`);\n" +
      // Phase C: lsp 経路は常に saya.process ベースの __lspManager.dispatch を経由する。
      // 旧典型 bridge への fallback は撤去済み。lsif 経路は専用 typed bridge
      // (saya.lsif.request) を使う。
      "  if (source === 'lsp') {\n" +
      "    if (!selectedServer || !selectedServer.hasCommand) {\n" +
      "      const message = 'no language server command configured for ' + method;\n" +
      "      console.log('[saya-lsp] ' + message);\n" +
      "      await saya.commands.execute(`lsp.status ${JSON.stringify({ message })}`);\n" +
      "      throw new Error(message);\n" +
      "    }\n" +
      "    if (globalThis.__sayaLspManager == null) {\n" +
      "      globalThis.__sayaLspManager = (new Function(" +
      __lspModuleInitSourceLiteral +
      "))();\n" +
      "    }\n" +
      "    const manager = globalThis.__sayaLspManager;\n" +
      "    try {\n" +
  "      const response = await manager.dispatch(request);\n" +
  "      if (selectedServer) rememberServerCapabilities(selectedServer, response);\n" +
  "      for (const notification of manager.drainNotifications()) {\n" +
  "        await routeFeatureResponse(notification);\n" +
  "      }\n" +
  "      await routeFeatureResponse(response);\n" +
  "      return response;\n" +
      "    } catch (error) {\n" +
      // K 連打などで前回 inflight が cancel されたケース (error.lspSuperseded) は
      // routeFeatureResponse まで届けず静かに drop する。
      "      if (error && error.lspSuperseded === true) {\n" +
      "        console.log(`[saya-lsp] superseded request dropped silently: ${method}`);\n" +
      "        return null;\n" +
      "      }\n" +
      "      const message = error && error.message ? String(error.message) : String(error);\n" +
      "      console.log(`[saya-lsp] request failed ${method}: ${message}`);\n" +
      "      await saya.commands.execute(`lsp.status ${JSON.stringify({ message: `Language server not ready: ${method}` })}`);\n" +
      "      throw error;\n" +
      "    }\n" +
      "  }\n" +
      "  if (source === 'lsif' && saya.lsif && typeof saya.lsif.request === 'function') {\n" +
      "    try {\n" +
      "      const response = await saya.lsif.request(request);\n" +
      "      await routeFeatureResponse(response);\n" +
      "      return response;\n" +
      "    } catch (error) {\n" +
      "      const message = error && error.message ? String(error.message) : String(error);\n" +
      "      console.log(`[saya-lsp] lsif request failed ${method}: ${message}`);\n" +
      "      await saya.commands.execute(`lsp.status ${JSON.stringify({ message: `LSIF lookup failed: ${method}` })}`);\n" +
      "      throw error;\n" +
      "    }\n" +
      "  }\n" +
      "  await saya.commands.execute(`${bridgeCommand} ${JSON.stringify(request)}`);\n" +
      "};",
  )();
}

export function encodeLspMessage(message) {
  const body = JSON.stringify(message);
  const length = utf8ByteLength(body);
  return `Content-Length: ${length}\r\n\r\n${body}`;
}

export function createLspMessageParser(
  onMessage,
  onError = (error) => {
    throw error;
  },
) {
  let buffer = "";

  return {
    accept(chunk) {
      buffer = buffer + decodeLspChunk(chunk);

      while (true) {
        const headerEnd = buffer.indexOf("\r\n\r\n");
        if (headerEnd < 0) {
          return;
        }

        const header = buffer.slice(0, headerEnd);
        const contentLengthLine = header
          .split("\r\n")
          .find((line) => line.toLowerCase().startsWith("content-length"));
        const contentLength = contentLengthLine ? contentLengthLine.split(/\s+/).pop() : null;

        if (!contentLength) {
          onError(new Error("LSP message is missing Content-Length header"));
          buffer = buffer.slice(headerEnd + 4);
          continue;
        }

        const byteLength = Number(contentLength);
        const bodyStart = headerEnd + 4;
        const body = buffer.slice(bodyStart);
        const available = utf8ByteLength(body);
        if (available < byteLength) {
          return;
        }

        const consumedTextLength = utf8PrefixCharLength(body, byteLength);
        const messageText = body.slice(0, consumedTextLength);
        buffer = body.slice(consumedTextLength);

        try {
          onMessage(JSON.parse(messageText));
        } catch (error) {
          onError(error instanceof Error ? error : new Error(String(error)));
        }
      }
    },
  };
}

export function createLspJsonRpcClient(transport) {
  let nextId = 1;
  const pending = new Map();

  return {
    async notify(method, params) {
      await transport.send(encodeLspMessage({ jsonrpc: "2.0", method, params }));
    },

    async request(method, params) {
      const id = nextId;
      nextId = nextId + 1;
      const response = new Promise((resolve, reject) => {
        pending.set(id, { resolve, reject });
      });
      await transport.send(encodeLspMessage({ jsonrpc: "2.0", id, method, params }));
      return response;
    },

    handleMessage(message) {
      if (!("id" in message) || message.id === null || typeof message.id !== "number") {
        return;
      }

      const response = message;
      if (response.id === null || typeof response.id !== "number") {
        return;
      }

      const waiter = pending.get(response.id);
      if (!waiter) {
        return;
      }
      pending.delete(response.id);

      if (response.error) {
        waiter.reject(response.error);
      } else {
        waiter.resolve("result" in response ? response.result : null);
      }
    },
  };
}

export function parseLsifLine(line) {
  const trimmed = line.trim();
  if (!trimmed) {
    return null;
  }
  const entry = JSON.parse(trimmed);
  if (entry.type !== "vertex" && entry.type !== "edge") {
    throw new Error(`unsupported LSIF entry type: ${String(entry.type)}`);
  }
  return entry;
}

export function setupSayaLspClient(options = {}) {
  const commandNames = defaultCommandNames(options.commands);
  const bridgeCommand = options.bridgeCommand ?? "lsp.request";
  const clientName = options.clientName ?? "saya";
  const rootUri = options.rootUri ?? null;
  const languageId = options.languageId ?? "plaintext";
  const languageIdByExtension = options.languageIdByExtension ?? {};
  const trace = options.trace ?? "off";
  const positionEncoding = options.positionEncoding ?? "utf-16";
  const servers = normalizeLspServers(options, commandNames);
  const completionTriggerCharacters = options.completionTriggerCharacters ?? [
    ".",
    ":",
    ">",
    "/",
  ];
  const formattingOptions = options.formattingOptions ?? {
    tabSize: 4,
    insertSpaces: true,
  };
  const renameNewName = options.renameNewName ?? "";
  const codeActionKinds = options.codeActionKinds ?? [
    "quickfix",
    "refactor",
    "source.organizeImports",
  ];
  const enableBufferEvents = options.enableBufferEvents ?? true;
  const lsifEnabled = options.lsif?.enabled ?? false;
  const lsifBridgeCommand = options.lsif?.bridgeCommand ?? "lsif.request";
  const lsifDumpPath = options.lsif?.dumpPath ?? "";

  for (const kind of [
    "initialize",
    "initialized",
    "hover",
    "definition",
    "references",
    "documentSymbol",
    "completion",
    "completionResolve",
    "signatureHelp",
    "formatting",
    "rangeFormatting",
    "rename",
    "codeAction",
    "codeActionResolve",
    "shutdown",
  ]) {
    saya.commands.register(
      commandNames[kind],
      createRuntimeBridgeCallbackSource(
        "lsp",
        bridgeCommand,
        lspMethodForCommand(kind),
        clientName,
        rootUri,
        languageId,
        languageIdByExtension,
        servers,
        trace,
        positionEncoding,
        completionTriggerCharacters,
        formattingOptions,
        renameNewName,
        codeActionKinds,
        "",
      ),
    );
  }

  saya.commands.register(commandNames.nextDiagnostic, async () => {
    await saya.commands.execute("lsp.nextDiagnostic");
  });
  saya.commands.register(commandNames.previousDiagnostic, async () => {
    await saya.commands.execute("lsp.previousDiagnostic");
  });

  if (enableBufferEvents) {
    saya.events.on(
      "bufferOpen",
      createRuntimeBridgeCallbackSource(
        "lsp",
        bridgeCommand,
        "textDocument/didOpen",
        clientName,
        rootUri,
        languageId,
        languageIdByExtension,
        servers,
        trace,
        positionEncoding,
        completionTriggerCharacters,
        formattingOptions,
        renameNewName,
        codeActionKinds,
        "",
      ),
    );
    saya.events.on(
      "bufferChanged",
      createRuntimeBridgeCallbackSource(
        "lsp",
        bridgeCommand,
        "textDocument/didChange",
        clientName,
        rootUri,
        languageId,
        languageIdByExtension,
        servers,
        trace,
        positionEncoding,
        completionTriggerCharacters,
        formattingOptions,
        renameNewName,
        codeActionKinds,
        "",
      ),
    );
    saya.events.on(
      "bufferWritePost",
      createRuntimeBridgeCallbackSource(
        "lsp",
        bridgeCommand,
        "textDocument/didSave",
        clientName,
        rootUri,
        languageId,
        languageIdByExtension,
        servers,
        trace,
        positionEncoding,
        completionTriggerCharacters,
        formattingOptions,
        renameNewName,
        codeActionKinds,
        "",
      ),
    );
    saya.events.on(
      "bufferClosed",
      createRuntimeBridgeCallbackSource(
        "lsp",
        bridgeCommand,
        "textDocument/didClose",
        clientName,
        rootUri,
        languageId,
        languageIdByExtension,
        servers,
        trace,
        positionEncoding,
        completionTriggerCharacters,
        formattingOptions,
        renameNewName,
        codeActionKinds,
        "",
      ),
    );
  }

  saya.keymap.set("normal", options.keymap?.hover ?? "K", saya.commands.execute(commandNames.hover));
  saya.keymap.set(
    "normal",
    options.keymap?.definition ?? "gd",
    saya.commands.execute(commandNames.definition),
  );
  saya.keymap.set(
    "normal",
    options.keymap?.references ?? "gR",
    saya.commands.execute(commandNames.references),
  );
  saya.keymap.set(
    "normal",
    options.keymap?.documentSymbol ?? "gO",
    saya.commands.execute(commandNames.documentSymbol),
  );
  saya.keymap.set(
    "insert",
    options.keymap?.completion ?? "<C-Space>",
    saya.commands.execute(commandNames.completion),
  );
  saya.keymap.set(
    "insert",
    options.keymap?.signatureHelp ?? "<C-k>",
    saya.commands.execute(commandNames.signatureHelp),
  );
  saya.keymap.set(
    "normal",
    options.keymap?.formatting ?? "gq",
    saya.commands.execute(commandNames.formatting),
  );
  saya.keymap.set(
    "visual",
    options.keymap?.rangeFormatting ?? "gq",
    saya.commands.execute(commandNames.rangeFormatting),
  );
  saya.keymap.set(
    "normal",
    options.keymap?.rename ?? "grn",
    saya.commands.execute(commandNames.rename),
  );
  saya.keymap.set(
    "normal",
    options.keymap?.codeAction ?? "gra",
    saya.commands.execute(commandNames.codeAction),
  );
  saya.keymap.set(
    "normal",
    options.keymap?.nextDiagnostic ?? "]d",
    saya.commands.execute(commandNames.nextDiagnostic),
  );
  saya.keymap.set(
    "normal",
    options.keymap?.previousDiagnostic ?? "[d",
    saya.commands.execute(commandNames.previousDiagnostic),
  );

  if (lsifEnabled) {
    saya.commands.register(
      commandNames.lsifHover,
      createRuntimeBridgeCallbackSource(
        "lsif",
        lsifBridgeCommand,
        "textDocument/hover",
        clientName,
        rootUri,
        languageId,
        languageIdByExtension,
        servers,
        trace,
        positionEncoding,
        completionTriggerCharacters,
        formattingOptions,
        renameNewName,
        codeActionKinds,
        lsifDumpPath,
      ),
    );
    saya.commands.register(
      commandNames.lsifDefinition,
      createRuntimeBridgeCallbackSource(
        "lsif",
        lsifBridgeCommand,
        "textDocument/definition",
        clientName,
        rootUri,
        languageId,
        languageIdByExtension,
        servers,
        trace,
        positionEncoding,
        completionTriggerCharacters,
        formattingOptions,
        renameNewName,
        codeActionKinds,
        lsifDumpPath,
      ),
    );
    saya.keymap.set(
      "normal",
      options.keymap?.lsifHover ?? "gK",
      saya.commands.execute(commandNames.lsifHover),
    );
    saya.keymap.set(
      "normal",
      options.keymap?.lsifDefinition ?? "gD",
      saya.commands.execute(commandNames.lsifDefinition),
    );
  }
}
