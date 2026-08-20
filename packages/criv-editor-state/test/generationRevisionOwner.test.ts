import assert from "node:assert/strict";
import test from "node:test";

import { GenerationRevisionOwner } from "../src/generationRevisionOwner.ts";

test("orders ready replacement, failure clearing, and recovery by State generation", async () => {
  const owner = new GenerationRevisionOwner<FakeRevision>();
  const first = new FakeRevision("first");
  const second = new FakeRevision("second");
  const recovered = new FakeRevision("recovered");

  assert.equal(
    (
      await owner.replace(
        1,
        async () => first,
        (value) => value.name,
      )
    ).kind,
    "committed",
  );
  assert.equal(
    (
      await owner.replace(
        2,
        async () => second,
        (value) => value.name,
      )
    ).kind,
    "committed",
  );
  assert.equal(first.disposals, 1);

  assert.equal(owner.clear(3), true);
  assert.equal(second.disposals, 1);
  assert.equal(
    (
      await owner.replace(
        4,
        async () => recovered,
        (value) => value.name,
      )
    ).kind,
    "committed",
  );
  assert.equal(recovered.disposals, 0);
});

test("disposes late render candidates after a newer status or close", async () => {
  const owner = new GenerationRevisionOwner<FakeRevision>();
  const lateAfterClear = deferred<FakeRevision>();
  const lateAfterClose = deferred<FakeRevision>();
  const clearedCandidate = new FakeRevision("cleared");
  const closedCandidate = new FakeRevision("closed");

  const clearedResult = owner.replace(
    1,
    () => lateAfterClear.promise,
    (value) => value.name,
  );
  owner.clear(2);
  lateAfterClear.resolve(clearedCandidate);
  assert.equal((await clearedResult).kind, "superseded");
  assert.equal(clearedCandidate.disposals, 1);

  const closedResult = owner.replace(
    3,
    () => lateAfterClose.promise,
    (value) => value.name,
  );
  owner.dispose();
  owner.dispose();
  lateAfterClose.resolve(closedCandidate);
  assert.equal((await closedResult).kind, "superseded");
  assert.equal(closedCandidate.disposals, 1);
});

test("allows a new render in one State generation and rejects older work", async () => {
  const owner = new GenerationRevisionOwner<FakeRevision>();
  let starts = 0;
  await owner.replace(
    7,
    async () => {
      starts += 1;
      return new FakeRevision("current");
    },
    (value) => value.name,
  );

  assert.equal(
    (
      await owner.replace(
        7,
        async () => {
          starts += 1;
          return new FakeRevision("equal");
        },
        (value) => value.name,
      )
    ).kind,
    "committed",
  );
  assert.equal(owner.clear(6), false);
  assert.equal(starts, 2);
});

test("invalidates document work without consuming the State generation", async () => {
  const owner = new GenerationRevisionOwner<FakeRevision>();
  const pending = deferred<FakeRevision>();
  const late = new FakeRevision("late-document");
  const current = new FakeRevision("current-document");
  const lateResult = owner.replace(
    9,
    () => pending.promise,
    (value) => value.name,
  );

  owner.invalidate();
  pending.resolve(late);
  assert.equal((await lateResult).kind, "superseded");
  assert.equal(late.disposals, 1);
  assert.equal(
    (
      await owner.replace(
        9,
        async () => current,
        (value) => value.name,
      )
    ).kind,
    "committed",
  );
});

class FakeRevision {
  disposals = 0;
  readonly name: string;

  constructor(name: string) {
    this.name = name;
  }

  dispose(): void {
    this.disposals += 1;
  }
}

function deferred<Value>() {
  let resolve!: (value: Value) => void;
  const promise = new Promise<Value>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}
