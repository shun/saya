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
      "    buffer,\n" +
      "    editor,\n" +
      "    event: eventPayload,\n" +
      "  };\n" +
      "  console.log(`[saya-lsp] dispatch ${source}:${method} for ${buffer.path ?? '<scratch>'}`);\n" +
      "  if ((source === 'lsp' || source === 'lsif') && saya.lsp && typeof saya.lsp.request === 'function') {\n" +
      "    try {\n" +
      "      const response = await saya.lsp.request(request);\n" +
      "      if (selectedServer) rememberServerCapabilities(selectedServer, response);\n" +
      "      await routeFeatureResponse(response);\n" +
      "      return response;\n" +
      "    } catch (error) {\n" +
      "      const message = error && error.message ? String(error.message) : String(error);\n" +
      "      console.log(`[saya-lsp] request failed ${method}: ${message}`);\n" +
      "      await saya.commands.execute(`lsp.status ${JSON.stringify({ message: `Language server not ready: ${method}` })}`);\n" +
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
