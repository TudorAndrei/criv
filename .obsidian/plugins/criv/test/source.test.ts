import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";
import { createPackagedCrivStateModule } from "@criv/editor-state/packaged";
import type { CrivInitialProjections } from "@criv/editor-state";

import type { SourceResolver } from "../src/ports";
import {
  crivLinkRanges,
  frontmatterPatternTargets,
  linkedSourcesFromMarkdown,
  resolvePattern,
  resolveSource,
  resolveSourceResult,
} from "../src/source/model";

interface LinkCase {
  target: string;
  source: string | null;
  ambiguous: boolean | null;
  pattern: string | null;
  note: string | null;
}

const fixture = JSON.parse(
  readFileSync(resolve(__dirname, "../../../../fixtures/link-resolution.json"), "utf8"),
) as { state: unknown; cases: LinkCase[] };

async function loadedFixture(): Promise<{
  projections: CrivInitialProjections;
  resolver: SourceResolver;
}> {
  const revision = await createPackagedCrivStateModule("unavailable").loadState(
    JSON.stringify(fixture.state),
  );
  const projections = revision.initialProjections();
  const sources = new Map(projections.sources.map((source) => [source.path, source]));
  return {
    projections,
    resolver: {
      lookupSourceTarget: (target) => revision.lookupSourceTarget(target),
      sourceEntry: (path) => sources.get(path),
    },
  };
}

/** Targets the criv CLI resolves but the Wasm lookup does not yet. */
const WASM_LOOKUP_GAPS = new Set(["source:lib.rs#run", "helper.rs#help"]);

test("resolves the shared link fixture like the criv CLI", async (t) => {
  const { projections, resolver } = await loadedFixture();

  // Obsidian resolves note links itself; criv adds source and pattern links.
  for (const testCase of fixture.cases.filter((candidate) => candidate.note === null)) {
    const todo = WASM_LOOKUP_GAPS.has(testCase.target)
      ? "Wasm lookup does not resolve a basename with a symbol fragment"
      : undefined;
    await t.test(testCase.target, { todo }, () => {
      const result = resolveSourceResult(resolver, testCase.target);
      if (testCase.ambiguous) {
        assert.equal(result.kind, "ambiguous");
      } else {
        assert.equal(result.kind === "resolved" ? result.source.entry.path : null, testCase.source);
      }
      assert.equal(resolvePattern(projections, testCase.target), testCase.pattern);
    });
  }
});

test("lists each linked source file once and previews a symbol at its lines", async () => {
  const { resolver } = await loadedFixture();

  const sources = linkedSourcesFromMarkdown(
    "[[src/lib.rs#run]] [[source:src/lib.rs]] [[src/helper.rs#help|helper]] [[missing.rs]]",
    resolver,
  );

  assert.deepEqual(
    sources.map((source) => [source.entry.path, source.lines]),
    [
      ["src/lib.rs", { start: 1, end: 1 }],
      ["src/helper.rs", { start: 1, end: 1 }],
    ],
  );
  assert.deepEqual(resolveSource(resolver, "source:src/lib.rs#L4-L6")?.lines, {
    start: 4,
    end: 6,
  });
});

test("marks unresolved and ambiguous links but not resolved ones", async () => {
  const { projections, resolver } = await loadedFixture();

  const ranges = crivLinkRanges(
    "[[src/lib.rs]] [[missing.rs]] [[shared.rs]] [[match:ADR-0001/no-block-on]] [[A note]]",
    projections,
    resolver,
  );

  assert.deepEqual(
    ranges.map((range) => [range.status, range.kind, range.target]),
    [
      ["resolved", "source", "src/lib.rs"],
      ["unresolved", "unknown", "missing.rs"],
      ["ambiguous", "source", "shared.rs"],
      ["resolved", "pattern", "match:ADR-0001/no-block-on"],
    ],
  );
});

test("names frontmatter pattern targets by their owning note", () => {
  const projections = {
    registeredPatterns: ["ADR-0001/no-block-on"],
    patternMatches: {
      "ADR-0001/no-block-on": [{ file: "src/lib.rs", range: "L1:C1-L1:C10", captures: {} }],
    },
  } as unknown as CrivInitialProjections;

  const targets = frontmatterPatternTargets(
    {
      id: "ADR-0001",
      policy: { patterns: [{ id: "no-block-on" }, "missing"] },
      targets: { patterns: ["local", { ref: "ADR-0001/no-block-on" }] },
    },
    projections,
  );

  assert.deepEqual(
    targets.map((target) => [target.source, target.id, target.status, target.matches.length]),
    [
      ["targets", "local", "local", 0],
      ["targets", "ADR-0001/no-block-on", "resolved", 1],
      ["policy", "ADR-0001/no-block-on", "resolved", 1],
      ["policy", "ADR-0001/missing", "unresolved", 0],
    ],
  );
});
