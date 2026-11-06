import { readdir, rm } from "node:fs/promises";
import { join, resolve } from "node:path";
import process from "node:process";

import { build } from "esbuild";

const root = resolve(import.meta.dirname, "..");
const suite = process.argv.includes("--integration") ? "integration" : "unit";
const testRoot = resolve(root, "test", suite);
const outdir = resolve(root, "dist-test", suite);
const entryName = suite === "unit" ? /\.test\.ts$/ : /^runner\.ts$/;

async function testEntries(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const nested = await Promise.all(
    entries.map((entry) => {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) {
        return testEntries(path);
      }
      return entry.isFile() && entryName.test(entry.name) ? [path] : [];
    }),
  );
  return nested.flat().sort();
}

await rm(outdir, { recursive: true, force: true });
await build({
  entryPoints: await testEntries(testRoot),
  bundle: true,
  platform: "node",
  format: "cjs",
  target: "node18",
  external: ["vscode"],
  outbase: testRoot,
  outdir,
  logLevel: "warning",
});
