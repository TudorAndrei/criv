import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import * as esbuild from "esbuild";

const __dirname = dirname(fileURLToPath(import.meta.url));
const pluginRoot = resolve(__dirname, "..");
const outFile = resolve(tmpdir(), `criv-wasm-test-${process.pid}.mjs`);
const wasmPath = resolve(pluginRoot, "pkg/criv_wasm.js");
const stateRaw = readFileSync(
  resolve(__dirname, "../../../../fixtures/state/criv.state.v1.json"),
  "utf8",
);

await esbuild.build({
  entryPoints: [resolve(pluginRoot, "src/wasm.ts")],
  outfile: outFile,
  bundle: true,
  format: "esm",
  platform: "node",
  target: "node18",
  external: ["./pkg/criv_wasm.js"],
});

const bridgeModule = await import(pathToFileURL(outFile).href);
const compiledWasm = await import(pathToFileURL(wasmPath).href);
const bridge = bridgeModule.createCrivWasmBridge(async () => compiledWasm);
const revision = await bridge.loadState(stateRaw);
const projections = revision.initialProjections();

assert.equal(projections.state.schema, "criv.state.v1");
assert.equal(projections.summary.node_count, 6);
assert.deepEqual(
  projections.sources.map((entry) => entry.path),
  ["src/lib.rs"],
);
assert.equal(revision.suggestSelectors("run", 10)[0].target, "src/lib.rs#fn:run");
revision.dispose();
assert.throws(
  () => revision.suggestSelectors("run", 10),
  (error) => {
    assert.equal(error.code, bridgeModule.CRIV_LOADED_STATE_DISPOSED);
    return true;
  },
);

let attempts = 0;
const missing = bridgeModule.createCrivWasmBridge(async () => {
  attempts += 1;
  throw new Error("missing runtime");
});
for (let request = 0; request < 2; request += 1) {
  await assert.rejects(missing.loadState(stateRaw), (error) => {
    assert.equal(error.code, bridgeModule.CRIV_WASM_LOAD_ERROR);
    assert.match(error.message, /rebuild the companion and reload Obsidian/i);
    return true;
  });
}
assert.equal(attempts, 1);

const corrupt = bridgeModule.createCrivWasmBridge(async () => ({}));
await assert.rejects(corrupt.loadState(stateRaw), (error) => {
  assert.equal(error.code, bridgeModule.CRIV_WASM_LOAD_ERROR);
  return true;
});
