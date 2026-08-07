import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { resolve } from "node:path";
import test from "node:test";

import {
  CRIV_LOADED_STATE_DISPOSED,
  CRIV_WASM_LOAD_ERROR,
  CrivLoadedStateDisposedError,
  CrivWasmLoadError,
  createCrivWasmBridge,
} from "../../src/wasm";

const require = createRequire(__filename);
const compiledWasm = require(resolve(__dirname, "../../pkg/criv_wasm.js")) as unknown;
const stateRaw = readFileSync(
  resolve(__dirname, "../../../../fixtures/state/criv.state.v1.json"),
  "utf8",
);

test("uses the compiled canonical exports for validation and projections", async () => {
  const bridge = createCrivWasmBridge(async () => compiledWasm);
  const revision = await bridge.loadState(stateRaw);
  const projections = revision.initialProjections();

  assert.equal(projections.state.schema, "criv.state.v1");
  assert.equal(projections.summary.node_count, 6);
  assert.deepEqual(
    projections.sources.map((entry) => entry.path),
    ["src/lib.rs"],
  );
  assert.equal(projections.nodes.length, 6);
  assert.equal(revision.suggestSelectors("run", 10)[0]?.target, "src/lib.rs#fn:run");
  assert.equal(revision.lookupNode("src/lib.rs#fn:run")?.kind, "function");

  await assert.rejects(
    bridge.loadState(stateRaw.replace("criv.state.v1", "criv.state.v2")),
    /unsupported criv state schema/i,
  );
  revision.dispose();
});

test("frees one loaded revision once and rejects later use", async () => {
  let frees = 0;
  const bridge = createCrivWasmBridge(async () => ({
    LoadedState: class {
      initialProjections() {
        return { state: {}, summary: {}, sources: [], nodes: [] };
      }
      lookupNode() {
        return undefined;
      }
      suggestSelectors() {
        return [];
      }
      free() {
        frees += 1;
      }
    },
  }));
  const revision = await bridge.loadState(stateRaw);

  revision.dispose();
  revision.dispose();

  assert.equal(frees, 1);
  assert.throws(
    () => revision.lookupNode("src/lib.rs"),
    (error: unknown) => {
      assert.ok(error instanceof CrivLoadedStateDisposedError);
      assert.equal(error.code, CRIV_LOADED_STATE_DISPOSED);
      return true;
    },
  );
});

test("caches one stable descriptive error for a missing runtime", async () => {
  let attempts = 0;
  const bridge = createCrivWasmBridge(async () => {
    attempts += 1;
    throw new Error("missing module");
  });

  for (let request = 0; request < 2; request += 1) {
    await assert.rejects(bridge.loadState(stateRaw), (error: unknown) => {
      assert.ok(error instanceof CrivWasmLoadError);
      assert.equal(error.code, CRIV_WASM_LOAD_ERROR);
      assert.match(error.message, /rebuild the companion and reload the editor/i);
      return true;
    });
  }
  assert.equal(attempts, 1);
});

test("rejects a corrupt runtime instead of changing projection semantics", async () => {
  const bridge = createCrivWasmBridge(async () => ({}));

  await assert.rejects(bridge.loadState(stateRaw), (error: unknown) => {
    assert.ok(error instanceof CrivWasmLoadError);
    assert.equal(error.code, CRIV_WASM_LOAD_ERROR);
    return true;
  });
});
