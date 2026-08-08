import assert from "node:assert/strict";
import test from "node:test";

import { WorkspaceStateStore, type WorkspaceStateHost } from "../../src/stateStore";
import type { CrivInitialProjections, CrivLoadedState, CrivWasmBridge } from "../../src/wasm";

test("publishes the editor snapshot and delegates queries to the active revision", async () => {
  const host = new FakeHost(["ready"]);
  const revision = new FakeRevision("ready");
  const bridge = new FakeBridge([Promise.resolve(revision)]);
  const store = new WorkspaceStateStore(host, bridge);

  await store.refresh();

  assert.equal(store.status.kind, "ready");
  assert.equal(store.status.kind === "ready" && store.status.snapshot.summary.schema, "ready");
  assert.equal(store.lookupNode("target")?.id, "ready:target");
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

  assert.equal(store.status.kind, "invalid-state");
  assert.equal(current.disposals, 1);
  assert.equal(store.lookupNode("target"), undefined);
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

class FakeHost implements WorkspaceStateHost {
  readonly root = fakeUri("root");
  readonly stateUri = fakeUri("root/.criv/state.json");
  reads = 0;
  watcherDisposals = 0;

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
