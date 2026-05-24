import { setCandidateRank, uniqueByLabel } from "../candidates.ts";
import type {
  SayaCompletionCandidate,
  SayaCompletionQuery,
  SayaCompletionRange,
  SayaCompletionSource,
  SayaCompletionSourceResult,
  SayaCompletionTriggerContext,
  SayaPathCompletionSourceOptions,
  SayaReadonlyBufferSnapshot,
} from "../types.ts";

declare const saya: any;

export interface SayaPathCompletionPrefix {
  prefix: string;
  range: SayaCompletionRange;
  directoryPrefix: string;
  entryPrefix: string;
}

export function pathPrefix(
  buffer: SayaReadonlyBufferSnapshot,
): SayaPathCompletionPrefix | null {
  const line = String(buffer.currentLine ?? "");
  const cursor = Math.max(
    0,
    Math.min(Number(buffer.cursorCol) || 0, line.length),
  );
  const before = line.slice(0, cursor);
  let start = before.length;
  while (start > 0 && !isPathPrefixBoundary(before[start - 1])) {
    start -= 1;
  }
  const prefix = before.slice(start);
  if (!isPathLikePrefix(prefix)) return null;
  const slash = prefix.lastIndexOf("/");
  const directoryPrefix = slash >= 0 ? prefix.slice(0, slash + 1) : "";
  const entryPrefix = slash >= 0 ? prefix.slice(slash + 1) : prefix;
  return {
    prefix,
    range: {
      start: {
        line: Number(buffer.cursorRow) || 0,
        character: start,
      },
      end: { line: Number(buffer.cursorRow) || 0, character: cursor },
    },
    directoryPrefix,
    entryPrefix,
  };
}

function isPathPrefixBoundary(ch: string): boolean {
  return ch.trim() === "" || "\"'`<>()[]{}".includes(ch);
}

function isPathLikePrefix(prefix: string): boolean {
  if (!prefix) return false;
  return prefix.startsWith("/") ||
    prefix.startsWith("./") ||
    prefix.startsWith("../") ||
    prefix.includes("/");
}

function dirname(path: string | null | undefined): string {
  const value = String(path ?? "");
  if (!value) return ".";
  const trimmed = value.replace(/\/+$/, "");
  const slash = trimmed.lastIndexOf("/");
  if (slash < 0) return ".";
  if (slash === 0) return "/";
  return trimmed.slice(0, slash);
}

function normalizePath(path: string): string {
  const absolute = path.startsWith("/");
  const parts: string[] = [];
  for (const part of path.split("/")) {
    if (!part || part === ".") continue;
    if (part === "..") {
      if (parts.length > 0 && parts[parts.length - 1] !== "..") {
        parts.pop();
      } else if (!absolute) {
        parts.push(part);
      }
      continue;
    }
    parts.push(part);
  }
  const normalized = parts.join("/");
  if (absolute) return `/${normalized}`.replace(/\/$/, "") || "/";
  return normalized || ".";
}

function joinPath(base: string, child: string): string {
  if (!child || child === ".") return normalizePath(base || ".");
  if (child.startsWith("/")) return normalizePath(child);
  if (!base || base === ".") return normalizePath(child);
  if (base === "/") return normalizePath(`/${child}`);
  return normalizePath(`${base.replace(/\/+$/, "")}/${child}`);
}

function pathCompletionListDirectory(
  buffer: SayaReadonlyBufferSnapshot,
  prefix: SayaPathCompletionPrefix,
): string {
  if (prefix.directoryPrefix.startsWith("/")) {
    return normalizePath(prefix.directoryPrefix);
  }
  return joinPath(dirname(buffer.path), prefix.directoryPrefix || ".");
}

function pathCandidateLabel(
  directoryPrefix: string,
  name: string,
  isDirectory: boolean,
): string {
  return `${directoryPrefix}${name}${isDirectory ? "/" : ""}`;
}

function pathKindRank(kind: string): number {
  if (kind === "directory") return 0;
  if (kind === "file") return 1;
  return 2;
}

function comparePathCandidates(
  left: SayaCompletionCandidate,
  right: SayaCompletionCandidate,
): number {
  const leftRank = pathKindRank(String(left.detail ?? "").toLowerCase());
  const rightRank = pathKindRank(String(right.detail ?? "").toLowerCase());
  if (leftRank !== rightRank) return leftRank - rightRank;
  if (left.label.length !== right.label.length) {
    return left.label.length - right.label.length;
  }
  return left.label.localeCompare(right.label);
}

function entryField(entry: unknown, key: string): unknown {
  return entry && typeof entry === "object" ? Reflect.get(entry, key) : null;
}

export function detectPathCompletionPrefix(
  buffer: SayaReadonlyBufferSnapshot,
): SayaPathCompletionPrefix | null {
  return pathPrefix(buffer);
}

export function resolvePathCompletionDirectory(
  buffer: SayaReadonlyBufferSnapshot,
  prefix: SayaPathCompletionPrefix,
): string {
  return pathCompletionListDirectory(buffer, prefix);
}

export function createPathCompletionSource(
  options: SayaPathCompletionSourceOptions = {},
): SayaCompletionSource {
  const sourceName = options.sourceName ?? "path";
  const optional = options.optional ?? true;
  const maxItems = options.maxItems ?? Number.POSITIVE_INFINITY;
  const showHidden = options.showHidden ?? true;
  return {
    id: sourceName,
    minPrefixLength: options.minPrefixLength,
    triggerCharacters: options.triggerCharacters,
    __sayaBundledSource: {
      kind: "path",
      id: sourceName,
      optional,
      maxItems: Number.isFinite(maxItems) ? maxItems : null,
      showHidden,
      minPrefixLength: options.minPrefixLength,
      triggerCharacters: options.triggerCharacters,
    },
    trigger(context: SayaCompletionTriggerContext): SayaCompletionQuery | null {
      const prefixInfo = pathPrefix(context.buffer);
      if (!prefixInfo) return null;
      return {
        ...context,
        sourceId: sourceName,
        prefix: prefixInfo.prefix,
        replaceRange: prefixInfo.range,
      };
    },
    async complete(
      query: SayaCompletionQuery,
    ): Promise<SayaCompletionSourceResult> {
      const prefixInfo = pathPrefix(query.buffer);
      if (!prefixInfo) {
        return {
          sourceId: query.sourceId,
          prefix: query.prefix,
          replaceRange: query.replaceRange,
          candidates: [],
        };
      }
      try {
        const listDirectory = pathCompletionListDirectory(
          query.buffer,
          prefixInfo,
        );
        const entries = await saya.fs.readDir(listDirectory, {
          showHidden,
          sortBy: "kind",
        });
        const entryPrefix = prefixInfo.entryPrefix.toLowerCase();
        const candidates: SayaCompletionCandidate[] = [];
        for (const entry of Array.isArray(entries) ? entries : []) {
          if (!entry || typeof entry !== "object") continue;
          const name = String(entryField(entry, "name") ?? "");
          const kind = String(entryField(entry, "kind") ?? "file")
            .toLowerCase();
          if (!name || !name.toLowerCase().startsWith(entryPrefix)) {
            continue;
          }
          const isDirectory = kind === "directory";
          const label = pathCandidateLabel(
            prefixInfo.directoryPrefix,
            name,
            isDirectory,
          );
          if (label.toLowerCase() === prefixInfo.prefix.toLowerCase()) {
            continue;
          }
          candidates.push(setCandidateRank({
            label,
            insertText: label,
            kind: isDirectory ? "Folder" : "File",
            detail: isDirectory ? "directory" : kind,
            source: query.sourceId,
          }, { distance: 0, kindRank: pathKindRank(kind) }));
        }
        return {
          sourceId: query.sourceId,
          prefix: query.prefix,
          replaceRange: query.replaceRange,
          candidates: uniqueByLabel(candidates.sort(comparePathCandidates))
            .slice(
              0,
              maxItems,
            ),
        };
      } catch (error) {
        if (!optional) throw error;
        console.debug(
          `[saya-completion] optional path source skipped: ${String(error)}`,
        );
        return {
          sourceId: query.sourceId,
          prefix: query.prefix,
          replaceRange: query.replaceRange,
          candidates: [],
        };
      }
    },
  };
}
