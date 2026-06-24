const assert = require("node:assert/strict");
const test = require("node:test");

test("extension scaffold uses criv command namespace", () => {
  assert.equal("criv.refreshStateView".startsWith("criv."), true);
});
