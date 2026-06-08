import type { SayaPluginOperationContext } from "../index.ts";
import type { SayaPluginSpec } from "../spec.ts";

export async function syncLocalPlugin(
  spec: SayaPluginSpec,
  context: SayaPluginOperationContext,
  logs: string[],
): Promise<void> {
  const source = typeof spec.source === "object" ? spec.source : null;
  logs.push(
    `[saya-plugin-manager][protocol] sync local plugin: plugin=${spec.name} path=${
      source?.kind === "local" ? source.path : String(spec.source)
    } cache_root=${context.cacheRoot}`,
  );
}
