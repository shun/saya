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
  const shown: unknown[] = [];
  const listed: Array<{ path: string; options?: unknown }> = [];
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
          `completion tests must not use side-effectful saya.filer.list: ${path} ${JSON.stringify(options)}`,
        );
      },
    },
    keymap: {
      set() {},
    },
  };
  return { executed, registered, shown, listed };
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
  if (fake.shown.length !== 1) {
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
});

Deno.test("bundled completion command source runs without startup closure state", async () => {
  const fake = installSayaFake({ result: [] }, {
    cursorCol: 3,
    currentLine: "pri",
    text: "pri\nprintln\nprivate\n",
  });
  await setupSayaCompletion({ sourceTimeoutMs: 0 });

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
