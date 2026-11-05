import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

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
