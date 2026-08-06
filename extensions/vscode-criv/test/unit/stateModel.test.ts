import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import {
  buildStateSnapshot,
  c4Artifacts,
  registeredPatterns,
  type CrivStateEnvelope,
} from "../../src/stateModel";

const stateContractRaw = readFileSync(
  resolve(__dirname, "../../../../fixtures/state/criv.state.v1.json"),
  "utf8",
);
const stateContract = JSON.parse(stateContractRaw) as CrivStateEnvelope;

test("builds host state from a canonical validated projection", () => {
  assert.equal(stateContract.graph?.nodes?.length, 6);
  assert.equal(stateContract.graph?.edges?.length, 5);
  assert.deepEqual(registeredPatterns(stateContract), ["ADR-0001/entrypoint"]);
  assert.deepEqual(stateContract.patterns?.["ADR-0001/entrypoint"], [
    {
      file: "src/lib.rs",
      range: "L1:C1-L1:C12",
      captures: { BODY: "", NAME: "run" },
    },
  ]);
  assert.equal(stateContract.patterns?.["ADR-0002/draft-entrypoint"], undefined);

  const snapshot = buildStateSnapshot(
    stateContract,
    {
      schema: "criv.state.v1",
      node_count: 6,
      edge_count: 5,
      source_count: 1,
      pattern_count: 1,
    },
    [],
    [],
  );
  assert.deepEqual(snapshot.registeredPatterns, ["ADR-0001/entrypoint"]);
});

test("reads registered patterns from explicit state field", () => {
  const envelope: CrivStateEnvelope = {
    schema: "criv.state.v1",
    "registered-patterns": ["adr/source-selector", "code/entrypoint"],
  };
  assert.deepEqual(registeredPatterns(envelope), ["adr/source-selector", "code/entrypoint"]);
});

test("keeps LikeC4 view source ownership in host state", () => {
  const raw = JSON.stringify({
    schema: "criv.state.v1",
    architecture: {
      likec4Version: "1.59.2",
      revision: 1,
      model: {
        raw: {},
        views: [
          {
            id: "context",
            title: "System context",
            sourcePath: "01-system-context.c4",
          },
        ],
        sourceLinks: [],
      },
    },
  });
  const envelope = JSON.parse(raw) as CrivStateEnvelope;

  const snapshot = buildStateSnapshot(
    envelope,
    {
      schema: "criv.state.v1",
      node_count: 0,
      edge_count: 0,
      source_count: 0,
      pattern_count: 0,
    },
    [],
    [],
  );

  assert.equal(snapshot.architecture?.views[0]?.sourcePath, "01-system-context.c4");
});

test("collects c4 artifacts from source entries and graph nodes", () => {
  assert.deepEqual(
    c4Artifacts(
      [
        { path: "docs/architecture/01-system-context.c4", frecency: 1 },
        { path: "src/lib.rs", frecency: 1 },
      ],
      [
        {
          id: "code:docs/architecture/04-code.c4",
          kind: "code",
          label: "Code diagram",
          path: "docs/architecture/04-code.c4",
          source_target: "docs/architecture/04-code.c4",
        },
      ],
    ),
    [
      {
        path: "docs/architecture/01-system-context.c4",
        label: "docs/architecture/01-system-context.c4",
        target: "docs/architecture/01-system-context.c4",
      },
      {
        path: "docs/architecture/04-code.c4",
        label: "Code diagram",
        target: "docs/architecture/04-code.c4",
      },
    ],
  );
});

test("builds a loaded state snapshot from wasm projections", () => {
  const envelope = {
    schema: "criv.state.v1",
    "registered-patterns": ["adr/source-selector"],
  } satisfies CrivStateEnvelope;

  const snapshot = buildStateSnapshot(
    envelope,
    {
      schema: "criv.state.v1",
      node_count: 1,
      edge_count: 0,
      source_count: 1,
      pattern_count: 1,
    },
    [{ path: "docs/architecture/04-code.c4", frecency: 1 }],
    [],
  );

  assert.deepEqual(snapshot.registeredPatterns, ["adr/source-selector"]);
  assert.deepEqual(snapshot.c4Artifacts, [
    {
      path: "docs/architecture/04-code.c4",
      label: "docs/architecture/04-code.c4",
      target: "docs/architecture/04-code.c4",
    },
  ]);
});
