// deno-lint-ignore-file no-explicit-any

(globalThis as any).__lspUtf8 = {};
(globalThis as any).__lspJsonRpc = {
  createCancelToken() {
    const controller = new AbortController();
    return {
      signal: controller.signal,
      cancel(reason?: unknown) {
        controller.abort(reason);
      },
    };
  },
};
(globalThis as any).__lspTransport = {};
(globalThis as any).__lspLifecycle = {};
(globalThis as any).__lspSession = {};

const { setupSayaLspClient } = await import("./index.ts");

type NotifyCall = { method: string; params: any };

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function installSayaFake(
  session: {
    notify: (method: string, params: any) => Promise<unknown>;
    request?: (
      method: string,
      params: any,
      options?: unknown,
    ) => Promise<unknown>;
    takeNotifications?: () => unknown[];
    initializeResult?: unknown;
  },
  buffer: Record<string, unknown> = {},
) {
  const commands = new Map<string, (payload?: unknown) => unknown>();
  const events = new Map<string, (payload: unknown) => unknown>();
  const executed: string[] = [];

  delete (globalThis as any).__sayaLspManager;
  delete (globalThis as any).__sayaLspDocumentSyncState;
  delete (globalThis as any).__sayaLspServerState;

  (globalThis as any).saya = {
    buffer: {
      current: () =>
        Promise.resolve({
          id: 1,
          path: "/workspace/main.ts",
          lineCount: 1,
          cursorRow: 0,
          cursorCol: 0,
          currentLine: "",
          text: "",
          ...buffer,
        }),
    },
    editor: {
      current: () => Promise.resolve({ mode: "Normal" }),
    },
    commands: {
      register(name: string, callback: (payload?: unknown) => unknown) {
        commands.set(name, callback);
      },
      execute(name: string) {
        executed.push(name);
        return Promise.resolve(true);
      },
    },
    events: {
      on(name: string, callback: (payload: unknown) => unknown) {
        events.set(name, callback);
      },
    },
    lsp: {
      connect() {
        return Promise.resolve({
          initializeResult: session.initializeResult ?? { capabilities: {} },
          notify: session.notify,
          takeNotifications: session.takeNotifications ?? (() => []),
          request: session.request ?? (() => Promise.resolve(null)),
          close: () => Promise.resolve(undefined),
        });
      },
    },
  };

  return { commands, events, executed };
}

function setupBufferEvents(
  session: Parameters<typeof installSayaFake>[0],
  buffer: Record<string, unknown> = {},
) {
  const fake = installSayaFake(session, buffer);
  setupSayaLspClient({
    enableBufferEvents: true,
    languageId: "typescript",
    servers: [{
      name: "tsserver",
      command: "fake-language-server",
      languages: ["typescript"],
      filePatterns: ["**/*.ts"],
    }],
  });
  return fake;
}

Deno.test("LSP bufferChanged enqueues document sync without awaiting notify", async () => {
  const releases: Array<() => void> = [];
  const calls: NotifyCall[] = [];
  const fake = setupBufferEvents({
    notify(method, params) {
      calls.push({ method, params });
      return new Promise((resolve) => {
        releases.push(() => resolve(undefined));
      });
    },
  });

  const changed = fake.events.get("bufferChanged");
  if (!changed) throw new Error("bufferChanged handler was not registered");

  const result = await Promise.race([
    Promise.resolve(changed({
      buffer: {
        path: "/workspace/main.ts",
        text: "const value = 1;\n",
        currentLine: "const value = 1;",
      },
    })),
    delay(30).then(() => "timed-out"),
  ]);

  if (result === "timed-out") {
    throw new Error("bufferChanged handler waited for LSP notify");
  }

  await delay(180);
  if (calls.length !== 1 || calls[0].method !== "textDocument/didOpen") {
    throw new Error(`unexpected notify calls: ${JSON.stringify(calls)}`);
  }

  releases.shift()?.();
  await delay(0);
  if (calls.length !== 2 || calls[1].method !== "textDocument/didChange") {
    throw new Error(
      `expected didChange after didOpen release: ${JSON.stringify(calls)}`,
    );
  }
  releases.shift()?.();
  await delay(0);
});

Deno.test("LSP document sync works without setTimeout in live runtime", async () => {
  const originalSetTimeout = (globalThis as any).setTimeout;
  const originalClearTimeout = (globalThis as any).clearTimeout;
  try {
    delete (globalThis as any).setTimeout;
    delete (globalThis as any).clearTimeout;

    const calls: NotifyCall[] = [];
    const fake = setupBufferEvents({
      notify(method, params) {
        calls.push({ method, params });
        return Promise.resolve(undefined);
      },
    });

    const opened = fake.events.get("bufferOpen");
    const changed = fake.events.get("bufferChanged");
    if (!opened || !changed) {
      throw new Error("buffer event handlers were not registered");
    }

    await opened({
      buffer: {
        path: "/workspace/main.ts",
        text: "initial\n",
        currentLine: "initial",
      },
    });
    await changed({
      buffer: {
        path: "/workspace/main.ts",
        text: "changed\n",
        currentLine: "changed",
      },
    });
    await new Promise((resolve) => originalSetTimeout(resolve, 0));

    const methods = calls.map((call) => call.method);
    if (
      JSON.stringify(methods) !==
        JSON.stringify(["textDocument/didOpen", "textDocument/didChange"])
    ) {
      throw new Error(
        `unexpected no-timer notify order: ${JSON.stringify(methods)}`,
      );
    }
  } finally {
    (globalThis as any).setTimeout = originalSetTimeout;
    (globalThis as any).clearTimeout = originalClearTimeout;
  }
});

Deno.test("LSP bufferChanged still returns immediately without setTimeout", async () => {
  const originalSetTimeout = (globalThis as any).setTimeout;
  const originalClearTimeout = (globalThis as any).clearTimeout;
  try {
    delete (globalThis as any).setTimeout;
    delete (globalThis as any).clearTimeout;

    let notifyStarted = false;
    const fake = setupBufferEvents({
      notify() {
        notifyStarted = true;
        return new Promise(() => {});
      },
    });

    const changed = fake.events.get("bufferChanged");
    if (!changed) throw new Error("bufferChanged handler was not registered");

    const result = await Promise.resolve(changed({
      buffer: {
        path: "/workspace/main.ts",
        text: "const value = 1;\n",
        currentLine: "const value = 1;",
      },
    }));

    if ((result as any)?.method !== "textDocument/didChange") {
      throw new Error(`unexpected handler result: ${JSON.stringify(result)}`);
    }
    await new Promise((resolve) => originalSetTimeout(resolve, 0));
    if (!notifyStarted) {
      throw new Error("expected background microtask flush to start notify");
    }
  } finally {
    (globalThis as any).setTimeout = originalSetTimeout;
    (globalThis as any).clearTimeout = originalClearTimeout;
  }
});

Deno.test("LSP document sync coalesces pending didChange to the latest text", async () => {
  const calls: NotifyCall[] = [];
  const fake = setupBufferEvents({
    notify(method, params) {
      calls.push({ method, params });
      return Promise.resolve(undefined);
    },
  });

  const changed = fake.events.get("bufferChanged");
  if (!changed) throw new Error("bufferChanged handler was not registered");

  await changed({
    buffer: {
      path: "/workspace/main.ts",
      text: "old\n",
      currentLine: "old",
    },
  });
  await changed({
    buffer: {
      path: "/workspace/main.ts",
      text: "new\n",
      currentLine: "new",
    },
  });

  await delay(180);
  const methods = calls.map((call) => call.method);
  if (
    JSON.stringify(methods) !==
      JSON.stringify(["textDocument/didOpen", "textDocument/didChange"])
  ) {
    throw new Error(`unexpected notify order: ${JSON.stringify(methods)}`);
  }
  const didChangeCalls = calls.filter((call) =>
    call.method === "textDocument/didChange"
  );
  if (didChangeCalls.length !== 1) {
    throw new Error(`expected one didChange, got ${JSON.stringify(calls)}`);
  }
  const text = didChangeCalls[0].params.contentChanges[0].text;
  if (text !== "new\n") {
    throw new Error(`expected latest text, got ${JSON.stringify(text)}`);
  }
});

Deno.test("LSP document sync sends didOpen before pending didChange", async () => {
  const calls: NotifyCall[] = [];
  const fake = setupBufferEvents({
    notify(method, params) {
      calls.push({ method, params });
      return Promise.resolve(undefined);
    },
  });

  const opened = fake.events.get("bufferOpen");
  const changed = fake.events.get("bufferChanged");
  if (!opened || !changed) {
    throw new Error("buffer event handlers were not registered");
  }

  await opened({
    buffer: {
      path: "/workspace/main.ts",
      text: "initial\n",
      currentLine: "initial",
    },
  });
  await changed({
    buffer: {
      path: "/workspace/main.ts",
      text: "changed\n",
      currentLine: "changed",
    },
  });

  await delay(180);
  const methods = calls.map((call) => call.method);
  if (
    JSON.stringify(methods) !==
      JSON.stringify(["textDocument/didOpen", "textDocument/didChange"])
  ) {
    throw new Error(`unexpected notify order: ${JSON.stringify(methods)}`);
  }
});

Deno.test("LSP bufferWritePost flushes pending didChange before didSave", async () => {
  const calls: NotifyCall[] = [];
  const fake = setupBufferEvents({
    notify(method, params) {
      calls.push({ method, params });
      return Promise.resolve(undefined);
    },
  });

  const changed = fake.events.get("bufferChanged");
  const saved = fake.events.get("bufferWritePost");
  if (!changed || !saved) {
    throw new Error("buffer event handlers were not registered");
  }

  await changed({
    buffer: {
      path: "/workspace/main.ts",
      text: "saved\n",
      currentLine: "saved",
    },
  });
  await saved({
    buffer: {
      path: "/workspace/main.ts",
      text: "saved\n",
      currentLine: "saved",
    },
  });

  await delay(20);
  const methods = calls.map((call) => call.method);
  if (
    JSON.stringify(methods) !==
      JSON.stringify([
        "textDocument/didOpen",
        "textDocument/didChange",
        "textDocument/didSave",
      ])
  ) {
    throw new Error(`unexpected notify order: ${JSON.stringify(methods)}`);
  }
});

Deno.test("LSP completion flushes pending document sync before request", async () => {
  const calls: Array<
    NotifyCall | { method: string; kind: "request"; params: any }
  > = [];
  const fake = setupBufferEvents({
    notify(method, params) {
      calls.push({ method, params });
      return Promise.resolve(undefined);
    },
    request(method, params) {
      calls.push({ kind: "request", method, params });
      return Promise.resolve({ items: [] });
    },
  }, {
    path: "/workspace/main.ts",
    text: "setup",
    currentLine: "setup",
    cursorCol: 5,
  });

  const changed = fake.events.get("bufferChanged");
  const completion = fake.commands.get("lsp.completion");
  if (!changed || !completion) {
    throw new Error("expected bufferChanged and lsp.completion handlers");
  }

  await changed({
    buffer: {
      path: "/workspace/main.ts",
      text: "setup",
      currentLine: "setup",
      cursorCol: 5,
    },
  });
  await completion();

  const methods = calls.map((call) => call.method);
  if (
    JSON.stringify(methods) !==
      JSON.stringify([
        "textDocument/didOpen",
        "textDocument/didChange",
        "textDocument/completion",
      ])
  ) {
    throw new Error(`unexpected call order: ${JSON.stringify(calls)}`);
  }
});
