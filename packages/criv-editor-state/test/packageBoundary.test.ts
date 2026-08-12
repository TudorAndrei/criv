import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";
import test from "node:test";

test("keeps the editor State package free of host and model dependencies", async () => {
  const sourceRoot = new URL("../src/", import.meta.url);
  const sources = await readdir(sourceRoot);
  const source = (
    await Promise.all(
      sources.filter((name) => name.endsWith(".ts")).map((name) => readFile(new URL(name, sourceRoot), "utf8")),
    )
  ).join("\n");

  for (const forbidden of ["@criv/likec4", "likec4/", 'from "vscode"', 'from "obsidian"', 'from "node:fs']) {
    assert.equal(source.includes(forbidden), false, `forbidden shared State import: ${forbidden}`);
  }
});
