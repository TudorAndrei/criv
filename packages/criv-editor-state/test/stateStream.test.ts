import assert from "node:assert/strict";
import test from "node:test";

import { CrivWasmLoadError, type CrivInitialProjections } from "../src/contract.ts";
import type { CrivLoadedState, CrivStateModule } from "../src/stateModule.ts";
import { CrivStateMissingError, CrivStateStream, type CrivStateHost } from "../src/stateStream.ts";

test("publishes the loaded projections and delegates queries to the active revision", async () => {
  const revision = new FakeRevision("ready");
  const stream = new CrivStateStream(new FakeHost(["ready"]), new FakeModule([revision]));

  const status = await stream.refresh();

  assert.equal(status.kind, "ready");
  assert.equal(status.kind === "ready" && status.source, "root");
  assert.equal(status.kind === "ready" && status.projections.summary.schema, "ready");
  assert.deepEqual(stream.lookupSourceTarget("src/lib.rs"), {
    kind: "resolved",
    canonical_target: "ready:src/lib.rs",
    node: { id: "ready", kind: "code", label: "ready" },
  });
  assert.deepEqual(
    stream.suggestSelectors("run", 5).map((suggestion) => suggestion.target),
    ["ready:run:5"],
  );
});

test("publishes one monotonic generation through every status and recovery", async () => {
  const readyA = new FakeRevision("ready-a");
  const readyB = new FakeRevision("ready-b");
  const readyC = new FakeRevision("ready-c");
  const host = new FakeHost([
    "ready-a",
    new CrivStateMissingError("State is missing."),
    "invalid",
    "ready-b",
    "unavailable",
    "ready-c",
  ]);
  const stream = new CrivStateStream(
    host,
    new FakeModule([
      readyA,
      new Error("unsupported criv state schema: criv.state.v2"),
      readyB,
      new CrivWasmLoadError("Wasm is unavailable.", { cause: new Error("load failed") }),
      readyC,
    ]),
  );
  const statuses = [stream.status];
  stream.onDidChange((status) => statuses.push(status));

  for (let refresh = 0; refresh < 6; refresh += 1) {
    await stream.refresh();
  }

  assert.deepEqual(
    statuses.map((status) => [
      status.generation,
      status.kind,
      "message" in status ? status.message : undefined,
    ]),
    [
      [0, "loading", undefined],
      [1, "loading", undefined],
      [1, "ready", undefined],
      [2, "missing", "State is missing."],
      [3, "loading", undefined],
      [3, "invalid", "Invalid criv State: unsupported criv state schema: criv.state.v2"],
      [4, "loading", undefined],
      [4, "ready", undefined],
      [5, "unavailable", "Wasm is unavailable."],
      [6, "loading", undefined],
      [6, "ready", undefined],
    ],
  );
  assert.equal(readyA.disposals, 1);
  assert.equal(readyB.disposals, 1);
  assert.equal(readyC.disposals, 0);
});

test("a failed refresh clears the active revision", async () => {
  const current = new FakeRevision("valid");
  const stream = new CrivStateStream(
    new FakeHost(["valid", "invalid"]),
    new FakeModule([current, new Error("invalid State")]),
  );

  await stream.refresh();
  await stream.refresh();

  assert.equal(stream.status.kind, "invalid");
  assert.equal(current.disposals, 1);
  assert.deepEqual(stream.lookupSourceTarget("src/lib.rs"), { kind: "unresolved" });
  assert.deepEqual(stream.suggestSelectors("run", 5), []);
});

test("a missing location publishes no source", async () => {
  const stream = new CrivStateStream(
    {
      locate: async () => {
        throw new CrivStateMissingError("Open a workspace.");
      },
      read: async () => assert.fail("read without a location"),
    },
    new FakeModule([]),
  );

  assert.deepEqual(await stream.refresh(), {
    generation: 1,
    kind: "missing",
    source: undefined,
    message: "Open a workspace.",
  });
});

test("publishes only the latest-started load and disposes the late candidate", async () => {
  const oldLoad = deferred<CrivLoadedState>();
  const newLoad = deferred<CrivLoadedState>();
  const oldRevision = new FakeRevision("old");
  const newRevision = new FakeRevision("new");
  const stateModule = new FakeModule([oldLoad.promise, newLoad.promise]);
  const stream = new CrivStateStream(new FakeHost(["old", "new"]), stateModule);

  const oldResult = stream.refresh();
  await waitFor(() => stateModule.loads === 1);
  const newResult = stream.refresh();
  await waitFor(() => stateModule.loads === 2);
  newLoad.resolve(newRevision);
  await newResult;
  oldLoad.resolve(oldRevision);
  await oldResult;

  assert.equal(stream.status.generation, 2);
  assert.equal(stream.status.kind === "ready" && stream.status.projections.summary.schema, "new");
  assert.equal(oldRevision.disposals, 1);
  assert.equal(newRevision.disposals, 0);
});

test("dispose stops publication and disposes a late candidate once", async () => {
  const load = deferred<CrivLoadedState>();
  const revision = new FakeRevision("late");
  const stateModule = new FakeModule([load.promise]);
  const host = new FakeHost(["late"]);
  const stream = new CrivStateStream(host, stateModule);
  const statuses: string[] = [];
  stream.onDidChange((status) => statuses.push(status.kind));

  const pending = stream.refresh();
  await waitFor(() => stateModule.loads === 1);
  stream.dispose();
  stream.dispose();
  load.resolve(revision);
  await pending;
  await stream.refresh();

  assert.deepEqual(statuses, ["loading"]);
  assert.equal(revision.disposals, 1);
  assert.equal(host.reads, 1);
  assert.equal(host.disposals, 1);
});

class FakeHost implements CrivStateHost<string> {
  reads = 0;
  disposals = 0;
  private readonly states: (string | Error)[];

  constructor(states: (string | Error)[]) {
    this.states = states;
  }

  async locate(): Promise<string> {
    return "root";
  }

  async read(): Promise<string> {
    const state = this.states[this.reads];
    this.reads += 1;
    if (state === undefined || state instanceof Error) {
      throw state ?? new Error("unexpected read");
    }
    return state;
  }

  dispose(): void {
    this.disposals += 1;
  }
}

class FakeModule implements CrivStateModule {
  loads = 0;
  private readonly revisions: (CrivLoadedState | Promise<CrivLoadedState> | Error)[];

  constructor(revisions: (CrivLoadedState | Promise<CrivLoadedState> | Error)[]) {
    this.revisions = revisions;
  }

  async loadState(): Promise<CrivLoadedState> {
    const revision = this.revisions[this.loads];
    this.loads += 1;
    if (revision === undefined || revision instanceof Error) {
      throw revision ?? new Error("unexpected load");
    }
    return revision;
  }
}

class FakeRevision implements CrivLoadedState {
  disposals = 0;
  private readonly name: string;

  constructor(name: string) {
    this.name = name;
  }

  initialProjections(): CrivInitialProjections {
    return {
      summary: {
        schema: this.name,
        node_count: 0,
        edge_count: 0,
        source_count: 0,
        asset_count: 0,
        pattern_count: 0,
      },
      sources: [],
      assets: [],
      nodes: [],
      registeredPatterns: [],
      patternMatches: {},
    };
  }

  lookupSourceTarget(target: string) {
    return {
      kind: "resolved" as const,
      canonical_target: `${this.name}:${target}`,
      node: { id: this.name, kind: "code", label: this.name },
    };
  }

  suggestSelectors(query: string, limit: number) {
    return [
      {
        target: `${this.name}:${query}:${limit}`,
        label: query,
        kind: "file",
        path: query,
        detail: "",
      },
    ];
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

async function waitFor(predicate: () => boolean): Promise<void> {
  for (let attempt = 0; attempt < 20; attempt += 1) {
    if (predicate()) {
      return;
    }
    await Promise.resolve();
  }
  throw new Error("condition was not reached");
}
