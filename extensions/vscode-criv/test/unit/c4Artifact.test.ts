import assert from "node:assert/strict";
import test from "node:test";

import { parseC4Artifact } from "../../src/c4Artifact";

test("recognizes LikeC4 as the only C4 format", () => {
  const summary = parseC4Artifact(
    "docs/architecture/model.c4",
    "specification { element system }\nmodel { app = system 'App' }\n",
  );
  assert.equal(summary.format, "likec4");
  assert.deepEqual(summary.diagnostics, []);
});

test("rejects legacy renderer formats", () => {
  for (const source of ["C4Context", "digraph architecture { a -> b }"]) {
    const summary = parseC4Artifact("docs/architecture/model.c4", source);
    assert.equal(summary.format, "unknown");
    assert.equal(summary.diagnostics[0]?.code, "unknown-c4-format");
  }
});
