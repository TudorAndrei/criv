import assert from "node:assert/strict";
import test from "node:test";

import { LoadedRevisionOwner } from "@criv/editor-state";

test("the newest host preview replaces and disposes the old preview", async () => {
  const owner = new LoadedRevisionOwner<FakePreview>();
  const old = new FakePreview("old");
  const ready = new FakePreview("ready");

  await owner.replace(
    async () => old,
    (preview) => preview.name,
  );
  const result = await owner.replace(
    async () => ready,
    (preview) => preview.name,
  );

  assert.deepEqual(result, { kind: "committed", value: "ready" });
  assert.equal(old.disposals, 1);
  assert.equal(ready.disposals, 0);
});

test("an invalid status clears the visible preview and close is exact", async () => {
  const owner = new LoadedRevisionOwner<FakePreview>();
  const ready = new FakePreview("ready");
  await owner.replace(
    async () => ready,
    (preview) => preview.name,
  );

  owner.clear();
  assert.equal(owner.current, undefined);
  assert.equal(ready.disposals, 1);

  owner.dispose();
  owner.dispose();
  assert.equal(ready.disposals, 1);
});

class FakePreview {
  disposals = 0;

  constructor(readonly name: string) {}

  dispose(): void {
    this.disposals += 1;
  }
}
