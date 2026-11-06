import { readFile, readdir, rm } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { build } from "esbuild";

const packagedEntry = fileURLToPath(new URL("./src/packaged.ts", import.meta.url));
const wasmPath = fileURLToPath(new URL("./pkg/criv_wasm_bg.wasm", import.meta.url));

/** Embeds the generated Wasm runtime so a bundle does not depend on its install directory. */
export const crivWasmBytesPlugin = {
  name: "criv-wasm-bytes",
  setup(build) {
    // The generated glue reads import.meta.url only in its unused fetch-based loader.
    build.initialOptions.logOverride = {
      ...build.initialOptions.logOverride,
      "empty-import-meta": "silent",
    };
    build.onResolve({ filter: /^\.\/wasmBytes\.ts$/ }, (args) =>
      args.importer === packagedEntry
        ? { path: wasmPath, namespace: "criv-wasm-bytes" }
        : undefined,
    );
    build.onLoad({ filter: /.*/, namespace: "criv-wasm-bytes" }, async () => ({
      contents: await readFile(wasmPath),
      loader: "binary",
    }));
  },
};

async function testEntries(directory, entryName) {
  const entries = await readdir(directory, { withFileTypes: true });
  const nested = await Promise.all(
    entries.map((entry) => {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) {
        return testEntries(path, entryName);
      }
      return entry.isFile() && entryName.test(entry.name) ? [path] : [];
    }),
  );
  return nested.flat().sort();
}

/** Bundles each test file under testRoot whose name matches entryName into outdir for node:test. */
export async function bundleTests({ testRoot, outdir, entryName, plugins = [], ...options }) {
  await rm(outdir, { recursive: true, force: true });
  await build({
    entryPoints: await testEntries(testRoot, entryName),
    bundle: true,
    platform: "node",
    format: "cjs",
    plugins: [...plugins, crivWasmBytesPlugin],
    outbase: testRoot,
    outdir,
    logLevel: "warning",
    ...options,
  });
}
