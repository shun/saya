import { compareCandidates, uniqueByLabel } from "./candidates.ts";
import { createBufferWordSource } from "./sources/buffer.ts";
import { createLspCompletionSource } from "./sources/lsp.ts";
import { createPathCompletionSource } from "./sources/path.ts";
import type {
  SayaCompletionCandidate,
  SayaCompletionKeyBindings,
  SayaCompletionOptions,
  SayaCompletionQuery,
  SayaCompletionSource,
  SayaCompletionSourceResult,
  SayaCompletionTriggerContext,
  SayaCompletionTriggerReason,
} from "./types.ts";

declare const saya: any;

let nextCompletionRequestId = 1;

type BundledCompletionSourceDescriptor = Record<string, any>;

const DEFAULT_AUTO_TRIGGER_DELAY_MS = 80;
const DEFAULT_COMPLETION_KEYS: Required<SayaCompletionKeyBindings> = {
  confirm: ["<Enter>", "<Tab>", "<C-y>"],
  close: ["<Esc>", "<C-[>"],
  next: ["<Down>", "<C-n>"],
  previous: ["<Up>", "<C-p>"],
  pageNext: ["<PageDown>"],
  pagePrevious: ["<PageUp>"],
};

interface BundledCompletionRuntimeOptions {
  minPrefixLength: number;
  maxItems: number;
  sourceTimeoutMs: number;
  keys?: Required<SayaCompletionKeyBindings>;
  sources: BundledCompletionSourceDescriptor[];
}

interface BundledCompletionAutoTriggerOptions {
  commandName: string;
  autoTriggerDelayMs: number;
}

export function prefixFilter(
  result: SayaCompletionSourceResult,
  query: SayaCompletionQuery,
): SayaCompletionSourceResult {
  const prefix = query.prefix.toLowerCase();
  if (!prefix) return result;
  return {
    ...result,
    candidates: result.candidates.filter((candidate) =>
      candidate.label.toLowerCase() !== prefix &&
      candidate.label.toLowerCase().startsWith(prefix)
    ),
  };
}

export function labelSorter(
  candidates: SayaCompletionCandidate[],
  _result: SayaCompletionSourceResult,
): SayaCompletionCandidate[] {
  return [...candidates].sort(compareCandidates);
}

export async function setupSayaCompletion(options: SayaCompletionOptions = {}) {
  const commandName = options.commandName ?? "completion.trigger";
  const key = options.key;
  const minPrefixLength = options.minPrefixLength ?? 1;
  const maxItems = options.maxItems ?? 50;
  const sourceTimeoutMs = options.sourceTimeoutMs ?? 1000;
  const autoTrigger = options.autoTrigger ?? false;
  const autoTriggerDelayMs = options.autoTriggerDelayMs ??
    DEFAULT_AUTO_TRIGGER_DELAY_MS;
  const keys = options.keys === undefined
    ? undefined
    : normalizeCompletionKeys(options.keys);
  const sources = options.sources ?? [];
  const filters = options.filters ?? [prefixFilter];
  const sorters = options.sorters ?? [labelSorter];
  const bundledSourceDescriptors = bundledSourceDescriptorsFor(sources);
  const canUseStartupSafeRuntimeCommand = bundledSourceDescriptors != null &&
    options.filters == null && options.sorters == null;
  if (canUseStartupSafeRuntimeCommand) {
    const run = createBundledCompletionRuntimeCommand({
      minPrefixLength,
      maxItems,
      sourceTimeoutMs,
      keys,
      sources: bundledSourceDescriptors,
    });
    saya.commands.register(commandName, run);
    if (key !== undefined) {
      saya.keymap.set("insert", key, saya.commands.execute(commandName));
    }
    if (autoTrigger) {
      saya.events.on(
        "bufferChanged",
        createBundledCompletionAutoTriggerCallback({
          commandName,
          autoTriggerDelayMs,
        }),
      );
    }
    return { commandName, key, keys, autoTrigger, autoTriggerDelayMs };
  }

  const run = async (
    reason: SayaCompletionTriggerReason = { kind: "manual" },
  ) => {
    const buffer = await saya.buffer.current();
    const editor = await saya.editor.current();
    if (reason.kind === "auto" && !isInsertMode(editor.mode)) {
      return false;
    }
    const triggerContext: SayaCompletionTriggerContext = {
      buffer,
      editor,
      reason,
    };
    const queries = sources.flatMap((source) => {
      const query = source.trigger(triggerContext);
      return query != null &&
          acceptsCompletionQuery(source, query, minPrefixLength, reason)
        ? [{ source, query }]
        : [];
    });
    if (queries.length === 0) return false;

    let results = (await Promise.all(
      queries.map(({ source, query }) =>
        completeWithTimeout(
          source,
          query,
          sourceTimeoutMs,
        )
      ),
    )).filter((result) => result.candidates.length > 0);
    if (results.length === 0) return false;

    results = results.map((result) => {
      const sourceQuery = queries.find(({ query }) =>
        query.sourceId === result.sourceId
      );
      if (!sourceQuery) return result;
      let filtered = result;
      for (const filter of filters) {
        filtered = filter(filtered, sourceQuery.query);
      }
      return filtered;
    }).filter((result) => result.candidates.length > 0);
    if (results.length === 0) return false;

    const selected = selectResultGroup(results, sources);
    let candidates = selected.results.flatMap((result) => result.candidates);
    for (const sorter of sorters) {
      candidates = sorter(candidates, selected.results[0]);
    }
    candidates = uniqueByLabel(candidates).slice(0, maxItems);
    if (candidates.length === 0) return false;
    return await saya.completion.show({
      sessionId: `buffer:${buffer.id}:${buffer.cursorRow}:${buffer.cursorCol}`,
      requestId: nextCompletionRequestId++,
      replaceRange: selected.replaceRange,
      candidates,
      selectedIndex: 0,
      ...(keys === undefined ? {} : { keys }),
    });
  };

  saya.commands.register(commandName, run);
  if (key !== undefined) {
    saya.keymap.set("insert", key, saya.commands.execute(commandName));
  }
  if (autoTrigger) {
    let timer: ReturnType<typeof setTimeout> | undefined;
    let generation = 0;
    saya.events.on("bufferChanged", (payload: unknown) => {
      const character = autoTriggerCharacter(payload);
      generation += 1;
      const currentGeneration = generation;
      const start = async () => {
        const shown = await run({ kind: "auto", character });
        if (!shown && currentGeneration === generation) {
          await closeCompletionMenu();
        }
        return shown;
      };
      if (
        !Number.isFinite(autoTriggerDelayMs) || autoTriggerDelayMs <= 0 ||
        typeof setTimeout !== "function" ||
        typeof clearTimeout !== "function"
      ) {
        return start();
      }
      if (timer !== undefined) clearTimeout(timer);
      timer = setTimeout(() => {
        timer = undefined;
        void start();
      }, autoTriggerDelayMs);
    });
  }
  return { commandName, key, keys, autoTrigger, autoTriggerDelayMs };
}

async function closeCompletionMenu(): Promise<void> {
  try {
    const completionSurface = Reflect.get(saya, "completion");
    if (!completionSurface || typeof completionSurface !== "object") return;
    const close = Reflect.get(completionSurface, "close");
    if (typeof close !== "function") return;
    await close.call(completionSurface);
  } catch (error) {
    console.debug(
      `[saya-completion] failed to close stale completion menu: ${
        String(error)
      }`,
    );
  }
}

function isInsertMode(mode: unknown): boolean {
  return String(mode ?? "").toLowerCase() === "insert";
}

function acceptsCompletionQuery(
  source: SayaCompletionSource,
  query: SayaCompletionQuery,
  globalMinPrefixLength: number,
  reason: SayaCompletionTriggerReason,
): boolean {
  const minPrefixLength = source.minPrefixLength ?? globalMinPrefixLength;
  if (query.prefix.length >= minPrefixLength) return true;
  if (reason.kind !== "auto") return false;
  const character = reason.character;
  return character != null &&
    (source.triggerCharacters ?? []).includes(character);
}

function autoTriggerCharacter(payload: unknown): string | undefined {
  const buffer = payload && typeof payload === "object"
    ? Reflect.get(payload, "buffer")
    : null;
  if (!buffer || typeof buffer !== "object") return undefined;
  const line = String(Reflect.get(buffer, "currentLine") ?? "");
  const cursor = Math.max(
    0,
    Math.min(Number(Reflect.get(buffer, "cursorCol")) || 0, line.length),
  );
  return cursor > 0 ? line[cursor - 1] : undefined;
}

function normalizeCompletionKeys(
  keys: SayaCompletionKeyBindings | undefined,
): Required<SayaCompletionKeyBindings> {
  return {
    confirm: normalizeKeyList(keys?.confirm, DEFAULT_COMPLETION_KEYS.confirm),
    close: normalizeKeyList(keys?.close, DEFAULT_COMPLETION_KEYS.close),
    next: normalizeKeyList(keys?.next, DEFAULT_COMPLETION_KEYS.next),
    previous: normalizeKeyList(
      keys?.previous,
      DEFAULT_COMPLETION_KEYS.previous,
    ),
    pageNext: normalizeKeyList(
      keys?.pageNext,
      DEFAULT_COMPLETION_KEYS.pageNext,
    ),
    pagePrevious: normalizeKeyList(
      keys?.pagePrevious,
      DEFAULT_COMPLETION_KEYS.pagePrevious,
    ),
  };
}

function normalizeKeyList(
  keys: string[] | undefined,
  defaults: string[],
): string[] {
  if (keys === undefined) return [...defaults];
  return keys
    .map((key) => String(key).trim())
    .filter((key, index, normalized) =>
      key.length > 0 && normalized.indexOf(key) === index
    );
}

function bundledSourceDescriptorsFor(
  sources: SayaCompletionSource[],
): BundledCompletionSourceDescriptor[] | null {
  const descriptors: BundledCompletionSourceDescriptor[] = [];
  for (const source of sources) {
    const descriptor = source.__sayaBundledSource;
    if (descriptor == null) return null;
    descriptors.push(descriptor);
  }
  return descriptors;
}

function createBundledCompletionRuntimeCommand(
  options: BundledCompletionRuntimeOptions,
) {
  const minPrefixLength = JSON.stringify(options.minPrefixLength);
  const maxItems = JSON.stringify(options.maxItems);
  const sourceTimeoutMs = JSON.stringify(options.sourceTimeoutMs);
  const keys = JSON.stringify(options.keys);
  const sourceDescriptors = JSON.stringify(options.sources);
  const source = `
    return async function sayaCompletionTrigger() {
      const minPrefixLength = ${minPrefixLength};
      const maxItems = ${maxItems};
      const sourceTimeoutMs = ${sourceTimeoutMs};
      const keys = ${keys};
      const sourceDescriptors = ${sourceDescriptors};
      const requestIdKey = "__sayaCompletionNextRequestId";
      const triggerReasonKey = "__sayaCompletionTriggerReason";
      globalThis[requestIdKey] = Number.isFinite(Number(globalThis[requestIdKey]))
        ? Number(globalThis[requestIdKey])
        : 1;
      const readTriggerReason = () => {
        const value = globalThis[triggerReasonKey];
        globalThis[triggerReasonKey] = undefined;
        if (!value || typeof value !== "object") return { kind: "manual" };
        if (value.kind !== "auto") return { kind: "manual" };
        return {
          kind: "auto",
          character: typeof value.character === "string" ? value.character : undefined,
        };
      };
      const acceptsQuery = (prefix, sourceMinPrefixLength, triggerCharacters, reason) => {
        if (prefix.length >= sourceMinPrefixLength) return true;
        return reason.kind === "auto" &&
          typeof reason.character === "string" &&
          triggerCharacters.includes(reason.character);
      };

      const setRank = (candidate, rank) => {
        Object.defineProperty(candidate, "__rank", {
          value: rank,
          enumerable: false,
          configurable: true,
        });
        return candidate;
      };
      const compareCandidates = (left, right) => {
        const leftRank = left.__rank ?? {};
        const rightRank = right.__rank ?? {};
        const leftDistance = leftRank.distance ?? Number.POSITIVE_INFINITY;
        const rightDistance = rightRank.distance ?? Number.POSITIVE_INFINITY;
        if (leftDistance !== rightDistance) return leftDistance - rightDistance;
        const leftKindRank = leftRank.kindRank ?? 0;
        const rightKindRank = rightRank.kindRank ?? 0;
        if (leftKindRank !== rightKindRank) return leftKindRank - rightKindRank;
        if (left.label.length !== right.label.length) {
          return left.label.length - right.label.length;
        }
        return left.label.localeCompare(right.label);
      };
      const uniqueByLabel = (candidates) => {
        const seen = new Set();
        const result = [];
        for (const candidate of candidates) {
          const label = String(candidate.label ?? "").trim();
          if (!label || seen.has(label)) continue;
          seen.add(label);
          const normalized = { ...candidate, label };
          const rank = candidate.__rank;
          if (rank != null) {
            Object.defineProperty(normalized, "__rank", {
              value: rank,
              enumerable: false,
              configurable: true,
            });
          }
          result.push(normalized);
        }
        return result;
      };
      const wordPrefix = (buffer) => {
        const line = String(buffer.currentLine ?? "");
        const cursor = Math.max(0, Math.min(Number(buffer.cursorCol) || 0, line.length));
        const before = line.slice(0, cursor);
        const match = before.match(/[A-Za-z0-9_]+$/);
        const prefix = match ? match[0] : "";
        return {
          prefix,
          range: {
            start: { line: Number(buffer.cursorRow) || 0, character: cursor - prefix.length },
            end: { line: Number(buffer.cursorRow) || 0, character: cursor },
          },
        };
      };
      const pathBoundaryChars = new Set([
        '"',
        "'",
        String.fromCharCode(96),
        "<",
        ">",
        "(",
        ")",
        "[",
        "]",
        "{",
        "}",
      ]);
      const isPathPrefixBoundary = (ch) =>
        String(ch).trim() === "" || pathBoundaryChars.has(String(ch));
      const isPathLikePrefix = (prefix) =>
        !!prefix && (
          prefix.startsWith("/") ||
          prefix.startsWith("./") ||
          prefix.startsWith("../") ||
          prefix.includes("/")
        );
      const dirname = (path) => {
        const value = String(path ?? "");
        if (!value) return ".";
        const trimmed = value.replace(/\\/+$/, "");
        const slash = trimmed.lastIndexOf("/");
        if (slash <= 0) return slash === 0 ? "/" : ".";
        return trimmed.slice(0, slash);
      };
      const normalizePath = (path) => {
        const value = String(path || ".");
        const absolute = value.startsWith("/");
        const parts = [];
        for (const part of value.split("/")) {
          if (!part || part === ".") continue;
          if (part === "..") {
            if (parts.length > 0 && parts[parts.length - 1] !== "..") {
              parts.pop();
            } else if (!absolute) {
              parts.push("..");
            }
          } else {
            parts.push(part);
          }
        }
        const joined = parts.join("/");
        if (absolute) return "/" + joined;
        return joined || ".";
      };
      const joinPath = (base, child) => {
        if (!child || child === ".") return normalizePath(base || ".");
        if (child.startsWith("/")) return normalizePath(child);
        if (!base || base === ".") return normalizePath(child);
        if (base === "/") return normalizePath("/" + child);
        return normalizePath(String(base).replace(/\\/+$/, "") + "/" + child);
      };
      const pathPrefix = (buffer) => {
        const line = String(buffer.currentLine ?? "");
        const cursor = Math.max(0, Math.min(Number(buffer.cursorCol) || 0, line.length));
        const before = line.slice(0, cursor);
        let start = before.length;
        while (start > 0 && !isPathPrefixBoundary(before[start - 1])) start -= 1;
        const prefix = before.slice(start);
        if (!isPathLikePrefix(prefix)) return null;
        const slash = prefix.lastIndexOf("/");
        return {
          prefix,
          range: {
            start: { line: Number(buffer.cursorRow) || 0, character: start },
            end: { line: Number(buffer.cursorRow) || 0, character: cursor },
          },
          directoryPrefix: slash >= 0 ? prefix.slice(0, slash + 1) : "",
          entryPrefix: slash >= 0 ? prefix.slice(slash + 1) : prefix,
        };
      };
      const pathDirectory = (buffer, prefixInfo) => {
        if (prefixInfo.directoryPrefix.startsWith("/")) return normalizePath(prefixInfo.directoryPrefix);
        return joinPath(dirname(buffer.path), prefixInfo.directoryPrefix || ".");
      };
      const pathLabel = (directoryPrefix, name, isDirectory) =>
        directoryPrefix + name + (isDirectory ? "/" : "");
      const pathKindRank = (kind) => kind === "directory" ? 0 : (kind === "file" ? 1 : 2);
      const comparePathCandidates = (left, right) => {
        const leftRank = pathKindRank(String(left.detail ?? "").toLowerCase());
        const rightRank = pathKindRank(String(right.detail ?? "").toLowerCase());
        if (leftRank !== rightRank) return leftRank - rightRank;
        if (left.label.length !== right.label.length) return left.label.length - right.label.length;
        return left.label.localeCompare(right.label);
      };
      const field = (value, key) =>
        value && typeof value === "object" ? Reflect.get(value, key) : null;
      const documentation = (value) => {
        if (typeof value === "string") return value.split(/\\r\\n|\\r|\\n/).filter((line) => line.trim().length > 0);
        if (value && typeof value === "object") return documentation(field(value, "value"));
        return [];
      };
      const normalizeLspItem = (item) => {
        if (!item || typeof item !== "object") return null;
        const label = String(field(item, "label") ?? "").trim();
        if (!label) return null;
        const textEdit = field(item, "textEdit");
        const insertText = field(item, "insertText") ?? field(textEdit, "newText") ?? label;
        return {
          label,
          insertText: typeof insertText === "string" ? insertText : label,
          kind: field(item, "kind") == null ? null : String(field(item, "kind")),
          detail: typeof field(item, "detail") === "string" ? String(field(item, "detail")) : null,
          documentation: documentation(field(item, "documentation")),
          source: "lsp",
        };
      };
      const lspItems = (response) => {
        const result = response && typeof response === "object" ? field(response, "result") : response;
        if (Array.isArray(result)) return result;
        const items = field(result, "items");
        return Array.isArray(items) ? items : [];
      };
      const lspRange = (item) => {
        const textEdit = field(item, "textEdit");
        const range = field(textEdit, "range");
        const start = field(range, "start");
        const end = field(range, "end");
        if (!start || !end) return null;
        const startLine = Number(field(start, "line"));
        const startCharacter = Number(field(start, "character"));
        const endLine = Number(field(end, "line"));
        const endCharacter = Number(field(end, "character"));
        if (
          !Number.isFinite(startLine) || !Number.isFinite(startCharacter) ||
          !Number.isFinite(endLine) || !Number.isFinite(endCharacter)
        ) return null;
        return {
          start: { line: Math.max(0, startLine), character: Math.max(0, startCharacter) },
          end: { line: Math.max(0, endLine), character: Math.max(0, endCharacter) },
        };
      };
      const withTimeout = async (sourceId, prefix, replaceRange, producer) => {
        if (
          !Number.isFinite(sourceTimeoutMs) ||
          sourceTimeoutMs <= 0 ||
          typeof setTimeout !== "function" ||
          typeof clearTimeout !== "function"
        ) return await producer();
        let timeoutId;
        const timeout = new Promise((resolve) => {
          timeoutId = setTimeout(() => {
            console.debug("[saya-completion] source timed out: " + sourceId);
            resolve({ sourceId, prefix, replaceRange, candidates: [] });
          }, sourceTimeoutMs);
        });
        try {
          return await Promise.race([Promise.resolve(producer()), timeout]);
        } finally {
          if (timeoutId !== undefined) clearTimeout(timeoutId);
        }
      };

      const buffer = await saya.buffer.current();
      const editor = await saya.editor.current();
      const triggerReason = readTriggerReason();
      if (triggerReason.kind === "auto" && String(editor?.mode ?? "").toLowerCase() !== "insert") {
        return false;
      }
      const queries = [];
      for (const descriptor of sourceDescriptors) {
        const sourceMinPrefixLength = descriptor.minPrefixLength ?? minPrefixLength;
        const triggerCharacters = Array.isArray(descriptor.triggerCharacters) ? descriptor.triggerCharacters : [];
        if (descriptor.kind === "lsp") {
          const prefixInfo = wordPrefix(buffer);
          if (acceptsQuery(prefixInfo.prefix, sourceMinPrefixLength, triggerCharacters, triggerReason)) {
            queries.push({ sourceId: descriptor.id, prefix: prefixInfo.prefix, replaceRange: prefixInfo.range, descriptor });
          }
        } else if (descriptor.kind === "path") {
          const pathInfo = pathPrefix(buffer);
          if (pathInfo && acceptsQuery(pathInfo.prefix, sourceMinPrefixLength, triggerCharacters, triggerReason)) {
            queries.push({ sourceId: descriptor.id, prefix: pathInfo.prefix, replaceRange: pathInfo.range, pathInfo, descriptor });
          }
        } else if (descriptor.kind === "buffer") {
          const prefixInfo = wordPrefix(buffer);
          if (acceptsQuery(prefixInfo.prefix, sourceMinPrefixLength, triggerCharacters, triggerReason)) {
            queries.push({ sourceId: descriptor.id, prefix: prefixInfo.prefix, replaceRange: prefixInfo.range, descriptor });
          }
        }
      }
      if (queries.length === 0) return false;

      const results = [];
      for (const query of queries) {
        if (query.descriptor.kind === "lsp") {
          const result = await withTimeout(query.sourceId, query.prefix, query.replaceRange, async () => {
            try {
              const response = await saya.commands.execute(query.descriptor.commandName || "lsp.completion");
              const items = lspItems(response);
              let replaceRange = query.replaceRange;
              for (const item of items) {
                const range = lspRange(item);
                if (range) {
                  replaceRange = range;
                  break;
                }
              }
              return {
                sourceId: query.sourceId,
                prefix: query.prefix,
                replaceRange,
                candidates: uniqueByLabel(items.map(normalizeLspItem).filter(Boolean).map((candidate) => ({ ...candidate, source: query.sourceId }))),
              };
            } catch (error) {
              console.debug("[saya-completion] optional LSP source skipped: " + String(error));
              return { sourceId: query.sourceId, prefix: query.prefix, replaceRange: query.replaceRange, candidates: [] };
            }
          });
          if (result.candidates.length > 0) results.push(result);
        } else if (query.descriptor.kind === "path") {
          const result = await withTimeout(query.sourceId, query.prefix, query.replaceRange, async () => {
            try {
              const entries = await saya.fs.readDir(pathDirectory(buffer, query.pathInfo), {
                showHidden: query.descriptor.showHidden !== false,
                sortBy: "kind",
              });
              const entryPrefix = query.pathInfo.entryPrefix.toLowerCase();
              const candidates = [];
              for (const entry of Array.isArray(entries) ? entries : []) {
                if (!entry || typeof entry !== "object") continue;
                const name = String(field(entry, "name") ?? "");
                const kind = String(field(entry, "kind") ?? "file").toLowerCase();
                if (!name || !name.toLowerCase().startsWith(entryPrefix)) continue;
                const isDirectory = kind === "directory";
                const label = pathLabel(query.pathInfo.directoryPrefix, name, isDirectory);
                if (label.toLowerCase() === query.pathInfo.prefix.toLowerCase()) continue;
                candidates.push(setRank({
                  label,
                  insertText: label,
                  kind: isDirectory ? "Folder" : "File",
                  detail: isDirectory ? "directory" : kind,
                  source: query.sourceId,
                }, { distance: 0, kindRank: pathKindRank(kind) }));
              }
              const sourceMaxItems = query.descriptor.maxItems !== null &&
                  Number.isFinite(Number(query.descriptor.maxItems))
                ? Number(query.descriptor.maxItems)
                : maxItems;
              return {
                sourceId: query.sourceId,
                prefix: query.prefix,
                replaceRange: query.replaceRange,
                candidates: uniqueByLabel(candidates.sort(comparePathCandidates)).slice(0, sourceMaxItems),
              };
            } catch (error) {
              console.debug("[saya-completion] optional path source skipped: " + String(error));
              return { sourceId: query.sourceId, prefix: query.prefix, replaceRange: query.replaceRange, candidates: [] };
            }
          });
          if (result.candidates.length > 0) results.push(result);
        } else {
          const result = await withTimeout(query.sourceId, query.prefix, query.replaceRange, async () => {
            const text = String(buffer.text ?? "");
            const cursorOffset = text
              .split("\\n")
              .slice(0, Number(buffer.cursorRow) || 0)
              .reduce((sum, line) => sum + line.length + 1, 0) +
              (Number(buffer.cursorCol) || 0);
            const candidates = [];
            const seen = new Set();
            for (const match of text.matchAll(/[A-Za-z0-9_]+/g)) {
              const word = match[0];
              const start = match.index ?? 0;
              if (
                word.toLowerCase() === query.prefix.toLowerCase() ||
                !word.toLowerCase().startsWith(query.prefix.toLowerCase()) ||
                seen.has(word)
              ) continue;
              seen.add(word);
              const end = start + word.length;
              const distance = cursorOffset < start
                ? start - cursorOffset
                : (cursorOffset > end ? cursorOffset - end : 0);
              candidates.push(setRank({
                label: word,
                insertText: word,
                kind: "Text",
                detail: "buffer word",
                source: query.sourceId,
              }, { distance, kindRank: 2 }));
            }
            return {
              sourceId: query.sourceId,
              prefix: query.prefix,
              replaceRange: query.replaceRange,
              candidates: uniqueByLabel(candidates.sort(compareCandidates)),
            };
          });
          if (result.candidates.length > 0) results.push(result);
        }
      }
      if (results.length === 0) return false;

      const groups = new Map();
      for (const result of results) {
        const key = JSON.stringify(result.replaceRange);
        groups.set(key, [...(groups.get(key) ?? []), result]);
      }
      const order = new Map(sourceDescriptors.map((descriptor, index) => [descriptor.id, index]));
      const selected = [...groups.values()].map((group) => ({
        replaceRange: group[0].replaceRange,
        results: group,
        order: Math.min(...group.map((result) => order.get(result.sourceId) ?? Number.POSITIVE_INFINITY)),
      })).sort((left, right) => left.order - right.order)[0];
      let candidates = selected.results.flatMap((result) => result.candidates);
      candidates = uniqueByLabel(candidates.sort(compareCandidates)).slice(0, maxItems);
      if (candidates.length === 0) return false;
      return await saya.completion.show({
        sessionId: "buffer:" + buffer.id + ":" + buffer.cursorRow + ":" + buffer.cursorCol,
        requestId: globalThis[requestIdKey]++,
        replaceRange: selected.replaceRange,
        candidates,
        selectedIndex: 0,
        ...(keys === undefined ? {} : { keys }),
      });
    };
  `;
  return new Function(source)();
}

function createBundledCompletionAutoTriggerCallback(
  options: BundledCompletionAutoTriggerOptions,
) {
  const commandName = JSON.stringify(options.commandName);
  const autoTriggerDelayMs = JSON.stringify(options.autoTriggerDelayMs);
  const source = `
    return async function sayaCompletionAutoTrigger(payload) {
      const commandName = ${commandName};
      const autoTriggerDelayMs = ${autoTriggerDelayMs};
      const timerKey = "__sayaCompletionAutoTriggerTimer:" + commandName;
      const generationKey = "__sayaCompletionAutoTriggerGeneration:" + commandName;
      const triggerReasonKey = "__sayaCompletionTriggerReason";
      const triggerCharacter = (payload) => {
        const buffer = payload && typeof payload === "object" ? Reflect.get(payload, "buffer") : null;
        if (!buffer || typeof buffer !== "object") return undefined;
        const line = String(Reflect.get(buffer, "currentLine") ?? "");
        const cursor = Math.max(0, Math.min(Number(Reflect.get(buffer, "cursorCol")) || 0, line.length));
        return cursor > 0 ? line[cursor - 1] : undefined;
      };
      const closeCompletionMenu = async () => {
        try {
          const completionSurface = Reflect.get(saya, "completion");
          if (!completionSurface || typeof completionSurface !== "object") return;
          const close = Reflect.get(completionSurface, "close");
          if (typeof close !== "function") return;
          await close.call(completionSurface);
        } catch (error) {
          console.debug("[saya-completion] failed to close stale completion menu: " + String(error));
        }
      };
      const start = async () => {
        globalThis[generationKey] = Number.isFinite(Number(globalThis[generationKey]))
          ? Number(globalThis[generationKey]) + 1
          : 1;
        const generation = globalThis[generationKey];
        const editor = await saya.editor.current();
        if (String(editor?.mode ?? "").toLowerCase() !== "insert") {
          if (globalThis[generationKey] === generation) {
            await closeCompletionMenu();
          }
          return false;
        }
        globalThis[triggerReasonKey] = {
          kind: "auto",
          character: triggerCharacter(payload),
        };
        const shown = await saya.commands.execute(commandName);
        if (!shown && globalThis[generationKey] === generation) {
          await closeCompletionMenu();
        }
        return shown;
      };
      if (
        !Number.isFinite(autoTriggerDelayMs) ||
        autoTriggerDelayMs <= 0 ||
        typeof setTimeout !== "function" ||
        typeof clearTimeout !== "function"
      ) {
        return await start();
      }
      const existing = globalThis[timerKey];
      if (existing !== undefined) clearTimeout(existing);
      globalThis[timerKey] = setTimeout(() => {
        globalThis[timerKey] = undefined;
        void start();
      }, autoTriggerDelayMs);
      return undefined;
    };
  `;
  return new Function(source)();
}

async function completeWithTimeout(
  source: SayaCompletionSource,
  query: SayaCompletionQuery,
  timeoutMs: number,
): Promise<SayaCompletionSourceResult> {
  if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) {
    return await source.complete(query);
  }
  let timeoutId: ReturnType<typeof setTimeout> | undefined;
  const timeout: Promise<SayaCompletionSourceResult> = new Promise(
    (resolve) => {
      timeoutId = setTimeout(() => {
        console.debug(`[saya-completion] source timed out: ${source.id}`);
        resolve({
          sourceId: query.sourceId,
          prefix: query.prefix,
          replaceRange: query.replaceRange,
          candidates: [],
        });
      }, timeoutMs);
    },
  );
  try {
    return await Promise.race([
      Promise.resolve(source.complete(query)),
      timeout,
    ]);
  } finally {
    if (timeoutId !== undefined) clearTimeout(timeoutId);
  }
}

function selectResultGroup(
  results: SayaCompletionSourceResult[],
  sources: SayaCompletionSource[],
): {
  replaceRange: SayaCompletionSourceResult["replaceRange"];
  results: SayaCompletionSourceResult[];
} {
  const groups: Map<string, SayaCompletionSourceResult[]> = new Map();
  for (const result of results) {
    const key = JSON.stringify(result.replaceRange);
    groups.set(key, [...(groups.get(key) ?? []), result]);
  }
  const sourceOrder = new Map(
    sources.map((source, index) => [source.id, index]),
  );
  return [...groups.values()]
    .map((group) => ({
      replaceRange: group[0].replaceRange,
      results: group,
      order: Math.min(
        ...group.map((result) =>
          sourceOrder.get(result.sourceId) ?? Number.POSITIVE_INFINITY
        ),
      ),
    }))
    .sort((left, right) => left.order - right.order)[0];
}

export {
  createPathCompletionSource,
  detectPathCompletionPrefix,
  resolvePathCompletionDirectory,
} from "./sources/path.ts";
export { createBufferWordSource } from "./sources/buffer.ts";
export { createLspCompletionSource } from "./sources/lsp.ts";

export type {
  SayaBufferWordSourceOptions,
  SayaCompletionCandidate,
  SayaCompletionOptions,
  SayaCompletionQuery,
  SayaCompletionRange,
  SayaCompletionSource,
  SayaCompletionSourceResult,
  SayaCompletionTriggerContext,
  SayaCompletionTriggerReason,
  SayaLspCompletionSourceOptions,
  SayaPathCompletionSourceOptions,
} from "./types.ts";
export type { SayaPathCompletionPrefix } from "./sources/path.ts";
