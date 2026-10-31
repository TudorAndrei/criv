import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const wasm = await import("../pkg/criv_wasm.js");
const raw = readFileSync(
  new URL("../../../../fixtures/state/criv.state.v1.json", import.meta.url),
  "utf8",
);
const loaded = new wasm.LoadedState(raw);
try {
  const projections = loaded.initialProjections();
  assert.equal("state" in projections, false);
  assert.deepEqual(
    projections.sources.map((source) => source.path),
    ["src/lib.rs"],
  );
} finally {
  loaded.free();
}
