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

test("the preview command supports the official LikeC4 language", () => {
  const titleMenu = extensionPackage.contributes.menus["editor/title"] ?? [];
  const preview = titleMenu.find((item) => item.command === "criv.previewC4");

  assert.ok(preview);
  assert.match(preview.when, /likec4/);
  assert.match(preview.when, /criv-c4/);
});
