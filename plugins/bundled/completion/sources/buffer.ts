import {
  compareCandidates,
  setCandidateRank,
  uniqueByLabel,
} from "../candidates.ts";
import type {
  SayaCompletionCandidate,
  SayaCompletionQuery,
  SayaCompletionRange,
  SayaCompletionSource,
  SayaCompletionSourceResult,
  SayaCompletionTriggerContext,
  SayaReadonlyBufferSnapshot,
} from "../types.ts";

export function wordPrefix(
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

function offsetFromPosition(
  text: string,
  row: number,
  col: number,
): number {
  const targetRow = Math.max(0, Number(row) || 0);
  const targetCol = Math.max(0, Number(col) || 0);
  let offset = 0;
  let currentRow = 0;

  while (currentRow < targetRow && offset < text.length) {
    const nextLine = text.indexOf("\n", offset);
    if (nextLine === -1) return text.length;
    offset = nextLine + 1;
    currentRow += 1;
  }

  const lineEnd = text.indexOf("\n", offset);
  const maxCol = (lineEnd === -1 ? text.length : lineEnd) - offset;
  return offset + Math.min(targetCol, Math.max(0, maxCol));
}

function distanceToCursor(start: number, end: number, cursor: number): number {
  if (cursor < start) return start - cursor;
  if (cursor > end) return cursor - end;
  return 0;
}

export function createBufferWordSource(): SayaCompletionSource {
  return {
    id: "buffer",
    trigger(context: SayaCompletionTriggerContext): SayaCompletionQuery | null {
      const prefixInfo = wordPrefix(context.buffer);
      return {
        ...context,
        sourceId: "buffer",
        prefix: prefixInfo.prefix,
        replaceRange: prefixInfo.range,
      };
    },
    complete(query: SayaCompletionQuery): SayaCompletionSourceResult {
      const text = String(query.buffer.text ?? "");
      const cursor = offsetFromPosition(
        text,
        query.buffer.cursorRow,
        query.buffer.cursorCol,
      );
      const candidates: SayaCompletionCandidate[] = [];
      const words = text.matchAll(/[A-Za-z_][A-Za-z0-9_]*/g);
      for (const word of words) {
        const label = word[0];
        if (!label.toLowerCase().startsWith(query.prefix.toLowerCase())) {
          continue;
        }
        if (label.toLowerCase() === query.prefix.toLowerCase()) continue;
        const start = word.index ?? 0;
        const end = start + label.length;
        candidates.push(setCandidateRank({
          label,
          insertText: label,
          kind: "Text",
          source: query.sourceId,
        }, { distance: distanceToCursor(start, end, cursor) }));
      }
      return {
        sourceId: query.sourceId,
        prefix: query.prefix,
        replaceRange: query.replaceRange,
        candidates: uniqueByLabel(candidates.sort(compareCandidates)),
      };
    },
  };
}
