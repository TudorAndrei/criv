import assert from "node:assert/strict";
import test from "node:test";

import { assetResourceUrl } from "../src/source/assets";

test("adds the asset hash to its resource URL", () => {
  const hash = "a".repeat(64);
  assert.equal(
    assetResourceUrl("app://vault/docs/diagram.png", hash),
    `app://vault/docs/diagram.png?criv-asset=${hash}`,
  );
  assert.equal(
    assetResourceUrl("app://vault/docs/diagram.png?existing=1", "a b"),
    "app://vault/docs/diagram.png?existing=1&criv-asset=a%20b",
  );
});
