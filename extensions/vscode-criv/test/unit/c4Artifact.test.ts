import assert from "node:assert/strict";
import test from "node:test";

import { parseC4Artifact, sanitizeDotSvg } from "../../src/c4Artifact";

test("summarizes Mermaid C4 artifacts", () => {
  const summary = parseC4Artifact(
    "docs/architecture/01-system-context.c4",
    ["%% criv:format mermaid", "C4Context", "Person(user, User)"].join("\n"),
  );

  assert.equal(summary.format, "mermaid");
  assert.equal(summary.level, "context");
  assert.deepEqual(summary.diagnostics, []);
});

test("summarizes DOT code artifacts", () => {
  const summary = parseC4Artifact(
    "docs/architecture/04-code.c4",
    ["// criv:format dot", "digraph criv_code {", "  a -> b", "}"].join("\n"),
  );

  assert.equal(summary.format, "dot");
  assert.equal(summary.level, "code");
  assert.deepEqual(summary.diagnostics, []);
});

test("reports unknown format and level mismatch", () => {
  const unknown = parseC4Artifact("docs/architecture/custom.c4", "not a diagram");
  assert.equal(unknown.format, "unknown");
  assert.ok(unknown.diagnostics.some((diagnostic) => diagnostic.code === "unknown-c4-format"));

  const mismatch = parseC4Artifact("docs/architecture/02-container.c4", "C4Context");
  assert.ok(mismatch.diagnostics.some((diagnostic) => diagnostic.code === "mismatched-c4-level"));
});

test("sanitizes DOT SVG before DOM insertion", () => {
  const svg = sanitizeDotSvg(
    '<svg onload="evil()"><script>alert(1)</script><a href="https://example.com" target="_blank"><text>ok</text></a></svg>',
  );

  assert.equal(svg.includes("<script>"), false);
  assert.equal(svg.includes("onload"), false);
  assert.equal(svg.includes("href="), false);
  assert.equal(svg.includes("target="), false);
  assert.equal(svg.includes("<text>ok</text>"), true);
});
