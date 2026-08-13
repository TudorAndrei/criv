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

const { default: CrivPlugin, CrivC4View } = await import(pathToFileURL(outFile).href);
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

async function testManyObsidianC4Leaves() {
  installFakeDocument();
  const plugin = new FakePreviewPlugin();
  const renderers = [];
  const views = ["one", "two", "three"].map((name) =>
    createC4View(name, plugin, (surface, options) => {
      const renderer = new FakeRenderer(surface, options);
      renderers.push(renderer);
      return renderer;
    }),
  );
  await Promise.all(views.map((view) => view.onOpen()));
  assert.equal(plugin.listeners.size, 3);

  await publishToViews(views, readyStatus(1, "model-a", "owned-a", "shared"));
  assert.equal(renderers.length, 3, views.map((view) => view.containerEl.text()).join(" | "));
  for (const renderer of renderers) {
    renderer.selectView("shared");
  }

  await publishToViews(views, readyStatus(2, "model-b", "owned-b", "shared"));
  assert.equal(renderers.length, 6);
  assert.deepEqual(
    renderers.slice(3).map((renderer) => renderer.viewId),
    ["shared", "shared", "shared"],
  );
  assert.deepEqual(
    renderers.slice(0, 3).map((renderer) => renderer.disposals),
    [1, 1, 1],
  );

  await publishToViews(views, { generation: 3, kind: "missing", message: "State missing" });
  assert.deepEqual(
    renderers.slice(3, 6).map((renderer) => renderer.disposals),
    [1, 1, 1],
  );
  await publishToViews(views, readyStatus(4, "model-c", "owned-c"));
  assert.deepEqual(
    renderers.slice(6, 9).map((renderer) => renderer.viewId),
    ["owned-c-one", "owned-c-two", "owned-c-three"],
  );

  await publishToViews(views, { generation: 5, kind: "invalid", message: "State invalid" });
  await publishToViews(views, readyStatus(6, "model-d", "owned-d"));
  await publishToViews(views, {
    generation: 7,
    kind: "unavailable",
    message: "Wasm unavailable",
  });
  await publishToViews(views, readyStatus(8, "model-e", "owned-e"));
  const navigationRenderer = renderers.at(-1);
  navigationRenderer.options.onOpenSource("source:src/validated.rs");
  assert.deepEqual(plugin.openedTargets, ["source:src/validated.rs"]);

  await publishToViews(views, readyStatus(9, "model-no-view"));
  assert.equal(renderers.length, 15);
  assert.ok(views.every((view) => view.containerEl.text().includes("declares no named view")));
  assert.ok(renderers.every((renderer) => renderer.disposals === 1));

  await Promise.all(views.map((view) => view.onClose()));
  await Promise.all(views.map((view) => view.onClose()));
  assert.equal(plugin.listeners.size, 0);
  assert.ok(renderers.every((renderer) => renderer.disposals === 1));
}

async function testLateObsidianC4Renders() {
  installFakeDocument();
  const source = deferred();
  const plugin = new FakePreviewPlugin(source.promise);
  const renderers = [];
  const view = createC4View("late", plugin, (surface, options) => {
    const renderer = new FakeRenderer(surface, options);
    renderers.push(renderer);
    return renderer;
  });
  const pending = view.acceptStateStatus(readyStatus(1, "late-model", "late-view"));
  await view.acceptStateStatus({ generation: 2, kind: "invalid", message: "newer invalid" });
  source.resolve("model source");
  await pending;
  assert.equal(renderers.length, 0);

  const closeSource = deferred();
  plugin.source = closeSource.promise;
  const pendingClose = view.acceptStateStatus(readyStatus(3, "closed-model", "closed-view"));
  await view.onClose();
  closeSource.resolve("closed source");
  await pendingClose;
  assert.equal(renderers.length, 0);
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

async function testObsidianC4NavigationUsesValidatedLookup() {
  const opened = [];
  globalThis.window = {
    open(url) {
      opened.push(url);
    },
  };
  const state = {
    ...validState,
    "source-index": [{ path: "src/validated.rs", frecency: 1 }],
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

class FakePreviewPlugin {
  listeners = new Set();
  openedTargets = [];
  status = { generation: 0, kind: "loading" };

  constructor(source = Promise.resolve("model source")) {
    this.source = source;
  }

  currentStateStatus() {
    return this.status;
  }

  onStateStatusChange(listener) {
    this.listeners.add(listener);
    let disposed = false;
    return {
      dispose: () => {
        if (!disposed) {
          disposed = true;
          this.listeners.delete(listener);
        }
      },
    };
  }

  openValidatedSource(target) {
    this.openedTargets.push(target);
  }
}

class FakeRenderer {
  disposals = 0;
  model = null;
  viewId = null;

  constructor(surface, options) {
    this.surface = surface;
    this.options = options;
  }

  replace(model, viewId) {
    this.model = model;
    this.viewId = viewId;
  }

  currentViewId() {
    return this.viewId;
  }

  views() {
    return this.model?.views ?? [];
  }

  selectView(viewId) {
    this.viewId = viewId;
  }

  exportSvg() {
    return "<svg/>";
  }

  dispose() {
    this.disposals += 1;
  }
}

class FakeElement {
  children = [];
  parent = null;
  textValue = "";
  value = "";
  classList = { contains: () => false };

  constructor(tag = "div") {
    this.tag = tag;
  }

  get firstChild() {
    return this.children[0] ?? null;
  }

  appendChild(child) {
    if (child.parent) {
      const index = child.parent.children.indexOf(child);
      if (index >= 0) {
        child.parent.children.splice(index, 1);
      }
    }
    child.parent = this;
    this.children.push(child);
    return child;
  }

  createDiv(options = {}) {
    return this.createEl("div", options);
  }

  createSpan(options = {}) {
    return this.createEl("span", options);
  }

  createEl(tag, options = {}) {
    const child = new FakeElement(tag);
    child.textValue = options.text ?? "";
    child.value = options.value ?? "";
    this.appendChild(child);
    return child;
  }

  empty() {
    for (const child of this.children) {
      child.parent = null;
    }
    this.children = [];
    this.textValue = "";
  }

  addClass() {}
  toggleClass() {}
  setAttribute() {}

  text() {
    return `${this.textValue}${this.children.map((child) => child.text()).join("")}`;
  }
}

function installFakeDocument() {
  globalThis.document = {
    body: new FakeElement("body"),
    createElement(tag) {
      return new FakeElement(tag);
    },
  };
}

function createC4View(name, plugin, rendererFactory) {
  const app = {
    vault: {
      cachedRead() {
        return plugin.source;
      },
      modify() {},
    },
  };
  const view = new CrivC4View({ app }, plugin, rendererFactory);
  view.containerEl = new FakeElement("container");
  view.containerEl.appendChild(new FakeElement("header"));
  view.containerEl.appendChild(new FakeElement("content"));
  view.file = {
    path: `docs/architecture/${name}.c4`,
    basename: name,
  };
  return view;
}

async function publishToViews(views, status) {
  await Promise.all(views.map((view) => view.acceptStateStatus(status)));
}

function readyStatus(generation, modelName, ownedView, retainedView) {
  const views = ownedView
    ? ["one", "two", "three"].map((name) => ({
        id: `${ownedView}-${name}`,
        title: `${modelName} / ${name}`,
        sourcePath: `docs/architecture/${name}.c4`,
      }))
    : [];
  if (retainedView) {
    views.push({
      id: retainedView,
      title: `${modelName} / shared`,
      sourcePath: "docs/architecture/shared.c4",
    });
  }
  return {
    generation,
    kind: "ready",
    state: {
      registeredPatterns: [],
      patternMatches: {},
      architecture: {
        workspace: "docs/architecture",
        views,
        marker: modelName,
      },
    },
  };
}

await testManyObsidianC4Leaves();
await testLateObsidianC4Renders();
await testObsidianShutdownDuringStateLoad();
await testObsidianC4NavigationUsesValidatedLookup();

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
