import assert from "node:assert/strict";
import test from "node:test";

import extensionPackage from "../../package.json";

test("VSIX packaging does not traverse workspace dependencies", () => {
  assert.match(extensionPackage.scripts.package, /(?:^|\s)--no-dependencies(?:\s|$)/);
});

test("C4 files use the LikeC4 preview as their default editor", () => {
  const customEditors = extensionPackage.contributes.customEditors ?? [];
  const preview = customEditors.find((editor) => editor.viewType === "criv.c4Preview");

  assert.ok(preview);
  assert.equal(preview.priority, "default");
  assert.deepEqual(preview.selector, [{ filenamePattern: "*.c4" }]);
});
