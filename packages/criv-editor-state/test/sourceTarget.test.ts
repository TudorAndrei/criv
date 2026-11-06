import assert from "node:assert/strict";
import test from "node:test";

import type { CrivSourceTargetLookupResult } from "../src/contract.ts";
import {
  ambiguousSourceTargetMessage,
  parseLineFragment,
  patternTargetId,
  resolveSourceTarget,
  safeVaultPath,
  sourceTargetParts,
} from "../src/sourceTarget.ts";

test("normalizes safe vault-relative paths", () => {
  assert.equal(safeVaultPath("src/lib.rs"), "src/lib.rs");
  assert.equal(safeVaultPath(" ./src//lib.rs "), "src/lib.rs");
  assert.equal(safeVaultPath("src\\windows\\path.rs"), "src/windows/path.rs");
});

test("rejects paths that leave the vault", () => {
  for (const path of [
    "../secret.rs",
    "src/../secret.rs",
    "/etc/passwd",
    "C:\\Users\\name\\secret.rs",
    "\\\\server\\share\\secret.rs",
    "src\0secret.rs",
    "",
    ".",
    undefined,
    42,
  ]) {
    assert.equal(safeVaultPath(path), undefined, String(path));
  }
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

test("names ambiguous candidates and the hidden remainder", () => {
  const candidates = [
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
