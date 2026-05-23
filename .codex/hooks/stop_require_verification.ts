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

  const touchesStartupCallbackBoundary = paths.some((path) =>
    path.startsWith("plugins/bundled/") ||
    path.includes("runtime/startup") ||
    path.includes("runtime/live") ||
    path.includes("callback_registry_seed") ||
    path.includes("integration_typescript_runtime")
  );
  const touchesCompletionUx = paths.some((path) =>
    path.includes("completion") ||
    path.includes("input_loop") ||
    path.includes("input/router") ||
    path.includes("floating_window") ||
    path === "src/main.rs" ||
    path === "tests/integration_binary_smoke.rs"
  );
  const touchesCompletionSourceOrRuntimeApi = paths.some((path) =>
    path.includes("plugins/bundled/completion") ||
    path.includes("runtime/live") ||
    path.includes("runtime/integration") ||
    path.includes("src/main.rs")
  );

  commands.add(
    "For behavior changes, prove the intended effect by observing the final state, not only logs or UI events. If the change should edit a buffer, save a file, update runtime state, open/close UI, or change config-visible behavior, assert the resulting buffer/file/state by reading it back in a test or smoke.",
  );
  commands.add(
    "If logs are added or used for diagnosis, verify the expected log line appears on the exercised path, but do not use logs as the only success criterion when state should change.",
  );

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

  if (touchesStartupCallbackBoundary) {
    commands.add(
      "For startup-registered commands/events, verify the Function.toString boundary: run a test that rebuilds the registered callback from callback.toString() without startup closure state, or run a Layer 2 CallbackRegistrySeed/live-runtime command/event test.",
    );
  }

  if (touchesCompletionUx) {
    commands.add(
      "For completion UX changes, run a binary or release smoke that proves multiple candidates render, selection moves (for example Down changes the selected row), Enter confirms the selected candidate, and the resulting buffer/save contents are asserted. Logs alone are not enough.",
    );
    commands.add(
      "Suggested completion checks: gtimeout 120 cargo test --test integration_binary_smoke bundled_completion_binary_smoke_can_select_second_candidate && gtimeout 120 cargo test --test completion_float",
    );
  }

  if (touchesCompletionSourceOrRuntimeApi) {
    commands.add(
      "For completion sources that inspect filesystem/workspace/editor state, prove they use readonly host APIs and do not mutate the active buffer/window/session. Add or run a fake-host test that fails on side-effectful APIs, plus a binary smoke that reads the final buffer/file back.",
    );
    commands.add(
      "Suggested path completion guard: gtimeout 120 cargo test --test integration_binary_smoke bundled_path_completion_does_not_replace_buffer_with_directory_listing",
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
