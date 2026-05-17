export type SayaPluginSource =
  | { kind: "local"; path: string }
  | { kind: "git"; url: string; rev?: string }
  | { kind: "github"; repo: `${string}/${string}`; rev?: string };

export interface SayaPluginLazySpec {
  commands?: string[];
  events?: string[];
  filetypes?: string[];
}

export interface SayaBundledPluginManifest {
  version: 1;
  name: string;
  module: string;
  setup?: string;
  lazy?: SayaPluginLazySpec;
  depends?: string[];
  before?: string[];
  after?: string[];
}

export interface SayaPluginSpec {
  name: string;
  source: SayaPluginSource | string;
  module?: string;
  setup?: string;
  lazy?: SayaPluginLazySpec;
  depends?: string[];
  before?: string[];
  after?: string[];
  protocols?: Record<string, unknown>;
}

export interface SayaPluginOperationContext {
  cacheRoot: string;
  sourceHash: string;
  bundled?: SayaBundledPluginManifest[];
  writeTextFile?: (path: string, text: string) => Promise<void> | void;
  removeFile?: (path: string) => Promise<void> | void;
  ensureDir?: (path: string) => Promise<void> | void;
  runCommand?: (
    command: string,
    args: string[],
    options?: { cwd?: string },
  ) => Promise<void> | void;
}
