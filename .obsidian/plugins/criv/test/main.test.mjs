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

mkdirSync(dirname(outFile), { recursive: true });
await esbuild.build({
  entryPoints: [resolve(pluginRoot, "src/main.ts")],
  outfile: outFile,
  bundle: true,
  format: "esm",
  platform: "node",
  target: "node18",
  external: ["./pkg/criv_wasm.js"],
  plugins: [aliasPlugin()],
});

const { default: CrivPlugin } = await import(pathToFileURL(outFile).href);
const validState = {
  schema: "criv.state.v0",
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

{
  const { plugin } = createPlugin({ ".criv/state.json": validStateRaw });
  const loaded = await plugin.loadState();
  assert.deepEqual(loaded, validState);
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
    ".criv/state.json": JSON.stringify({ ...validState, schema: "criv.state.v1" }),
  });
  assert.equal(await plugin.loadState(), null);
  assert.equal(plugin.cachedState(), null);
  assert.equal(plugin.stateStatus(), "Unsupported criv state schema criv.state.v1");
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
  assert.deepEqual(loaded, validState);
  assert.equal(await plugin.getState(), loaded);
  assert.equal(reads(), 1);
}

{
  const { plugin } = createPlugin({ ".criv/state.json": validStateRaw });
  const summary = await plugin.readState();
  assert.deepEqual(summary, {
    schema: "criv.state.v0",
    node_count: 1,
    edge_count: 1,
    source_count: 2,
    pattern_count: 1,
    first_node_id: "note:README.md",
    first_edge: "note:README.md:mentions:source:src/lib.rs",
    first_source_path: "src/lib.rs",
  });
}

function createPlugin(files) {
  let readCount = 0;
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
  const plugin = new CrivPlugin(app, {});
  plugin.settings = {
    statePath: ".criv/state.json",
    externalEditorUrl: "vscode://file/{path}",
  };
  return { plugin, reads: () => readCount };
}

function aliasPlugin() {
  const aliases = {
    "@codemirror/state": resolve(stubDir, "codemirror-state.mjs"),
    "@codemirror/view": resolve(stubDir, "codemirror-view.mjs"),
    "@viz-js/viz": resolve(stubDir, "viz-js-viz.mjs"),
    mermaid: resolve(stubDir, "mermaid.mjs"),
    obsidian: resolve(stubDir, "obsidian.mjs"),
  };
  return {
    name: "alias-stubs",
    setup(build) {
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
