type StopHookInput = {
  cwd?: string;
  hook_event_name?: string;
  stop_hook_active?: boolean;
  last_assistant_message?: string | null;
};

type GitChange = {
  path: string;
  status: string;
};

const textDecoder = new TextDecoder();
const markerPath = ".codex/.verification/last-success.json";

async function readStdin(): Promise<StopHookInput> {
  const chunks: Uint8Array[] = [];
  for await (const chunk of Deno.stdin.readable) {
    chunks.push(chunk);
  }
  const text = textDecoder.decode(concat(chunks)).trim();
  if (text.length === 0) {
    return {};
  }

  return JSON.parse(text) as StopHookInput;
}

function concat(chunks: Uint8Array[]): Uint8Array {
  const total = chunks.reduce((sum, chunk) => sum + chunk.length, 0);
  const output = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    output.set(chunk, offset);
    offset += chunk.length;
  }
  return output;
}

async function run(
  command: string[],
  cwd: string,
): Promise<{ code: number; stdout: string; stderr: string }> {
  const process = new Deno.Command(command[0], {
    args: command.slice(1),
    cwd,
    stdout: "piped",
    stderr: "piped",
  });
  const output = await process.output();
  return {
    code: output.code,
    stdout: textDecoder.decode(output.stdout),
    stderr: textDecoder.decode(output.stderr),
  };
}

async function gitRoot(cwd: string): Promise<string | null> {
  const result = await run(["git", "rev-parse", "--show-toplevel"], cwd);
  if (result.code !== 0) {
    return null;
  }
  return result.stdout.trim();
}

async function gitChanges(root: string): Promise<GitChange[]> {
  const result = await run(
    ["git", "status", "--porcelain=v1", "--untracked-files=all"],
    root,
  );
  if (result.code !== 0) {
    return [];
  }

  return result.stdout
    .split("\n")
    .map((line) => line.trimEnd())
    .filter((line) => line.length > 0)
    .map((line) => ({
      status: line.slice(0, 2),
      path: line.slice(3),
    }));
}

function isCodeOrConfigPath(path: string): boolean {
  if (path === markerPath) {
    return false;
  }

  if (path === ".gitignore" || path === "Cargo.toml" || path === "Cargo.lock") {
    return true;
  }

  return path.startsWith(".codex/hooks/") || [
    ".rs",
    ".ts",
    ".tsx",
    ".js",
    ".jsx",
    ".json",
    ".toml",
    ".md",
  ].some((suffix) => path.endsWith(suffix));
}

function verificationCommandsFor(paths: string[]): string[] {
  const commands = new Set<string>();

  if (paths.some((path) => path.endsWith(".rs") || path === "Cargo.toml")) {
    commands.add("gtimeout 120s cargo test");
  }

  if (
    paths.some((path) =>
      path.endsWith(".ts") || path.endsWith(".tsx") || path.endsWith(".js") ||
      path.endsWith(".jsx") || path.endsWith(".json")
    )
  ) {
    commands.add("gtimeout 30s deno check <changed TypeScript entrypoint>");
  }

  if (
    paths.some((path) =>
      path.includes("runtime/") ||
      path.includes("startup") ||
      path.includes("plugin") ||
      path.includes("selector") ||
      path.includes("commands")
    )
  ) {
    commands.add(
      "Run the affected runtime path, not only startup smoke. For selector changes, execute ',' -> prompt input -> selector open and confirm /tmp/saya.log.",
    );
  }

  if (commands.size === 0) {
    commands.add("Run the most relevant smoke test for the changed behavior.");
  }

  return [...commands];
}

async function fileMtimeMs(path: string): Promise<number | null> {
  try {
    return (await Deno.stat(path)).mtime?.getTime() ?? null;
  } catch (error) {
    if (error instanceof Deno.errors.NotFound) {
      return null;
    }
    throw error;
  }
}

async function latestChangedMtimeMs(
  root: string,
  paths: string[],
): Promise<number> {
  const mtimes = await Promise.all(
    paths.map((path) => fileMtimeMs(`${root}/${path}`)),
  );
  return Math.max(
    0,
    ...mtimes.filter((mtime): mtime is number => mtime !== null),
  );
}

async function hasFreshVerificationMarker(
  root: string,
  changedPaths: string[],
): Promise<boolean> {
  const markerMtime = await fileMtimeMs(`${root}/${markerPath}`);
  if (markerMtime === null) {
    return false;
  }

  const latestChangeMtime = await latestChangedMtimeMs(root, changedPaths);
  return markerMtime >= latestChangeMtime;
}

function block(reason: string): never {
  console.log(JSON.stringify({ decision: "block", reason }));
  Deno.exit(0);
}

const input = await readStdin();

if (input.hook_event_name && input.hook_event_name !== "Stop") {
  Deno.exit(0);
}

if (input.stop_hook_active) {
  Deno.exit(0);
}

const cwd = input.cwd ?? Deno.cwd();
const root = await gitRoot(cwd);
if (root === null) {
  Deno.exit(0);
}

const changedPaths = (await gitChanges(root))
  .map((change) => change.path)
  .filter(isCodeOrConfigPath);

if (changedPaths.length === 0) {
  Deno.exit(0);
}

if (await hasFreshVerificationMarker(root, changedPaths)) {
  Deno.exit(0);
}

const commands = verificationCommandsFor(changedPaths);
block(
  [
    "Code/config files changed after the last verification marker.",
    "",
    "Changed files:",
    ...changedPaths.map((path) => `- ${path}`),
    "",
    "Before stopping, run and report relevant verification. Suggested checks:",
    ...commands.map((command) => `- ${command}`),
    "",
    `After successful verification, update ${markerPath}.`,
  ].join("\n"),
);
