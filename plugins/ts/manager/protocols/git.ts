import type { SayaPluginOperationContext } from "../index.ts";
import type { SayaPluginSpec } from "../spec.ts";

export async function syncGitPlugin(
  spec: SayaPluginSpec,
  context: SayaPluginOperationContext,
  logs: string[],
  operation: "sync" | "update",
  url: string,
  revision: string,
): Promise<void> {
  const installRoot = `${context.cacheRoot}/plugins/repos`;
  const installPath = `${installRoot}/${spec.name}`;
  await context.ensureDir?.(installRoot);
  if (context.runCommand) {
    if (operation === "sync") {
      await context.runCommand("git", [
        "clone",
        "--filter=blob:none",
        url,
        installPath,
      ]);
    } else {
      await context.runCommand("git", ["fetch", "--all", "--tags"], {
        cwd: installPath,
      });
    }
    if (revision !== "HEAD") {
      await context.runCommand("git", ["checkout", revision], {
        cwd: installPath,
      });
    }
  }
  logs.push(
    `[saya-plugin-manager][protocol] ${operation} git plugin: plugin=${spec.name} url=${url} rev=${revision} command_runner=${
      context.runCommand ? "present" : "absent"
    }`,
  );
}
