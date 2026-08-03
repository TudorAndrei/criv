import assert from "node:assert/strict";
import test from "node:test";

import extensionPackage from "../../package.json";

test("VSIX packaging does not traverse workspace dependencies", () => {
  assert.match(extensionPackage.scripts.package, /(?:^|\s)--no-dependencies(?:\s|$)/);
});
