import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import type { CrivNodeKind } from "../src/contract.ts";

test("CrivNodeKind names every wire NodeKind", async () => {
  const wireKinds = JSON.parse(
    await readFile(new URL("../../../fixtures/editor/node-kinds.v1.json", import.meta.url), "utf8"),
  ) as string[];
  const kinds = {
    alias: true,
    behaviour: true,
    callback: true,
    class: true,
    code: true,
    decision: true,
    doc: true,
    "doc-heading": true,
    "dynamic-call": true,
    exception: true,
    "external-call": true,
    "external-module": true,
    function: true,
    guard: true,
    implementation: true,
    import: true,
    macro: true,
    "macro-callback": true,
    method: true,
    module: true,
    pattern: true,
    protocol: true,
    require: true,
    struct: true,
    use: true,
  } satisfies Record<CrivNodeKind, true>;

  assert.deepEqual(Object.keys(kinds), wireKinds);
});
