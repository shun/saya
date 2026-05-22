import type {
  SayaCompletionCandidate,
  SayaCompletionContext,
  SayaCompletionOptions,
  SayaCompletionRange,
  SayaCompletionSource,
  SayaLspCompletionSourceOptions,
  SayaReadonlyBufferSnapshot,
} from "./types.ts";

declare const saya: any;

let nextCompletionRequestId = 1;

function wordPrefix(
  buffer: SayaReadonlyBufferSnapshot,
): { prefix: string; range: SayaCompletionRange } {
  const line = String(buffer.currentLine ?? "");
  const cursor = Math.max(
    0,
    Math.min(Number(buffer.cursorCol) || 0, line.length),
  );
  const before = line.slice(0, cursor);
  const match = before.match(/[A-Za-z0-9_]+$/);
  const prefix = match ? match[0] : "";
  return {
    prefix,
    range: {
      start: {
        line: Number(buffer.cursorRow) || 0,
        character: cursor - prefix.length,
      },
      end: { line: Number(buffer.cursorRow) || 0, character: cursor },
    },
  };
}

function uniqueByLabel(
  candidates: SayaCompletionCandidate[],
): SayaCompletionCandidate[] {
  const seen = new Set<string>();
  const result: SayaCompletionCandidate[] = [];
  for (const candidate of candidates) {
    const label = String(candidate.label ?? "").trim();
    if (!label || seen.has(label)) continue;
    seen.add(label);
    result.push({ ...candidate, label });
  }
  return result;
}

export function createBufferWordSource(): SayaCompletionSource {
  return {
    name: "buffer",
    complete(context: SayaCompletionContext): SayaCompletionCandidate[] {
      const words =
        String(context.buffer.text ?? "").match(/[A-Za-z_][A-Za-z0-9_]*/g) ??
          [];
      return uniqueByLabel(words.map((label) => ({
        label,
        insertText: label,
        kind: "Text",
        source: "buffer",
      })));
    },
  };
}

function normalizeDocumentation(documentation: unknown): string[] {
  if (typeof documentation === "string") {
    return documentation.split(/\r\n|\r|\n/).filter((line) =>
      line.trim().length > 0
    );
  }
  if (
    documentation && typeof documentation === "object" &&
    "value" in documentation
  ) {
    return normalizeDocumentation((documentation as { value?: unknown }).value);
  }
  return [];
}

function normalizeCompletionItem(
  item: unknown,
  sourceName: string,
): SayaCompletionCandidate | null {
  if (!item || typeof item !== "object") return null;
  const record = item as Record<string, unknown>;
  const label = String(record.label ?? "").trim();
  if (!label) return null;
  const textEdit = record.textEdit && typeof record.textEdit === "object"
    ? record.textEdit as Record<string, unknown>
    : null;
  const insertText = record.insertText ?? textEdit?.newText ?? label;
  return {
    label,
    insertText: typeof insertText === "string" ? insertText : label,
    kind: record.kind == null ? null : String(record.kind),
    detail: typeof record.detail === "string" ? record.detail : null,
    documentation: normalizeDocumentation(record.documentation),
    source: sourceName,
  };
}

function completionItemsFromResponse(response: unknown): unknown[] {
  const result =
    response && typeof response === "object" && "result" in response
      ? (response as { result?: unknown }).result
      : response;
  if (Array.isArray(result)) return result;
  if (
    result && typeof result === "object" &&
    Array.isArray((result as { items?: unknown }).items)
  ) {
    return (result as { items: unknown[] }).items;
  }
  return [];
}

function lspRangeFromItem(item: unknown): SayaCompletionRange | null {
  if (!item || typeof item !== "object") return null;
  const textEdit = (item as { textEdit?: unknown }).textEdit;
  if (!textEdit || typeof textEdit !== "object") return null;
  const range = (textEdit as { range?: unknown }).range;
  if (!range || typeof range !== "object") return null;
  const start = (range as { start?: unknown }).start;
  const end = (range as { end?: unknown }).end;
  if (!start || !end || typeof start !== "object" || typeof end !== "object") {
    return null;
  }
  const startRecord = start as Record<string, unknown>;
  const endRecord = end as Record<string, unknown>;
  const startLine = Number(startRecord.line);
  const startCharacter = Number(startRecord.character);
  const endLine = Number(endRecord.line);
  const endCharacter = Number(endRecord.character);
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
  const first = items.map(lspRangeFromItem).find((
    range,
  ): range is SayaCompletionRange => range != null);
  return first ?? fallback;
}

export function createLspCompletionSource(
  options: SayaLspCompletionSourceOptions = {},
): SayaCompletionSource {
  const commandName = options.commandName ?? "lsp.completion";
  const sourceName = options.sourceName ?? "lsp";
  const optional = options.optional ?? true;
  return {
    name: sourceName,
    async complete(
      context: SayaCompletionContext,
    ): Promise<SayaCompletionCandidate[]> {
      try {
        const response = await saya.commands.execute(commandName);
        const items = completionItemsFromResponse(response);
        context.replaceRange = chooseLspReplaceRange(
          items,
          context.replaceRange,
        );
        return uniqueByLabel(
          items
            .map((item) => normalizeCompletionItem(item, sourceName))
            .filter((candidate): candidate is SayaCompletionCandidate =>
              candidate != null
            ),
        );
      } catch (error) {
        if (!optional) throw error;
        console.debug(
          `[saya-completion] optional LSP source skipped: ${String(error)}`,
        );
        return [];
      }
    },
  };
}

export function prefixFilter(
  candidates: SayaCompletionCandidate[],
  context: SayaCompletionContext,
): SayaCompletionCandidate[] {
  const prefix = context.prefix.toLowerCase();
  if (!prefix) return candidates;
  return candidates.filter((candidate) =>
    candidate.label.toLowerCase().startsWith(prefix)
  );
}

export function labelSorter(
  candidates: SayaCompletionCandidate[],
): SayaCompletionCandidate[] {
  return [...candidates].sort((a, b) => a.label.localeCompare(b.label));
}

export async function setupSayaCompletion(options: SayaCompletionOptions = {}) {
  const commandName = options.commandName ?? "completion.trigger";
  const key = options.key ?? "<C-Space>";
  const minPrefixLength = options.minPrefixLength ?? 1;
  const maxItems = options.maxItems ?? 50;
  const sourceTimeoutMs = options.sourceTimeoutMs ?? 1000;
  const sources = options.sources ??
    [createLspCompletionSource(), createBufferWordSource()];
  const filters = options.filters ?? [prefixFilter];
  const sorters = options.sorters ?? [labelSorter];

  const run = async () => {
    const buffer = await saya.buffer.current();
    const editor = await saya.editor.current();
    const prefixInfo = wordPrefix(buffer);
    if (prefixInfo.prefix.length < minPrefixLength) return false;
    const context: SayaCompletionContext = {
      buffer,
      editor,
      prefix: prefixInfo.prefix,
      replaceRange: prefixInfo.range,
    };
    let candidates = (await Promise.all(
      sources.map((source) =>
        completeWithTimeout(source, context, sourceTimeoutMs)
      ),
    )).flat();
    for (const filter of filters) candidates = filter(candidates, context);
    for (const sorter of sorters) candidates = sorter(candidates, context);
    candidates = uniqueByLabel(candidates).slice(0, maxItems);
    if (candidates.length === 0) return false;
    return await saya.completion.show({
      sessionId: `buffer:${buffer.id}:${buffer.cursorRow}:${buffer.cursorCol}`,
      requestId: nextCompletionRequestId++,
      replaceRange: context.replaceRange,
      candidates,
      selectedIndex: 0,
    });
  };

  saya.commands.register(commandName, run);
  saya.keymap.set("insert", key, saya.commands.execute(commandName));
  return { commandName, key };
}

async function completeWithTimeout(
  source: SayaCompletionSource,
  context: SayaCompletionContext,
  timeoutMs: number,
): Promise<SayaCompletionCandidate[]> {
  if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) {
    return await source.complete(context);
  }
  let timeoutId: number | undefined;
  const timeout = new Promise<SayaCompletionCandidate[]>((resolve) => {
    timeoutId = setTimeout(() => {
      console.debug(`[saya-completion] source timed out: ${source.name}`);
      resolve([]);
    }, timeoutMs);
  });
  try {
    return await Promise.race([
      Promise.resolve(source.complete(context)),
      timeout,
    ]);
  } finally {
    if (timeoutId !== undefined) clearTimeout(timeoutId);
  }
}

export type {
  SayaCompletionCandidate,
  SayaCompletionContext,
  SayaCompletionOptions,
  SayaCompletionRange,
  SayaCompletionSource,
  SayaLspCompletionSourceOptions,
} from "./types.ts";
