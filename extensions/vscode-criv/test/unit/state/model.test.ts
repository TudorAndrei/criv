import assert from "node:assert/strict";
import test from "node:test";

import { buildStateSnapshot } from "../../../src/state/model";
import type { CrivInitialProjections } from "../../../src/state/wasm";

test("publishes the canonical Wasm projection without host State parsing", () => {
  const projections: CrivInitialProjections = {
    summary: {
      schema: "criv.state.v1",
      node_count: 1,
      edge_count: 0,
      source_count: 1,
      asset_count: 1,
      pattern_count: 1,
    },
    sources: [{ path: "src/lib.rs" }],
    assets: [
      {
        path: "docs/diagram.png",
        mime: "image/png",
        bytes: 128,
        hash: "a".repeat(64),
      },
    ],
    nodes: [
      {
        id: "code:src/lib.rs",
        kind: "code",
        label: "Library",
        path: "src/lib.rs",
      },
    ],
    registeredPatterns: ["ADR-0002/note"],
    patternMatches: {},
  };

  const snapshot = buildStateSnapshot(projections);

  assert.equal(snapshot.registeredPatterns, projections.registeredPatterns);
  assert.equal(snapshot.assets, projections.assets);
  assert.equal(snapshot.sources, projections.sources);
  assert.equal(snapshot.graphNodes, projections.nodes);
});
