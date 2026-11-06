import assert from "node:assert/strict";
import test from "node:test";
import { errorMessage } from "@criv/editor-state";

import { CheckRunOwner } from "../../../src/diagnostics/runs";

test("a save check cancels a manual check and rejects its late result", async () => {
  const owner = new CheckRunOwner();
  const output = new Output();
  const manual = deferred<string>();
  const save = deferred<string>();
  let manualSignal: AbortSignal | undefined;

  const manualRun = owner.run(
    (signal) => {
      manualSignal = signal;
      return manual.promise;
    },
    output.publish,
    output.fail,
  );
  const saveRun = owner.run(() => save.promise, output.publish, output.fail);

  assert.equal(manualSignal?.aborted, true);
  save.resolve("save diagnostics");
  assert.deepEqual(await saveRun, { kind: "current", value: "save diagnostics" });
  manual.resolve("manual diagnostics");
  assert.deepEqual(await manualRun, { kind: "stale" });
  assert.deepEqual(output.published, ["save diagnostics"]);
});

test("a stale failure does not escape after a newer check starts", async () => {
  const owner = new CheckRunOwner();
  const output = new Output();
  const oldRun = deferred<string>();
  const newRun = deferred<string>();

  const oldResult = owner.run(() => oldRun.promise, output.publish, output.fail);
  const newResult = owner.run(() => newRun.promise, output.publish, output.fail);
  oldRun.reject(new Error("late failure"));
  newRun.resolve("new diagnostics");

  assert.deepEqual(await oldResult, { kind: "stale" });
  assert.deepEqual(await newResult, { kind: "current", value: "new diagnostics" });
  assert.deepEqual(output.failures, []);
});

test("a newer check invalidates an older publication in progress", async () => {
  const owner = new CheckRunOwner();
  const output = new Output();
  const publicationStarted = deferred<void>();
  const releasePublication = deferred<void>();

  const oldResult = owner.run(
    async () => "old diagnostics",
    async (value, signal) => {
      publicationStarted.resolve(undefined);
      await releasePublication.promise;
      if (!signal.aborted) {
        output.publish(value);
      }
    },
    output.fail,
  );
  await publicationStarted.promise;
  const newResult = owner.run(async () => "new diagnostics", output.publish, output.fail);
  releasePublication.resolve(undefined);

  assert.deepEqual(await oldResult, { kind: "stale" });
  assert.deepEqual(await newResult, { kind: "current", value: "new diagnostics" });
  assert.deepEqual(output.published, ["new diagnostics"]);
});

test("the current failure is published to its caller", async () => {
  const owner = new CheckRunOwner();
  const output = new Output();

  const result = await owner.run(
    async () => Promise.reject(new Error("check failed")),
    output.publish,
    output.fail,
  );

  assert.equal(result.kind, "failed");
  assert.deepEqual(output.failures, ["check failed"]);
});

test("disposal cancels the active check and rejects late publication", async () => {
  const owner = new CheckRunOwner();
  const output = new Output();
  const active = deferred<string>();
  let signal: AbortSignal | undefined;
  const result = owner.run(
    (runSignal) => {
      signal = runSignal;
      return active.promise;
    },
    output.publish,
    output.fail,
  );

  owner.dispose();
  assert.equal(signal?.aborted, true);
  active.resolve("late diagnostics");

  assert.deepEqual(await result, { kind: "stale" });
  assert.deepEqual(await owner.run(async () => "after dispose", output.publish, output.fail), {
    kind: "stale",
  });
  assert.deepEqual(output.published, []);
});

class Output {
  readonly published: string[] = [];
  readonly failures: string[] = [];
  readonly publish = (value: string) => {
    this.published.push(value);
  };
  readonly fail = (error: unknown) => {
    this.failures.push(errorMessage(error));
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}
