import assert from "node:assert/strict";
import test from "node:test";
import { errorMessage } from "@criv/editor-state";

import { CheckRunOwner } from "../../../src/diagnostics/runs";

test("a save check cancels a manual check and drops its late result", async () => {
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
  await saveRun;
  manual.resolve("manual diagnostics");
  await manualRun;
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
  await Promise.all([oldResult, newResult]);

  assert.deepEqual(output.published, ["new diagnostics"]);
  assert.deepEqual(output.failures, []);
});

test("the current failure is published to its caller", async () => {
  const owner = new CheckRunOwner();
  const output = new Output();

  await owner.run(
    async () => Promise.reject(new Error("check failed")),
    output.publish,
    output.fail,
  );

  assert.deepEqual(output.failures, ["check failed"]);
});

test("disposal cancels the active check and drops late and later results", async () => {
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
  await result;
  await owner.run(async () => "after dispose", output.publish, output.fail);

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
