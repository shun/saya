export interface SayaDiredOptions {
  commandName?: string;
  key?: string;
  root?: string;
}

function quoteRuntimeString(value: string): string {
  return JSON.stringify(value);
}

export function setupSayaDired(options: SayaDiredOptions = {}): void {
  const commandName = options.commandName ?? "dired.open";
  const key = options.key ?? "-";
  const root = options.root ?? "";

  const callback = new Function(
    "return async () => {\n" +
      `  const configuredRoot = ${quoteRuntimeString(root)};\n` +
      "  const buffer = await saya.buffer.current();\n" +
      "  const currentPath = configuredRoot || buffer.path || \".\";\n" +
      "  const directory = currentPath.endsWith(\"/\")\n" +
      "    ? (currentPath.slice(0, -1) || \"/\")\n" +
      "    : (currentPath.lastIndexOf(\"/\") >= 0 ? currentPath.slice(0, currentPath.lastIndexOf(\"/\")) || \"/\" : \".\");\n" +
      "  await saya.commands.execute(`edit ${directory}`);\n" +
      "};",
  )();

  saya.commands.register(commandName, callback);
  saya.keymap.set("normal", key, saya.commands.execute(commandName));
}
