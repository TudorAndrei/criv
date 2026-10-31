import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import extensionPackage from "../../package.json";

test("VSIX packaging does not traverse workspace dependencies", () => {
  assert.match(extensionPackage.scripts.package, /(?:^|\s)--no-dependencies(?:\s|$)/);
});

test("the extension contributes the native asset open command", () => {
  const commands = extensionPackage.contributes.commands ?? [];
  assert.ok(commands.some((command) => command.command === "criv.openAsset"));
  const source = readFileSync(resolve(__dirname, "../../src/extension.ts"), "utf8");
  assert.match(source, /executeCommand\("vscode\.open", uri\)/);
});
