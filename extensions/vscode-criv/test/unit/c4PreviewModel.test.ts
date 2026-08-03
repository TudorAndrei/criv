import assert from "node:assert/strict";
import test from "node:test";

import { preferredC4ViewId } from "../../src/c4PreviewModel";

const views = [
  { id: "codeJavaScript", title: "JavaScript modules", sourcePath: "04-code.c4" },
  { id: "components", title: "Components", sourcePath: "03-components.c4" },
  { id: "context", title: "System context", sourcePath: "01-system-context.c4" },
];

test("selects the state view owned by the opened C4 file", () => {
  assert.equal(preferredC4ViewId("docs/architecture/01-system-context.c4", views), "context");
});

test("returns no preference when no state view belongs to the file", () => {
  assert.equal(preferredC4ViewId("docs/architecture/model.c4", views), undefined);
});
