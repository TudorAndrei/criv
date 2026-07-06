import assert from "node:assert/strict";
import { existsSync, mkdirSync, readdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import * as esbuild from "esbuild";

const __dirname = dirname(fileURLToPath(import.meta.url));
const pluginRoot = resolve(__dirname, "..");
const outFile = resolve(tmpdir(), `criv-core-test-${process.pid}.mjs`);

mkdirSync(dirname(outFile), { recursive: true });
await esbuild.build({
  entryPoints: [resolve(pluginRoot, "src/core.ts")],
  outfile: outFile,
  bundle: true,
  format: "esm",
  platform: "node",
  target: "node18",
});

const core = await import(pathToFileURL(outFile).href);
const { instance: vizInstance } = await import("@viz-js/viz");
const c4FixtureDir = resolve(__dirname, "../../../../fixtures/c4");
assert.equal(existsSync(c4FixtureDir), true, `missing fixture directory ${c4FixtureDir}`);
const c4Expected = JSON.parse(readFileSync(resolve(c4FixtureDir, "expected.json"), "utf8"));
const c4FixtureNames = readdirSync(c4FixtureDir).filter((name) => name.endsWith(".c4"));
assert.ok(c4FixtureNames.length > 0, "expected shared C4 fixtures");
for (const fixtureName of c4FixtureNames) {
  const summary = core.parseC4Artifact(
    fixtureName,
    readFileSync(resolve(c4FixtureDir, fixtureName), "utf8"),
  );
  const actual = summary.diagnostics
    .map(({ code, line }) => ({ code, line }))
    .sort(compareDiagnostics);
  assert.deepEqual(actual, c4Expected[fixtureName] ?? [], `diagnostics for ${fixtureName}`);
}

const fixture = JSON.parse(
  readFileSync(resolve(pluginRoot, "fixtures/link-resolution.json"), "utf8"),
);
const state = fixture.state;

for (const testCase of fixture.cases) {
  const source = core.resolveSource(state, testCase.target);
  const pattern = core.resolvePattern(state, testCase.target);
  assert.equal(source?.entry.path ?? null, testCase.source, `source for ${testCase.target}`);
  assert.equal(pattern, testCase.pattern, `pattern for ${testCase.target}`);
}

assert.deepEqual(
  core
    .linkedSourcesFromMarkdown("[[src/lib.rs#run]] [[lib.rs]] [[missing.rs]]", state)
    .map((source) => source.entry.path),
  ["src/lib.rs"],
);

assert.equal(
  core.frontmatterPatternTargets(
    {
      id: "ADR-0001",
      policy: { patterns: [{ id: "no-block-on" }] },
    },
    state,
  )[0].matches[0].range,
  "L1:C1-L1:C10",
);

const rankedState = {
  ...state,
  "source-index": [
    { path: "src/slow.rs", frecency: 0 },
    { path: "crates/criv-wasm/src/lib.rs", frecency: 40 },
    { path: "src/lib.rs", frecency: 5 },
  ],
};

assert.equal(core.sourceSuggestions(rankedState, "src/lib.rs", 2)[0].path, "src/lib.rs");
assert.equal(
  core.sourceSuggestions(rankedState, "lib.rs", 2)[0].path,
  "crates/criv-wasm/src/lib.rs",
);
assert.equal(core.sourceSuggestions(rankedState, "", 1)[0].path, "crates/criv-wasm/src/lib.rs");

const wasmPath = resolve(__dirname, "../../../../extensions/vscode-criv/pkg/criv_wasm.js");
if (existsSync(wasmPath)) {
  const require = createRequire(import.meta.url);
  const wasm = require(wasmPath);
  const parityState = {
    schema: "criv.state.v0",
    graph: {
      nodes: [
        {
          id: "symbol:src/lib.rs#fn:run",
          kind: "function",
          label: "run",
          path: "src/lib.rs#L10-L20",
        },
        {
          id: "symbol:src/main.rs#fn:start",
          kind: "function",
          label: "start",
          path: "src/main.rs#L5-L9",
        },
      ],
      edges: [],
    },
    "registered-patterns": [],
    "source-index": [
      { path: "src/tie-low.rs", frecency: 1 },
      { path: "src/tie-high.rs", frecency: 50 },
      { path: "crates/criv-wasm/src/lib.rs", frecency: 40 },
      { path: "src/lib.rs", frecency: 5 },
      { path: "lib.rs", frecency: 0 },
      { path: "src/slow_lib.rs", frecency: 0 },
      { path: "src/main.rs", frecency: 2 },
      { path: "src/xray.ts", frecency: 7 },
      { path: "docs/adr.md", frecency: 0 },
    ],
  };
  const rawParityState = JSON.stringify(parityState);
  for (const query of ["", "lib", "main.rs", "src/x", "slr"]) {
    const wasmFilePaths = wasm
      .suggest_source_selectors(rawParityState, query, 20)
      .filter((suggestion) => suggestion.kind === "file")
      .map((suggestion) => suggestion.path);
    const fallbackPaths = core
      .sourceSuggestions(parityState, query, 20)
      .map((suggestion) => suggestion.path);
    assert.deepEqual(wasmFilePaths, fallbackPaths, `wasm/TS source suggestion parity: ${query}`);
  }
} else {
  console.warn(`Skipping wasm/TS source suggestion parity; missing ${wasmPath}`);
}

const unsafeSourceState = {
  ...state,
  "source-index": [
    { path: "src/lib.rs", frecency: 1 },
    { path: "../.ssh/id_rsa", frecency: 100 },
    { path: "/etc/passwd", frecency: 100 },
    { path: "C:\\Users\\name\\.ssh\\id_rsa", frecency: 100 },
    { path: "\\\\server\\share\\secret.rs", frecency: 100 },
    { path: "src\\windows\\path.rs", frecency: 2 },
  ],
};

assert.deepEqual(
  core.sourceEntries(unsafeSourceState).map((entry) => entry.path),
  ["src/lib.rs", "src/windows/path.rs"],
);
assert.equal(core.resolveSource(unsafeSourceState, "../.ssh/id_rsa"), null);
assert.equal(core.safeVaultPath("../.ssh/id_rsa"), null);
assert.equal(core.safeVaultPath("/etc/passwd"), null);
assert.equal(core.safeVaultPath("C:\\Users\\name\\.ssh\\id_rsa"), null);
assert.equal(core.safeVaultPath("src\\lib.rs"), "src/lib.rs");

const validStateRaw = JSON.stringify(state);
assert.deepEqual(core.interpretState(validStateRaw, "criv.state.v0"), { state });
assert.deepEqual(core.interpretState(validStateRaw, "criv.state.v1"), {
  error: "Unsupported criv state schema criv.state.v0",
  kind: "schema",
});
assert.deepEqual(core.interpretState(JSON.stringify({ graph: {} }), "criv.state.v0"), {
  error: "Unsupported criv state schema unknown",
  kind: "schema",
});
const badJson = core.interpretState("{", "criv.state.v0");
assert.equal(badJson.kind, "parse");
assert.match(badJson.error, /JSON|Unexpected|property name/i);

assert.deepEqual(core.parseLineRange("L4"), { start: 4, end: 4 });
assert.deepEqual(core.parseLineRange("L4-L8"), { start: 4, end: 8 });
assert.deepEqual(core.parseLineRange("l4-8"), { start: 4, end: 8 });
assert.deepEqual(core.parseLineRange("L8-L4"), { start: 8, end: 8 });
assert.equal(core.parseLineRange("4-8"), null);
assert.equal(core.parseLineRange(null), null);

assert.equal(
  core.renderErrorsMessage([{ message: "first" }, { level: "warning", message: "second" }]),
  "first; second",
);
assert.equal(core.renderErrorsMessage([]), "Graphviz render failed");

const targets = [];
core.addTarget(targets, " src/lib.rs ");
core.addTarget(targets, " ");
core.addTextTargets(targets, "Open [[docs/adr/0001.md|ADR 1]] and [[src/lib.rs#run]]");
core.addTextTargets(targets, "[[README.md]]");
assert.deepEqual(targets, [
  "src/lib.rs",
  "Open [[docs/adr/0001.md|ADR 1]] and [[src/lib.rs#run]]",
  "docs/adr/0001.md|ADR 1",
  "src/lib.rs#run",
  "Open [[docs/adr/0001.md|ADR 1]] and [[src/lib.rs#run",
  "[[README.md]]",
  "README.md",
  "README.md",
]);

const ranges = core.crivLinkRanges(
  "[[src/lib.rs]] [[missing.rs]] [[match:ADR-0001/no-block-on]]",
  state,
);
assert.deepEqual(
  ranges.map((range) => `${range.status}:${range.kind}:${range.target}`),
  [
    "resolved:source:src/lib.rs",
    "unresolved:unknown:missing.rs",
    "resolved:pattern:match:ADR-0001/no-block-on",
  ],
);

const sanitizedSyntheticSvg = core.sanitizeDotSvg(
  `<?xml version="1.0"?>
<!DOCTYPE svg>
<svg onload="alert(1)">
  <script>alert(1)</script>
  <foreignObject><div onclick="alert(1)">unsafe</div></foreignObject>
  <a xlink:href="javascript:alert(1)" href="https://example.com" target="_blank">
    <text onclick='alert(1)'>safe label</text>
  </a>
</svg>`,
);
assert.equal(sanitizedSyntheticSvg.includes("safe label"), true);
assert.equal(/<\s*script\b/i.test(sanitizedSyntheticSvg), false);
assert.equal(/<\s*foreignObject\b/i.test(sanitizedSyntheticSvg), false);
assert.equal(/\s+on[a-z0-9_-]+\s*=/i.test(sanitizedSyntheticSvg), false);
assert.equal(/\s+(?:href|xlink:href|target)\s*=/i.test(sanitizedSyntheticSvg), false);
assert.equal(/<!DOCTYPE/i.test(sanitizedSyntheticSvg), false);

const viz = await vizInstance();
const vizResult = viz.render(
  `digraph {
  a [label="onload=alert(1)", URL="javascript:alert(1)", tooltip="tooltip text"];
}`,
  { engine: "dot", format: "svg" },
);
assert.equal(vizResult.status, "success");
assert.equal(vizResult.output.includes("javascript:alert(1)"), true);
const sanitizedVizSvg = core.sanitizeDotSvg(vizResult.output);
assert.equal(sanitizedVizSvg.includes("javascript:alert(1)"), false);
assert.equal(/\s+(?:href|xlink:href|target)\s*=/i.test(sanitizedVizSvg), false);
assert.equal(sanitizedVizSvg.includes("tooltip text"), true);
assert.equal(sanitizedVizSvg.includes("onload=alert(1)"), true);

assert.deepEqual(
  core.parseC4Artifact(
    "docs/architecture/02-container.c4",
    `C4Container
Container(cli, "criv CLI", "Rust", "Runs checks")
%% criv:source src/lib.rs#run
`,
  ).diagnostics,
  [],
);

assert.equal(
  core.parseC4Artifact(
    "docs/architecture/04-code.c4",
    `// criv:generated true
digraph criv_code {
  cli -> vault;
}
`,
  ).format,
  "dot",
);

const c4Mismatch = core.parseC4Artifact(
  "docs/architecture/01-context.c4",
  `%% criv:format dot
C4Container
Container(cli, "criv CLI", "Rust", "Runs checks")
`,
);
assert.deepEqual(
  c4Mismatch.diagnostics.map((diagnostic) => diagnostic.code),
  ["mismatched-c4-format", "mismatched-c4-level"],
);

const c4BadDirective = core.parseC4Artifact(
  "docs/architecture/diagram.c4",
  `%% criv:level code
flowchart TD
  a --> b
`,
);
assert.deepEqual(
  c4BadDirective.diagnostics.map((diagnostic) => diagnostic.code),
  ["missing-c4-level", "unknown-c4-directive", "unknown-c4-format"],
);

const pluginC4FixtureDir = resolve(pluginRoot, "fixtures/c4");
for (const fixtureName of readdirSync(pluginC4FixtureDir).filter((name) => name.endsWith(".c4"))) {
  const fixturePath = resolve(pluginC4FixtureDir, fixtureName);
  const summary = core.parseC4Artifact(
    `docs/architecture/${fixtureName}`,
    readFileSync(fixturePath, "utf8"),
  );
  assert.deepEqual(summary.diagnostics, [], `diagnostics for ${fixtureName}`);
  assert.equal(summary.format, "mermaid", `format for ${fixtureName}`);
}

function compareDiagnostics(left, right) {
  return (
    left.code.localeCompare(right.code) ||
    (left.line ?? Number.MAX_SAFE_INTEGER) - (right.line ?? Number.MAX_SAFE_INTEGER)
  );
}
