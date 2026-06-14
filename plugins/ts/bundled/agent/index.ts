declare const saya: any;

export type SayaAgentToolName = "codex" | "gemini" | "claude" | string;

export interface SayaAgentToolSpec {
  command: string[];
}

export interface SayaAgentToolMap {
  [name: string]: SayaAgentToolSpec;
}

export interface SayaAgentLayout {
  position?: "left" | "right" | "top" | "bottom";
  size?: string | number;
}

export interface SayaAgentPromptEntry {
  name: string;
  prompt: string;
}

export type SayaAgentCallback = (...args: unknown[]) => unknown;

export interface SayaAgentSelectedRange {
  startLine: number;
  endLine: number;
  text: string;
}

export interface SayaAgentPathBuffer {
  path?: string | null;
}

export interface SayaAgentFileBuffer extends SayaAgentPathBuffer {
  text: string;
}

export interface SayaAgentLineBuffer extends SayaAgentPathBuffer {
  cursorRow: number;
  currentLine: string;
}

export interface SayaAgentPanelSummary {
  id: string;
}

export interface SayaAgentSetupOptions {
  id?: string;
  defaultTool?: SayaAgentToolName;
  layout?: SayaAgentLayout;
  tools?: Record<string, SayaAgentToolSpec>;
  commands?: Partial<
    Record<
      | "toggle"
      | "focus"
      | "unfocus"
      | "close"
      | "detach"
      | "sendCurrentFile"
      | "sendCurrentLine"
      | "sendSelectedRange"
      | "sendPrompt",
      string
    >
  >;
  promptLibrary?: SayaAgentPromptEntry[];
  selectedRange?:
    | (() => Promise<SayaAgentSelectedRange | null>)
    | SayaAgentSelectedRange
    | null;
}

export interface NormalizedSayaAgentConfig {
  id: string;
  defaultTool: string;
  layout: Required<SayaAgentLayout>;
  tools: SayaAgentToolMap;
  commands: Required<NonNullable<SayaAgentSetupOptions["commands"]>>;
  promptLibrary: SayaAgentPromptEntry[];
}

const defaultCommands = {
  toggle: "panel.toggle",
  focus: "panel.focus",
  unfocus: "panel.unfocus",
  close: "panel.close",
  detach: "panel.detach",
  sendCurrentFile: "agent.sendCurrentFile",
  sendCurrentLine: "agent.sendCurrentLine",
  sendSelectedRange: "agent.sendSelectedRange",
  sendPrompt: "agent.sendPrompt",
};

export function normalizeSayaAgentConfig(
  options: SayaAgentSetupOptions = {},
): NormalizedSayaAgentConfig {
  let tools: SayaAgentToolMap;
  tools = {
    codex: { command: ["codex"] },
    gemini: { command: ["gemini"] },
    claude: { command: ["claude"] },
    ...(options.tools ?? {}),
  };
  const defaultTool = String(options.defaultTool ?? "codex");
  if (!tools[defaultTool]) {
    throw new Error(`unknown default Saya agent tool: ${defaultTool}`);
  }
  const id = options.id ?? "ai-agent";
  const optionLayout = options.layout ?? {};
  const position = optionLayout.position ?? "right";
  const size = optionLayout.size ?? "35%";
  const layout = { position, size };
  const commands = { ...defaultCommands, ...(options.commands ?? {}) };
  const promptLibrary = options.promptLibrary ?? [];
  return {
    id,
    defaultTool,
    layout,
    tools,
    commands,
    promptLibrary,
  };
}

export function renderCurrentFilePrompt(buffer: SayaAgentFileBuffer): string {
  return [
    `Review the current file: ${buffer.path ?? "[No Name]"}`,
    "",
    "```",
    buffer.text,
    "```",
    "",
  ].join("\n");
}

export function renderCurrentLinePrompt(buffer: SayaAgentLineBuffer): string {
  return [
    `Review the current line in ${buffer.path ?? "[No Name]"}:${
      buffer.cursorRow + 1
    }`,
    "",
    buffer.currentLine,
    "",
  ].join("\n");
}

export function renderSelectedRangePrompt(
  buffer: SayaAgentPathBuffer,
  range: SayaAgentSelectedRange,
): string {
  return [
    `Review the selected range in ${buffer.path ?? "[No Name]"}:${
      range.startLine + 1
    }-${range.endLine + 1}`,
    "",
    "```",
    range.text,
    "```",
    "",
  ].join("\n");
}

export function setupSayaAgent(
  options: SayaAgentSetupOptions = {},
): NormalizedSayaAgentConfig {
  const config = normalizeSayaAgentConfig(options);
  register(config.commands.toggle, callbackSource(config, "toggle"));
  register(config.commands.focus, callbackSource(config, "focus"));
  register(config.commands.unfocus, callbackSource(config, "unfocus"));
  register(config.commands.close, callbackSource(config, "close"));
  register(config.commands.detach, callbackSource(config, "detach"));
  register(
    config.commands.sendCurrentFile,
    callbackSource(config, "sendCurrentFile"),
  );
  register(
    config.commands.sendCurrentLine,
    callbackSource(config, "sendCurrentLine"),
  );
  register(
    config.commands.sendSelectedRange,
    callbackSource(config, "sendSelectedRange"),
  );
  register(config.commands.sendPrompt, callbackSource(config, "sendPrompt"));
  return config;
}

function register(name: string, callback: SayaAgentCallback): void {
  sayaHost().commands.register(name, callback);
}

function callbackSource(
  config: NormalizedSayaAgentConfig,
  action: string,
): any {
  const encoded = JSON.stringify(config);
  const source = `
    const config = ${encoded};
    const bufferPromptFile = (buffer) => [
      \`Review the current file: \${buffer.path ?? "[No Name]"}\`,
      "",
      "\`\`\`",
      buffer.text,
      "\`\`\`",
      "",
    ].join("\\n");
    const bufferPromptLine = (buffer) => [
      \`Review the current line in \${buffer.path ?? "[No Name]"}:\${buffer.cursorRow + 1}\`,
      "",
      buffer.currentLine,
      "",
    ].join("\\n");
    const selectionPrompt = (buffer, range) => [
      \`Review the selected range in \${buffer.path ?? "[No Name]"}:\${range.startLine + 1}-\${range.endLine + 1}\`,
      "",
      "\`\`\`",
      range.text,
      "\`\`\`",
      "",
    ].join("\\n");
    const ensurePanel = async (focus = true, closeBehavior = "detach") => {
      const existing = (await saya.panel.list()).find((panel) => panel.id === config.id);
      if (existing) {
        if (focus) await saya.panel.focus(config.id);
        return existing;
      }
      const tool = config.tools[config.defaultTool];
      return await saya.panel.open({
        id: config.id,
        position: config.layout.position,
        size: String(config.layout.size),
        content: { kind: "terminal", command: tool.command, closeBehavior },
        focus,
      });
    };
    switch (${JSON.stringify(action)}) {
      case "toggle": {
        const existing = (await saya.panel.list()).find((panel) => panel.id === config.id);
        if (existing) await saya.panel.close(config.id);
        else await ensurePanel(false);
        break;
      }
      case "focus":
        await ensurePanel(true);
        break;
      case "unfocus":
        await saya.panel.unfocus();
        break;
      case "close":
        await saya.panel.close(config.id);
        break;
      case "detach":
        await saya.panel.close(config.id);
        break;
      case "sendCurrentFile": {
        const buffer = await saya.buffer.current();
        await ensurePanel(false);
        await saya.panel.send(config.id, bufferPromptFile(buffer));
        break;
      }
      case "sendCurrentLine": {
        const buffer = await saya.buffer.current();
        await ensurePanel(false);
        await saya.panel.send(config.id, bufferPromptLine(buffer));
        break;
      }
      case "sendSelectedRange": {
        const buffer = await saya.buffer.current();
        const range = await saya.buffer.selection();
        if (!range) return;
        await ensurePanel(false);
        await saya.panel.send(config.id, selectionPrompt(buffer, range));
        break;
      }
      case "sendPrompt": {
        const entry = config.promptLibrary[0];
        if (!entry) return;
        await ensurePanel(false);
        await saya.panel.send(config.id, entry.prompt.endsWith("\\n") ? entry.prompt : entry.prompt + "\\n");
        break;
      }
    }
  `;
  return new Function(`return async () => {${source}}`)();
}

async function ensurePanel(config: NormalizedSayaAgentConfig) {
  const existing = (await sayaHost().panel.list()).find((
    panel: SayaAgentPanelSummary,
  ) => panel.id === config.id);
  if (existing) {
    return existing;
  }
  const tool = config.tools[config.defaultTool];
  const id = config.id;
  const position = config.layout.position;
  const size = String(config.layout.size);
  const command = tool.command;
  const content = { kind: "terminal", command, closeBehavior: "detach" };
  return await sayaHost().panel.open({
    id,
    position,
    size,
    content,
    focus: false,
  });
}

function sayaHost(): any {
  return saya;
}
