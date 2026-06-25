import assert from "node:assert/strict";
import test from "node:test";

import { buildC4PreviewHtml } from "../../src/c4PreviewHtml";

test("builds webview HTML with strict CSP and local script resources", () => {
  const html = buildC4PreviewHtml({
    cspSource: "vscode-resource:",
    nonce: "abc123",
    mermaidUri: "vscode-resource:/mermaid.min.js",
    vizUri: "vscode-resource:/viz-global.js",
    payload: { format: "mermaid", source: "C4Context", sources: ["src/lib.rs"] },
  });

  assert.match(html, /default-src 'none'/);
  assert.match(html, /script-src 'nonce-abc123' 'wasm-unsafe-eval'/);
  assert.doesNotMatch(html, /'unsafe-eval'/);
  assert.match(html, /src="vscode-resource:\/mermaid\.min\.js"/);
  assert.match(html, /src="vscode-resource:\/viz-global\.js"/);
});

test("keeps source fallback and render-error surface in preview HTML", () => {
  const html = buildC4PreviewHtml({
    cspSource: "vscode-resource:",
    nonce: "abc123",
    mermaidUri: "vscode-resource:/mermaid.min.js",
    vizUri: "vscode-resource:/viz-global.js",
    payload: { format: "unknown", source: "<bad>", sources: [] },
  });

  assert.match(html, /Unknown \.c4 format/);
  assert.match(html, /fallback/);
  assert.match(html, /\\u003cbad\\u003e/);
});
