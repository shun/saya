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
