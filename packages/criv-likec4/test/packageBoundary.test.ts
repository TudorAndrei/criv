import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const packageJsonUrl = new URL("../package.json", import.meta.url);

test("publishes only the protocol and renderer entry points", async () => {
  const packageJson = JSON.parse(await readFile(packageJsonUrl, "utf8")) as {
    exports: Record<string, string>;
  };

  assert.deepEqual(packageJson.exports, {
    "./protocol": "./src/protocol.ts",
    "./renderer": "./src/renderer.ts",
  });
});

test("rejects the removed root and Node.js entry points", async () => {
  const packageRoot: string = "@criv/likec4";
  const nodeEntry: string = "@criv/likec4/node";
  await assert.rejects(import(packageRoot), { code: "ERR_PACKAGE_PATH_NOT_EXPORTED" });
  await assert.rejects(import(nodeEntry), { code: "ERR_PACKAGE_PATH_NOT_EXPORTED" });
});

test("keeps the protocol entry free of runtime and host imports", async () => {
  const protocol = await readFile(new URL("../src/protocol.ts", import.meta.url), "utf8");
  for (const forbidden of [
    "likec4/",
    'from "react"',
    'from "react-dom',
    'from "node:',
    'from "vscode"',
    'from "obsidian"',
  ]) {
    assert.doesNotMatch(protocol, new RegExp(forbidden.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")));
  }
});
