import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import {
  buildStateSnapshot,
  c4Artifacts,
  parseStateEnvelope,
  registeredPatterns,
} from "../../src/stateModel";

const stateContractRaw = readFileSync(
  resolve(__dirname, "../../../../fixtures/state/criv.state.v0.json"),
  "utf8",
);

test("validates criv state schema before projection", () => {
  const parsed = parseStateEnvelope(stateContractRaw);
  assert.equal(parsed.ok, true);
  if (parsed.ok) {
    assert.equal(parsed.envelope.graph?.nodes?.length, 4);
    assert.equal(parsed.envelope.graph?.edges?.length, 3);
    assert.deepEqual(registeredPatterns(parsed.envelope), ["ADR-0001/entrypoint"]);
    assert.deepEqual(parsed.envelope.patterns?.["ADR-0001/entrypoint"], [
      {
        file: "src/lib.rs",
        range: "L1:C1-L1:C12",
        captures: { BODY: "", NAME: "run" },
      },
    ]);
  }

  const invalid = parseStateEnvelope(stateContractRaw.replace("criv.state.v0", "criv.state.v1"));
  assert.equal(invalid.ok, false);
  if (!invalid.ok) {
    assert.match(invalid.error, /Unsupported criv state schema/);
  }
});

test("reads registered patterns from explicit state field", () => {
  const parsed = parseStateEnvelope(
    JSON.stringify({
      schema: "criv.state.v0",
      "registered-patterns": ["adr/source-selector", "code/entrypoint"],
    }),
  );
  assert.equal(parsed.ok, true);
  if (parsed.ok) {
    assert.deepEqual(registeredPatterns(parsed.envelope), [
      "adr/source-selector",
      "code/entrypoint",
    ]);
  }
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

test("builds a loaded state snapshot from parsed state and wasm projections", () => {
  const raw = JSON.stringify({
    schema: "criv.state.v0",
    "registered-patterns": ["adr/source-selector"],
  });
  const parsed = parseStateEnvelope(raw);
  assert.equal(parsed.ok, true);
  if (!parsed.ok) {
    return;
  }

  const snapshot = buildStateSnapshot(
    raw,
    parsed.envelope,
    {
      schema: "criv.state.v0",
      node_count: 1,
      edge_count: 0,
      source_count: 1,
      pattern_count: 1,
    },
    [{ path: "docs/architecture/04-code.c4", frecency: 1 }],
    [],
  );

  assert.equal(snapshot.raw, raw);
  assert.deepEqual(snapshot.registeredPatterns, ["adr/source-selector"]);
  assert.deepEqual(snapshot.c4Artifacts, [
    {
      path: "docs/architecture/04-code.c4",
      label: "docs/architecture/04-code.c4",
      target: "docs/architecture/04-code.c4",
    },
  ]);
});
