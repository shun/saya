// deno-lint-ignore-file no-explicit-any

import {
  createBufferWordSource,
  createLspCompletionSource,
  labelSorter,
  prefixFilter,
  setupSayaCompletion,
} from "./index.ts";
import type { SayaCompletionContext } from "./types.ts";

function installSayaFake(response: unknown, buffer?: Record<string, unknown>) {
  const executed: string[] = [];
  const registered = new Map<string, () => unknown>();
  const shown: unknown[] = [];
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
    keymap: {
      set() {},
    },
  };
  return { executed, registered, shown };
}

Deno.test("buffer source collects filtered deduped words ordered near the cursor", () => {
  const context: SayaCompletionContext = {
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
    prefix: "pri",
    replaceRange: {
      start: { line: 2, character: 0 },
      end: { line: 2, character: 3 },
    },
  };

  const source = createBufferWordSource();
  const candidates = labelSorter(
    prefixFilter(source.complete(context) as any, context),
  );
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
      name: "slow",
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
