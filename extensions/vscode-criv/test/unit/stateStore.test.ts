import assert from "node:assert/strict";
import test from "node:test";

import { WorkspaceStateStore, type WorkspaceStateHost } from "../../src/stateStore";
import type { CrivInitialProjections, CrivLoadedState, CrivWasmBridge } from "../../src/wasm";

test("keeps the latest refresh and disposes a late revision", async () => {
  const host = new FakeHost(["old", "new"]);
  const oldLoad = deferred<CrivLoadedState>();
  const newLoad = deferred<CrivLoadedState>();
  const oldRevision = new FakeRevision("old");
  const newRevision = new FakeRevision("new");
  const bridge = new FakeBridge([oldLoad.promise, newLoad.promise]);
  const store = new WorkspaceStateStore(host, bridge);

  const oldRefresh = store.refresh();
  await waitFor(() => bridge.loads === 1);
  const newRefresh = store.refresh();
  await waitFor(() => bridge.loads === 2);
  newLoad.resolve(newRevision);
  await newRefresh;
  oldLoad.resolve(oldRevision);
  await oldRefresh;

  assert.equal(store.status.kind, "ready");
  assert.equal(store.status.kind === "ready" && store.status.snapshot.summary.schema, "new");
  assert.equal(oldRevision.disposals, 1);
  assert.equal(newRevision.disposals, 0);
  assert.equal(store.lookupNode("target")?.id, "new:target");
  assert.equal(store.suggestSelectors("query", 5)[0]?.target, "new:query");
  assert.equal(host.reads, 2);
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

  assert.equal(store.status.kind, "invalid-state");
  assert.equal(current.disposals, 1);
  assert.equal(store.lookupNode("target"), undefined);
});

test("disposes a revision that finishes after store shutdown", async () => {
  const host = new FakeHost(["late"]);
  const load = deferred<CrivLoadedState>();
  const late = new FakeRevision("late");
  const bridge = new FakeBridge([load.promise]);
  const store = new WorkspaceStateStore(host, bridge);

  const refresh = store.refresh();
  await waitFor(() => bridge.loads === 1);
  store.dispose();
  load.resolve(late);
  await refresh;
  await store.refresh();

  assert.equal(late.disposals, 1);
  assert.equal(host.reads, 1);
  assert.equal(bridge.loads, 1);
});

class FakeHost implements WorkspaceStateHost {
  readonly root = fakeUri("root");
  readonly stateUri = fakeUri("root/.criv/state.json");
  reads = 0;

  constructor(private readonly states: string[]) {}

  async findWorkspaceRoot() {
    return this.root;
  }

  stateFile() {
    return this.stateUri;
  }

  async readState() {
    const state = this.states[this.reads];
    this.reads += 1;
    if (state === undefined) {
      throw new Error("missing State");
    }
    return state;
  }

  watchState(_root: unknown, _refresh: () => void) {
    return { dispose() {} };
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
      state: { schema: this.name },
      summary: {
        schema: this.name,
        node_count: 0,
        edge_count: 0,
        source_count: 0,
        pattern_count: 0,
      },
      sources: [],
      nodes: [],
    };
  }

  lookupNode(target: string) {
    return { id: `${this.name}:${target}`, kind: "code", label: target };
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

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
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
