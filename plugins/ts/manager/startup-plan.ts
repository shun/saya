import type { NormalizedPluginSpec } from "./spec.ts";

export interface SayaStartupPlan {
  version: 1;
  sourceHash: string;
  entries: Array<
    | { kind: "command"; name: string; callbackSource: string }
    | { kind: "event"; name: string; callbackSource: string }
    | { kind: "warning"; message: string }
  >;
}

export function buildStartupPlan(
  plugins: NormalizedPluginSpec[],
  sourceHash: string,
): SayaStartupPlan {
  return {
    version: 1,
    sourceHash,
    entries: plugins
      .filter((spec) => !spec.lazy)
      .map((spec) => ({
        kind: "command" as const,
        name: `${spec.name}.setup`,
        callbackSource:
          `async () => { console.info("[saya-plugin-manager][startup] eager setup plugin=${spec.name} class=${spec.class}"); }`,
      })),
  };
}
