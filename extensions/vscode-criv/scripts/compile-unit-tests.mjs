import { readdir, rm } from "node:fs/promises";
import { join, resolve } from "node:path";

import { build } from "esbuild";

const root = resolve(import.meta.dirname, "..");
const testRoot = resolve(root, "test/unit");
const outdir = resolve(root, "dist-test/unit");
const entries = await testEntries(testRoot);

async function testEntries(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const nested = await Promise.all(
    entries.map((entry) => {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) {
        return testEntries(path);
      }
      return entry.isFile() && entry.name.endsWith(".test.ts") ? [path] : [];
    }),
  );
  return nested.flat().sort();
}

await rm(outdir, { recursive: true, force: true });
await build({
  entryPoints: entries,
  bundle: true,
  platform: "node",
  format: "cjs",
  target: "node18",
  external: ["../../pkg/criv_wasm.js"],
  outbase: testRoot,
  outdir,
});
