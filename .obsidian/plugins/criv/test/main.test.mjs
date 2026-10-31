import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import * as esbuild from "esbuild";

const __dirname = dirname(fileURLToPath(import.meta.url));
const pluginRoot = resolve(__dirname, "..");
const outFile = resolve(tmpdir(), `criv-main-test-${process.pid}.mjs`);
const stubDir = resolve(__dirname, "stubs");
const wasmPath = resolve(pluginRoot, "pkg/criv_wasm.js");

mkdirSync(dirname(outFile), { recursive: true });
await esbuild.build({
  entryPoints: [resolve(pluginRoot, "src/main.ts")],
  outfile: outFile,
  bundle: true,
  format: "esm",
  platform: "node",
  target: "node18",
  plugins: [aliasPlugin()],
});

const { default: CrivPlugin } = await import(pathToFileURL(outFile).href);
const validState = {
  schema: "criv.state.v1",
  graph: {
    nodes: [{ id: "note:README.md", kind: "doc", label: "README", path: "README.md" }],
    edges: [{ from: "note:README.md", kind: "references", to: "code:src/lib.rs" }],
  },
  patterns: {},
  "registered-patterns": ["ADR-0001/no-block-on"],
  "source-index": [{ path: "src/lib.rs" }, { path: "src/main.rs" }],
};
const validStateRaw = JSON.stringify(validState);

class FakeRevision {
  disposals = 0;

  constructor(state) {
    this.state = state;
  }

  initialProjections() {
    return {
      summary: {
        schema: this.state.schema,
        node_count: this.state.graph?.nodes?.length ?? 0,
        edge_count: this.state.graph?.edges?.length ?? 0,
        source_count: this.state["source-index"]?.length ?? 0,
        asset_count: this.state["asset-index"]?.length ?? 0,
        pattern_count: this.state["registered-patterns"]?.length ?? 0,
      },
      sources: this.state["source-index"] ?? [],
      assets: this.state["asset-index"] ?? [],
      nodes: [],
      registeredPatterns: this.state["registered-patterns"] ?? [],
      patternMatches: this.state.patterns ?? {},
    };
  }

  lookupSourceTarget(target) {
    return this.state.lookup?.[target] ?? { kind: "unresolved" };
  }

  suggestSelectors() {
    return [];
  }

  dispose() {
    this.disposals += 1;
  }
}

{
  const { plugin } = createPlugin({ ".criv/state.json": validStateRaw });
  const loaded = await plugin.loadState();
  assert.deepEqual(loaded, projectedState(validState));
  assert.equal(plugin.cachedState(), loaded);
  assert.equal(plugin.stateStatus(), "criv state is unavailable at .criv/state.json.");
  assert.deepEqual(plugin.currentStateStatus(), {
    generation: 1,
    kind: "ready",
    state: loaded,
  });
}

{
  const { plugin } = createPlugin({});
  assert.equal(await plugin.loadState(), null);
  assert.equal(plugin.cachedState(), null);
  assert.equal(plugin.stateStatus(), "Could not read .criv/state.json: missing .criv/state.json");
  assert.equal(plugin.currentStateStatus().kind, "missing");
  assert.equal(plugin.currentStateStatus().generation, 1);
}

{
  const { plugin } = createPlugin({
    ".criv/state.json": JSON.stringify({ ...validState, schema: "criv.state.v2" }),
  });
  assert.equal(await plugin.loadState(), null);
  assert.equal(plugin.cachedState(), null);
  assert.equal(
    plugin.stateStatus(),
    "Could not read .criv/state.json: unsupported criv state schema: criv.state.v2",
  );
  assert.equal(plugin.currentStateStatus().kind, "invalid");
}

{
  const { plugin } = createPlugin({ ".criv/state.json": validStateRaw });
  plugin.settings.statePath = "../state.json";
  assert.equal(await plugin.loadState(), null);
  assert.equal(plugin.cachedState(), null);
  assert.equal(plugin.stateStatus(), "Invalid criv state path ../state.json.");
  assert.equal(plugin.currentStateStatus().kind, "invalid");
}

{
  const { plugin, reads } = createPlugin({ ".criv/state.json": validStateRaw });
  const loaded = await plugin.getState();
  assert.deepEqual(loaded, projectedState(validState));
  assert.equal(await plugin.getState(), loaded);
  assert.equal(reads(), 1);
}

{
  const { plugin } = createPlugin({ ".criv/state.json": validStateRaw });
  const summary = await plugin.readState();
  assert.deepEqual(summary, {
    schema: "criv.state.v1",
    node_count: 1,
    edge_count: 1,
    source_count: 2,
    asset_count: 0,
    pattern_count: 1,
    first_node_id: "note:README.md",
    first_edge: "note:README.md:references:code:src/lib.rs",
    first_source_path: "src/lib.rs",
    first_asset_path: undefined,
  });
}

{
  const current = new FakeRevision(validState);
  const changed = new FakeRevision({
    ...validState,
    "registered-patterns": ["ADR-0001/changed"],
  });
  const revisions = [current, changed];
  let loadCount = 0;
  const { plugin, setStat, reads } = createPlugin(
    {
      ".criv/state.json": validStateRaw,
      "state/other.json": JSON.stringify({ ...validState, marker: "changed" }),
    },
    async () => revisions[loadCount++],
  );

  setStat({ mtime: 1, size: validStateRaw.length });
  await plugin.loadState();
  await plugin.pollState();
  assert.equal(reads(), 1);

  await plugin.updateStatePath("state/other.json");
  assert.deepEqual(plugin.cachedState().registeredPatterns, ["ADR-0001/changed"]);
  assert.equal(current.disposals, 1);

  plugin.onunload();
  assert.equal(changed.disposals, 1);
}

{
  const files = { ".criv/state.json": validStateRaw };
  const current = new FakeRevision(validState);
  const changed = new FakeRevision({
    ...validState,
    "registered-patterns": ["ADR-0001/polled"],
  });
  const revisions = [current, changed];
  let loadCount = 0;
  const { plugin, setStat, reads } = createPlugin(files, async () => revisions[loadCount++]);

  setStat({ mtime: 1, size: validStateRaw.length });
  await plugin.loadState();
  files[".criv/state.json"] = JSON.stringify({ ...validState, marker: "polled" });
  setStat({ mtime: 2, size: files[".criv/state.json"].length });
  await plugin.pollState();

  assert.equal(reads(), 2);
  assert.deepEqual(plugin.cachedState().registeredPatterns, ["ADR-0001/polled"]);
  assert.equal(current.disposals, 1);
}

{
  const oldLoad = deferred();
  const newLoad = deferred();
  const oldRevision = new FakeRevision({ ...validState, marker: "old" });
  const newRevision = new FakeRevision({
    ...validState,
    "registered-patterns": ["ADR-0001/newest"],
  });
  let loadCount = 0;
  const { plugin } = createPlugin({ ".criv/state.json": validStateRaw }, async () => {
    loadCount += 1;
    return loadCount === 1 ? oldLoad.promise : newLoad.promise;
  });
  const statuses = [plugin.currentStateStatus()];
  const subscription = plugin.onStateStatusChange((status) => statuses.push(status));

  const oldResult = plugin.loadState();
  await waitFor(() => loadCount === 1);
  const newResult = plugin.loadState();
  await waitFor(() => loadCount === 2);
  newLoad.resolve(newRevision);
  await newResult;
  oldLoad.resolve(oldRevision);
  await oldResult;

  assert.equal(plugin.currentStateStatus().generation, 2);
  assert.equal(plugin.currentStateStatus().kind, "ready");
  assert.deepEqual(plugin.cachedState().registeredPatterns, ["ADR-0001/newest"]);
  assert.deepEqual(
    statuses.map((status) => [status.generation, status.kind]),
    [
      [0, "loading"],
      [1, "loading"],
      [2, "loading"],
      [2, "ready"],
    ],
  );
  assert.equal(oldRevision.disposals, 1);
  subscription.dispose();
}

async function testObsidianShutdownDuringStateLoad() {
  const load = deferred();
  const late = new FakeRevision(validState);
  let loads = 0;
  const { plugin } = createPlugin({ ".criv/state.json": validStateRaw }, async () => {
    loads += 1;
    return load.promise;
  });
  const statuses = [];
  plugin.onStateStatusChange((status) => statuses.push(status.kind));
  const pending = plugin.loadState();
  await waitFor(() => loads === 1);
  plugin.onunload();
  plugin.onunload();
  load.resolve(late);
  await pending;

  assert.deepEqual(statuses, ["loading"]);
  assert.equal(late.disposals, 1);
}

async function testObsidianNavigationUsesValidatedLookup() {
  const opened = [];
  globalThis.window = {
    open(url) {
      opened.push(url);
    },
  };
  const state = {
    ...validState,
    "source-index": [{ path: "src/validated.rs" }],
    lookup: {
      "src/validated.rs": {
        kind: "resolved",
        canonical_target: "src/validated.rs",
        node: { id: "code:src/validated.rs", kind: "code", label: "validated" },
      },
    },
  };
  const { plugin } = createPlugin({ ".criv/state.json": JSON.stringify(state) }, async () =>
    Promise.resolve(new FakeRevision(state)),
  );
  await plugin.loadState();

  plugin.openValidatedSource("source:src/validated.rs");
  plugin.openValidatedSource("source:src/rejected.rs");

  assert.deepEqual(opened, ["vscode://file/src/validated.rs"]);
}

function projectedState(state) {
  return {
    registeredPatterns: state["registered-patterns"] ?? [],
    patternMatches: state.patterns ?? {},
  };
}

{
  const current = new FakeRevision(validState);
  let loadCount = 0;
  const { plugin } = createPlugin({ ".criv/state.json": validStateRaw }, async () => {
    loadCount += 1;
    if (loadCount === 1) {
      return current;
    }
    throw new Error("unsupported criv state schema: criv.state.v2");
  });

  await plugin.loadState();
  await plugin.loadState();

  assert.equal(plugin.cachedState(), null);
  assert.equal(current.disposals, 1);
}

function createPlugin(files, loader) {
  let readCount = 0;
  let stat = { mtime: 0, size: 0 };
  const app = {
    vault: {
      adapter: {
        async read(path) {
          readCount += 1;
          if (Object.hasOwn(files, path)) {
            return files[path];
          }
          throw new Error(`missing ${path}`);
        },
        async stat() {
          return stat;
        },
      },
      cachedRead() {
        return "";
      },
    },
    workspace: {
      updateOptions() {},
      on() {
        return {};
      },
      onLayoutReady() {},
      getLeavesOfType() {
        return [];
      },
    },
    metadataCache: {
      on() {
        return {};
      },
    },
  };
  const plugin = new CrivPlugin(app, {}, loader);
  plugin.settings = {
    statePath: ".criv/state.json",
    externalEditorUrl: "vscode://file/{path}",
  };
  return { plugin, reads: () => readCount, setStat: (value) => (stat = value) };
}

await testObsidianShutdownDuringStateLoad();
await testObsidianNavigationUsesValidatedLookup();

function aliasPlugin() {
  const aliases = {
    "@codemirror/state": resolve(stubDir, "codemirror-state.mjs"),
    "@codemirror/view": resolve(stubDir, "codemirror-view.mjs"),
    obsidian: resolve(stubDir, "obsidian.mjs"),
  };
  return {
    name: "alias-stubs",
    setup(build) {
      build.onResolve({ filter: /^\.\.\/\.\.\/pkg\/criv_wasm\.js$/ }, () => ({
        path: wasmPath,
        external: true,
      }));
      for (const [moduleName, modulePath] of Object.entries(aliases)) {
        build.onResolve({ filter: new RegExp(`^${escapeRegExp(moduleName)}$`) }, () => ({
          path: modulePath,
        }));
      }
    },
  };
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function deferred() {
  let resolve;
  const promise = new Promise((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}

async function waitFor(predicate) {
  for (let attempt = 0; attempt < 20; attempt += 1) {
    if (predicate()) {
      return;
    }
    await Promise.resolve();
  }
  throw new Error("condition was not reached");
}
