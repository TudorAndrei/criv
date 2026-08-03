import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { resolve } from "node:path";
import test from "node:test";

import { CRIV_WASM_LOAD_ERROR, CrivWasmLoadError, createCrivWasmBridge } from "../../src/wasm";

const require = createRequire(__filename);
const compiledWasm = require(resolve(__dirname, "../../pkg/criv_wasm.js")) as unknown;
const stateRaw = readFileSync(
  resolve(__dirname, "../../../../fixtures/state/criv.state.v0.json"),
  "utf8",
);

test("uses the compiled canonical exports for validation and projections", async () => {
  const bridge = createCrivWasmBridge(async () => compiledWasm);

  const state = await bridge.validatedState(stateRaw);
  const summary = await bridge.summarizeState(stateRaw);
  const sources = await bridge.sourceEntries(stateRaw);
  const nodes = await bridge.graphNodes(stateRaw);
  const suggestions = await bridge.suggestSourceSelectors(stateRaw, "run", 10);

  assert.equal(state.schema, "criv.state.v0");
  assert.equal(summary.node_count, 6);
  assert.deepEqual(
    sources.map((entry) => entry.path),
    ["src/lib.rs"],
  );
  assert.equal(nodes.length, 6);
  assert.equal(suggestions[0]?.target, "src/lib.rs#fn:run");
  assert.equal((await bridge.lookupGraphNode(stateRaw, "src/lib.rs#fn:run"))?.kind, "function");

  await assert.rejects(
    bridge.validatedState(stateRaw.replace("criv.state.v0", "criv.state.v1")),
    /unsupported criv state schema/i,
  );
});

test("caches one stable descriptive error for a missing runtime", async () => {
  let attempts = 0;
  const bridge = createCrivWasmBridge(async () => {
    attempts += 1;
    throw new Error("missing module");
  });

  for (let request = 0; request < 2; request += 1) {
    await assert.rejects(bridge.summarizeState(stateRaw), (error: unknown) => {
      assert.ok(error instanceof CrivWasmLoadError);
      assert.equal(error.code, CRIV_WASM_LOAD_ERROR);
      assert.match(error.message, /rebuild the companion and reload the editor/i);
      return true;
    });
  }
  assert.equal(attempts, 1);
});

test("rejects a corrupt runtime instead of changing projection semantics", async () => {
  const bridge = createCrivWasmBridge(async () => ({ summarize_state() {} }));

  await assert.rejects(bridge.summarizeState(stateRaw), (error: unknown) => {
    assert.ok(error instanceof CrivWasmLoadError);
    assert.equal(error.code, CRIV_WASM_LOAD_ERROR);
    return true;
  });
});
