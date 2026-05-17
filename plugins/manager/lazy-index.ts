import type { NormalizedPluginSpec } from "./spec.ts";

export interface SayaLazyTarget {
  plugin: string;
  module: string;
  exportName: string;
}

export interface SayaLazyIndex {
  version: 1;
  commands: Record<string, SayaLazyTarget>;
  events: Record<string, SayaLazyTarget[]>;
}

export function buildLazyIndex(plugins: NormalizedPluginSpec[]): SayaLazyIndex {
  const lazyIndex: SayaLazyIndex = {
    version: 1,
    commands: {},
    events: {},
  };
  for (const spec of plugins) {
    const target = {
      plugin: spec.name,
      module: spec.module,
      exportName: spec.setup,
    };
    for (const command of spec.lazy?.commands ?? []) {
      lazyIndex.commands[command] = target;
    }
    for (const event of spec.lazy?.events ?? []) {
      lazyIndex.events[event] = [...(lazyIndex.events[event] ?? []), target];
    }
  }
  return lazyIndex;
}
