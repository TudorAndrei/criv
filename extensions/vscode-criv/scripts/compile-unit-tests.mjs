import { readdir, rm } from "node:fs/promises";
import { resolve } from "node:path";

import { build } from "esbuild";

const root = resolve(import.meta.dirname, "..");
const testRoot = resolve(root, "test/unit");
const outdir = resolve(root, "dist-test/unit");
const entries = (await readdir(testRoot))
  .filter((name) => name.endsWith(".test.ts"))
  .map((name) => resolve(testRoot, name));

await rm(outdir, { recursive: true, force: true });
await build({
  entryPoints: entries,
  bundle: true,
  platform: "node",
  format: "cjs",
  target: "node18",
  external: ["../pkg/criv_wasm.js"],
  outdir,
});
