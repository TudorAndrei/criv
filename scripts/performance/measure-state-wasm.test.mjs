import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import test from "node:test";

test("reports the packaged Wasm storage operations as JSON", () => {
  const root = mkdtempSync(join(tmpdir(), "criv-state-wasm-test-"));
  const packageRoot = join(root, "pkg");
  mkdirSync(packageRoot);
  writeFileSync(
    join(packageRoot, "package.json"),
    JSON.stringify({ main: "criv_wasm.js", type: "commonjs" }),
  );
  writeFileSync(
    join(packageRoot, "criv_wasm.js"),
    `class LoadedState {
      constructor(raw) { this.state = JSON.parse(raw); }
      initialProjections() {
        return {
          state: this.state,
          summary: { node_count: this.state.graph.nodes.length },
          sources: this.state["source-index"],
          nodes: this.state.graph.nodes
        };
      }
      lookupNode(target) { return this.state.graph.nodes.find(node => node.id === target); }
      suggestSelectors() {
        return this.state["source-index"].map(source => ({ ...source, target: source.path }));
      }
      free() {}
    }
    module.exports = { LoadedState };\n`,
  );
  writeFileSync(join(packageRoot, "criv_wasm_bg.wasm"), Buffer.from([0, 1, 2, 3]));
  const state = join(root, "state.json");
  const raw = JSON.stringify({
    schema: "criv.state.v1",
    graph: { nodes: [{ id: "node", path: "src/lib.rs" }], edges: [] },
    "registered-patterns": [],
    patterns: {},
    "source-index": [{ path: "src/lib.rs" }],
  });
  writeFileSync(state, raw);
  const report = join(root, "report.json");

  const result = spawnSync(
    process.execPath,
    [
      fileURLToPath(new URL("./measure-state-wasm.mjs", import.meta.url)),
      "--state",
      state,
      "--package",
      packageRoot,
      "--samples",
      "1",
      "--allow-low-samples",
      "--output",
      report,
    ],
    { encoding: "utf8" },
  );

  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout, "");
  const output = JSON.parse(readFileSync(report, "utf8"));
  assert.equal(output.schema, "criv.state-wasm-loaded-revision.v1");
  assert.equal(output.samples, 1);
  assert.equal(output.state_bytes, Buffer.byteLength(raw));
  assert.equal(output.wasm_module_bytes, 4);
  assert.deepEqual(Object.keys(output.operations), [
    "cold_load_and_initial_projections",
    "initial_projections_after_load",
    "lookup_present",
    "lookup_missing",
    "selector_empty",
    "selector_exact",
    "selector_suffix",
    "selector_missing",
    "replacement_lifetime",
  ]);
  for (const operation of Object.values(output.operations)) {
    assert.equal(operation.timing.samples, 1);
    assert.equal(operation.raw.length, 1);
  }
  assert.equal(output.operations.replacement_lifetime.replacement_lifetime.cycles, 20);
  assert.equal(
    output.operations.replacement_lifetime.replacement_lifetime.passes_110_percent_limit,
    true,
  );
});
