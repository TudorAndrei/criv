import assert from "node:assert/strict";

import { bumpedVersions } from "../version-bump.mjs";

// New release keeping the same minAppVersion gets an entry.
assert.deepEqual(bumpedVersions({ "0.1.0": "1.5.0" }, "0.2.0", "1.5.0"), {
  "0.1.0": "1.5.0",
  "0.2.0": "1.5.0",
});

// Re-running for an already-recorded version is a no-op.
assert.equal(bumpedVersions({ "0.1.0": "1.5.0" }, "0.1.0", "1.5.0"), null);

// A changed floor for an existing version is updated.
assert.deepEqual(bumpedVersions({ "0.1.0": "1.5.0" }, "0.1.0", "1.6.0"), {
  "0.1.0": "1.6.0",
});

console.log("version-bump tests passed");
