import assert from "node:assert/strict";
import test from "node:test";
import type { App, Plugin } from "obsidian";

import type { StatePort } from "../src/ports";
import { ObsidianSettingsOwner } from "../src/settings";
import { ObsidianSourcePanelOwner } from "../src/source/panel";
import { FileSystemAdapter, Notice, Plugin as StubPlugin } from "./stubs/obsidian";

function openedUrls(adapter: unknown, basePath = "/home/me/My Vault"): string[] {
  const opened: string[] = [];
  Object.assign(globalThis, { window: { open: (url: string) => opened.push(url) } });
  const app = { vault: { adapter: adapter ?? new FileSystemAdapter(basePath) } } as App;
  const settings = new ObsidianSettingsOwner(
    app,
    new StubPlugin(app, {}) as unknown as Plugin,
    async () => {},
  );
  const panel = new ObsidianSourcePanelOwner(app, {} as StatePort, (path) =>
    settings.externalEditorUrl(path),
  );
  panel.openExternal("src/lib.rs");
  return opened;
}

test("opens a source in the external editor by its absolute path", () => {
  assert.deepEqual(openedUrls(undefined), ["vscode://file/home/me/My%20Vault/src/lib.rs"]);
  assert.deepEqual(openedUrls(undefined, "C:\\Vault"), ["vscode://file/C:/Vault/src/lib.rs"]);
});

test("reports that a vault without a file system path cannot open an external editor", () => {
  Notice.messages.length = 0;

  assert.deepEqual(openedUrls({}), []);
  assert.deepEqual(Notice.messages, [
    "Opening a source in an external editor needs the desktop app.",
  ]);
});
