import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { resolve } from "node:path";
import test from "node:test";

import { CRIV_WASM_LOAD_ERROR, CrivWasmLoadError, createCrivWasmBridge } from "../../src/wasm";

const require = createRequire(__filename);
const compiledWasm = require(resolve(__dirname, "../../pkg/criv_wasm.js")) as unknown;
const stateRaw = readFileSync(
  resolve(__dirname, "../../../../fixtures/state/criv.state.v1.json"),
  "utf8",
);
const lookupFixture = JSON.parse(
  readFileSync(
    resolve(__dirname, "../../../../fixtures/editor/source-target-lookup.v1.json"),
    "utf8",
  ),
) as {
  state: unknown;
  cases: {
    target: string;
    kind: string;
    canonical_target?: string;
    total_candidate_count?: number;
  }[];
};

test("loads the generated package through the VS Code Wasm adapter", async () => {
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
  const lookup = revision.lookupSourceTarget("src/lib.rs#fn:run");
  assert.equal(lookup.kind === "resolved" && lookup.node.kind, "function");

  await assert.rejects(
    bridge.loadState(stateRaw.replace("criv.state.v1", "criv.state.v2")),
    /unsupported criv state schema/i,
  );
  revision.dispose();
});

test("keeps VS Code recovery text in its package loader adapter", async () => {
  let attempts = 0;
  const bridge = createCrivWasmBridge(async () => {
    attempts += 1;
    throw new Error("missing module");
  });

  for (let request = 0; request < 2; request += 1) {
    await assert.rejects(bridge.loadState(stateRaw), (error: unknown) => {
      assert.ok(error instanceof CrivWasmLoadError);
      assert.equal(error.code, CRIV_WASM_LOAD_ERROR);
      assert.equal(
        error.message,
        "Could not load the packaged criv Wasm runtime. Rebuild the companion and reload the editor.",
      );
      return true;
    });
  }
  assert.equal(attempts, 1);
});

test("uses the shared source-target lookup contract", async () => {
  const bridge = createCrivWasmBridge(async () => compiledWasm);
  const revision = await bridge.loadState(JSON.stringify(lookupFixture.state));

  for (const expected of lookupFixture.cases) {
    const actual = revision.lookupSourceTarget(expected.target);
    assert.equal(actual.kind, expected.kind, `lookup kind for ${expected.target}`);
    if (actual.kind === "resolved") {
      assert.equal(actual.canonical_target, expected.canonical_target);
    }
    if (actual.kind === "ambiguous") {
      assert.equal(actual.total_candidate_count, expected.total_candidate_count);
      assert(actual.candidates.length <= 5);
    }
  }
  revision.dispose();
});
