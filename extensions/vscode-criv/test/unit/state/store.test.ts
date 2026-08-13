import assert from "node:assert/strict";
import test from "node:test";

import { WorkspaceStateStore, type WorkspaceStateHost } from "../../../src/state/store";
import {
  CrivWasmLoadError,
  type CrivInitialProjections,
  type CrivLoadedState,
  type CrivWasmBridge,
} from "../../../src/state/wasm";

test("publishes the editor snapshot and delegates queries to the active revision", async () => {
  const host = new FakeHost(["ready"]);
  const revision = new FakeRevision("ready");
  const bridge = new FakeBridge([Promise.resolve(revision)]);
  const store = new WorkspaceStateStore(host, bridge);

  await store.refresh();

  assert.equal(store.status.kind, "ready");
  assert.equal(store.status.kind === "ready" && store.status.snapshot.summary.schema, "ready");
  const lookup = store.lookupSourceTarget("target");
  assert.equal(lookup.kind === "resolved" && lookup.node.id, "ready:target");
  assert.equal(store.suggestSelectors("query", 5)[0]?.target, "ready:query");
  assert.equal(host.reads, 1);
});

test("clears the current revision when the latest refresh fails", async () => {
  const host = new FakeHost(["valid", "invalid"]);
  const current = new FakeRevision("valid");
  const bridge = new FakeBridge([
    Promise.resolve(current),
    Promise.reject(new Error("unsupported criv state schema: criv.state.v2")),
  ]);
  const store = new WorkspaceStateStore(host, bridge);

  await store.refresh();
  await store.refresh();

  assert.equal(store.status.kind, "invalid");
  assert.equal(current.disposals, 1);
  assert.deepEqual(store.lookupSourceTarget("target"), { kind: "unresolved" });
});

test("disposes the watcher and active revision during editor shutdown", async () => {
  const host = new FakeHost(["ready"]);
  const active = new FakeRevision("ready");
  const bridge = new FakeBridge([Promise.resolve(active)]);
  const store = new WorkspaceStateStore(host, bridge);

  await store.refresh();
  store.dispose();
  store.dispose();
  await store.refresh();

  assert.equal(active.disposals, 1);
  assert.equal(host.watcherDisposals, 1);
  assert.equal(host.reads, 1);
  assert.equal(bridge.loads, 1);
});

test("publishes one monotonic generation through every State status and recovery", async () => {
  const host = new FakeHost([
    "ready-a",
    new Error("missing State"),
    "invalid",
    "ready-b",
    "unavailable",
    "ready-c",
  ]);
  const readyA = new FakeRevision("ready-a");
  const readyB = new FakeRevision("ready-b");
  const readyC = new FakeRevision("ready-c");
  const bridge = new FakeBridge([
    Promise.resolve(readyA),
    Promise.reject(new Error("unsupported criv state schema: criv.state.v2")),
    Promise.resolve(readyB),
    Promise.reject(new CrivWasmLoadError("Wasm is unavailable", new Error("load failed"))),
    Promise.resolve(readyC),
  ]);
  const store = new WorkspaceStateStore(host, bridge);
  const statuses = [store.status];
  const subscription = store.onDidChangeStatus((status) => statuses.push(status));

  await store.refresh();
  await store.refresh();
  await store.refresh();
  await store.refresh();
  await store.refresh();
  await store.refresh();

  assert.deepEqual(
    statuses.map((status) => [status.generation, status.kind]),
    [
      [0, "loading"],
      [1, "loading"],
      [1, "ready"],
      [2, "missing"],
      [3, "loading"],
      [3, "invalid"],
      [4, "loading"],
      [4, "ready"],
      [5, "unavailable"],
      [6, "loading"],
      [6, "ready"],
    ],
  );
  assert.equal(readyA.disposals, 1);
  assert.equal(readyB.disposals, 1);
  assert.equal(store.status.kind, "ready");
  subscription.dispose();
});

test("publishes only the latest-started State load and disposes late candidates", async () => {
  const host = new FakeHost(["old", "new"]);
  const oldLoad = deferred<CrivLoadedState>();
  const newLoad = deferred<CrivLoadedState>();
  const oldRevision = new FakeRevision("old");
  const newRevision = new FakeRevision("new");
  const bridge = new FakeBridge([oldLoad.promise, newLoad.promise]);
  const store = new WorkspaceStateStore(host, bridge);

  const oldResult = store.refresh();
  await waitFor(() => bridge.loads === 1);
  const newResult = store.refresh();
  await waitFor(() => bridge.loads === 2);
  newLoad.resolve(newRevision);
  await newResult;
  oldLoad.resolve(oldRevision);
  await oldResult;

  assert.equal(store.status.generation, 2);
  assert.equal(store.status.kind === "ready" && store.status.snapshot.summary.schema, "new");
  assert.equal(oldRevision.disposals, 1);
  assert.equal(newRevision.disposals, 0);
});

test("workspace shutdown stops publication and disposes a late State candidate once", async () => {
  const host = new FakeHost(["late"]);
  const load = deferred<CrivLoadedState>();
  const revision = new FakeRevision("late");
  const bridge = new FakeBridge([load.promise]);
  const store = new WorkspaceStateStore(host, bridge);
  const statuses: string[] = [];
  store.onDidChangeStatus((status) => statuses.push(status.kind));

  const pending = store.refresh();
  await waitFor(() => bridge.loads === 1);
  store.dispose();
  store.dispose();
  load.resolve(revision);
  await pending;

  assert.deepEqual(statuses, ["loading"]);
  assert.equal(revision.disposals, 1);
  assert.equal(host.watcherDisposals, 1);
});

class FakeHost implements WorkspaceStateHost {
  readonly root = fakeUri("root");
  readonly stateUri = fakeUri("root/.criv/state.json");
  reads = 0;
  watcherDisposals = 0;

  constructor(private readonly states: (string | Error)[]) {}

  async findWorkspaceRoot() {
    return this.root;
  }

  stateFile() {
    return this.stateUri;
  }

  async readState() {
    const state = this.states[this.reads];
    this.reads += 1;
    if (state instanceof Error) {
      throw state;
    }
    if (state === undefined) {
      throw new Error("missing State");
    }
    return state;
  }

  watchState(_root: unknown, _refresh: () => void) {
    return { dispose: () => (this.watcherDisposals += 1) };
  }
}

class FakeBridge implements CrivWasmBridge {
  loads = 0;

  constructor(private readonly revisions: Promise<CrivLoadedState>[]) {}

  loadState(_raw: string): Promise<CrivLoadedState> {
    const revision = this.revisions[this.loads];
    this.loads += 1;
    if (!revision) {
      throw new Error("unexpected load");
    }
    return revision;
  }
}

class FakeRevision implements CrivLoadedState {
  disposals = 0;

  constructor(private readonly name: string) {}

  initialProjections(): CrivInitialProjections {
    return {
      summary: {
        schema: this.name,
        node_count: 0,
        edge_count: 0,
        source_count: 0,
        pattern_count: 0,
      },
      sources: [],
      nodes: [],
      registeredPatterns: [],
      patternMatches: {},
      c4Artifacts: [],
    };
  }

  lookupSourceTarget(target: string) {
    return {
      kind: "resolved" as const,
      canonical_target: target,
      node: { id: `${this.name}:${target}`, kind: "code", label: target },
    };
  }

  suggestSelectors(query: string) {
    return [
      {
        target: `${this.name}:${query}`,
        label: query,
        kind: "file",
        path: query,
        detail: "file",
      },
    ];
  }

  dispose(): void {
    this.disposals += 1;
  }
}

function fakeUri(value: string) {
  return { toString: () => value } as never;
}

function deferred<Value>() {
  let resolve!: (value: Value) => void;
  const promise = new Promise<Value>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}

async function waitFor(predicate: () => boolean): Promise<void> {
  for (let attempt = 0; attempt < 20; attempt += 1) {
    if (predicate()) {
      return;
    }
    await Promise.resolve();
  }
  throw new Error("condition was not reached");
}
