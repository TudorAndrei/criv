import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const wasm = require("../pkg/criv_wasm.js");
const fixture = JSON.parse(
  readFileSync(
    new URL("../../../fixtures/editor/likec4-projection.v1.json", import.meta.url),
    "utf8",
  ),
);
const loaded = new wasm.LoadedState(JSON.stringify(fixture.state));
try {
  const projections = loaded.initialProjections();
  assert.equal("state" in projections, false);
  assert.deepEqual(projections.architecture, fixture.expected.architecture);
} finally {
  loaded.free();
}
