import assert from "node:assert/strict";
import { mkdirSync, readFileSync } from "node:fs";
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
