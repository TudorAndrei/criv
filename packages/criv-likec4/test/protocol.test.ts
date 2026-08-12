import assert from "node:assert/strict";
import test from "node:test";

import contract from "../../../assets/likec4-contract.json" with { type: "json" };
import {
  CRIV_LIKEC4_PROTOCOL_VERSION,
  CRIV_LIKEC4_VERSION,
} from "../src/protocol.ts";

test("uses the repository LikeC4 contract versions", () => {
  assert.equal(CRIV_LIKEC4_PROTOCOL_VERSION, contract.protocolVersion);
  assert.equal(CRIV_LIKEC4_VERSION, contract.likec4Version);
});
