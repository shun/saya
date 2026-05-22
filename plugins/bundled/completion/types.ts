export interface SayaReadonlyBufferSnapshot {
  id: number;
  path: string | null;
  lineCount: number;
  cursorRow: number;
  cursorCol: number;
  currentLine: string;
  text: string;
}

export interface SayaReadonlyEditorSnapshot {
  mode: string;
}

export interface SayaCompletionPosition {
  line: number;
  character: number;
}

export interface SayaCompletionRange {
  start: SayaCompletionPosition;
  end: SayaCompletionPosition;
}

export interface SayaCompletionCandidate {
  label: string;
  insertText?: string | null;
  kind?: string | null;
  detail?: string | null;
  documentation?: string[];
  source?: string | null;
}

export interface SayaCompletionContext {
  buffer: SayaReadonlyBufferSnapshot;
  editor: SayaReadonlyEditorSnapshot;
  prefix: string;
  replaceRange: SayaCompletionRange;
}

export interface SayaCompletionSource {
  name: string;
  complete(
    context: SayaCompletionContext,
  ): Promise<SayaCompletionCandidate[]> | SayaCompletionCandidate[];
}

export type SayaCompletionFilter = (
  candidates: SayaCompletionCandidate[],
  context: SayaCompletionContext,
) => SayaCompletionCandidate[];

export type SayaCompletionSorter = (
  candidates: SayaCompletionCandidate[],
  context: SayaCompletionContext,
) => SayaCompletionCandidate[];

export interface SayaCompletionOptions {
  commandName?: string;
  key?: string;
  minPrefixLength?: number;
  maxItems?: number;
  sourceTimeoutMs?: number;
  sources?: SayaCompletionSource[];
  filters?: SayaCompletionFilter[];
  sorters?: SayaCompletionSorter[];
}

export interface SayaLspCompletionSourceOptions {
  commandName?: string;
  sourceName?: string;
  optional?: boolean;
}
