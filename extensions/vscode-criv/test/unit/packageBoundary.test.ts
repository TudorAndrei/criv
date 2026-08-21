import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import extensionPackage from "../../package.json";

test("VSIX packaging does not traverse workspace dependencies", () => {
  assert.match(extensionPackage.scripts.package, /(?:^|\s)--no-dependencies(?:\s|$)/);
});

test("C4 files use the LikeC4 preview as their default editor", () => {
  const customEditors = extensionPackage.contributes.customEditors ?? [];
  const preview = customEditors.find((editor) => editor.viewType === "criv.c4Preview");

  assert.ok(preview);
  assert.equal(preview.priority, "default");
  assert.deepEqual(preview.selector, [{ filenamePattern: "*.c4" }]);
});

test("the preview command supports the official LikeC4 language", () => {
  const titleMenu = extensionPackage.contributes.menus["editor/title"] ?? [];
  const preview = titleMenu.find((item) => item.command === "criv.previewC4");

  assert.ok(preview);
  assert.match(preview.when, /likec4/);
  assert.match(preview.when, /criv-c4/);
});

test("the extension contributes the native asset open command", () => {
  const commands = extensionPackage.contributes.commands ?? [];
  assert.ok(commands.some((command) => command.command === "criv.openAsset"));
  const source = readFileSync(resolve(__dirname, "../../src/extension.ts"), "utf8");
  assert.match(source, /executeCommand\("vscode\.open", uri\)/);
});

test("the webview unload path disposes the shared renderer", () => {
  const source = readFileSync(resolve(__dirname, "../../src/c4/webview.ts"), "utf8");

  assert.match(source, /window\.addEventListener\("unload", \(\) => renderer\.dispose\(\)\)/);
});
