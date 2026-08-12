import assert from "node:assert/strict";
import test from "node:test";

import { Window } from "happy-dom";

import type { CrivLikeC4Model } from "../src/protocol.ts";
import {
  CRIV_LIKEC4_RENDERER_DISPOSED,
  CRIV_LIKEC4_UNKNOWN_VIEW,
  CrivLikeC4Renderer,
} from "../src/renderer.ts";

const model: CrivLikeC4Model = {
  protocolVersion: 1,
  likec4Version: "1.59.2",
  workspace: "docs/architecture",
  model: {
    _stage: "layouted",
    projectId: "test",
    project: { id: "test", title: "Test" },
    specification: {
      customColors: {},
      deployments: {},
      elements: {},
      relationships: {},
      tags: [],
    },
    elements: {},
    relations: {},
    globals: { dynamicPredicates: {}, predicates: {}, styles: {} },
    views: {
      index: {
        _type: "element",
        _stage: "layouted",
        id: "index",
        title: "System context",
        hash: "index",
        autoLayout: { direction: "TB" },
        bounds: { x: 0, y: 0, width: 800, height: 600 },
        nodes: [],
        edges: [],
      },
      code: {
        _type: "element",
        _stage: "layouted",
        id: "code",
        title: "Code",
        hash: "code",
        autoLayout: { direction: "TB" },
        bounds: { x: 0, y: 0, width: 800, height: 600 },
        nodes: [],
        edges: [],
      },
    },
    deployments: { elements: {}, relations: {} },
    imports: {},
    manualLayouts: {},
  },
  views: [
    { id: "index", title: "System context", sourcePath: "systems.c4" },
    { id: "code", title: "Code" },
  ],
  sourceLinks: [{ element: "criv", target: "src/lib.rs" }],
};

test("requires an existing view and owns synchronous view selection", () => {
  const window = installDom();
  const selected: string[] = [];
  const renderer = new CrivLikeC4Renderer(
    window.document.createElement("div") as unknown as HTMLElement,
    {
    onSelectView: (viewId) => selected.push(viewId),
    },
  );

  assert.throws(() => renderer.replace(model, "missing"), hasCode(CRIV_LIKEC4_UNKNOWN_VIEW));
  renderer.replace(model, "index");
  assert.equal(renderer.currentViewId(), "index");
  assert.equal(renderer.selectView("index"), false);
  assert.equal(renderer.selectView("code"), true);
  assert.throws(() => renderer.selectView("missing"), hasCode(CRIV_LIKEC4_UNKNOWN_VIEW));
  assert.deepEqual(selected, ["index", "code"]);
});

test("exports the rendered shadow tree and reports not-ready", () => {
  const window = installDom();
  const container = window.document.createElement("div") as unknown as HTMLElement;
  const renderer = new CrivLikeC4Renderer(container);

  assert.equal(renderer.exportSvg(), null);
  const host = window.document.createElement("div");
  host.className = "likec4-view";
  (host as unknown as { getBoundingClientRect: () => { width: number; height: number } })
    .getBoundingClientRect = () => ({ width: 640.2, height: 479.1 });
  host.attachShadow({ mode: "open" }).innerHTML = "<p>Diagram</p>";
  container.appendChild(host as unknown as Node);

  const svg = renderer.exportSvg();
  assert.match(svg ?? "", /width="641" height="480"/);
  assert.match(svg ?? "", /<p>Diagram<\/p>/);
  renderer.dispose();
  assert.equal(container.childNodes.length, 0);
});

test("disposes once and rejects every later operation", () => {
  const window = installDom();
  const renderer = new CrivLikeC4Renderer(
    window.document.createElement("div") as unknown as HTMLElement,
  );

  renderer.dispose();
  renderer.dispose();

  for (const operation of [
    () => renderer.replace(model, "index"),
    () => renderer.selectView("index"),
    () => renderer.currentViewId(),
    () => renderer.views(),
    () => renderer.exportSvg(),
  ]) {
    assert.throws(operation, hasCode(CRIV_LIKEC4_RENDERER_DISPOSED));
  }
});

function installDom(): Window {
  const window = new Window();
  Object.assign(globalThis, {
    window,
    document: window.document,
    HTMLElement: window.HTMLElement,
    Node: window.Node,
    MutationObserver: window.MutationObserver,
  });
  return window;
}

function hasCode(code: string): (error: unknown) => boolean {
  return (error) =>
    error instanceof Error && "code" in error && (error as Error & { code: string }).code === code;
}
