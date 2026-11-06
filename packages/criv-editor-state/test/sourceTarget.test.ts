import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { lookupAsset } from "../src/assets.ts";
import type {
  CrivGraphNode,
  CrivSourceTargetCandidate,
  CrivSourceTargetLookupResult,
} from "../src/contract.ts";
import {
  ambiguousSourceTargetMessage,
  isSourceLinkTarget,
  parseLineFragment,
  patternTargetId,
  resolvedSourceLocation,
  resolveSourceTarget,
  safeVaultPath,
  sourceTargetParts,
  sourceTargetProblem,
  wikilinks,
} from "../src/sourceTarget.ts";

const pathFixture = JSON.parse(
  await readFile(
    new URL("../../../fixtures/editor/safe-vault-path.v1.json", import.meta.url),
    "utf8",
  ),
) as { cases: { input: string; expected: string | null }[] };

test("follows the shared path-safety fixture", () => {
  for (const { input, expected } of pathFixture.cases) {
    assert.equal(safeVaultPath(input), expected ?? undefined, JSON.stringify(input));
  }
  assert.equal(safeVaultPath(42), undefined);
});

test("opens only listed assets at a safe path", () => {
  const asset = { path: "docs/diagram.png", mime: "image/png", bytes: 1, hash: "a" };
  assert.deepEqual(lookupAsset([asset], "./docs\\diagram.png"), { kind: "resolved", asset });
  assert.deepEqual(lookupAsset([asset], "docs/missing.png"), { kind: "unauthorized" });
  assert.deepEqual(lookupAsset([asset], "../diagram.png"), { kind: "invalid" });
});

test("reads the line fragments the criv CLI accepts", () => {
  assert.deepEqual(parseLineFragment("L4"), { start: 4, end: 4 });
  assert.deepEqual(parseLineFragment("L4-L8"), { start: 4, end: 8 });
  assert.deepEqual(parseLineFragment("L8-L4"), { start: 8, end: 8 });
  for (const fragment of ["l4", "L4-8", "L0", "L4:H2", "Lx", "fn:run", undefined]) {
    assert.equal(parseLineFragment(fragment), undefined, String(fragment));
  }
});

test("splits a source target after its alias and source prefix", () => {
  assert.deepEqual(sourceTargetParts(" source:src/lib.rs#L2-L3 | label "), {
    body: "src/lib.rs#L2-L3",
    path: "src/lib.rs",
    fragment: "L2-L3",
    lines: { start: 2, end: 3 },
  });
  assert.deepEqual(sourceTargetParts("src/lib.rs#fn:run"), {
    body: "src/lib.rs#fn:run",
    path: "src/lib.rs",
    fragment: "fn:run",
    lines: undefined,
  });
});

test("reads pattern ids from both pattern link forms", () => {
  assert.equal(patternTargetId("match:ADR-0001/no-block-on"), "ADR-0001/no-block-on");
  assert.equal(patternTargetId("ADR-0001#match:ADR-0001/no-block-on|x"), "ADR-0001/no-block-on");
  assert.equal(patternTargetId("src/lib.rs#run"), undefined);
});

test("looks up the path for a line fragment and the full selector otherwise", () => {
  const requests: string[] = [];
  const lookup = (target: string): CrivSourceTargetLookupResult => {
    requests.push(target);
    return target === "missing.rs"
      ? { kind: "unresolved" }
      : {
          kind: "resolved",
          canonical_target: target,
          node: { id: target, kind: "code", label: target },
        };
  };

  assert.deepEqual(resolveSourceTarget(lookup, "source:src/lib.rs#L10-L20"), {
    kind: "resolved",
    canonical_target: "src/lib.rs",
    node: { id: "src/lib.rs", kind: "code", label: "src/lib.rs" },
    lines: { start: 10, end: 20 },
  });
  assert.equal(resolveSourceTarget(lookup, "src/lib.rs#fn:run").kind, "resolved");
  assert.deepEqual(resolveSourceTarget(lookup, "missing.rs"), { kind: "unresolved" });
  assert.deepEqual(resolveSourceTarget(lookup, "match:ADR-0001/no-block-on"), {
    kind: "unresolved",
  });
  assert.deepEqual(resolveSourceTarget(lookup, "source:#L1"), { kind: "unresolved" });
  assert.deepEqual(requests, ["src/lib.rs", "src/lib.rs#fn:run", "missing.rs"]);
});

test("opens the node file at the requested lines or else the node lines", () => {
  const node: CrivGraphNode = {
    id: "symbol:src/lib.rs#fn:run",
    kind: "function",
    label: "run",
    path: "src/lib.rs#L10-L20",
    line_range: "L10-L20",
  };
  const resolved = { kind: "resolved" as const, canonical_target: "src/lib.rs#fn:run", node };

  assert.deepEqual(resolvedSourceLocation(resolved), {
    path: "src/lib.rs",
    lines: { start: 10, end: 20 },
  });
  assert.deepEqual(resolvedSourceLocation({ ...resolved, lines: { start: 12, end: 12 } }), {
    path: "src/lib.rs",
    lines: { start: 12, end: 12 },
  });
  assert.equal(
    resolvedSourceLocation({ ...resolved, node: { ...node, path: "../escape.rs" } }),
    undefined,
  );
});

test("reads wikilink targets without aliases and recognizes source targets", () => {
  assert.deepEqual(wikilinks("a [[src/lib.rs#run | label]] b [[A note]]"), [
    { target: "src/lib.rs#run", start: 2, end: 28 },
    { target: "A note", start: 31, end: 41 },
  ]);
  for (const target of [
    "src/lib.rs",
    "source:lib.rs#fn:run",
    "source:src/missing",
    "lib/sample.ex#My.App.run/1",
  ]) {
    assert.equal(isSourceLinkTarget(target), true, target);
  }
  for (const target of ["A note", "docs/testing", "match:ADR-0001/x", "src/lib.rs.d/"]) {
    assert.equal(isSourceLinkTarget(target), false, target);
  }
});

test("warns only for an unresolved or ambiguous target", () => {
  assert.equal(
    sourceTargetProblem("missing.rs", { kind: "unresolved" }),
    "Unresolved criv target: missing.rs.",
  );
  const candidates: CrivSourceTargetCandidate[] = [
    {
      canonical_target: "a.rs#fn:run",
      node_id: "symbol:a.rs#fn:run",
      kind: "function",
      label: "run",
    },
  ];
  assert.equal(
    sourceTargetProblem("run", { kind: "ambiguous", candidates, total_candidate_count: 2 }),
    "Ambiguous criv source target run: a.rs#fn:run (function: run); 1 more.",
  );
  assert.equal(
    sourceTargetProblem("src/lib.rs", {
      kind: "resolved",
      canonical_target: "src/lib.rs",
      node: { id: "code:src/lib.rs", kind: "code", label: "src/lib.rs" },
    }),
    undefined,
  );
});

test("names ambiguous candidates and the hidden remainder", () => {
  const candidates: CrivSourceTargetCandidate[] = [
    {
      canonical_target: "src/a.rs#fn:run",
      node_id: "symbol:src/a.rs#fn:run",
      kind: "function",
      label: "run",
    },
    {
      canonical_target: "src/a.rs#fn:run",
      node_id: "symbol:src/a.rs#method:run",
      kind: "method",
      label: "run",
    },
    {
      canonical_target: "src/b.rs#fn:run",
      node_id: "symbol:src/b.rs#fn:run",
      kind: "function",
      label: "run",
    },
  ];

  assert.equal(
    ambiguousSourceTargetMessage("run", candidates, 4),
    "Ambiguous criv source target run: src/a.rs#fn:run (function: run) [symbol:src/a.rs#fn:run]; src/a.rs#fn:run (method: run) [symbol:src/a.rs#method:run]; src/b.rs#fn:run (function: run); 1 more.",
  );
});
