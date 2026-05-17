import {
  buildPluginArtifacts,
  definePlugins,
  doctorPlugins,
  type SayaBundledPluginManifest,
  type SayaPluginSpec,
  syncPlugins,
  updatePlugins,
} from "./index.ts";

const bundled: SayaBundledPluginManifest[] = [
  {
    version: 1,
    name: "dired",
    module: "plugins/bundled/dired/index.ts",
    setup: "setupSayaDired",
    lazy: { commands: ["dired.open"], events: ["bufferOpen"] },
  },
];

Deno.test("definePlugins rejects duplicate names", () => {
  try {
    definePlugins([
      { name: "dup", source: { kind: "local", path: "./a.ts" } },
      { name: "dup", source: { kind: "local", path: "./b.ts" } },
    ]);
    throw new Error("duplicate plugin should fail");
  } catch (error) {
    if (!String(error).includes("duplicate plugin name")) {
      throw error;
    }
  }
});

Deno.test("buildPluginArtifacts includes bundled manifests in startup and lazy artifacts", () => {
  const artifacts = buildPluginArtifacts([], "hash-bundled", { bundled });

  if (artifacts.lockfile.plugins.length !== 0) {
    throw new Error("bundled plugins must not be installable lockfile entries");
  }
  if (!artifacts.lazyIndex.commands["dired.open"]) {
    throw new Error("bundled lazy command was not generated");
  }
  if (artifacts.lazyIndex.commands["dired.open"].plugin !== "dired") {
    throw new Error("bundled lazy command target is wrong");
  }
  if (!artifacts.lazyIndex.events.bufferOpen?.[0]) {
    throw new Error("bundled lazy event was not generated");
  }
  if (
    !artifacts.logs.some((line) =>
      line.includes("bundled_count=1") && line.includes("external_count=0")
    )
  ) {
    throw new Error("bundled/external artifact log is missing");
  }
});

Deno.test("buildPluginArtifacts orders dependencies and generates lazy index logs", () => {
  const artifacts = buildPluginArtifacts(
    [
      {
        name: "feature",
        source: { kind: "github", repo: "example/feature" },
        depends: ["base"],
        lazy: { commands: ["FeatureOpen"], events: ["bufferOpen"] },
        protocols: { github: { shallow: true } },
      },
      {
        name: "base",
        source: { kind: "git", url: "https://example.invalid/base.git" },
      },
    ],
    "hash-a",
  );

  if (
    artifacts.lockfile.plugins.map((plugin) => plugin.name).join(",") !==
      "base,feature"
  ) {
    throw new Error("dependency order was not preserved");
  }
  if (!artifacts.lazyIndex.commands.FeatureOpen) {
    throw new Error("lazy command index was not generated");
  }
  if (!artifacts.lazyIndex.events.bufferOpen?.[0]) {
    throw new Error("lazy event index was not generated");
  }
  if (
    !artifacts.logs.some((line) => line.includes("[saya-plugin-manager][lazy]"))
  ) {
    throw new Error("lazy generation log is missing");
  }
});

Deno.test("syncPlugins writes lock startup plan and lazy index artifacts", async () => {
  const root = await Deno.makeTempDir({ prefix: "saya-plugin-manager-test-" });
  const written = new Map<string, string>();
  const artifacts = await syncPlugins(
    [
      {
        name: "local-tools",
        source: { kind: "local", path: "./plugins/local-tools.ts" },
        lazy: { commands: ["LocalTools"] },
      },
    ],
    {
      cacheRoot: root,
      sourceHash: "hash-b",
      bundled,
      writeTextFile(path, text) {
        written.set(path, text);
      },
    },
  );

  if (artifacts.lockfile.plugins.length !== 1) {
    throw new Error("lockfile plugin count mismatch");
  }
  if (!artifacts.lazyIndex.commands["dired.open"]) {
    throw new Error("sync did not include bundled manifest lazy command");
  }
  for (
    const file of ["plugin-lock.json", "startup-plan.json", "lazy-index.json"]
  ) {
    if (!written.has(`${root}/plugins/${file}`)) {
      throw new Error(`missing written artifact: ${file}`);
    }
  }
  const lockfile = JSON.parse(written.get(`${root}/plugins/plugin-lock.json`)!);
  if (
    lockfile.plugins.some((plugin: { name: string }) => plugin.name === "dired")
  ) {
    throw new Error("bundled plugin leaked into lockfile");
  }
  const doctor = doctorPlugins(artifacts);
  if (!doctor.ok) {
    throw new Error(`doctor failed: ${doctor.messages.join(",")}`);
  }
});

Deno.test("syncPlugins and updatePlugins execute protocol commands through injected runner", async () => {
  const root = await Deno.makeTempDir({
    prefix: "saya-plugin-manager-protocol-",
  });
  const calls: string[] = [];
  const context = {
    cacheRoot: root,
    sourceHash: "hash-c",
    writeTextFile() {},
    ensureDir(path: string) {
      calls.push(`ensure:${path}`);
    },
    runCommand(command: string, args: string[], options?: { cwd?: string }) {
      calls.push(`${command}:${args.join(" ")}:${options?.cwd ?? ""}`);
    },
  };

  const specs: SayaPluginSpec[] = [
    {
      name: "github-tools",
      source: { kind: "github" as const, repo: "example/tools", rev: "main" },
    },
  ];

  const syncArtifacts = await syncPlugins(specs, context);
  const updateArtifacts = await updatePlugins(specs, context);

  if (!calls.some((call) => call.includes("clone --filter=blob:none"))) {
    throw new Error(`sync did not clone through runner: ${calls.join("|")}`);
  }
  if (!calls.some((call) => call.includes("fetch --all --tags"))) {
    throw new Error(`update did not fetch through runner: ${calls.join("|")}`);
  }
  if (
    !syncArtifacts.logs.some((line) =>
      line.includes("[saya-plugin-manager][protocol] sync git")
    )
  ) {
    throw new Error("sync protocol log is missing");
  }
  if (
    !updateArtifacts.logs.some((line) =>
      line.includes("[saya-plugin-manager][protocol] update git")
    )
  ) {
    throw new Error("update protocol log is missing");
  }
});
