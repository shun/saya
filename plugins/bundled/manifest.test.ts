import { buildPluginArtifacts } from "../manager/index.ts";
import type { SayaBundledPluginManifest } from "../manager/spec.ts";

Deno.test("bundled manifests generate lazy command placeholders", async () => {
  const manifests = await Promise.all(
    [
      new URL("./dired/manifest.json", import.meta.url),
      new URL("./completion/manifest.json", import.meta.url),
      new URL("./lsp-client/manifest.json", import.meta.url),
    ].map(async (url) =>
      JSON.parse(await Deno.readTextFile(url)) as SayaBundledPluginManifest
    ),
  );
  const artifacts = buildPluginArtifacts([], "hash-manifest", {
    bundled: manifests,
  });

  if (!artifacts.lazyIndex.commands["dired.open"]) {
    throw new Error("dired command placeholder missing");
  }
  if (!artifacts.lazyIndex.commands["lsp.start"]) {
    throw new Error("lsp command placeholder missing");
  }
  if (!artifacts.lazyIndex.commands["completion.trigger"]) {
    throw new Error("completion command placeholder missing");
  }
  if (
    artifacts.lazyIndex.commands["completion.trigger"].plugin !==
      "saya-completion"
  ) {
    throw new Error("completion command should target saya-completion");
  }
  if (artifacts.lockfile.plugins.length !== 0) {
    throw new Error("bundled manifests must not create lockfile entries");
  }
});

Deno.test("existing bundled import shims point at the new layout", async () => {
  const diredShim = await Deno.readTextFile(
    new URL("../saya-dired.ts", import.meta.url),
  );
  const lspShim = await Deno.readTextFile(
    new URL("../saya-lsp-client.ts", import.meta.url),
  );

  if (!diredShim.includes("./bundled/dired/index.ts")) {
    throw new Error("dired shim does not point at bundled/dired");
  }
  if (!lspShim.includes("./bundled/lsp-client/index.ts")) {
    throw new Error("lsp-client shim does not point at bundled/lsp-client");
  }
});
