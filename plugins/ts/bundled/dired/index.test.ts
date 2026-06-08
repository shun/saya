// deno-lint-ignore-file no-explicit-any

import { setupSayaDired } from "./index.ts";

function installSayaFake() {
  const commands = new Map<string, unknown>();
  const keymaps: Array<{ mode: string; lhs: string; action: unknown }> = [];
  (globalThis as any).saya = {
    buffer: {
      current: () => Promise.resolve({ path: "/workspace/main.ts" }),
    },
    commands: {
      register(name: string, callback: unknown) {
        commands.set(name, callback);
      },
      execute(name: string) {
        return { __sayaStartupCommandReference: true, name };
      },
    },
    filer: {
      currentEntry: () => Promise.resolve(null),
      list: () => Promise.resolve([]),
      mark: () => Promise.resolve(true),
      unmark: () => Promise.resolve(true),
      clearMarks: () => Promise.resolve(true),
      bulkDeletePreview: () => Promise.resolve(true),
    },
    keymap: {
      set(mode: string, lhs: string, action: unknown) {
        keymaps.push({ mode, lhs, action });
      },
    },
  };
  return { commands, keymaps };
}

Deno.test("dired setup registers commands without implicit normal-mode keymaps", () => {
  const fake = installSayaFake();
  setupSayaDired();

  for (
    const name of [
      "dired.open",
      "dired.enter",
      "dired.up",
      "dired.refresh",
      "dired.mark",
      "dired.unmark",
      "dired.clearMarks",
      "dired.bulkDeletePreview",
    ]
  ) {
    if (!fake.commands.has(name)) {
      throw new Error(`missing command: ${name}`);
    }
  }
  if (fake.keymaps.length !== 0) {
    throw new Error(
      `unexpected implicit keymaps: ${JSON.stringify(fake.keymaps)}`,
    );
  }
});

Deno.test("dired setup installs default operation keymap only after keymap opt-in", () => {
  const fake = installSayaFake();
  setupSayaDired({ keymap: {} });

  const mappings = fake.keymaps.map((entry) => [entry.mode, entry.lhs]);
  if (
    JSON.stringify(mappings) !==
      JSON.stringify([
        ["normal", "-"],
        ["normal", "<Enter>"],
        ["normal", "gr"],
        ["normal", "m"],
        ["normal", "M"],
        ["normal", "gM"],
        ["normal", "D"],
      ])
  ) {
    throw new Error(`unexpected dired keymaps: ${JSON.stringify(mappings)}`);
  }
});
