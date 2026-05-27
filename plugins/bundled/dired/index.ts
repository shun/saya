// deno-fmt-ignore-file

declare const saya: any;

export interface SayaDiredCommandNames {
  open?: string;
  enter?: string;
  up?: string;
  refresh?: string;
  mark?: string;
  unmark?: string;
  clearMarks?: string;
  bulkDeletePreview?: string;
}

export interface SayaDiredKeymap {
  up?: string;
  enter?: string;
  refresh?: string;
  mark?: string;
  unmark?: string;
  clearMarks?: string;
  bulkDeletePreview?: string;
}

export interface SayaDiredHiddenFilePolicy {
  show: "show";
  hide: "hide";
}

export interface SayaDiredSortPolicy {
  name: "name";
  kind: "kind";
  modifiedTime: "modifiedTime";
  size: "size";
}

export interface SayaDiredConfirmStrategy {
  preview: "preview";
  disabled: "disabled";
}

export interface SayaDiredOptions {
  commands?: SayaDiredCommandNames;
  keymap?: SayaDiredKeymap;
  commandName?: string;
  enterCommandName?: string;
  upCommandName?: string;
  refreshCommandName?: string;
  markCommandName?: string;
  unmarkCommandName?: string;
  clearMarksCommandName?: string;
  bulkDeletePreviewCommandName?: string;
  key?: string;
  enterKey?: string;
  refreshKey?: string;
  markKey?: string;
  unmarkKey?: string;
  clearMarksKey?: string;
  bulkDeletePreviewKey?: string;
  root?: string;
  hiddenFilePolicy?: "show" | "hide";
  sortPolicy?: "name" | "kind" | "modifiedTime" | "size";
  filter?: string;
  confirmStrategy?: "preview" | "disabled";
}

function quoteRuntimeString(value: string): string {
  return JSON.stringify(value);
}

function hasExplicitDiredKeymap(options: SayaDiredOptions): boolean {
  if (options.keymap !== undefined) return true;
  return options.key !== undefined ||
    options.enterKey !== undefined ||
    options.refreshKey !== undefined ||
    options.markKey !== undefined ||
    options.unmarkKey !== undefined ||
    options.clearMarksKey !== undefined ||
    options.bulkDeletePreviewKey !== undefined;
}

export function setupSayaDired(options: SayaDiredOptions = {}): void {
  const commands = options.commands ?? {};
  const keymap = options.keymap ?? {};
  const registerKeymap = hasExplicitDiredKeymap(options);
  const commandName = commands.open ?? options.commandName ?? "dired.open";
  const enterCommandName =
    commands.enter ?? options.enterCommandName ?? "dired.enter";
  const upCommandName = commands.up ?? options.upCommandName ?? "dired.up";
  const refreshCommandName =
    commands.refresh ?? options.refreshCommandName ?? "dired.refresh";
  const markCommandName = commands.mark ?? options.markCommandName ?? "dired.mark";
  const unmarkCommandName =
    commands.unmark ?? options.unmarkCommandName ?? "dired.unmark";
  const clearMarksCommandName =
    commands.clearMarks ?? options.clearMarksCommandName ?? "dired.clearMarks";
  const bulkDeletePreviewCommandName =
    commands.bulkDeletePreview ??
    options.bulkDeletePreviewCommandName ??
    "dired.bulkDeletePreview";
  const key = keymap.up ?? options.key ?? "-";
  const enterKey = keymap.enter ?? options.enterKey ?? "<Enter>";
  const refreshKey = keymap.refresh ?? options.refreshKey ?? "gr";
  const markKey = keymap.mark ?? options.markKey ?? "m";
  const unmarkKey = keymap.unmark ?? options.unmarkKey ?? "M";
  const clearMarksKey = keymap.clearMarks ?? options.clearMarksKey ?? "gM";
  const bulkDeletePreviewKey =
    keymap.bulkDeletePreview ?? options.bulkDeletePreviewKey ?? "D";
  const root = options.root ?? "";
  const hiddenFilePolicy = options.hiddenFilePolicy ?? "show";
  const sortPolicy = options.sortPolicy ?? "kind";
  const filter = options.filter ?? "";
  const confirmStrategy = options.confirmStrategy ?? "preview";

  const helpers =
    "  const trimTrailingSlash = (path) => {\n" +
    "    if (path.length > 1 && path.endsWith('/')) return path.slice(0, -1);\n" +
    "    return path;\n" +
    "  };\n" +
    "  const dirname = (path) => {\n" +
    "    const normalized = trimTrailingSlash(path || '.');\n" +
    "    const index = normalized.lastIndexOf('/');\n" +
    "    if (index < 0) return '.';\n" +
    "    if (index === 0) return '/';\n" +
    "    return normalized.slice(0, index);\n" +
    "  };\n" +
    "  const parentDirectory = (path) => {\n" +
    "    const normalized = trimTrailingSlash(path || '.');\n" +
    "    if (normalized === '.') return '..';\n" +
    "    if (/^\\.\\.(\\/\\.\\.)*$/.test(normalized)) return normalized + '/..';\n" +
    "    const index = normalized.lastIndexOf('/');\n" +
    "    if (index < 0) return '.';\n" +
    "    if (index === 0) return '/';\n" +
    "    return normalized.slice(0, index);\n" +
    "  };\n" +
    "  const joinPath = (base, name) => {\n" +
    "    const normalized = trimTrailingSlash(base || '.');\n" +
    "    if (normalized === '/') return '/' + name;\n" +
    "    if (normalized === '.') return name;\n" +
    "    return normalized + '/' + name;\n" +
    "  };\n" +
    "  const escapeEditPath = (path) => String(path).replace(/\\\\/g, '\\\\\\\\').replace(/ /g, '\\\\ ');\n";

  const openCallback = new Function(
    "return async () => {\n" +
      helpers +
      `  const configuredRoot = ${quoteRuntimeString(root)};\n` +
      "  const currentPath = configuredRoot || await saya.buffer.currentPath() || \".\";\n" +
      "  await saya.commands.execute(`edit ${escapeEditPath(dirname(currentPath))}`);\n" +
      "};",
  )();

  const upCallback = new Function(
    "return async () => {\n" +
      helpers +
      "  const currentPath = await saya.buffer.currentPath() || \".\";\n" +
      "  await saya.commands.execute(`edit ${escapeEditPath(parentDirectory(currentPath))}`);\n" +
      "};",
  )();

  const refreshCallback = new Function(
    "return async () => {\n" +
      helpers +
      `  const configuredRoot = ${quoteRuntimeString(root)};\n` +
      `  const showHidden = ${hiddenFilePolicy === "show"};\n` +
      `  const sortBy = ${quoteRuntimeString(sortPolicy)};\n` +
      `  const filter = ${quoteRuntimeString(filter)};\n` +
      "  const directory = configuredRoot || await saya.buffer.currentPath() || '.';\n" +
      "  await saya.filer.list(directory, { showHidden, sortBy, filter });\n" +
      "  await saya.commands.execute(`edit ${escapeEditPath(directory)}`);\n" +
      "};",
  )();

  const enterCallback = new Function(
    "return async () => {\n" +
      helpers +
      "  const entry = await saya.filer.currentEntry();\n" +
      "  if (!entry) return;\n" +
      "  await saya.commands.execute(`edit ${escapeEditPath(entry.path)}`);\n" +
      "};",
  )();

  const markCallback = new Function(
    "return async () => {\n" +
      "  const entry = await saya.filer.currentEntry();\n" +
      "  if (!entry) return;\n" +
      "  await saya.filer.mark(entry.path);\n" +
      "};",
  )();

  const unmarkCallback = new Function(
    "return async () => {\n" +
      "  const entry = await saya.filer.currentEntry();\n" +
      "  if (!entry) return;\n" +
      "  await saya.filer.unmark(entry.path);\n" +
      "};",
  )();

  const clearMarksCallback = new Function(
    "return async () => {\n" +
      "  await saya.filer.clearMarks();\n" +
      "};",
  )();

  const bulkDeletePreviewCallback = new Function(
    "return async () => {\n" +
      `  const confirmStrategy = ${quoteRuntimeString(confirmStrategy)};\n` +
      "  if (confirmStrategy === 'disabled') return;\n" +
      "  await saya.filer.bulkDeletePreview();\n" +
      "};",
  )();

  saya.commands.register(commandName, openCallback);
  saya.commands.register(upCommandName, upCallback);
  saya.commands.register(refreshCommandName, refreshCallback);
  saya.commands.register(enterCommandName, enterCallback);
  saya.commands.register(markCommandName, markCallback);
  saya.commands.register(unmarkCommandName, unmarkCallback);
  saya.commands.register(clearMarksCommandName, clearMarksCallback);
  saya.commands.register(bulkDeletePreviewCommandName, bulkDeletePreviewCallback);
  if (!registerKeymap) return;
  saya.keymap.set("normal", key, saya.commands.execute(upCommandName));
  saya.keymap.set("normal", enterKey, saya.commands.execute(enterCommandName));
  saya.keymap.set("normal", refreshKey, saya.commands.execute(refreshCommandName));
  saya.keymap.set("normal", markKey, saya.commands.execute(markCommandName));
  saya.keymap.set("normal", unmarkKey, saya.commands.execute(unmarkCommandName));
  saya.keymap.set("normal", clearMarksKey, saya.commands.execute(clearMarksCommandName));
  saya.keymap.set(
    "normal",
    bulkDeletePreviewKey,
    saya.commands.execute(bulkDeletePreviewCommandName),
  );
}
