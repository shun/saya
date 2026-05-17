import { buildLazyIndex, type SayaLazyIndex } from "./lazy-index.ts";
import { buildLockfile, type SayaPluginLockfile } from "./lockfile.ts";
import { syncGitPlugin } from "./protocols/git.ts";
import { githubRepoToUrl } from "./protocols/github.ts";
import { syncLocalPlugin } from "./protocols/local.ts";
import { buildStartupPlan, type SayaStartupPlan } from "./startup-plan.ts";
import {
  definePlugins,
  normalizeBundledPlugins,
  type NormalizedPluginSpec,
  normalizeExternalPlugins,
  type SayaBundledPluginManifest,
  type SayaPluginSpec,
} from "./spec.ts";

export { buildLazyIndex } from "./lazy-index.ts";
export type { SayaLazyIndex, SayaLazyTarget } from "./lazy-index.ts";
export { buildLockfile } from "./lockfile.ts";
export type { SayaLockedPlugin, SayaPluginLockfile } from "./lockfile.ts";
export { buildStartupPlan } from "./startup-plan.ts";
export type { SayaStartupPlan } from "./startup-plan.ts";
export {
  definePlugins,
  encodeSource,
  normalizeBundledPlugins,
  normalizeExternalPlugins,
  revisionForSource,
} from "./spec.ts";
export type {
  SayaBundledPluginManifest,
  SayaPluginLazySpec,
  SayaPluginSource,
  SayaPluginSpec,
} from "./spec.ts";

export interface SayaPluginManagerArtifacts {
  lockfile: SayaPluginLockfile;
  startupPlan: SayaStartupPlan;
  lazyIndex: SayaLazyIndex;
  logs: string[];
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

export function buildPluginArtifacts(
  specs: SayaPluginSpec[],
  sourceHash: string,
  options: { bundled?: SayaBundledPluginManifest[] } = {},
): SayaPluginManagerArtifacts {
  const external = normalizeExternalPlugins(specs);
  const bundled = normalizeBundledPlugins(options.bundled ?? []);
  const normalized = resolvePluginOrder([...bundled, ...external]);
  const logs = [
    `[saya-plugin-manager][sync] build artifacts: bundled_count=${bundled.length} external_count=${external.length} source_hash=${sourceHash}`,
  ];
  const lockfile = buildLockfile(normalized);
  const startupPlan = buildStartupPlan(normalized, sourceHash);
  const lazyIndex = buildLazyIndex(normalized);
  logs.push(
    `[saya-plugin-manager][lazy] index generated: commands=${
      Object.keys(lazyIndex.commands).length
    } events=${Object.keys(lazyIndex.events).length}`,
  );
  logs.push(
    `[saya-plugin-manager][lockfile] external plugins locked: plugin_count=${lockfile.plugins.length}`,
  );
  return { lockfile, startupPlan, lazyIndex, logs };
}

export async function syncPlugins(
  specs: SayaPluginSpec[],
  context: SayaPluginOperationContext,
): Promise<SayaPluginManagerArtifacts> {
  const artifacts = buildPluginArtifacts(specs, context.sourceHash, {
    bundled: context.bundled,
  });
  for (const spec of definePlugins(specs)) {
    await installOrUpdatePlugin(spec, context, artifacts.logs, "sync");
  }
  await writeArtifacts(context, artifacts);
  artifacts.logs.push(
    `[saya-plugin-manager][operation] sync cache_root=${context.cacheRoot} external_plugin_count=${artifacts.lockfile.plugins.length}`,
  );
  return artifacts;
}

export async function updatePlugins(
  specs: SayaPluginSpec[],
  context: SayaPluginOperationContext,
): Promise<SayaPluginManagerArtifacts> {
  const artifacts = buildPluginArtifacts(specs, context.sourceHash, {
    bundled: context.bundled,
  });
  for (const spec of definePlugins(specs)) {
    await installOrUpdatePlugin(spec, context, artifacts.logs, "update");
  }
  await writeArtifacts(context, artifacts);
  artifacts.logs.push(
    `[saya-plugin-manager][operation] update cache_root=${context.cacheRoot} external_plugin_count=${artifacts.lockfile.plugins.length}`,
  );
  return artifacts;
}

export function listPlugins(artifacts: SayaPluginManagerArtifacts): string[] {
  const names = artifacts.lockfile.plugins.map((plugin) => plugin.name);
  artifacts.logs.push(
    `[saya-plugin-manager][operation] list plugin_count=${names.length}`,
  );
  return names;
}

export async function cleanPlugins(
  context: SayaPluginOperationContext,
): Promise<string[]> {
  const removed = ["startup-plan.json", "lazy-index.json"];
  for (const file of removed) {
    await context.removeFile?.(`${context.cacheRoot}/plugins/${file}`);
  }
  return removed;
}

export function doctorPlugins(
  artifacts: SayaPluginManagerArtifacts,
): { ok: boolean; messages: string[] } {
  const messages: string[] = [];
  const names = new Set<string>();
  for (const plugin of artifacts.lockfile.plugins) {
    if (names.has(plugin.name)) {
      messages.push(`duplicate plugin in lockfile: ${plugin.name}`);
    }
    names.add(plugin.name);
  }
  artifacts.logs.push(
    `[saya-plugin-manager][operation] doctor ok=${messages.length === 0}`,
  );
  return { ok: messages.length === 0, messages };
}

async function writeArtifacts(
  context: SayaPluginOperationContext,
  artifacts: SayaPluginManagerArtifacts,
): Promise<void> {
  const writer = context.writeTextFile;
  if (!writer) {
    return;
  }
  await writer(
    `${context.cacheRoot}/plugins/plugin-lock.json`,
    JSON.stringify(artifacts.lockfile, null, 2),
  );
  await writer(
    `${context.cacheRoot}/plugins/startup-plan.json`,
    JSON.stringify(artifacts.startupPlan, null, 2),
  );
  await writer(
    `${context.cacheRoot}/plugins/lazy-index.json`,
    JSON.stringify(artifacts.lazyIndex, null, 2),
  );
}

async function installOrUpdatePlugin(
  spec: SayaPluginSpec,
  context: SayaPluginOperationContext,
  logs: string[],
  operation: "sync" | "update",
): Promise<void> {
  const source = spec.source;
  if (typeof source === "string") {
    logs.push(
      `[saya-plugin-manager][protocol] ${operation} delegated protocol string: plugin=${spec.name} source=${source}`,
    );
    return;
  }
  if (source.kind === "local") {
    await syncLocalPlugin(spec, context, logs);
    return;
  }

  const url = source.kind === "github"
    ? githubRepoToUrl(source.repo)
    : source.url;
  const revision = source.rev ?? "HEAD";
  await syncGitPlugin(spec, context, logs, operation, url, revision);
}

function resolvePluginOrder(
  specs: NormalizedPluginSpec[],
): NormalizedPluginSpec[] {
  const byName = new Map(specs.map((spec) => [spec.name, spec]));
  const edges = new Map<string, Set<string>>();
  for (const spec of specs) {
    edges.set(spec.name, edges.get(spec.name) ?? new Set());
  }
  for (const spec of specs) {
    for (const dependency of [...spec.depends, ...spec.after]) {
      if (!byName.has(dependency)) {
        throw new TypeError(
          `unknown plugin dependency: ${spec.name} -> ${dependency}`,
        );
      }
      edges.get(dependency)?.add(spec.name);
    }
    for (const later of spec.before) {
      if (!byName.has(later)) {
        throw new TypeError(
          `unknown plugin before target: ${spec.name} -> ${later}`,
        );
      }
      edges.get(spec.name)?.add(later);
    }
  }

  const incoming = new Map<string, number>();
  for (const spec of specs) {
    incoming.set(spec.name, 0);
  }
  for (const targets of edges.values()) {
    for (const target of targets) {
      incoming.set(target, (incoming.get(target) ?? 0) + 1);
    }
  }

  const ready = specs
    .map((spec) => spec.name)
    .filter((name) => incoming.get(name) === 0);
  const ordered: NormalizedPluginSpec[] = [];
  while (ready.length > 0) {
    const name = ready.shift()!;
    ordered.push(byName.get(name)!);
    for (const target of edges.get(name) ?? []) {
      incoming.set(target, (incoming.get(target) ?? 0) - 1);
      if (incoming.get(target) === 0) {
        ready.push(target);
      }
    }
  }

  if (ordered.length !== specs.length) {
    throw new TypeError("plugin dependency cycle detected");
  }
  return ordered;
}
