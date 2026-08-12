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
    nodes: [{ id: "note:README.md", kind: "note", label: "README", path: "README.md" }],
    edges: [{ from: "note:README.md", kind: "mentions", to: "source:src/lib.rs" }],
  },
  patterns: {},
  "registered-patterns": ["ADR-0001/no-block-on"],
  "source-index": [
    { path: "src/lib.rs", frecency: 5 },
    { path: "src/main.rs", frecency: 2 },
  ],
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
        pattern_count: this.state["registered-patterns"]?.length ?? 0,
      },
      sources: this.state["source-index"] ?? [],
      nodes: [],
      registeredPatterns: this.state["registered-patterns"] ?? [],
      patternMatches: this.state.patterns ?? {},
      architecture: this.state.architecture,
      c4Artifacts: [],
    };
  }

  lookupSourceTarget() {
    return { kind: "unresolved" };
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
}

{
  const { plugin } = createPlugin({});
  assert.equal(await plugin.loadState(), null);
  assert.equal(plugin.cachedState(), null);
  assert.equal(plugin.stateStatus(), "Could not read .criv/state.json: missing .criv/state.json");
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
}

{
  const { plugin } = createPlugin({ ".criv/state.json": validStateRaw });
  plugin.settings.statePath = "../state.json";
  assert.equal(await plugin.loadState(), null);
  assert.equal(plugin.cachedState(), null);
  assert.equal(plugin.stateStatus(), "Invalid criv state path ../state.json.");
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
    pattern_count: 1,
    first_node_id: "note:README.md",
    first_edge: "note:README.md:mentions:source:src/lib.rs",
    first_source_path: "src/lib.rs",
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

function projectedState(state) {
  return {
    registeredPatterns: state["registered-patterns"] ?? [],
    patternMatches: state.patterns ?? {},
    architecture: state.architecture,
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

function aliasPlugin() {
  const aliases = {
    "@codemirror/state": resolve(stubDir, "codemirror-state.mjs"),
    "@codemirror/view": resolve(stubDir, "codemirror-view.mjs"),
    obsidian: resolve(stubDir, "obsidian.mjs"),
  };
  return {
    name: "alias-stubs",
    setup(build) {
      build.onResolve({ filter: /^\.\/pkg\/criv_wasm\.js$/ }, () => ({
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
