import {
  encodeSource,
  type NormalizedPluginSpec,
  revisionForSource,
} from "./spec.ts";

export interface SayaLockedPlugin {
  name: string;
  source: string;
  revision: string;
  depends: string[];
  before: string[];
  after: string[];
  protocols: Record<string, unknown>;
}

export interface SayaPluginLockfile {
  version: 1;
  plugins: SayaLockedPlugin[];
}

export function buildLockfile(
  plugins: NormalizedPluginSpec[],
): SayaPluginLockfile {
  return {
    version: 1,
    plugins: plugins
      .filter((spec) => spec.class === "external")
      .map((spec) => ({
        name: spec.name,
        source: encodeSource(
          spec.source as Exclude<typeof spec.source, "bundled">,
        ),
        revision: revisionForSource(
          spec.source as Exclude<typeof spec.source, "bundled">,
        ),
        depends: spec.depends,
        before: spec.before,
        after: spec.after,
        protocols: spec.protocols,
      })),
  };
}
