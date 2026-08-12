import assert from "node:assert/strict";
import test from "node:test";

import { LoadedRevisionOwner } from "../src/revisionOwner.ts";

test("commits a replacement and disposes the previous revision once", async () => {
  const owner = new LoadedRevisionOwner<FakeRevision>();
  const first = new FakeRevision("first");
  const second = new FakeRevision("second");

  assert.deepEqual(await owner.replace(async () => first, (revision) => revision.name), {
    kind: "committed",
    value: "first",
  });
  assert.deepEqual(await owner.replace(async () => second, (revision) => revision.name), {
    kind: "committed",
    value: "second",
  });

  assert.equal(owner.current, second);
  assert.equal(first.disposals, 1);
  assert.equal(second.disposals, 0);
});

test("keeps the latest result and disposes a late revision once", async () => {
  const owner = new LoadedRevisionOwner<FakeRevision>();
  const oldLoad = deferred<FakeRevision>();
  const newLoad = deferred<FakeRevision>();
  const oldRevision = new FakeRevision("old");
  const newRevision = new FakeRevision("new");

  const oldResult = owner.replace(() => oldLoad.promise, (revision) => revision.name);
  const newResult = owner.replace(() => newLoad.promise, (revision) => revision.name);
  newLoad.resolve(newRevision);
  assert.deepEqual(await newResult, { kind: "committed", value: "new" });
  oldLoad.resolve(oldRevision);
  assert.deepEqual(await oldResult, { kind: "superseded" });

  assert.equal(owner.current, newRevision);
  assert.equal(oldRevision.disposals, 1);
  assert.equal(newRevision.disposals, 0);
});

test("clears the active revision when the latest load or preparation fails", async () => {
  const owner = new LoadedRevisionOwner<FakeRevision>();
  const active = new FakeRevision("active");
  await owner.replace(async () => active, (revision) => revision.name);

  const loadError = new Error("load failed");
  assert.deepEqual(await owner.replace(async () => Promise.reject(loadError), () => "unused"), {
    kind: "failed",
    error: loadError,
  });
  assert.equal(active.disposals, 1);
  assert.equal(owner.current, undefined);

  const candidate = new FakeRevision("candidate");
  const prepareError = new Error("prepare failed");
  assert.deepEqual(
    await owner.replace(async () => candidate, () => {
      throw prepareError;
    }),
    { kind: "failed", error: prepareError },
  );
  assert.equal(candidate.disposals, 1);
  assert.equal(owner.current, undefined);
});

test("disposes active and late revisions once during shutdown", async () => {
  const owner = new LoadedRevisionOwner<FakeRevision>();
  const active = new FakeRevision("active");
  const late = new FakeRevision("late");
  const lateLoad = deferred<FakeRevision>();
  await owner.replace(async () => active, (revision) => revision.name);
  const lateResult = owner.replace(() => lateLoad.promise, (revision) => revision.name);

  owner.dispose();
  owner.dispose();
  lateLoad.resolve(late);

  assert.deepEqual(await lateResult, { kind: "superseded" });
  assert.equal(active.disposals, 1);
  assert.equal(late.disposals, 1);
  assert.deepEqual(await owner.replace(async () => new FakeRevision("unused"), () => "unused"), {
    kind: "closed",
  });
});

test("clear invalidates pending work and keeps the owner reusable", async () => {
  const owner = new LoadedRevisionOwner<FakeRevision>();
  const active = new FakeRevision("active");
  const late = new FakeRevision("late");
  const lateLoad = deferred<FakeRevision>();
  await owner.replace(async () => active, (revision) => revision.name);
  const lateResult = owner.replace(() => lateLoad.promise, (revision) => revision.name);

  owner.clear();
  lateLoad.resolve(late);

  assert.deepEqual(await lateResult, { kind: "superseded" });
  assert.equal(active.disposals, 1);
  assert.equal(late.disposals, 1);
  const replacement = new FakeRevision("replacement");
  assert.deepEqual(await owner.replace(async () => replacement, (revision) => revision.name), {
    kind: "committed",
    value: "replacement",
  });
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
  let reject!: (error: unknown) => void;
  const promise = new Promise<Value>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}
