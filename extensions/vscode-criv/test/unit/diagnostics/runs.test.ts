import assert from "node:assert/strict";
import test from "node:test";

import { CheckRunOwner } from "../../../src/diagnostics/runs";

interface Deferred<T> {
  promise: Promise<T>;
  resolve(value: T): void;
  reject(error: unknown): void;
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

test("a save check cancels a manual check and rejects its late result", async () => {
  const owner = new CheckRunOwner();
  const manual = deferred<string>();
  const save = deferred<string>();
  let manualSignal: AbortSignal | undefined;
  const publications: string[] = [];

  const manualRun = owner.run(
    (signal) => {
      manualSignal = signal;
      return manual.promise;
    },
    (value) => {
      publications.push(value);
    },
  );
  const saveRun = owner.run(
    () => save.promise,
    (value) => {
      publications.push(value);
    },
  );

  assert.equal(manualSignal?.aborted, true);

  save.resolve("save diagnostics");
  assert.deepEqual(await saveRun, { kind: "current", value: "save diagnostics" });

  manual.resolve("manual diagnostics");
  assert.deepEqual(await manualRun, { kind: "stale" });
  assert.deepEqual(publications, ["save diagnostics"]);
});

test("a manual check can replace a save check", async () => {
  const owner = new CheckRunOwner();
  const save = deferred<string>();
  const manual = deferred<string>();

  const saveRun = owner.run(() => save.promise);
  const manualRun = owner.run(() => manual.promise);

  save.resolve("save diagnostics");
  manual.resolve("manual diagnostics");

  assert.deepEqual(await saveRun, { kind: "stale" });
  assert.deepEqual(await manualRun, { kind: "current", value: "manual diagnostics" });
});

test("a stale failure does not escape after a newer check starts", async () => {
  const owner = new CheckRunOwner();
  const oldRun = deferred<string>();
  const newRun = deferred<string>();
  const failures: string[] = [];

  const oldResult = owner.run(
    () => oldRun.promise,
    undefined,
    (error) => {
      failures.push(error instanceof Error ? error.message : String(error));
    },
  );
  const newResult = owner.run(() => newRun.promise);
  oldRun.reject(new Error("late failure"));
  newRun.resolve("new diagnostics");

  assert.deepEqual(await oldResult, { kind: "stale" });
  assert.deepEqual(await newResult, { kind: "current", value: "new diagnostics" });
  assert.deepEqual(failures, []);
});

test("a newer check invalidates an older publication in progress", async () => {
  const owner = new CheckRunOwner();
  const publicationStarted = deferred<void>();
  const releasePublication = deferred<void>();
  const publications: string[] = [];

  const oldResult = owner.run(
    async () => "old diagnostics",
    async (value, signal) => {
      publicationStarted.resolve(undefined);
      await releasePublication.promise;
      if (!signal.aborted) {
        publications.push(value);
      }
    },
  );
  await publicationStarted.promise;

  const newResult = owner.run(
    async () => "new diagnostics",
    (value) => {
      publications.push(value);
    },
  );
  releasePublication.resolve(undefined);

  assert.deepEqual(await oldResult, { kind: "stale" });
  assert.deepEqual(await newResult, { kind: "current", value: "new diagnostics" });
  assert.deepEqual(publications, ["new diagnostics"]);
});

test("the current failure is published to its caller", async () => {
  const owner = new CheckRunOwner();
  const failures: string[] = [];

  const result = await owner.run(
    async () => Promise.reject(new Error("check failed")),
    undefined,
    (error) => {
      failures.push(error instanceof Error ? error.message : String(error));
    },
  );

  assert.equal(result.kind, "failed");
  assert.deepEqual(failures, ["check failed"]);
});

test("disposal cancels the active check and rejects late publication", async () => {
  const owner = new CheckRunOwner();
  const active = deferred<string>();
  let signal: AbortSignal | undefined;
  const publications: string[] = [];
  const result = owner.run(
    (runSignal) => {
      signal = runSignal;
      return active.promise;
    },
    (value) => {
      publications.push(value);
    },
  );

  owner.dispose();
  assert.equal(signal?.aborted, true);
  active.resolve("late diagnostics");

  assert.deepEqual(await result, { kind: "stale" });
  assert.deepEqual(publications, []);
  assert.deepEqual(await owner.run(async () => "after dispose"), { kind: "stale" });
});
