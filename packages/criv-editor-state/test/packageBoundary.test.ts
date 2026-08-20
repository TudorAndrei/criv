import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";
import test from "node:test";

test("keeps the editor State package free of host and model dependencies", async () => {
  const sourceRoot = new URL("../src/", import.meta.url);
  const source = (await readSourceFiles(sourceRoot)).join("\n");

  for (const forbidden of [
    "@criv/likec4",
    "likec4/",
    'from "vscode"',
    'from "obsidian"',
    'from "node:fs',
  ]) {
    assert.equal(source.includes(forbidden), false, `forbidden shared State import: ${forbidden}`);
  }
});

async function readSourceFiles(directory: URL): Promise<string[]> {
  const entries = await readdir(directory, { withFileTypes: true });
  const sources = await Promise.all(
    entries.map(async (entry) => {
      const path = new URL(entry.name + (entry.isDirectory() ? "/" : ""), directory);
      if (entry.isDirectory()) {
        return readSourceFiles(path);
      }
      return entry.isFile() && entry.name.endsWith(".ts") ? [await readFile(path, "utf8")] : [];
    }),
  );
  return sources.flat();
}
