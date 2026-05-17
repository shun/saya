export type SayaPluginSource =
  | { kind: "local"; path: string }
  | { kind: "git"; url: string; rev?: string }
  | { kind: "github"; repo: `${string}/${string}`; rev?: string };

export interface SayaPluginLazySpec {
  commands?: string[];
  events?: Array<
    "bufferOpen" | "bufferChanged" | "bufferWritePost" | "bufferClosed" | string
  >;
  filetypes?: string[];
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

export interface SayaPluginUseDeclaration {
  name?: string;
  local?: string;
  github?: `${string}/${string}`;
  rev?: string;
  module?: string;
  setup?: string;
  options?: unknown;
}

export interface SayaPluginLazyDeclaration extends SayaPluginUseDeclaration {
  commands?: string[];
  events?: Array<
    "bufferOpen" | "bufferChanged" | "bufferWritePost" | "bufferClosed" | string
  >;
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

export interface NormalizedPluginSpec {
  class: "bundled" | "external";
  name: string;
  source: SayaPluginSource | string | "bundled";
  module: string;
  setup: string;
  lazy?: SayaPluginLazySpec;
  depends: string[];
  before: string[];
  after: string[];
  protocols: Record<string, unknown>;
}

export function definePlugins(specs: SayaPluginSpec[]): SayaPluginSpec[] {
  const seen = new Set<string>();
  for (const spec of specs) {
    if (!spec.name || spec.name.trim() === "") {
      throw new TypeError("plugin name must be a non-empty string");
    }
    if (seen.has(spec.name)) {
      throw new TypeError(`duplicate plugin name: ${spec.name}`);
    }
    seen.add(spec.name);
  }
  return specs.map((spec) => ({
    ...spec,
    module: spec.module ?? defaultModuleForExternalSpec(spec),
    setup: spec.setup ?? "setup",
    depends: spec.depends ?? [],
    before: spec.before ?? [],
    after: spec.after ?? [],
  }));
}

export function defineUserPlugins(
  useDeclarations: SayaPluginUseDeclaration[] = [],
  lazyDeclarations: SayaPluginLazyDeclaration[] = [],
): SayaPluginSpec[] {
  const merged = new Map<string, SayaPluginSpec>();
  for (const declaration of useDeclarations) {
    mergeUserPluginSpec(merged, userDeclarationToPluginSpec(declaration, false));
  }
  for (const declaration of lazyDeclarations) {
    mergeUserPluginSpec(merged, userDeclarationToPluginSpec(declaration, true));
  }
  return definePlugins([...merged.values()]);
}

export function normalizeExternalPlugins(
  specs: SayaPluginSpec[],
): NormalizedPluginSpec[] {
  return definePlugins(specs).map((spec) => ({
    class: "external",
    name: spec.name,
    source: spec.source,
    module: spec.module ?? defaultModuleForExternalSpec(spec),
    setup: spec.setup ?? "setup",
    lazy: spec.lazy,
    depends: spec.depends ?? [],
    before: spec.before ?? [],
    after: spec.after ?? [],
    protocols: spec.protocols ?? {},
  }));
}

export function normalizeBundledPlugins(
  manifests: SayaBundledPluginManifest[],
): NormalizedPluginSpec[] {
  const seen = new Set<string>();
  return manifests.map((manifest) => {
    if (!manifest.name || manifest.name.trim() === "") {
      throw new TypeError("bundled plugin name must be a non-empty string");
    }
    if (!manifest.module || manifest.module.trim() === "") {
      throw new TypeError(
        `bundled plugin module is required: ${manifest.name}`,
      );
    }
    if (seen.has(manifest.name)) {
      throw new TypeError(`duplicate bundled plugin name: ${manifest.name}`);
    }
    seen.add(manifest.name);
    return {
      class: "bundled",
      name: manifest.name,
      source: "bundled",
      module: manifest.module,
      setup: manifest.setup ?? "setup",
      lazy: manifest.lazy,
      depends: manifest.depends ?? [],
      before: manifest.before ?? [],
      after: manifest.after ?? [],
      protocols: {},
    };
  });
}

export function encodeSource(source: SayaPluginSource | string): string {
  if (typeof source === "string") {
    return source;
  }
  switch (source.kind) {
    case "local":
      return `local:${source.path}`;
    case "git":
      return `git:${source.url}`;
    case "github":
      return `github:${source.repo}`;
  }
}

export function revisionForSource(source: SayaPluginSource | string): string {
  if (typeof source === "string") {
    return "unspecified";
  }
  if (source.kind === "local") {
    return "workspace";
  }
  return source.rev ?? "HEAD";
}

function defaultModuleForExternalSpec(spec: SayaPluginSpec): string {
  if (typeof spec.source === "object" && spec.source.kind === "local") {
    return spec.source.path;
  }
  return `saya-plugin://${spec.name}`;
}

function userDeclarationToPluginSpec(
  declaration: SayaPluginLazyDeclaration,
  lazy: boolean,
): SayaPluginSpec {
  const source = userDeclarationSource(declaration);
  const name = normalizeUserPluginName(
    declaration.name ?? inferUserPluginName(declaration),
  );
  return {
    name,
    source,
    module: declaration.module ?? "mod.ts",
    setup: declaration.setup ?? "setup",
    lazy: lazy
      ? {
        commands: declaration.commands ?? [],
        events: declaration.events ?? [],
      }
      : undefined,
    protocols: declaration.options === undefined
      ? undefined
      : { options: declaration.options },
  };
}

function userDeclarationSource(
  declaration: SayaPluginUseDeclaration,
): SayaPluginSource {
  const hasLocal = typeof declaration.local === "string" &&
    declaration.local.trim().length > 0;
  const hasGithub = typeof declaration.github === "string" &&
    declaration.github.trim().length > 0;
  if (hasLocal === hasGithub) {
    throw new TypeError("plugin declaration must set exactly one of local or github");
  }
  if (hasLocal) {
    return { kind: "local", path: declaration.local!.trim() };
  }
  const repo = declaration.github!.trim();
  if (!/^[^/\s]+\/[^/\s]+$/.test(repo)) {
    throw new TypeError("plugin github source must use owner/repository form");
  }
  return {
    kind: "github",
    repo: repo as `${string}/${string}`,
    rev: declaration.rev,
  };
}

function inferUserPluginName(declaration: SayaPluginUseDeclaration): string {
  const source = declaration.local ?? declaration.github ?? "";
  return source.replace(/\/+$/, "").split("/").pop()?.replace(/\.git$/, "") ??
    "";
}

function normalizeUserPluginName(name: string): string {
  const normalized = name.trim();
  if (normalized.length === 0) {
    throw new TypeError("plugin name must be a non-empty string");
  }
  return normalized;
}

function mergeUserPluginSpec(
  merged: Map<string, SayaPluginSpec>,
  spec: SayaPluginSpec,
): void {
  const existing = merged.get(spec.name);
  if (!existing) {
    merged.set(spec.name, spec);
    return;
  }
  existing.lazy = mergeLazySpec(existing.lazy, spec.lazy);
  existing.protocols = { ...(existing.protocols ?? {}), ...(spec.protocols ?? {}) };
}

function mergeLazySpec(
  left?: SayaPluginLazySpec,
  right?: SayaPluginLazySpec,
): SayaPluginLazySpec | undefined {
  if (!left) {
    return right;
  }
  if (!right) {
    return left;
  }
  return {
    commands: [...new Set([...(left.commands ?? []), ...(right.commands ?? [])])],
    events: [...new Set([...(left.events ?? []), ...(right.events ?? [])])],
    filetypes: [...new Set([...(left.filetypes ?? []), ...(right.filetypes ?? [])])],
  };
}
