import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync } from "node:fs";
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
const wasmPath = resolve(pluginRoot, "pkg/criv_wasm.js");
assert.equal(existsSync(wasmPath), true, `missing compiled Wasm runtime ${wasmPath}`);
const wasm = await import(pathToFileURL(wasmPath).href);

function wasmError(action) {
  try {
    action();
  } catch (error) {
    return String(error);
  }
  assert.fail("expected the canonical Wasm export to reject input");
}
const stateContractPath = resolve(__dirname, "../../../../fixtures/state/criv.state.v1.json");
assert.equal(
  existsSync(stateContractPath),
  true,
  `missing state contract fixture ${stateContractPath}`,
);
const stateContractRaw = readFileSync(stateContractPath, "utf8");
const stateContract = JSON.parse(stateContractRaw);
assert.deepEqual(wasm.validated_state(stateContractRaw), stateContract);
assert.match(
  wasmError(() => wasm.validated_state(stateContractRaw.replace("criv.state.v1", "criv.state.v2"))),
  /unsupported criv state schema/i,
);
assert.equal(stateContract.graph.nodes.length, 6);
assert.equal(stateContract.graph.edges.length, 5);
assert.deepEqual(stateContract["registered-patterns"], ["ADR-0001/entrypoint"]);
assert.deepEqual(stateContract.patterns, {
  "ADR-0001/entrypoint": [
    {
      file: "src/lib.rs",
      range: "L1:C1-L1:C12",
      captures: { BODY: "", NAME: "run" },
    },
  ],
});
assert.equal(stateContract.patterns["ADR-0002/draft-entrypoint"], undefined);
assert.equal(
  core.resolvePattern(stateContract, "match:ADR-0001/entrypoint"),
  "ADR-0001/entrypoint",
);
assert.equal(core.resolvePattern(stateContract, "match:ADR-0002/draft-entrypoint"), null);

const fixture = JSON.parse(
  readFileSync(resolve(pluginRoot, "fixtures/link-resolution.json"), "utf8"),
);
const state = fixture.state;
const sources = wasm.source_entries(JSON.stringify(state));

for (const testCase of fixture.cases) {
  const source = core.resolveSource(sources, testCase.target);
  const pattern = core.resolvePattern(state, testCase.target);
  assert.equal(source?.entry.path ?? null, testCase.source, `source for ${testCase.target}`);
  assert.equal(pattern, testCase.pattern, `pattern for ${testCase.target}`);
}

assert.deepEqual(
  core
    .linkedSourcesFromMarkdown("[[src/lib.rs#run]] [[lib.rs]] [[missing.rs]]", sources)
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

assert.equal(
  wasm.suggest_source_selectors(JSON.stringify(rankedState), "src/lib.rs", 2)[0].path,
  "src/lib.rs",
);
assert.equal(
  wasm.suggest_source_selectors(JSON.stringify(rankedState), "lib.rs", 2)[0].path,
  "crates/criv-wasm/src/lib.rs",
);
assert.equal(
  wasm.suggest_source_selectors(JSON.stringify(rankedState), "", 1)[0].path,
  "crates/criv-wasm/src/lib.rs",
);

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

const safeSources = wasm.source_entries(JSON.stringify(unsafeSourceState));
assert.deepEqual(
  safeSources.map((entry) => entry.path),
  ["src/lib.rs", "src/windows/path.rs"],
);
assert.equal(core.resolveSource(safeSources, "../.ssh/id_rsa"), null);
assert.equal(core.safeVaultPath("../.ssh/id_rsa"), null);
assert.equal(core.safeVaultPath("/etc/passwd"), null);
assert.equal(core.safeVaultPath("C:\\Users\\name\\.ssh\\id_rsa"), null);
assert.equal(core.safeVaultPath("src\\lib.rs"), "src/lib.rs");

const validStateRaw = JSON.stringify(state);
assert.deepEqual(wasm.validated_state(validStateRaw), state);
assert.match(
  wasmError(() => wasm.validated_state(validStateRaw.replace("criv.state.v1", "criv.state.v2"))),
  /unsupported criv state schema/i,
);
assert.match(
  wasmError(() => wasm.validated_state("{")),
  /invalid criv state JSON/i,
);

assert.deepEqual(core.parseLineRange("L4"), { start: 4, end: 4 });
assert.deepEqual(core.parseLineRange("L4-L8"), { start: 4, end: 8 });
assert.deepEqual(core.parseLineRange("l4-8"), { start: 4, end: 8 });
assert.deepEqual(core.parseLineRange("L8-L4"), { start: 8, end: 8 });
assert.equal(core.parseLineRange("4-8"), null);
assert.equal(core.parseLineRange(null), null);

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
  sources,
);
assert.deepEqual(
  ranges.map((range) => `${range.status}:${range.kind}:${range.target}`),
  [
    "resolved:source:src/lib.rs",
    "unresolved:unknown:missing.rs",
    "resolved:pattern:match:ADR-0001/no-block-on",
  ],
);

const likec4Summary = core.parseC4Artifact(
  "docs/architecture/model.c4",
  "specification { element system }\nmodel { app = system 'App' }\n",
);
assert.equal(likec4Summary.format, "likec4");
assert.deepEqual(likec4Summary.diagnostics, []);

for (const legacySource of ["C4Context", "digraph architecture { a -> b }"]) {
  const summary = core.parseC4Artifact("docs/architecture/model.c4", legacySource);
  assert.equal(summary.format, "unknown");
  assert.equal(summary.diagnostics[0].code, "unknown-c4-format");
}
