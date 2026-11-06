import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";
import type { App, PluginManifest } from "obsidian";

import CrivPlugin from "../src/main";
import { Notice, settingHandlers, type Plugin as StubPlugin } from "./stubs/obsidian";

const stateRaw = readFileSync(
  resolve(__dirname, "../../../../fixtures/state/criv.state.v1.json"),
  "utf8",
);
const readyNotice = "criv criv.state.v1: 6 nodes, 5 edges, 1 source files";

test("loads State when the layout is ready and reports its summary", async () => {
  const vault = new FakeVault({ ".criv/state.json": stateRaw });
  const plugin = await loadedPlugin(vault);

  await vault.layoutReady();

  assert.equal(await statusNotice(plugin), readyNotice);
});

test("reports a missing State file, an unsupported schema, and a path outside the vault", async () => {
  const missing = await loadedPlugin(new FakeVault({}));
  assert.equal(
    await statusNotice(missing),
    "Could not read .criv/state.json: missing .criv/state.json",
  );

  const unsupported = await loadedPlugin(
    new FakeVault({ ".criv/state.json": stateRaw.replace("criv.state.v1", "criv.state.v2") }),
  );
  assert.equal(
    await statusNotice(unsupported),
    "Invalid criv State: unsupported criv state schema: criv.state.v2",
  );

  const outside = await loadedPlugin(new FakeVault({}), { statePath: "../state.json" });
  assert.equal(await statusNotice(outside), "Invalid criv state path ../state.json.");
});

test("a failed State is not reloaded by each State query", async () => {
  const vault = new FakeVault({ ".criv/state.json": "{" });
  const plugin = await loadedPlugin(vault);
  await vault.layoutReady();

  await statusNotice(plugin);
  await statusNotice(plugin);

  assert.equal(vault.reads, 1);
});

test("polling reloads State only after the State file changes", async () => {
  const vault = new FakeVault({ ".criv/state.json": stateRaw });
  const plugin = await loadedPlugin(vault);
  await vault.layoutReady();

  await vault.poll();
  assert.equal(vault.reads, 1);

  vault.write(".criv/state.json", stateRaw.replace("criv.state.v1", "criv.state.v2"));
  await vault.poll();
  assert.equal(vault.reads, 2);
  assert.match((await statusNotice(plugin)) ?? "", /unsupported criv state schema/);
});

test("a new State path setting is saved and loaded", async () => {
  const vault = new FakeVault({ ".criv/state.json": "{", "state/other.json": stateRaw });
  const plugin = await loadedPlugin(vault);
  await vault.layoutReady();
  (plugin as unknown as StubPlugin).settingTab?.display();

  await settingHandlers.get("State path")?.("./state/other.json");

  assert.deepEqual((plugin as unknown as StubPlugin).data, {
    statePath: "state/other.json",
    externalEditorUrl: "vscode://file/{path}",
  });
  assert.equal(await statusNotice(plugin), readyNotice);
});

test("unloading stops State reloads", async () => {
  const vault = new FakeVault({ ".criv/state.json": stateRaw });
  const plugin = await loadedPlugin(vault);
  await vault.layoutReady();

  plugin.onunload();
  vault.write(".criv/state.json", stateRaw.replace("criv.state.v1", "criv.state.v2"));
  await command(plugin, "reload-criv-state");
  await vault.poll();

  assert.equal(vault.reads, 1);
});

class FakeVault {
  reads = 0;
  readonly layoutCallbacks: (() => void)[] = [];
  readonly pollers: (() => void)[] = [];
  private mtime = 1;
  private readonly stats = new Map<string, { mtime: number; size: number }>();
  private readonly files = new Map<string, string>();

  constructor(files: Record<string, string>) {
    for (const [path, text] of Object.entries(files)) {
      this.write(path, text);
    }
    Object.assign(globalThis, {
      document: {},
      window: { setInterval: (callback: () => void) => this.pollers.push(callback) },
    });
  }

  write(path: string, text: string): void {
    this.files.set(path, text);
    this.stats.set(path, { mtime: this.mtime++, size: text.length });
  }

  async layoutReady(): Promise<void> {
    for (const callback of this.layoutCallbacks) {
      callback();
    }
    await drainMicrotasks();
  }

  async poll(): Promise<void> {
    for (const poller of this.pollers) {
      poller();
    }
    await drainMicrotasks();
  }

  app(): App {
    return {
      vault: {
        adapter: {
          read: async (path: string) => {
            this.reads += 1;
            const text = this.files.get(path);
            if (text === undefined) {
              throw new Error(`missing ${path}`);
            }
            return text;
          },
          stat: async (path: string) => this.stats.get(path) ?? null,
        },
      },
      workspace: {
        on: () => ({}),
        onLayoutReady: (callback: () => void) => this.layoutCallbacks.push(callback),
        getLeavesOfType: () => [],
        getRightLeaf: () => null,
        updateOptions: () => {},
      },
      metadataCache: { on: () => ({}) },
    } as unknown as App;
  }
}

async function loadedPlugin(vault: FakeVault, data: object = {}): Promise<CrivPlugin> {
  const plugin = new CrivPlugin(vault.app(), {} as PluginManifest);
  (plugin as unknown as StubPlugin).data = data;
  await plugin.onload();
  return plugin;
}

async function command(plugin: CrivPlugin, id: string): Promise<void> {
  await (plugin as unknown as StubPlugin).commands.get(id)?.();
}

async function statusNotice(plugin: CrivPlugin): Promise<string | undefined> {
  Notice.messages.length = 0;
  await command(plugin, "show-criv-status");
  return Notice.messages.at(-1);
}

/**
 * The fake vault and the embedded Wasm load resolve through promises alone,
 * so one macrotask runs every continuation a callback started.
 */
async function drainMicrotasks(): Promise<void> {
  await new Promise((resolve) => setImmediate(resolve));
}
