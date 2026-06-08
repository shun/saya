import { uniqueByLabel } from "../candidates.ts";
import type {
  SayaCompletionCandidate,
  SayaCompletionQuery,
  SayaCompletionRange,
  SayaCompletionSource,
  SayaCompletionSourceResult,
  SayaCompletionTriggerContext,
  SayaLspCompletionSourceOptions,
} from "../types.ts";
import { wordPrefix } from "./buffer.ts";

declare const saya: any;

function field(value: unknown, key: string): unknown {
  return value && typeof value === "object" ? Reflect.get(value, key) : null;
}

function normalizeDocumentation(documentation: unknown): string[] {
  if (typeof documentation === "string") {
    return documentation.split(/\r\n|\r|\n/).filter((line) =>
      line.trim().length > 0
    );
  }
  if (documentation && typeof documentation === "object") {
    return normalizeDocumentation(field(documentation, "value"));
  }
  return [];
}

function completionItemKindName(kind: unknown): string | null {
  if (kind == null) return null;
  const names: Record<number, string> = {
    1: "Text",
    2: "Method",
    3: "Function",
    4: "Constructor",
    5: "Field",
    6: "Variable",
    7: "Class",
    8: "Interface",
    9: "Module",
    10: "Property",
    11: "Unit",
    12: "Value",
    13: "Enum",
    14: "Keyword",
    15: "Snippet",
    16: "Color",
    17: "File",
    18: "Reference",
    19: "Folder",
    20: "EnumMember",
    21: "Constant",
    22: "Struct",
    23: "Event",
    24: "Operator",
    25: "TypeParameter",
  };
  const numeric = Number(kind);
  if (Number.isInteger(numeric) && names[numeric]) return names[numeric];
  const text = String(kind).trim();
  return text.length > 0 ? text : null;
}

function normalizeCompletionItem(
  item: unknown,
  sourceName: string,
): SayaCompletionCandidate | null {
  if (!item || typeof item !== "object") return null;
  const label = String(field(item, "label") ?? "").trim();
  if (!label) return null;
  const textEditValue = field(item, "textEdit");
  const textEdit = textEditValue && typeof textEditValue === "object"
    ? textEditValue
    : null;
  const insertText = field(item, "insertText") ??
    field(textEdit, "newText") ?? label;
  return {
    label,
    insertText: typeof insertText === "string" ? insertText : label,
    kind: completionItemKindName(field(item, "kind")),
    detail: typeof field(item, "detail") === "string"
      ? String(field(item, "detail"))
      : null,
    documentation: normalizeDocumentation(field(item, "documentation")),
    source: sourceName,
  };
}

function isDeepCompletionItem(item: unknown): boolean {
  const label = String(field(item, "label") ?? "");
  return label.includes(".");
}

function completionItemsFromResponse(response: unknown): unknown[] {
  const result = response && typeof response === "object"
    ? field(response, "result")
    : response;
  if (Array.isArray(result)) return result;
  const items = field(result, "items");
  if (Array.isArray(items)) return items;
  return [];
}

function lspRangeFromItem(item: unknown): SayaCompletionRange | null {
  if (!item || typeof item !== "object") return null;
  const textEdit = field(item, "textEdit");
  if (!textEdit || typeof textEdit !== "object") return null;
  const range = field(textEdit, "range");
  if (!range || typeof range !== "object") return null;
  const start = field(range, "start");
  const end = field(range, "end");
  if (!start || !end || typeof start !== "object" || typeof end !== "object") {
    return null;
  }
  const startLine = Number(field(start, "line"));
  const startCharacter = Number(field(start, "character"));
  const endLine = Number(field(end, "line"));
  const endCharacter = Number(field(end, "character"));
  if (
    !Number.isFinite(startLine) || !Number.isFinite(startCharacter) ||
    !Number.isFinite(endLine) || !Number.isFinite(endCharacter)
  ) {
    return null;
  }
  return {
    start: {
      line: Math.max(0, startLine),
      character: Math.max(0, startCharacter),
    },
    end: { line: Math.max(0, endLine), character: Math.max(0, endCharacter) },
  };
}

function chooseLspReplaceRange(
  items: unknown[],
  fallback: SayaCompletionRange,
): SayaCompletionRange {
  for (const item of items) {
    const range = lspRangeFromItem(item);
    if (range) return range;
  }
  return fallback;
}

export function createLspCompletionSource(
  options: SayaLspCompletionSourceOptions = {},
): SayaCompletionSource {
  const commandName = options.commandName ?? "lsp.completion";
  const sourceName = options.sourceName ?? "lsp";
  const optional = options.optional ?? true;
  const includeDeepCompletions = options.includeDeepCompletions ?? true;
  return {
    id: sourceName,
    minPrefixLength: options.minPrefixLength,
    triggerCharacters: options.triggerCharacters,
    __sayaBundledSource: {
      kind: "lsp",
      id: sourceName,
      commandName,
      optional,
      includeDeepCompletions,
      minPrefixLength: options.minPrefixLength,
      triggerCharacters: options.triggerCharacters,
    },
    trigger(context: SayaCompletionTriggerContext): SayaCompletionQuery | null {
      const prefixInfo = wordPrefix(context.buffer);
      return {
        ...context,
        sourceId: sourceName,
        prefix: prefixInfo.prefix,
        replaceRange: prefixInfo.range,
      };
    },
    async complete(query): Promise<SayaCompletionSourceResult> {
      try {
        const response = await saya.commands.execute(commandName);
        const items = completionItemsFromResponse(response).filter((item) =>
          includeDeepCompletions || !isDeepCompletionItem(item)
        );
        const replaceRange = chooseLspReplaceRange(
          items,
          query.replaceRange,
        );
        const normalizedCandidates: SayaCompletionCandidate[] = [];
        for (const item of items) {
          const candidate = normalizeCompletionItem(item, sourceName);
          if (candidate) normalizedCandidates.push(candidate);
        }
        const candidates = uniqueByLabel(normalizedCandidates);
        return {
          sourceId: query.sourceId,
          prefix: query.prefix,
          replaceRange,
          candidates,
        };
      } catch (error) {
        if (!optional) throw error;
        console.debug(
          `[saya-completion] optional LSP source skipped: ${String(error)}`,
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
