import assert from "node:assert/strict";
import { mkdirSync, readdirSync, readFileSync } from "node:fs";
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

const c4FixtureDir = resolve(pluginRoot, "fixtures/c4");
for (const fixtureName of readdirSync(c4FixtureDir).filter((name) => name.endsWith(".c4"))) {
  const fixturePath = resolve(c4FixtureDir, fixtureName);
  const summary = core.parseC4Artifact(
    `docs/architecture/${fixtureName}`,
    readFileSync(fixturePath, "utf8"),
  );
  assert.deepEqual(summary.diagnostics, [], `diagnostics for ${fixtureName}`);
  assert.equal(summary.format, "mermaid", `format for ${fixtureName}`);
}
