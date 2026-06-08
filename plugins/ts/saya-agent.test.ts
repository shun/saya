import {
  normalizeSayaAgentConfig,
  renderCurrentFilePrompt,
  renderCurrentLinePrompt,
  renderSelectedRangePrompt,
  setupSayaAgent,
} from "./saya-agent.ts";

Deno.test("Saya agent normalizes Codex-first tool presets with Gemini and Claude available", () => {
  const config = normalizeSayaAgentConfig();

  if (config.defaultTool !== "codex") {
    throw new Error(`default tool should be codex: ${config.defaultTool}`);
  }
  if (
    JSON.stringify(config.tools.codex.command) !== JSON.stringify(["codex"])
  ) {
    throw new Error("codex command was not normalized");
  }
  if (!config.tools.gemini || !config.tools.claude) {
    throw new Error("gemini and claude presets should be available");
  }
  if (config.layout.position !== "right" || config.layout.size !== "35%") {
    throw new Error("default layout should be a right 35% panel");
  }
});

Deno.test("Saya agent prompt renderers include file line and selected range context without clipboard text", () => {
  const filePrompt = renderCurrentFilePrompt({
    path: "src/main.rs",
    text: "fn main() {}\n",
  });
  const linePrompt = renderCurrentLinePrompt({
    path: "src/main.rs",
    cursorRow: 4,
    currentLine: "let answer = 42;",
  });
  const rangePrompt = renderSelectedRangePrompt(
    { path: "src/main.rs" },
    { startLine: 2, endLine: 4, text: "alpha\nbeta\n" },
  );

  for (const prompt of [filePrompt, linePrompt, rangePrompt]) {
    if (prompt.toLowerCase().includes("clipboard")) {
      throw new Error(
        `prompt should not require clipboard copy/paste: ${prompt}`,
      );
    }
  }
  if (!filePrompt.includes("src/main.rs") || !filePrompt.includes("fn main")) {
    throw new Error("file prompt missing path or text");
  }
  if (
    !linePrompt.includes("src/main.rs:5") || !linePrompt.includes("let answer")
  ) {
    throw new Error("line prompt missing 1-based line context");
  }
  if (
    !rangePrompt.includes("src/main.rs:3-5") || !rangePrompt.includes("alpha")
  ) {
    throw new Error("range prompt missing selected range context");
  }
});

Deno.test("setupSayaAgent registers panel-backed commands that send buffer context", async () => {
  const commands = new Map<string, () => Promise<void> | void>();
  const opened: unknown[] = [];
  const sent: Array<[string, string]> = [];
  const focused: string[] = [];
  const closed: string[] = [];

  (globalThis as any).saya = {
    commands: {
      register(name: string, callback: () => Promise<void> | void) {
        commands.set(name, callback);
      },
    },
    panel: {
      async open(options: unknown) {
        opened.push(options);
        return { id: "ai-agent", kind: "terminal", focused: true };
      },
      async list() {
        return opened.length > 0 ? [{ id: "ai-agent", kind: "terminal" }] : [];
      },
      async send(id: string, text: string) {
        sent.push([id, text]);
        return true;
      },
      async focus(id: string) {
        focused.push(id);
        return true;
      },
      async close(id: string) {
        closed.push(id);
        return true;
      },
    },
    buffer: {
      async current() {
        return {
          path: "README.md",
          cursorRow: 1,
          currentLine: "hello",
          text: "hello\nworld\n",
        };
      },
      async selection() {
        return { startLine: 0, endLine: 1, text: "hello\n" };
      },
    },
  };

  setupSayaAgent({
    promptLibrary: [{ name: "review", prompt: "Please review\n" }],
  });

  await commands.get("panel.focus")?.();
  await commands.get("agent.sendCurrentFile")?.();
  await commands.get("agent.sendCurrentLine")?.();
  await commands.get("agent.sendSelectedRange")?.();
  await commands.get("agent.sendPrompt")?.();

  if (
    !commands.has("panel.toggle") ||
    !commands.has("panel.unfocus") ||
    !commands.has("panel.close")
  ) {
    throw new Error("panel open/close commands were not registered");
  }
  if (opened.length !== 1) {
    throw new Error(`agent should open one reusable panel: ${opened.length}`);
  }
  if (!(opened[0] as { focus?: boolean }).focus) {
    throw new Error("focus command did not request focused panel open");
  }
  if (closed.length !== 0) {
    throw new Error("send commands should not close/detach the panel");
  }
  if (sent.length !== 4) {
    throw new Error(`expected four sends, got ${sent.length}`);
  }
  if (!sent[0][1].includes("README.md") || !sent[0][1].includes("world")) {
    throw new Error("current file context was not sent");
  }
  if (!sent[1][1].includes("README.md:2") || !sent[1][1].includes("hello")) {
    throw new Error("current line context was not sent");
  }
  if (!sent[2][1].includes("README.md:1-2") || !sent[2][1].includes("hello")) {
    throw new Error("selected range context was not sent");
  }
  if (sent[3][1] !== "Please review\n") {
    throw new Error("prompt-library entry was not sent");
  }
});

Deno.test("panel.toggle opens without stealing editor focus", async () => {
  const commands = new Map<string, () => Promise<void> | void>();
  const opened: Array<{ focus?: boolean }> = [];

  (globalThis as any).saya = {
    commands: {
      register(name: string, callback: () => Promise<void> | void) {
        commands.set(name, callback);
      },
    },
    panel: {
      async open(options: { focus?: boolean }) {
        opened.push(options);
        return { id: "ai-agent", kind: "terminal", focused: options.focus };
      },
      async list() {
        return [];
      },
      async close() {
        return true;
      },
      async focus() {
        return true;
      },
      async unfocus() {
        return true;
      },
      async send() {
        return true;
      },
    },
    buffer: {
      async current() {
        return { path: null, cursorRow: 0, currentLine: "", text: "" };
      },
      async selection() {
        return null;
      },
    },
  };

  setupSayaAgent();

  if (commands.has("agent.toggle") || commands.has("agent.close")) {
    throw new Error("legacy agent panel aliases should not be registered");
  }

  await commands.get("panel.toggle")?.();

  if (opened.length !== 1) {
    throw new Error(`panel.toggle should open one panel, got ${opened.length}`);
  }
  if (opened[0].focus !== false) {
    throw new Error("panel.toggle should not steal editor focus");
  }
});
