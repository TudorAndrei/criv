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
    sources: [{ path: "docs/architecture/systems.c4" }],
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
        id: "code:docs/architecture/systems.c4",
        kind: "module",
        label: "System model",
        path: "docs/architecture/systems.c4",
      },
    ],
    registeredPatterns: ["ADR-0002/note"],
    patternMatches: {},
    architecture: {
      protocolVersion: 1,
      likec4Version: "1.59.2",
      workspace: "docs/architecture",
      model: { elements: {}, relations: {}, views: {} },
      views: [{ id: "index", title: "System context", sourcePath: "systems.c4" }],
      sourceLinks: [{ element: "criv", target: "src/lib.rs" }],
    },
    c4Artifacts: [
      {
        path: "docs/architecture/systems.c4",
        label: "System model",
        target: "docs/architecture/systems.c4",
      },
    ],
  };

  const snapshot = buildStateSnapshot(projections);

  assert.equal(snapshot.architecture, projections.architecture);
  assert.equal(snapshot.registeredPatterns, projections.registeredPatterns);
  assert.equal(snapshot.assets, projections.assets);
  assert.equal(snapshot.c4Artifacts, projections.c4Artifacts);
});
