import assert from "node:assert/strict";
import test from "node:test";

import { C4PreviewLifecycle } from "../../../src/c4/previewLifecycle";

for (const surface of ["custom editor", "command panel"]) {
  test(`${surface} replaces, clears, recovers, and disposes exact revisions`, async () => {
    const lifecycle = new C4PreviewLifecycle<FakePreview>();
    const rendered: string[] = [];
    const statuses: string[] = [];
    const first = new FakePreview("model-a");
    const second = new FakePreview("model-b");
    const recoveredMissing = new FakePreview("model-after-missing");
    const recoveredInvalid = new FakePreview("model-after-invalid");
    const recoveredUnavailable = new FakePreview("model-after-unavailable");

    await lifecycle.publish(status(0, "loading"), undefined, render(rendered), show(statuses));
    await lifecycle.publish(
      status(1, "ready"),
      async () => first,
      render(rendered),
      show(statuses),
    );
    await lifecycle.publish(
      status(2, "ready"),
      async () => second,
      render(rendered),
      show(statuses),
    );
    await lifecycle.publish(status(3, "missing"), undefined, render(rendered), show(statuses));
    await lifecycle.publish(
      status(4, "ready"),
      async () => recoveredMissing,
      render(rendered),
      show(statuses),
    );
    await lifecycle.publish(status(5, "invalid"), undefined, render(rendered), show(statuses));
    await lifecycle.publish(
      status(6, "ready"),
      async () => recoveredInvalid,
      render(rendered),
      show(statuses),
    );
    await lifecycle.publish(status(7, "unavailable"), undefined, render(rendered), show(statuses));
    await lifecycle.publish(
      status(8, "ready"),
      async () => recoveredUnavailable,
      render(rendered),
      show(statuses),
    );

    assert.deepEqual(rendered, [
      "model-a",
      "model-b",
      "model-after-missing",
      "model-after-invalid",
      "model-after-unavailable",
    ]);
    assert.deepEqual(statuses, ["loading:0", "missing:3", "invalid:5", "unavailable:7"]);
    assert.equal(first.disposals, 1);
    assert.equal(second.disposals, 1);
    assert.equal(recoveredMissing.disposals, 1);
    assert.equal(recoveredInvalid.disposals, 1);
    assert.equal(recoveredUnavailable.disposals, 0);

    lifecycle.dispose();
    lifecycle.dispose();
    assert.equal(recoveredUnavailable.disposals, 1);
  });

  test(`${surface} rejects late renders after newer status and close`, async () => {
    const lifecycle = new C4PreviewLifecycle<FakePreview>();
    const rendered: string[] = [];
    const statuses: string[] = [];
    const lateForStatus = deferred<FakePreview>();
    const lateForClose = deferred<FakePreview>();
    const stale = new FakePreview("stale-model");
    const closed = new FakePreview("closed-model");

    const staleResult = lifecycle.publish(
      status(10, "ready"),
      () => lateForStatus.promise,
      render(rendered),
      show(statuses),
    );
    await lifecycle.publish(status(11, "invalid"), undefined, render(rendered), show(statuses));
    lateForStatus.resolve(stale);
    await staleResult;

    const closedResult = lifecycle.publish(
      status(12, "ready"),
      () => lateForClose.promise,
      render(rendered),
      show(statuses),
    );
    lifecycle.dispose();
    lateForClose.resolve(closed);
    await closedResult;

    assert.deepEqual(rendered, []);
    assert.deepEqual(statuses, ["invalid:11"]);
    assert.equal(stale.disposals, 1);
    assert.equal(closed.disposals, 1);
  });
}

class FakePreview {
  disposals = 0;

  constructor(readonly name: string) {}

  dispose(): void {
    this.disposals += 1;
  }
}

function status(generation: number, kind: string) {
  return { generation, kind };
}

function render(rendered: string[]) {
  return (preview: FakePreview) => rendered.push(preview.name);
}

function show(statuses: string[]) {
  return (value: { generation: number; kind: string }) => {
    statuses.push(`${value.kind}:${value.generation}`);
  };
}

function deferred<Value>() {
  let resolve!: (value: Value) => void;
  const promise = new Promise<Value>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}
