import { readdir, rm } from "node:fs/promises";
import { join, resolve } from "node:path";

import { crivWasmBytesPlugin } from "@criv/editor-state/esbuild";
import { build } from "esbuild";

const root = resolve(import.meta.dirname, "..");
const testRoot = resolve(root, "test");
const outdir = resolve(root, "dist-test");
const stubs = {
  obsidian: resolve(testRoot, "stubs/obsidian.ts"),
  "@codemirror/state": resolve(testRoot, "stubs/codemirror.ts"),
  "@codemirror/view": resolve(testRoot, "stubs/codemirror.ts"),
};

const obsidianStubs = {
  name: "obsidian-stubs",
  setup(build) {
    build.onResolve({ filter: /^(obsidian|@codemirror\/(state|view))$/ }, (args) => ({
      path: stubs[args.path],
    }));
  },
};

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
  entryPoints: await testEntries(testRoot),
  bundle: true,
  platform: "node",
  format: "cjs",
  outExtension: { ".js": ".cjs" },
  target: "node22",
  plugins: [obsidianStubs, crivWasmBytesPlugin],
  outbase: testRoot,
  outdir,
  logLevel: "warning",
});
