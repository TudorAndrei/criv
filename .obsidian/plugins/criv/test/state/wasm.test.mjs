import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import * as esbuild from "esbuild";

const __dirname = dirname(fileURLToPath(import.meta.url));
const pluginRoot = resolve(__dirname, "../..");
const outFile = resolve(tmpdir(), `criv-wasm-test-${process.pid}.mjs`);
const wasmPath = resolve(pluginRoot, "pkg/criv_wasm.js");
const stateRaw = readFileSync(
  resolve(pluginRoot, "../../../fixtures/state/criv.state.v1.json"),
  "utf8",
);
const lookupFixture = JSON.parse(
  readFileSync(
    resolve(pluginRoot, "../../../fixtures/editor/source-target-lookup.v1.json"),
    "utf8",
  ),
);
const architectureFixture = JSON.parse(
  readFileSync(resolve(pluginRoot, "../../../fixtures/editor/likec4-projection.v1.json"), "utf8"),
);

await esbuild.build({
  entryPoints: [resolve(pluginRoot, "src/state/wasm.ts")],
  outfile: outFile,
  bundle: true,
  format: "esm",
  platform: "node",
  target: "node18",
  external: ["../../pkg/criv_wasm.js"],
});

const bridgeModule = await import(pathToFileURL(outFile).href);
const compiledWasm = await import(pathToFileURL(wasmPath).href);
const bridge = bridgeModule.createCrivWasmBridge(async () => compiledWasm);
const revision = await bridge.loadState(stateRaw);
const projections = revision.initialProjections();

assert.equal(projections.summary.schema, "criv.state.v1");
assert.equal("state" in projections, false);
assert.deepEqual(projections.registeredPatterns, ["ADR-0001/entrypoint"]);
assert.equal(projections.summary.node_count, 6);
assert.deepEqual(
  projections.sources.map((entry) => entry.path),
  ["src/lib.rs"],
);
assert.equal(revision.suggestSelectors("run", 10)[0].target, "src/lib.rs#fn:run");
revision.dispose();

const architectureRevision = await bridge.loadState(JSON.stringify(architectureFixture.state));
const architectureProjections = architectureRevision.initialProjections();
assert.deepEqual(architectureProjections.architecture, architectureFixture.expected.architecture);
assert.deepEqual(architectureProjections.c4Artifacts, architectureFixture.expected.c4Artifacts);
architectureRevision.dispose();

const lookupRevision = await bridge.loadState(JSON.stringify(lookupFixture.state));
for (const expected of lookupFixture.cases) {
  const actual = lookupRevision.lookupSourceTarget(expected.target);
  assert.equal(actual.kind, expected.kind, `lookup kind for ${expected.target}`);
  if (actual.kind === "resolved") {
    assert.equal(actual.canonical_target, expected.canonical_target);
  }
  if (actual.kind === "ambiguous") {
    assert.equal(actual.total_candidate_count, expected.total_candidate_count);
    assert(actual.candidates.length <= 5);
  }
}
lookupRevision.dispose();

let attempts = 0;
const missing = bridgeModule.createCrivWasmBridge(async () => {
  attempts += 1;
  throw new Error("missing runtime");
});
for (let request = 0; request < 2; request += 1) {
  await assert.rejects(missing.loadState(stateRaw), (error) => {
    assert.equal(error.code, bridgeModule.CRIV_WASM_LOAD_ERROR);
    assert.equal(
      error.message,
      "Could not load the packaged criv Wasm runtime. Rebuild the companion and reload Obsidian.",
    );
    return true;
  });
}
assert.equal(attempts, 1);
