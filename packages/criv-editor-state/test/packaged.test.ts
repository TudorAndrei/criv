import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { build } from "esbuild";

import { crivWasmBytesPlugin } from "../esbuild.mjs";
import {
  CRIV_STATE_JSON_INVALID,
  CRIV_STATE_SCHEMA_UNSUPPORTED,
  type CrivStateContractError,
  type CrivSourceTargetLookupResult,
} from "../src/contract.ts";
import type { CrivStateModule } from "../src/stateModule.ts";

const repositoryRoot = new URL("../../../", import.meta.url);
const stateRaw = await readFile(
  new URL("fixtures/state/criv.state.v1.json", repositoryRoot),
  "utf8",
);
const lookupFixture = JSON.parse(
  await readFile(new URL("fixtures/editor/source-target-lookup.v1.json", repositoryRoot), "utf8"),
) as {
  state: unknown;
  cases: {
    target: string;
    kind: string;
    canonical_target?: string;
    total_candidate_count?: number;
  }[];
};

const stateModule = await bundledStateModule();

test("loads State through the Wasm runtime embedded in a bundle outside the package", async () => {
  const revision = await stateModule.loadState(stateRaw);
  const projections = revision.initialProjections();

  assert.equal(projections.summary.node_count, 6);
  assert.equal("state" in projections, false);
  assert.deepEqual(projections.registeredPatterns, ["ADR-0001/entrypoint"]);
  assert.deepEqual(
    projections.sources.map((source) => source.path),
    ["src/lib.rs"],
  );
  assert.equal(revision.suggestSelectors("run", 10)[0]?.target, "src/lib.rs#fn:run");
  revision.dispose();
});

test("reports each rejected State with its contract code", async () => {
  const valid = JSON.parse(stateRaw) as Record<string, unknown>;
  const cases: [string, string][] = [
    [CRIV_STATE_JSON_INVALID, "{"],
    [CRIV_STATE_JSON_INVALID, JSON.stringify({ ...valid, graph: { nodes: true } })],
    [CRIV_STATE_SCHEMA_UNSUPPORTED, JSON.stringify({ ...valid, schema: "criv.state.v2" })],
  ];

  for (const [code, raw] of cases) {
    await assert.rejects(stateModule.loadState(raw), (error: unknown) => {
      assert.ok(error instanceof Error);
      assert.equal(error.name, "CrivStateContractError");
      assert.equal((error as CrivStateContractError).code, code);
      assert.doesNotMatch(error.message, /^criv-/);
      return true;
    });
  }
});

test("follows the shared source-target lookup fixture", async () => {
  const revision = await stateModule.loadState(JSON.stringify(lookupFixture.state));

  for (const expected of lookupFixture.cases) {
    const actual: CrivSourceTargetLookupResult = revision.lookupSourceTarget(expected.target);
    assert.equal(actual.kind, expected.kind, `lookup kind for ${expected.target}`);
    if (actual.kind === "resolved") {
      assert.equal(actual.canonical_target, expected.canonical_target);
    }
    if (actual.kind === "ambiguous") {
      assert.equal(actual.total_candidate_count, expected.total_candidate_count);
      assert.ok(actual.candidates.length <= 5);
    }
  }
  revision.dispose();
});

async function bundledStateModule(): Promise<CrivStateModule> {
  const outdir = await mkdtemp(join(tmpdir(), "criv-packaged-wasm-"));
  test.after(() => rm(outdir, { recursive: true, force: true }));
  const outfile = join(outdir, "state.cjs");
  await build({
    stdin: {
      contents: `module.exports = require("./src/packaged.ts").createPackagedCrivStateModule("unavailable");`,
      resolveDir: fileURLToPath(new URL("..", import.meta.url)),
      loader: "js",
    },
    bundle: true,
    format: "cjs",
    platform: "node",
    outfile,
    plugins: [crivWasmBytesPlugin],
    logLevel: "error",
  });
  return createRequire(import.meta.url)(outfile) as CrivStateModule;
}
