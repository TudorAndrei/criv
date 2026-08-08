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

function initialProjections(raw) {
  const revision = new wasm.LoadedState(raw);
  try {
    return revision.initialProjections();
  } finally {
    revision.free();
  }
}
const stateContractPath = resolve(__dirname, "../../../../fixtures/state/criv.state.v1.json");
assert.equal(
  existsSync(stateContractPath),
  true,
  `missing state contract fixture ${stateContractPath}`,
);
const stateContractRaw = readFileSync(stateContractPath, "utf8");
const stateContract = JSON.parse(stateContractRaw);
assert.deepEqual(initialProjections(stateContractRaw).state, stateContract);
assert.match(
  wasmError(() => new wasm.LoadedState(stateContractRaw.replace("criv.state.v1", "criv.state.v2"))),
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
const linkRevision = new wasm.LoadedState(JSON.stringify(state));
const sources = linkRevision.initialProjections().sources;
const sourceByPath = new Map(sources.map((source) => [source.path, source]));
const sourceResolver = {
  lookupNode: (target) => linkRevision.lookupNode(target),
  sourceEntry: (path) => sourceByPath.get(path),
};

for (const testCase of fixture.cases) {
  const source = core.resolveSource(sourceResolver, testCase.target);
  const pattern = core.resolvePattern(state, testCase.target);
  assert.equal(source?.entry.path ?? null, testCase.source, `source for ${testCase.target}`);
  assert.equal(pattern, testCase.pattern, `pattern for ${testCase.target}`);
}

assert.deepEqual(
  core
    .linkedSourcesFromMarkdown("[[src/lib.rs#run]] [[lib.rs]] [[missing.rs]]", sourceResolver)
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

const rankedRevision = new wasm.LoadedState(JSON.stringify(rankedState));
assert.equal(rankedRevision.suggestSelectors("src/lib.rs", 2)[0].path, "src/lib.rs");
assert.equal(rankedRevision.suggestSelectors("lib.rs", 2)[0].path, "crates/criv-wasm/src/lib.rs");
assert.equal(rankedRevision.suggestSelectors("", 1)[0].path, "crates/criv-wasm/src/lib.rs");
rankedRevision.free();

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

const unsafeRevision = new wasm.LoadedState(JSON.stringify(unsafeSourceState));
const safeSources = unsafeRevision.initialProjections().sources;
const safeSourceByPath = new Map(safeSources.map((source) => [source.path, source]));
const safeSourceResolver = {
  lookupNode: (target) => unsafeRevision.lookupNode(target),
  sourceEntry: (path) => safeSourceByPath.get(path),
};
assert.deepEqual(
  safeSources.map((entry) => entry.path),
  ["src/lib.rs", "src/windows/path.rs"],
);
assert.equal(core.resolveSource(safeSourceResolver, "../.ssh/id_rsa"), null);
assert.equal(core.safeVaultPath("../.ssh/id_rsa"), null);
assert.equal(core.safeVaultPath("/etc/passwd"), null);
assert.equal(core.safeVaultPath("C:\\Users\\name\\.ssh\\id_rsa"), null);
assert.equal(core.safeVaultPath("src\\lib.rs"), "src/lib.rs");

const validStateRaw = JSON.stringify(state);
assert.deepEqual(initialProjections(validStateRaw).state, state);
assert.match(
  wasmError(() => new wasm.LoadedState(validStateRaw.replace("criv.state.v1", "criv.state.v2"))),
  /unsupported criv state schema/i,
);
assert.match(
  wasmError(() => new wasm.LoadedState("{")),
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
  sourceResolver,
);
assert.deepEqual(
  ranges.map((range) => `${range.status}:${range.kind}:${range.target}`),
  [
    "resolved:source:src/lib.rs",
    "unresolved:unknown:missing.rs",
    "resolved:pattern:match:ADR-0001/no-block-on",
  ],
);
unsafeRevision.free();
linkRevision.free();
