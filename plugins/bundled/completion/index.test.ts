// deno-lint-ignore-file no-explicit-any

import {
  createBufferWordSource,
  createLspCompletionSource,
  createPathCompletionSource,
  detectPathCompletionPrefix,
  resolvePathCompletionDirectory,
  setupSayaCompletion,
} from "./index.ts";
import type {
  SayaCompletionQuery,
  SayaCompletionTriggerContext,
} from "./types.ts";

function installSayaFake(
  response: unknown,
  buffer?: Record<string, unknown>,
  fs?: { readDir?: (path: string, options?: unknown) => unknown },
) {
  const executed: string[] = [];
  const registered = new Map<string, () => unknown>();
  const events = new Map<string, (payload: unknown) => unknown>();
  const shown: unknown[] = [];
  const closed: unknown[] = [];
  const listed: Array<{ path: string; options?: unknown }> = [];
  const keymaps: Array<{ mode: string; lhs: string; action: unknown }> = [];
  let floats: unknown[] = [];
  (globalThis as any).saya = {
    buffer: {
      current: () =>
        Promise.resolve({
          id: 7,
          path: "/workspace/main.ts",
          lineCount: 1,
          cursorRow: 0,
          cursorCol: 4,
          currentLine: "prin",
          text: "prin\nprintln\nprivate\n",
          ...buffer,
        }),
    },
    editor: {
      current: () => Promise.resolve({ mode: "Insert" }),
    },
    commands: {
      register(name: string, callback: () => unknown) {
        registered.set(name, callback);
      },
      execute(name: string) {
        executed.push(name);
        if (name === "lsp.completion") return Promise.resolve(response);
        if (registered.has(name)) {
          return { __sayaStartupCommandReference: true, name };
        }
        throw new Error(`missing command: ${name}`);
      },
    },
    completion: {
      show(request: unknown) {
        shown.push(request);
        return Promise.resolve(true);
      },
      close() {
        closed.push("completion.close");
        floats = floats.filter((float: any) =>
          float?.kind !== "completionMenu" && float?.kind !== "completion-menu"
        );
        return Promise.resolve(true);
      },
    },
    window: {
      floats() {
        return Promise.resolve(floats);
      },
      close(id: unknown) {
        closed.push(id);
        floats = floats.filter((float: any) => float?.id !== id);
        return Promise.resolve(true);
      },
    },
    fs: {
      async readDir(path: string, options?: unknown) {
        listed.push({ path, options });
        if (fs?.readDir) return await fs.readDir(path, options);
        return [];
      },
    },
    filer: {
      async list(path: string, options?: unknown) {
        throw new Error(
          `completion tests must not use side-effectful saya.filer.list: ${path} ${
            JSON.stringify(options)
          }`,
        );
      },
    },
    keymap: {
      set(mode: string, lhs: string, action: unknown) {
        keymaps.push({ mode, lhs, action });
      },
    },
    events: {
      on(name: string, callback: (payload: unknown) => unknown) {
        events.set(name, callback);
      },
    },
  };
  return {
    executed,
    registered,
    events,
    shown,
    keymaps,
    listed,
    closed,
    setFloats(value: unknown[]) {
      floats = value;
    },
  };
}

function shownLength(fake: ReturnType<typeof installSayaFake>): number {
  return fake.shown.length;
}

function executeRegisteredCommands(
  fake: ReturnType<typeof installSayaFake>,
  lspResponse: unknown = { result: [] },
) {
  (globalThis as any).saya.commands.execute = (name: string) => {
    fake.executed.push(name);
    if (name === "lsp.completion") return Promise.resolve(lspResponse);
    const callback = fake.registered.get(name);
    if (!callback) throw new Error(`missing command: ${name}`);
    return callback();
  };
}

Deno.test("buffer source collects filtered deduped words ordered near the cursor", async () => {
  const context: SayaCompletionTriggerContext = {
    buffer: {
      id: 7,
      path: "/workspace/main.ts",
      lineCount: 3,
      cursorRow: 2,
      cursorCol: 3,
      currentLine: "pri",
      text: [
        "private printer",
        "private priority",
        "pri",
      ].join("\n"),
    },
    editor: { mode: "Insert" },
    reason: { kind: "manual" },
  };

  const source = createBufferWordSource();
  const query = source.trigger(context);
  if (!query) throw new Error("expected buffer query");
  const candidates = (await source.complete(query)).candidates;
  const labels = candidates.map((candidate) => candidate.label);

  if (
    JSON.stringify(labels) !==
      JSON.stringify(["priority", "private", "printer"])
  ) {
    throw new Error(`unexpected buffer labels: ${JSON.stringify(labels)}`);
  }
  if (labels.includes("pri")) {
    throw new Error("current prefix must not be included as a candidate");
  }
  if (labels.filter((label) => label === "private").length !== 1) {
    throw new Error(
      `expected private to be deduped, got ${JSON.stringify(labels)}`,
    );
  }
});

Deno.test("buffer completion request uses typed menu shape and max items", async () => {
  const fake = installSayaFake({ result: [] }, {
    cursorCol: 3,
    currentLine: "pri",
    text: "pri\nprintln\nprivate\npriority\n",
  });
  await setupSayaCompletion({
    sources: [createBufferWordSource()],
    sourceTimeoutMs: 0,
    maxItems: 2,
  });

  const result = await fake.registered.get("completion.trigger")?.();
  if (result !== true) {
    throw new Error(
      `expected completion trigger to show menu, got ${String(result)}`,
    );
  }
  if (shownLength(fake) !== 1) {
    throw new Error(
      `expected one typed completion show, got ${fake.shown.length}`,
    );
  }

  const request = fake.shown[0] as any;
  if (request.selectedIndex !== 0) {
    throw new Error(`unexpected selectedIndex: ${request.selectedIndex}`);
  }
  if (
    request.replaceRange.start.line !== 0 ||
    request.replaceRange.start.character !== 0 ||
    request.replaceRange.end.line !== 0 ||
    request.replaceRange.end.character !== 3
  ) {
    throw new Error(
      `unexpected replaceRange: ${JSON.stringify(request.replaceRange)}`,
    );
  }
  const labels = request.candidates.map((candidate: any) => candidate.label);
  if (JSON.stringify(labels) !== JSON.stringify(["println", "private"])) {
    throw new Error(`unexpected request candidates: ${JSON.stringify(labels)}`);
  }
  if (
    JSON.stringify(request.keys) !==
      JSON.stringify({
        confirm: ["<Enter>", "<Tab>", "<C-y>"],
        close: ["<C-e>"],
        next: ["<Down>", "<C-n>"],
        previous: ["<Up>", "<C-p>"],
        pageNext: ["<PageDown>"],
        pagePrevious: ["<PageUp>"],
      })
  ) {
    throw new Error(
      `menu keys should default to standard bindings: ${
        JSON.stringify(request.keys)
      }`,
    );
  }
});

Deno.test("completion setup with sources does not install manual keymap unless key is explicit", async () => {
  const fake = installSayaFake({ result: [] });
  await setupSayaCompletion({
    sources: [createBufferWordSource()],
    sourceTimeoutMs: 0,
  });

  if (!fake.registered.has("completion.trigger")) {
    throw new Error("completion command should still be registered");
  }
  if (fake.keymaps.length !== 0) {
    throw new Error(
      `unexpected implicit keymaps: ${JSON.stringify(fake.keymaps)}`,
    );
  }
});

Deno.test("completion setup can install explicit manual keymap without sources", async () => {
  const fake = installSayaFake({ result: [] });
  await setupSayaCompletion({ key: "<C-Space>", sourceTimeoutMs: 0 });

  if (fake.keymaps.length !== 1) {
    throw new Error(
      `expected one explicit keymap, got ${JSON.stringify(fake.keymaps)}`,
    );
  }
  if (
    fake.keymaps[0].mode !== "insert" || fake.keymaps[0].lhs !== "<C-Space>"
  ) {
    throw new Error(
      `unexpected explicit keymap: ${JSON.stringify(fake.keymaps[0])}`,
    );
  }
  const result = await fake.registered.get("completion.trigger")?.();
  if (result !== false || fake.shown.length !== 0) {
    throw new Error("keymap-only completion must stay quiet without sources");
  }
});

Deno.test("completion request forwards configured operation keys", async () => {
  const fake = installSayaFake({ result: [] }, {
    cursorCol: 3,
    currentLine: "pri",
    text: "pri\nprintln\nprivate\n",
  });
  await setupSayaCompletion({
    sources: [createBufferWordSource()],
    sourceTimeoutMs: 0,
    keys: {
      confirm: ["<Tab>"],
      close: ["<Esc>"],
      next: ["j"],
      previous: ["k"],
      pageNext: ["<C-f>"],
      pagePrevious: ["<C-b>"],
    },
  });

  await fake.registered.get("completion.trigger")?.();
  const request = fake.shown[0] as any;
  if (
    JSON.stringify(request.keys) !==
      JSON.stringify({
        confirm: ["<Tab>"],
        close: ["<Esc>"],
        next: ["j"],
        previous: ["k"],
        pageNext: ["<C-f>"],
        pagePrevious: ["<C-b>"],
      })
  ) {
    throw new Error(
      `unexpected configured keys: ${JSON.stringify(request.keys)}`,
    );
  }
});

Deno.test("completion command with no configured sources stays quiet", async () => {
  const fake = installSayaFake({ result: [] }, {
    cursorCol: 3,
    currentLine: "pri",
    text: "pri\nprintln\nprivate\n",
  });
  await setupSayaCompletion({ sourceTimeoutMs: 0 });

  const registered = fake.registered.get("completion.trigger");
  if (!registered) throw new Error("completion trigger should be registered");
  const result = await registered();
  if (result !== false) {
    throw new Error(
      `expected no-source completion to stay quiet, got ${result}`,
    );
  }
  if (fake.shown.length !== 0) {
    throw new Error(
      `no-source completion must not show menu: ${fake.shown.length}`,
    );
  }
});

Deno.test("bundled source command runs without startup closure state", async () => {
  const fake = installSayaFake({ result: [] }, {
    cursorCol: 3,
    currentLine: "pri",
    text: "pri\nprintln\nprivate\n",
  });
  await setupSayaCompletion({
    sources: [createBufferWordSource({ minPrefixLength: 2 })],
    sourceTimeoutMs: 0,
  });

  const registered = fake.registered.get("completion.trigger");
  if (!registered) throw new Error("completion trigger should be registered");
  const isolated = new Function(`
    const setTimeout = undefined;
    const clearTimeout = undefined;
    return (${registered.toString()});
  `)() as () => Promise<unknown>;
  const result = await isolated();
  if (result !== true) {
    throw new Error(`expected isolated callback to show menu, got ${result}`);
  }

  const request = fake.shown[0] as any;
  const labels = request.candidates.map((candidate: any) => candidate.label);
  if (JSON.stringify(labels) !== JSON.stringify(["println", "private"])) {
    throw new Error(`unexpected isolated labels: ${JSON.stringify(labels)}`);
  }
  if (
    JSON.stringify(request.keys) !==
      JSON.stringify({
        confirm: ["<Enter>", "<Tab>", "<C-y>"],
        close: ["<C-e>"],
        next: ["<Down>", "<C-n>"],
        previous: ["<Up>", "<C-p>"],
        pageNext: ["<PageDown>"],
        pagePrevious: ["<PageUp>"],
      })
  ) {
    throw new Error(
      `startup-safe command should include default completion keys: ${
        JSON.stringify(request.keys)
      }`,
    );
  }
});

Deno.test("bundled source command forwards configured operation keys", async () => {
  const fake = installSayaFake({ result: [] }, {
    cursorCol: 3,
    currentLine: "pri",
    text: "pri\nprintln\nprivate\n",
  });
  await setupSayaCompletion({
    sources: [createBufferWordSource({ minPrefixLength: 2 })],
    sourceTimeoutMs: 0,
    keys: {
      confirm: ["<Tab>"],
      close: ["<Esc>"],
      next: ["j"],
      previous: ["k"],
      pageNext: ["<C-f>"],
      pagePrevious: ["<C-b>"],
    },
  });

  const result = await fake.registered.get("completion.trigger")?.();
  if (result !== true) {
    throw new Error(
      `expected bundled source command to show menu, got ${result}`,
    );
  }

  const request = fake.shown[0] as any;
  if (
    JSON.stringify(request.keys) !==
      JSON.stringify({
        confirm: ["<Tab>"],
        close: ["<Esc>"],
        next: ["j"],
        previous: ["k"],
        pageNext: ["<C-f>"],
        pagePrevious: ["<C-b>"],
      })
  ) {
    throw new Error(
      `unexpected bundled command keys: ${JSON.stringify(request.keys)}`,
    );
  }
});

Deno.test("bundled buffer source minPrefixLength is explicit", async () => {
  const fake = installSayaFake({ result: [] }, {
    cursorCol: 1,
    currentLine: "p",
    text: "p\nprintln\nprivate\n",
  });
  await setupSayaCompletion({
    minPrefixLength: 1,
    sources: [createBufferWordSource({ minPrefixLength: 2 })],
    sourceTimeoutMs: 0,
  });

  const result = await fake.registered.get("completion.trigger")?.();

  if (result !== false) {
    throw new Error(
      `expected explicit buffer word source to stay quiet for one character, got ${result}`,
    );
  }
  if (fake.shown.length !== 0) {
    throw new Error(`unexpected one-character menu: ${fake.shown.length}`);
  }
});

Deno.test("bundled buffer source can be configured for one-character completion", async () => {
  const fake = installSayaFake({ result: [] }, {
    cursorCol: 1,
    currentLine: "p",
    text: "p\nprintln\nprivate\n",
  });
  await setupSayaCompletion({
    minPrefixLength: 1,
    sources: [createBufferWordSource({ minPrefixLength: 1 })],
    sourceTimeoutMs: 0,
  });

  const result = await fake.registered.get("completion.trigger")?.();

  if (result !== true) {
    throw new Error(
      `expected one-character buffer word completion, got ${result}`,
    );
  }
  const labels = (fake.shown[0] as any).candidates.map((candidate: any) =>
    candidate.label
  );
  if (JSON.stringify(labels) !== JSON.stringify(["println", "private"])) {
    throw new Error(
      `unexpected one-character labels: ${JSON.stringify(labels)}`,
    );
  }
});

Deno.test("bundled buffer source can auto trigger on one character when configured", async () => {
  const buffer = {
    cursorCol: 1,
    currentLine: "p",
    text: "p\nprintln\nprivate\n",
  };
  const fake = installSayaFake({ result: [] }, buffer);
  await setupSayaCompletion({
    autoTrigger: true,
    autoTriggerDelayMs: 0,
    minPrefixLength: 1,
    sources: [createBufferWordSource({ minPrefixLength: 1 })],
    sourceTimeoutMs: 0,
  });

  const handler = fake.events.get("bufferChanged");
  if (!handler) throw new Error("expected bufferChanged subscription");
  await handler({ buffer });
  await fake.registered.get("completion.trigger")?.();

  if (fake.shown.length !== 1) {
    throw new Error(
      `expected one-character auto buffer completion, got ${fake.shown.length}`,
    );
  }
  const labels = (fake.shown[0] as any).candidates.map((candidate: any) =>
    candidate.label
  );
  if (JSON.stringify(labels) !== JSON.stringify(["println", "private"])) {
    throw new Error(
      `unexpected one-character auto labels: ${JSON.stringify(labels)}`,
    );
  }
});

Deno.test("auto trigger with no configured sources stays quiet", async () => {
  const buffer = {
    path: "/workspace/main.ts",
    cursorCol: 3,
    currentLine: "pri",
    text: "pri\nprintln\nprivate\n",
  };
  const fake = installSayaFake({ result: [] }, buffer);
  await setupSayaCompletion({
    autoTrigger: true,
    autoTriggerDelayMs: 0,
    sourceTimeoutMs: 0,
  });
  (globalThis as any).saya.commands.execute = (name: string) => {
    const callback = fake.registered.get(name);
    if (!callback) throw new Error(`missing command: ${name}`);
    return callback();
  };

  const handler = fake.events.get("bufferChanged");
  if (!handler) throw new Error("expected bufferChanged subscription");
  await handler({ buffer });
  if (fake.shown.length !== 0) {
    throw new Error(
      `no-source auto trigger must not open: ${fake.shown.length}`,
    );
  }
});

Deno.test("explicit bundled path source auto trigger uses the startup-safe command path", async () => {
  const buffer = {
    path: "/workspace/main.ts",
    cursorCol: 1,
    currentLine: "/",
    text: "/",
  };
  const fake = installSayaFake({ result: [] }, buffer, {
    readDir: (path: string) => {
      if (path !== "/") throw new Error(`unexpected path: ${path}`);
      return [{ name: "usr", kind: "directory", path: "/usr" }];
    },
  });
  await setupSayaCompletion({
    autoTrigger: true,
    autoTriggerDelayMs: 0,
    sources: [
      createPathCompletionSource({
        minPrefixLength: 1,
        triggerCharacters: ["/", "."],
      }),
    ],
    sourceTimeoutMs: 0,
  });
  (globalThis as any).saya.commands.execute = (name: string) => {
    if (name === "lsp.completion") return Promise.resolve({ result: [] });
    const callback = fake.registered.get(name);
    if (!callback) throw new Error(`missing command: ${name}`);
    return callback();
  };

  const handler = fake.events.get("bufferChanged");
  if (!handler) throw new Error("expected bufferChanged subscription");
  await handler({ buffer });
  if (shownLength(fake) !== 1) {
    throw new Error("explicit path source must auto open for slash");
  }
  const labels = (fake.shown[0] as any).candidates.map((candidate: any) =>
    candidate.label
  );
  if (JSON.stringify(labels) !== JSON.stringify(["/usr/"])) {
    throw new Error(
      `unexpected explicit path auto labels: ${JSON.stringify(labels)}`,
    );
  }
});

Deno.test("buffer completion respects min prefix length", async () => {
  const fake = installSayaFake({ result: [] }, {
    cursorCol: 1,
    currentLine: "p",
    text: "p\nprintln\nprivate\n",
  });
  await setupSayaCompletion({
    sources: [createBufferWordSource()],
    sourceTimeoutMs: 0,
    minPrefixLength: 2,
  });

  const result = await fake.registered.get("completion.trigger")?.();
  if (result !== false) {
    throw new Error(
      `expected short prefix to skip completion, got ${String(result)}`,
    );
  }
  if (fake.shown.length !== 0) {
    throw new Error(
      `short prefix must not open menu, got ${fake.shown.length}`,
    );
  }
});

Deno.test("path prefix detection handles relative absolute and quoted prefixes", () => {
  const cases = [
    {
      line: "open ./src/ma",
      cursorCol: 13,
      prefix: "./src/ma",
      start: 5,
    },
    {
      line: "edit ../notes",
      cursorCol: 13,
      prefix: "../notes",
      start: 5,
    },
    {
      line: "source /usr/lo",
      cursorCol: 14,
      prefix: "/usr/lo",
      start: 7,
    },
    {
      line: 'import "./src/ma',
      cursorCol: 16,
      prefix: "./src/ma",
      start: 8,
    },
    {
      line: "plain word",
      cursorCol: 10,
      prefix: null,
      start: 0,
    },
  ];

  for (const testCase of cases) {
    const prefix = detectPathCompletionPrefix({
      id: 1,
      path: "/workspace/main.ts",
      lineCount: 1,
      cursorRow: 0,
      cursorCol: testCase.cursorCol,
      currentLine: testCase.line,
      text: testCase.line,
    });
    if (testCase.prefix == null) {
      if (prefix !== null) {
        throw new Error(`unexpected path prefix: ${JSON.stringify(prefix)}`);
      }
      continue;
    }
    if (!prefix || prefix.prefix !== testCase.prefix) {
      throw new Error(
        `unexpected prefix for ${testCase.line}: ${JSON.stringify(prefix)}`,
      );
    }
    if (
      prefix.range.start.character !== testCase.start ||
      prefix.range.end.character !== testCase.cursorCol
    ) {
      throw new Error(`unexpected range: ${JSON.stringify(prefix.range)}`);
    }
  }
});

Deno.test("path completion resolves relative paths from current buffer directory", () => {
  const buffer = {
    id: 1,
    path: "/workspace/app/main.ts",
    lineCount: 1,
    cursorRow: 0,
    cursorCol: 8,
    currentLine: "../lib/a",
    text: "../lib/a",
  };
  const prefix = detectPathCompletionPrefix(buffer);
  if (!prefix) throw new Error("expected path prefix");
  const directory = resolvePathCompletionDirectory(buffer, prefix);
  if (directory !== "/workspace/lib") {
    throw new Error(`unexpected resolved directory: ${directory}`);
  }
});

Deno.test("path source collects directory and file candidates from fs readDir", async () => {
  const context: SayaCompletionTriggerContext = {
    buffer: {
      id: 7,
      path: "/workspace/main.ts",
      lineCount: 1,
      cursorRow: 0,
      cursorCol: 5,
      currentLine: "./src",
      text: "./src",
    },
    editor: { mode: "Insert" },
    reason: { kind: "manual" },
  };
  const fake = installSayaFake({ result: [] }, {}, {
    readDir: (path: string) => {
      if (path !== "/workspace") {
        throw new Error(`unexpected list path: ${path}`);
      }
      return [
        { name: "src-file.ts", kind: "file", path: "/workspace/src-file.ts" },
        { name: "src", kind: "directory", path: "/workspace/src" },
        { name: "src-alpha.ts", kind: "file", path: "/workspace/src-alpha.ts" },
        { name: "src-long", kind: "directory", path: "/workspace/src-long" },
        { name: "src-dir", kind: "directory", path: "/workspace/src-dir" },
        { name: "exact.txt", kind: "file", path: "/workspace/exact.txt" },
      ];
    },
  });

  const source = createPathCompletionSource({ optional: false });
  const query = source.trigger(context);
  if (!query) throw new Error("expected path query");
  const result = await source.complete(query);
  const candidates = result.candidates;
  const labels = candidates.map((candidate) => candidate.label);
  const expected = [
    "./src/",
    "./src-dir/",
    "./src-long/",
    "./src-file.ts",
    "./src-alpha.ts",
  ];
  if (JSON.stringify(labels) !== JSON.stringify(expected)) {
    throw new Error(`unexpected path labels: ${JSON.stringify(labels)}`);
  }
  if (fake.listed.length !== 1 || fake.listed[0].path !== "/workspace") {
    throw new Error(`unexpected listed calls: ${JSON.stringify(fake.listed)}`);
  }
  if (candidates[0].insertText !== "./src/") {
    throw new Error(
      `directory insertText must keep slash: ${candidates[0].insertText}`,
    );
  }
});

Deno.test("path source excludes no-op labels dedupes orders and caps max items", async () => {
  const context: SayaCompletionTriggerContext = {
    buffer: {
      id: 7,
      path: "/workspace/main.ts",
      lineCount: 1,
      cursorRow: 0,
      cursorCol: 5,
      currentLine: "./src",
      text: "./src",
    },
    editor: { mode: "Insert" },
    reason: { kind: "manual" },
  };
  installSayaFake({ result: [] }, {}, {
    readDir: () => [
      { name: "src", kind: "file", path: "/workspace/src" },
      { name: "src-file.ts", kind: "file", path: "/workspace/src-file.ts" },
      { name: "src-file.ts", kind: "file", path: "/workspace/src-file.ts" },
      { name: "src-dir", kind: "directory", path: "/workspace/src-dir" },
      { name: "src-long", kind: "directory", path: "/workspace/src-long" },
    ],
  });

  const source = createPathCompletionSource({ maxItems: 3, optional: false });
  const query = source.trigger(context);
  if (!query) throw new Error("expected path query");
  const labels = (await source.complete(query)).candidates.map((candidate) =>
    candidate.label
  );
  const expected = ["./src-dir/", "./src-long/", "./src-file.ts"];
  if (JSON.stringify(labels) !== JSON.stringify(expected)) {
    throw new Error(`unexpected capped labels: ${JSON.stringify(labels)}`);
  }
});

Deno.test("path completion request reaches typed menu with path replace range", async () => {
  const fake = installSayaFake({ result: [] }, {
    path: "/workspace/main.ts",
    cursorCol: 5,
    currentLine: "./src",
    text: "./src",
  }, {
    readDir: () => [
      { name: "src", kind: "directory", path: "/workspace/src" },
      { name: "src-dir", kind: "directory", path: "/workspace/src-dir" },
      { name: "src-file.ts", kind: "file", path: "/workspace/src-file.ts" },
    ],
  });
  await setupSayaCompletion({
    sources: [createPathCompletionSource({ optional: false })],
    sourceTimeoutMs: 0,
    maxItems: 2,
  });

  const result = await fake.registered.get("completion.trigger")?.();
  if (result !== true) {
    throw new Error(`expected path completion menu, got ${String(result)}`);
  }
  const request = fake.shown[0] as any;
  if (request.selectedIndex !== 0) {
    throw new Error(`unexpected selectedIndex: ${request.selectedIndex}`);
  }
  if (
    request.replaceRange.start.character !== 0 ||
    request.replaceRange.end.character !== 5
  ) {
    throw new Error(
      `unexpected replaceRange: ${JSON.stringify(request.replaceRange)}`,
    );
  }
  const labels = request.candidates.map((candidate: any) => candidate.label);
  if (JSON.stringify(labels) !== JSON.stringify(["./src/", "./src-dir/"])) {
    throw new Error(`unexpected request labels: ${JSON.stringify(labels)}`);
  }
});

Deno.test("completion engine selects one replace range group by source order", async () => {
  const fake = installSayaFake({ result: [] }, {
    path: "/workspace/main.ts",
    cursorCol: 5,
    currentLine: "./src",
    text: "./src\nsrcBuffer\n",
  }, {
    readDir: () => [
      { name: "src", kind: "directory", path: "/workspace/src" },
      { name: "src-file.ts", kind: "file", path: "/workspace/src-file.ts" },
    ],
  });
  await setupSayaCompletion({
    sources: [
      createPathCompletionSource({ optional: false }),
      createBufferWordSource(),
    ],
    sourceTimeoutMs: 0,
  });

  const result = await fake.registered.get("completion.trigger")?.();
  if (result !== true) {
    throw new Error(`expected grouped path completion, got ${String(result)}`);
  }
  const request = fake.shown[0] as any;
  if (
    request.replaceRange.start.character !== 0 ||
    request.replaceRange.end.character !== 5
  ) {
    throw new Error(
      `expected path replaceRange group, got ${
        JSON.stringify(request.replaceRange)
      }`,
    );
  }
  const labels = request.candidates.map((candidate: any) => candidate.label);
  if (JSON.stringify(labels) !== JSON.stringify(["./src/", "./src-file.ts"])) {
    throw new Error(`unexpected grouped labels: ${JSON.stringify(labels)}`);
  }
});

Deno.test("LSP source maps completion response into typed completion menu", async () => {
  const fake = installSayaFake({
    method: "textDocument/completion",
    result: {
      items: [
        {
          label: "println",
          insertText: "println($0)",
          kind: 3,
          detail: "macro",
          documentation: { kind: "markdown", value: "Prints a line." },
          textEdit: {
            range: {
              start: { line: 0, character: 0 },
              end: { line: 0, character: 4 },
            },
            newText: "println($0)",
          },
        },
        {
          label: "logger.Println",
          insertText: "logger.Println($0)",
          kind: 2,
          detail: "func(v ...any)",
        },
      ],
    },
  });

  await setupSayaCompletion({
    sources: [createLspCompletionSource()],
    filters: [],
    sorters: [],
    sourceTimeoutMs: 0,
  });
  await fake.registered.get("completion.trigger")?.();

  if (!fake.executed.includes("lsp.completion")) {
    throw new Error(`expected lsp.completion, got ${fake.executed.join(",")}`);
  }
  if (fake.shown.length !== 1) {
    throw new Error(
      `expected one typed completion show, got ${fake.shown.length}`,
    );
  }
  const request = fake.shown[0] as any;
  if (
    request.replaceRange.start.character !== 0 ||
    request.replaceRange.end.character !== 4
  ) {
    throw new Error(
      `unexpected replaceRange: ${JSON.stringify(request.replaceRange)}`,
    );
  }
  if (request.candidates[0].label !== "println") {
    throw new Error(
      `unexpected candidate: ${JSON.stringify(request.candidates[0])}`,
    );
  }
  if (request.candidates[0].insertText !== "println($0)") {
    throw new Error(
      `unexpected insertText: ${request.candidates[0].insertText}`,
    );
  }
  if (request.candidates[0].kind !== "Function") {
    throw new Error(`unexpected candidate kind: ${request.candidates[0].kind}`);
  }
  if (
    !request.candidates.some((candidate: any) =>
      candidate.label === "logger.Println"
    )
  ) {
    throw new Error(
      `deep completion should be included by default: ${
        JSON.stringify(request.candidates)
      }`,
    );
  }
});

Deno.test("LSP source can hide deep completion candidates", async () => {
  const fake = installSayaFake({
    method: "textDocument/completion",
    result: {
      items: [
        { label: "Println", kind: 3, detail: "func(v ...any)" },
        {
          label: "Default().Println",
          kind: 2,
          detail: "func(v ...any)",
        },
      ],
    },
  }, {
    cursorCol: 3,
    currentLine: "Pri",
    text: "Pri\n",
  });

  await setupSayaCompletion({
    sources: [createLspCompletionSource({ includeDeepCompletions: false })],
    sourceTimeoutMs: 0,
  });
  await fake.registered.get("completion.trigger")?.();

  const labels = (fake.shown[0] as any).candidates.map((candidate: any) =>
    candidate.label
  );
  if (JSON.stringify(labels) !== JSON.stringify(["Println"])) {
    throw new Error(`unexpected filtered labels: ${JSON.stringify(labels)}`);
  }
});

Deno.test("ranking can prefer LSP direct candidates and keep deep completion last", async () => {
  const fake = installSayaFake({
    method: "textDocument/completion",
    result: {
      items: [
        { label: "Print", kind: 3, detail: "func(v ...any)" },
        {
          label: "Default().Print",
          kind: 2,
          detail: "func(v ...any)",
        },
      ],
    },
  }, {
    cursorCol: 3,
    currentLine: "Pri",
    text: "Pri\nPrintln\nPrinter\n",
  });

  await setupSayaCompletion({
    sources: [
      createLspCompletionSource({ minPrefixLength: 1 }),
      createBufferWordSource({ minPrefixLength: 1 }),
    ],
    ranking: {
      sourcePriority: ["lsp", "buffer"],
      deepCompletionPriority: "last",
      duplicateLabels: "preferFirstSource",
    },
    sourceTimeoutMs: 0,
  });
  await fake.registered.get("completion.trigger")?.();

  const labels = (fake.shown[0] as any).candidates.map((candidate: any) =>
    candidate.label
  );
  if (
    JSON.stringify(labels) !==
      JSON.stringify(["Print", "Println", "Printer", "Default().Print"])
  ) {
    throw new Error(`unexpected ranked labels: ${JSON.stringify(labels)}`);
  }
});

Deno.test("auto trigger disabled does not subscribe to buffer changes", async () => {
  const fake = installSayaFake({ result: [] }, {
    cursorCol: 3,
    currentLine: "pri",
    text: "pri\nprintln\n",
  });
  await setupSayaCompletion({
    autoTrigger: false,
    sources: [createBufferWordSource({ minPrefixLength: 2 })],
    sourceTimeoutMs: 0,
  });

  if (fake.events.has("bufferChanged")) {
    throw new Error("auto trigger disabled must not register bufferChanged");
  }
  if (fake.shown.length !== 0) {
    throw new Error(`unexpected menu count: ${fake.shown.length}`);
  }
});

Deno.test("auto trigger opens from insert buffer changes with debounce", async () => {
  const buffer = {
    cursorCol: 3,
    currentLine: "pri",
    text: "pri\nprintln\nprivate\n",
  };
  const fake = installSayaFake({ result: [] }, buffer);
  await setupSayaCompletion({
    autoTrigger: true,
    autoTriggerDelayMs: 5,
    sources: [createBufferWordSource({ minPrefixLength: 2 })],
    sourceTimeoutMs: 0,
  });
  executeRegisteredCommands(fake);

  const handler = fake.events.get("bufferChanged");
  if (!handler) throw new Error("expected bufferChanged subscription");
  handler({ buffer });
  handler({ buffer });
  await new Promise((resolve) => setTimeout(resolve, 20));

  if (fake.shown.length !== 1) {
    throw new Error(`expected one debounced menu, got ${fake.shown.length}`);
  }
  const labels = (fake.shown[0] as any).candidates.map((candidate: any) =>
    candidate.label
  );
  if (JSON.stringify(labels) !== JSON.stringify(["println", "private"])) {
    throw new Error(`unexpected auto labels: ${JSON.stringify(labels)}`);
  }
});

Deno.test("auto trigger respects insert mode and global and source min prefix lengths", async () => {
  const buffer = {
    cursorCol: 1,
    currentLine: "p",
    text: "p\nprintln\nprivate\n",
  };
  const fake = installSayaFake({ result: [] }, buffer);
  await setupSayaCompletion({
    autoTrigger: true,
    autoTriggerDelayMs: 0,
    minPrefixLength: 1,
    sources: [createBufferWordSource({ minPrefixLength: 2 })],
    sourceTimeoutMs: 0,
  });
  executeRegisteredCommands(fake);

  const handler = fake.events.get("bufferChanged");
  if (!handler) throw new Error("expected bufferChanged subscription");
  await handler({ buffer });
  if (fake.shown.length !== 0) {
    throw new Error("one-character buffer prefix must not auto open");
  }

  buffer.cursorCol = 2;
  buffer.currentLine = "pr";
  buffer.text = "pr\nprintln\nprivate\n";
  await handler({ buffer });
  if (shownLength(fake) !== 1) {
    throw new Error("two-character buffer prefix must auto open");
  }

  (globalThis as any).saya.editor.current = () =>
    Promise.resolve({ mode: "Normal" });
  buffer.cursorCol = 3;
  buffer.currentLine = "pri";
  buffer.text = "pri\nprintln\nprivate\n";
  await handler({ buffer });
  if (shownLength(fake) !== 1) {
    throw new Error("auto trigger must not open outside insert mode");
  }
});

Deno.test("auto trigger closes existing menu when prefix falls below source min length", async () => {
  const buffer = {
    cursorCol: 2,
    currentLine: "pr",
    text: "pr\nprintln\nprivate\n",
  };
  const fake = installSayaFake({ result: [] }, buffer);
  await setupSayaCompletion({
    autoTrigger: true,
    autoTriggerDelayMs: 0,
    minPrefixLength: 1,
    sources: [createBufferWordSource({ minPrefixLength: 2 })],
    sourceTimeoutMs: 0,
  });
  executeRegisteredCommands(fake);

  const handler = fake.events.get("bufferChanged");
  if (!handler) throw new Error("expected bufferChanged subscription");
  await handler({ buffer });
  if (fake.shown.length !== 1) {
    throw new Error("two-character buffer prefix must auto open");
  }

  fake.setFloats([{ id: 42, kind: "completionMenu" }]);
  buffer.cursorCol = 1;
  buffer.currentLine = "p";
  buffer.text = "p\nprintln\nprivate\n";
  await handler({ buffer });
  if (fake.closed.length !== 1) {
    throw new Error(
      `expected stale completion menu to close, got ${
        JSON.stringify(fake.closed)
      }`,
    );
  }
});

Deno.test("startup-safe auto trigger closes stale menu when no source is configured", async () => {
  const buffer = {
    cursorCol: 1,
    currentLine: "p",
    text: "p\nprintln\nprivate\n",
  };
  const fake = installSayaFake({ result: [] }, buffer);
  await setupSayaCompletion({
    autoTrigger: true,
    autoTriggerDelayMs: 0,
    sourceTimeoutMs: 0,
  });
  (globalThis as any).saya.commands.execute = (name: string) => {
    if (name === "lsp.completion") return Promise.resolve({ result: [] });
    const callback = fake.registered.get(name);
    if (!callback) throw new Error(`missing command: ${name}`);
    return callback();
  };
  fake.setFloats([{ id: 7, kind: "completionMenu" }]);

  const handler = fake.events.get("bufferChanged");
  if (!handler) throw new Error("expected bufferChanged subscription");
  await handler({ buffer });
  if (fake.shown.length !== 0) {
    throw new Error("no-source auto trigger must not open a menu");
  }
  if (fake.closed.length !== 1) {
    throw new Error(
      `expected generated auto callback to close stale menu, got ${
        JSON.stringify(fake.closed)
      }`,
    );
  }
});

Deno.test("explicit startup-safe auto trigger closes stale menu outside insert mode", async () => {
  const buffer = {
    cursorCol: 2,
    currentLine: "ty",
    text: "ty\ntype\n",
  };
  const fake = installSayaFake({ result: [] }, buffer);
  await setupSayaCompletion({
    autoTrigger: true,
    autoTriggerDelayMs: 0,
    sources: [createBufferWordSource({ minPrefixLength: 2 })],
    sourceTimeoutMs: 0,
  });
  executeRegisteredCommands(fake);

  const handler = fake.events.get("bufferChanged");
  if (!handler) throw new Error("expected bufferChanged subscription");
  await handler({ buffer });
  if (fake.shown.length !== 1) {
    throw new Error("insert-mode auto trigger should open before mode changes");
  }

  fake.setFloats([{ id: 11, kind: "completionMenu" }]);
  (globalThis as any).saya.editor.current = () =>
    Promise.resolve({ mode: "Normal" });
  await handler({ buffer });
  if (fake.closed.length !== 1) {
    throw new Error(
      `expected generated auto callback to close menu outside insert mode, got ${
        JSON.stringify(fake.closed)
      }`,
    );
  }
});

Deno.test("custom auto trigger closes stale menu outside insert mode", async () => {
  const buffer = {
    cursorCol: 2,
    currentLine: "ty",
    text: "ty\ntype\n",
  };
  const fake = installSayaFake({ result: [] }, buffer);
  await setupSayaCompletion({
    autoTrigger: true,
    autoTriggerDelayMs: 0,
    sources: [createBufferWordSource({ minPrefixLength: 2 })],
    sourceTimeoutMs: 0,
  });
  executeRegisteredCommands(fake);

  const handler = fake.events.get("bufferChanged");
  if (!handler) throw new Error("expected bufferChanged subscription");
  await handler({ buffer });
  if (fake.shown.length !== 1) {
    throw new Error(
      "insert-mode custom auto trigger should open before mode changes",
    );
  }

  fake.setFloats([{ id: 13, kind: "completionMenu" }]);
  (globalThis as any).saya.editor.current = () =>
    Promise.resolve({ mode: "Normal" });
  await handler({ buffer });
  if (fake.closed.length !== 1) {
    throw new Error(
      `expected custom auto callback to close menu outside insert mode, got ${
        JSON.stringify(fake.closed)
      }`,
    );
  }
});

Deno.test("explicit path auto trigger closes existing menu when path prefix is removed", async () => {
  const buffer = {
    path: "/workspace/main.ts",
    cursorCol: 1,
    currentLine: "/",
    text: "/",
  };
  const fake = installSayaFake({ result: [] }, buffer, {
    readDir: (path: string) => {
      if (path !== "/") throw new Error(`unexpected path: ${path}`);
      return [{ name: "usr", kind: "directory", path: "/usr" }];
    },
  });
  await setupSayaCompletion({
    autoTrigger: true,
    autoTriggerDelayMs: 0,
    sources: [
      createPathCompletionSource({
        minPrefixLength: 1,
        triggerCharacters: ["/", "."],
      }),
    ],
    sourceTimeoutMs: 0,
  });
  executeRegisteredCommands(fake);

  const handler = fake.events.get("bufferChanged");
  if (!handler) throw new Error("expected bufferChanged subscription");
  await handler({ buffer });
  if (fake.shown.length !== 1) {
    throw new Error("slash should auto open path completion");
  }

  fake.setFloats([{ id: 12, kind: "completionMenu" }]);
  buffer.cursorCol = 0;
  buffer.currentLine = "";
  buffer.text = "";
  await handler({ buffer });
  if (fake.closed.length !== 1) {
    throw new Error(
      `expected stale path completion menu to close, got ${
        JSON.stringify(fake.closed)
      }`,
    );
  }
});

Deno.test("auto trigger characters can bypass min prefix length", async () => {
  const fake = installSayaFake({ result: [] }, {
    cursorCol: 1,
    currentLine: ".",
    text: ".",
  });
  await setupSayaCompletion({
    autoTrigger: true,
    autoTriggerDelayMs: 0,
    minPrefixLength: 2,
    sources: [{
      id: "dot",
      triggerCharacters: ["."],
      trigger: (
        context: SayaCompletionTriggerContext,
      ): SayaCompletionQuery => ({
        ...context,
        sourceId: "dot",
        prefix: "",
        replaceRange: {
          start: { line: 0, character: 1 },
          end: { line: 0, character: 1 },
        },
      }),
      complete: (query: SayaCompletionQuery) => ({
        sourceId: query.sourceId,
        prefix: query.prefix,
        replaceRange: query.replaceRange,
        candidates: [{ label: "member", insertText: "member" }],
      }),
    }],
    filters: [],
    sorters: [],
    sourceTimeoutMs: 0,
  });
  executeRegisteredCommands(fake);

  const handler = fake.events.get("bufferChanged");
  if (!handler) throw new Error("expected bufferChanged subscription");
  await handler({
    buffer: {
      cursorCol: 1,
      currentLine: ".",
    },
  });
  if (fake.shown.length !== 1) {
    throw new Error("trigger character must open auto completion");
  }
});

Deno.test("manual trigger is not gated by trigger characters", async () => {
  const fake = installSayaFake({ result: [] }, {
    cursorCol: 1,
    currentLine: "x",
    text: "x",
  });
  await setupSayaCompletion({
    sources: [{
      id: "manual",
      triggerCharacters: ["."],
      trigger: (
        context: SayaCompletionTriggerContext,
      ): SayaCompletionQuery => ({
        ...context,
        sourceId: "manual",
        prefix: "x",
        replaceRange: {
          start: { line: 0, character: 0 },
          end: { line: 0, character: 1 },
        },
      }),
      complete: (query: SayaCompletionQuery) => ({
        sourceId: query.sourceId,
        prefix: query.prefix,
        replaceRange: query.replaceRange,
        candidates: [{ label: "xray", insertText: "xray" }],
      }),
    }],
    filters: [],
    sorters: [],
    sourceTimeoutMs: 0,
  });

  const result = await fake.registered.get("completion.trigger")?.();
  if (result !== true || fake.shown.length !== 1) {
    throw new Error("manual trigger must run without trigger character match");
  }
});

Deno.test("path completion auto opens for slash and relative prefixes", async () => {
  const buffer = {
    path: "/workspace/main.ts",
    cursorCol: 1,
    currentLine: "/",
    text: "/",
  };
  const fake = installSayaFake({ result: [] }, buffer, {
    readDir: (path: string) => {
      if (path === "/") {
        return [{ name: "usr", kind: "directory", path: "/usr" }];
      }
      if (path === "/workspace") {
        return [{ name: "src", kind: "directory", path: "/workspace/src" }];
      }
      throw new Error(`unexpected list path: ${path}`);
    },
  });
  await setupSayaCompletion({
    autoTrigger: true,
    autoTriggerDelayMs: 0,
    sources: [
      createPathCompletionSource({
        optional: false,
        minPrefixLength: 1,
        triggerCharacters: ["/", "."],
      }),
    ],
    sourceTimeoutMs: 0,
  });
  executeRegisteredCommands(fake);

  const handler = fake.events.get("bufferChanged");
  if (!handler) throw new Error("expected bufferChanged subscription");
  await handler({ buffer });

  buffer.cursorCol = 2;
  buffer.currentLine = "./";
  buffer.text = "./";
  await handler({ buffer });

  if (fake.shown.length !== 2) {
    throw new Error(`expected two path auto menus, got ${fake.shown.length}`);
  }
  const labels = fake.shown.map((request: any) =>
    request.candidates.map((candidate: any) => candidate.label)
  );
  if (
    JSON.stringify(labels) !== JSON.stringify([
      ["/usr/"],
      ["./src/"],
    ])
  ) {
    throw new Error(`unexpected path labels: ${JSON.stringify(labels)}`);
  }
});

Deno.test("completion source timeout drops slow external sources", async () => {
  const fake = installSayaFake({ result: [] });
  await setupSayaCompletion({
    sources: [{
      id: "slow",
      trigger: (
        context: SayaCompletionTriggerContext,
      ): SayaCompletionQuery => ({
        ...context,
        sourceId: "slow",
        prefix: "prin",
        replaceRange: {
          start: { line: 0, character: 0 },
          end: { line: 0, character: 4 },
        },
      }),
      complete: () => new Promise(() => {}),
    }],
    filters: [],
    sorters: [],
    sourceTimeoutMs: 1,
  });
  const result = await fake.registered.get("completion.trigger")?.();
  if (result !== false) {
    throw new Error(
      `expected timeout to produce no menu, got ${String(result)}`,
    );
  }
  if (fake.shown.length !== 0) {
    throw new Error(
      `timed out source must not open menu, got ${fake.shown.length}`,
    );
  }
});
