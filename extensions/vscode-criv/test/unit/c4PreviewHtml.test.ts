import assert from "node:assert/strict";
import test from "node:test";

import { buildC4PreviewHtml, buildC4PreviewStatusHtml } from "../../src/c4PreviewHtml";

test("builds a local LikeC4 webview with a strict default CSP", () => {
  const html = buildC4PreviewHtml({
    cspSource: "vscode-resource:",
    nonce: "abc123",
    rendererUri: "vscode-resource:/likec4-preview.js",
    payload: {
      colorScheme: "dark",
      model: {
        protocolVersion: 1,
        likec4Version: "1.59.2",
        revision: 1,
        workspace: "docs/architecture",
        model: {},
        views: [],
        sourceLinks: [],
      },
    },
  });

  assert.match(html, /default-src 'none'/);
  assert.match(html, /script-src 'nonce-abc123' 'wasm-unsafe-eval'/);
  assert.doesNotMatch(html, /https?:/);
  assert.match(html, /src="vscode-resource:\/likec4-preview\.js"/);
  assert.match(html, /LikeC4 architecture view/);
});

test("escapes unavailable-state text in the preview", () => {
  const html = buildC4PreviewStatusHtml("vscode-resource:", "Missing <state> & model");

  assert.match(html, /Missing &lt;state&gt; &amp; model/);
  assert.doesNotMatch(html, /Missing <state>/);
  assert.match(html, /default-src 'none'/);
});
