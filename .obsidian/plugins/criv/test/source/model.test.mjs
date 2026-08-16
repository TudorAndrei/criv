import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import * as esbuild from "esbuild";

const __dirname = dirname(fileURLToPath(import.meta.url));
const pluginRoot = resolve(__dirname, "../..");
const outFile = resolve(tmpdir(), `criv-core-test-${process.pid}.mjs`);

mkdirSync(dirname(outFile), { recursive: true });
await esbuild.build({
  entryPoints: [resolve(pluginRoot, "src/source/model.ts")],
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
const stateContractPath = resolve(pluginRoot, "../../../fixtures/state/criv.state.v1.json");
assert.equal(
  existsSync(stateContractPath),
  true,
  `missing state contract fixture ${stateContractPath}`,
);
const stateContractRaw = readFileSync(stateContractPath, "utf8");
const stateContract = JSON.parse(stateContractRaw);
const stateContractProjections = initialProjections(stateContractRaw);
assert.equal("state" in stateContractProjections, false);
assert.deepEqual(stateContractProjections.registeredPatterns, ["ADR-0001/entrypoint"]);
assert.deepEqual(stateContractProjections.patternMatches, stateContract.patterns);
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
  core.resolvePattern(projectedState(stateContract), "match:ADR-0001/entrypoint"),
  "ADR-0001/entrypoint",
);
assert.equal(
  core.resolvePattern(projectedState(stateContract), "match:ADR-0002/draft-entrypoint"),
  null,
);

const fixture = JSON.parse(
  readFileSync(resolve(pluginRoot, "fixtures/link-resolution.json"), "utf8"),
);
const state = fixture.state;
const projectedFixtureState = projectedState(state);
const linkRevision = new wasm.LoadedState(JSON.stringify(state));
const sources = linkRevision.initialProjections().sources;
const sourceByPath = new Map(sources.map((source) => [source.path, source]));
const sourceResolver = {
  lookupSourceTarget: (target) => linkRevision.lookupSourceTarget(target),
  sourceEntry: (path) => sourceByPath.get(path),
};

for (const testCase of fixture.cases) {
  const source = core.resolveSource(sourceResolver, testCase.target);
  const pattern = core.resolvePattern(projectedFixtureState, testCase.target);
  assert.equal(source?.entry.path ?? null, testCase.source, `source for ${testCase.target}`);
  assert.equal(pattern, testCase.pattern, `pattern for ${testCase.target}`);
}

assert.deepEqual(
  core
    .linkedSourcesFromMarkdown("[[src/lib.rs#run]] [[lib.rs]] [[missing.rs]]", sourceResolver)
    .map((source) => source.entry.path),
  ["src/lib.rs"],
);

const lookupFixture = JSON.parse(
  readFileSync(
    resolve(pluginRoot, "../../../fixtures/editor/source-target-lookup.v1.json"),
    "utf8",
  ),
);
const lookupRevision = new wasm.LoadedState(JSON.stringify(lookupFixture.state));
const lookupSources = lookupRevision.initialProjections().sources;
const lookupResolver = {
  lookupSourceTarget: (target) => lookupRevision.lookupSourceTarget(target),
  sourceEntry: (path) => lookupSources.find((source) => source.path === path),
};
const ambiguousSource = core.resolveSourceResult(lookupResolver, "source:src/lib.rs#run");
assert.equal(ambiguousSource.kind, "ambiguous");
assert.equal(ambiguousSource.totalCandidateCount, 2);
assert.deepEqual(core.resolveSourceResult(lookupResolver, "foo/lib.rs"), {
  kind: "unresolved",
});
lookupRevision.free();

assert.equal(
  core.frontmatterPatternTargets(
    {
      id: "ADR-0001",
      policy: { patterns: [{ id: "no-block-on" }] },
    },
    projectedFixtureState,
  )[0].matches[0].range,
  "L1:C1-L1:C10",
);

const rankedState = {
  ...state,
  "source-index": [
    { path: "src/slow.rs" },
    { path: "crates/criv-wasm/src/lib.rs" },
    { path: "src/lib.rs" },
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
    { path: "src/lib.rs" },
    { path: "../.ssh/id_rsa" },
    { path: "/etc/passwd" },
    { path: "C:\\Users\\name\\.ssh\\id_rsa" },
    { path: "\\\\server\\share\\secret.rs" },
    { path: "src\\windows\\path.rs" },
  ],
};

const unsafeRevision = new wasm.LoadedState(JSON.stringify(unsafeSourceState));
const safeSources = unsafeRevision.initialProjections().sources;
const safeSourceByPath = new Map(safeSources.map((source) => [source.path, source]));
const safeSourceResolver = {
  lookupSourceTarget: (target) => unsafeRevision.lookupSourceTarget(target),
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
assert.equal(core.decodeSourceLinkTarget("src%2Flib.rs%23fn%3Arun"), "src/lib.rs#fn:run");
assert.equal(core.decodeSourceLinkTarget("src%2Glib.rs"), null);

const validStateRaw = JSON.stringify(state);
assert.equal("state" in initialProjections(validStateRaw), false);
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
  projectedFixtureState,
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

function projectedState(state) {
  return {
    registeredPatterns: state["registered-patterns"] ?? [],
    patternMatches: state.patterns ?? {},
    architecture: state.architecture,
  };
}
