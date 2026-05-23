import { compareCandidates, uniqueByLabel } from "./candidates.ts";
import { createBufferWordSource } from "./sources/buffer.ts";
import { createLspCompletionSource } from "./sources/lsp.ts";
import { createPathCompletionSource } from "./sources/path.ts";
import type {
  SayaCompletionCandidate,
  SayaCompletionOptions,
  SayaCompletionQuery,
  SayaCompletionSource,
  SayaCompletionSourceResult,
  SayaCompletionTriggerContext,
} from "./types.ts";

declare const saya: any;

let nextCompletionRequestId = 1;

interface BundledCompletionRuntimeOptions {
  minPrefixLength: number;
  maxItems: number;
  sourceTimeoutMs: number;
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
  const key = options.key ?? "<C-Space>";
  const minPrefixLength = options.minPrefixLength ?? 1;
  const maxItems = options.maxItems ?? 50;
  const sourceTimeoutMs = options.sourceTimeoutMs ?? 1000;
  const usesCustomPipeline = options.sources != null ||
    options.filters != null || options.sorters != null;
  if (!usesCustomPipeline) {
    const run = createBundledCompletionRuntimeCommand({
      minPrefixLength,
      maxItems,
      sourceTimeoutMs,
    });
    saya.commands.register(commandName, run);
    saya.keymap.set("insert", key, saya.commands.execute(commandName));
    return { commandName, key };
  }

  const sources = options.sources ??
    [
      createLspCompletionSource(),
      createPathCompletionSource({ maxItems }),
      createBufferWordSource(),
    ];
  const filters = options.filters ?? [prefixFilter];
  const sorters = options.sorters ?? [labelSorter];

  const run = async () => {
    const buffer = await saya.buffer.current();
    const editor = await saya.editor.current();
    const triggerContext: SayaCompletionTriggerContext = {
      buffer,
      editor,
    };
    const queries = sources.flatMap((source) => {
      const query = source.trigger(triggerContext);
      return query != null && query.prefix.length >= minPrefixLength
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
    });
  };

  saya.commands.register(commandName, run);
  saya.keymap.set("insert", key, saya.commands.execute(commandName));
  return { commandName, key };
}

function createBundledCompletionRuntimeCommand(
  options: BundledCompletionRuntimeOptions,
) {
  const minPrefixLength = JSON.stringify(options.minPrefixLength);
  const maxItems = JSON.stringify(options.maxItems);
  const sourceTimeoutMs = JSON.stringify(options.sourceTimeoutMs);
  const source = `
    return async function sayaCompletionTrigger() {
      const minPrefixLength = ${minPrefixLength};
      const maxItems = ${maxItems};
      const sourceTimeoutMs = ${sourceTimeoutMs};
      const requestIdKey = "__sayaCompletionNextRequestId";
      globalThis[requestIdKey] = Number.isFinite(Number(globalThis[requestIdKey]))
        ? Number(globalThis[requestIdKey])
        : 1;

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
      void editor;
      const bufferPrefix = wordPrefix(buffer);
      const pathInfo = pathPrefix(buffer);
      const queries = [];
      const lspQuery = { sourceId: "lsp", prefix: bufferPrefix.prefix, replaceRange: bufferPrefix.range };
      if (lspQuery.prefix.length >= minPrefixLength) queries.push(lspQuery);
      if (pathInfo && pathInfo.prefix.length >= minPrefixLength) {
        queries.push({ sourceId: "path", prefix: pathInfo.prefix, replaceRange: pathInfo.range, pathInfo });
      }
      if (bufferPrefix.prefix.length >= minPrefixLength) {
        queries.push({ sourceId: "buffer", prefix: bufferPrefix.prefix, replaceRange: bufferPrefix.range });
      }
      if (queries.length === 0) return false;

      const results = [];
      for (const query of queries) {
        if (query.sourceId === "lsp") {
          const result = await withTimeout("lsp", query.prefix, query.replaceRange, async () => {
            try {
              const response = await saya.commands.execute("lsp.completion");
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
                sourceId: "lsp",
                prefix: query.prefix,
                replaceRange,
                candidates: uniqueByLabel(items.map(normalizeLspItem).filter(Boolean)),
              };
            } catch (error) {
              console.debug("[saya-completion] optional LSP source skipped: " + String(error));
              return { sourceId: "lsp", prefix: query.prefix, replaceRange: query.replaceRange, candidates: [] };
            }
          });
          if (result.candidates.length > 0) results.push(result);
        } else if (query.sourceId === "path") {
          const result = await withTimeout("path", query.prefix, query.replaceRange, async () => {
            try {
              const entries = await saya.fs.readDir(pathDirectory(buffer, query.pathInfo), {
                showHidden: true,
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
                  source: "path",
                }, { distance: 0, kindRank: pathKindRank(kind) }));
              }
              return {
                sourceId: "path",
                prefix: query.prefix,
                replaceRange: query.replaceRange,
                candidates: uniqueByLabel(candidates.sort(comparePathCandidates)).slice(0, maxItems),
              };
            } catch (error) {
              console.debug("[saya-completion] optional path source skipped: " + String(error));
              return { sourceId: "path", prefix: query.prefix, replaceRange: query.replaceRange, candidates: [] };
            }
          });
          if (result.candidates.length > 0) results.push(result);
        } else {
          const result = await withTimeout("buffer", query.prefix, query.replaceRange, async () => {
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
                source: "buffer",
              }, { distance, kindRank: 2 }));
            }
            return {
              sourceId: "buffer",
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
      const order = new Map([["lsp", 0], ["path", 1], ["buffer", 2]]);
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
      });
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
  SayaCompletionCandidate,
  SayaCompletionOptions,
  SayaCompletionQuery,
  SayaCompletionRange,
  SayaCompletionSource,
  SayaCompletionSourceResult,
  SayaCompletionTriggerContext,
  SayaLspCompletionSourceOptions,
  SayaPathCompletionSourceOptions,
} from "./types.ts";
export type { SayaPathCompletionPrefix } from "./sources/path.ts";
