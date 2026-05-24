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

export interface SayaCompletionTriggerContext {
  buffer: SayaReadonlyBufferSnapshot;
  editor: SayaReadonlyEditorSnapshot;
  reason: SayaCompletionTriggerReason;
}

export type SayaCompletionTriggerReason =
  | { kind: "manual" }
  | { kind: "auto"; character?: string };

export interface SayaCompletionQuery extends SayaCompletionTriggerContext {
  sourceId: string;
  prefix: string;
  replaceRange: SayaCompletionRange;
}

export interface SayaCompletionSourceResult {
  sourceId: string;
  prefix: string;
  replaceRange: SayaCompletionRange;
  candidates: SayaCompletionCandidate[];
}

export interface SayaCompletionSource {
  id: string;
  minPrefixLength?: number;
  triggerCharacters?: string[];
  __sayaBundledSource?: Record<string, unknown>;
  trigger(context: SayaCompletionTriggerContext): SayaCompletionQuery | null;
  complete(
    query: SayaCompletionQuery,
  ): Promise<SayaCompletionSourceResult> | SayaCompletionSourceResult;
}

export type SayaCompletionFilter = (
  result: SayaCompletionSourceResult,
  query: SayaCompletionQuery,
) => SayaCompletionSourceResult;

export type SayaCompletionSorter = (
  candidates: SayaCompletionCandidate[],
  result: SayaCompletionSourceResult,
) => SayaCompletionCandidate[];

export interface SayaCompletionKeyBindings {
  confirm?: string[];
  close?: string[];
  next?: string[];
  previous?: string[];
  pageNext?: string[];
  pagePrevious?: string[];
}

export interface SayaCompletionOptions {
  commandName?: string;
  key?: string;
  keys?: SayaCompletionKeyBindings;
  minPrefixLength?: number;
  maxItems?: number;
  sourceTimeoutMs?: number;
  autoTrigger?: boolean;
  autoTriggerDelayMs?: number;
  sources?: SayaCompletionSource[];
  filters?: SayaCompletionFilter[];
  sorters?: SayaCompletionSorter[];
}

export interface SayaLspCompletionSourceOptions {
  commandName?: string;
  sourceName?: string;
  optional?: boolean;
  minPrefixLength?: number;
  triggerCharacters?: string[];
}

export interface SayaPathCompletionSourceOptions {
  sourceName?: string;
  optional?: boolean;
  maxItems?: number;
  showHidden?: boolean;
  minPrefixLength?: number;
  triggerCharacters?: string[];
}

export interface SayaBufferWordSourceOptions {
  sourceName?: string;
  minPrefixLength?: number;
  triggerCharacters?: string[];
}
