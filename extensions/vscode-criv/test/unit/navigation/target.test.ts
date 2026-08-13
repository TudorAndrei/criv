import assert from "node:assert/strict";
import test from "node:test";

import {
  normalizeSourceTarget,
  parseLineFragment,
  parseSourceTarget,
  safeVaultPath,
} from "../../../src/navigation/target";

test("normalizes graph node source prefixes", () => {
  assert.equal(normalizeSourceTarget("code:src/lib.rs"), "src/lib.rs");
  assert.equal(normalizeSourceTarget("symbol:src/lib.rs#fn:run"), "src/lib.rs#fn:run");
  assert.equal(
    normalizeSourceTarget("docs/architecture/04-code.c4"),
    "docs/architecture/04-code.c4",
  );
});

test("parses source targets with one-based line fragments into zero-based ranges", () => {
  assert.deepEqual(parseSourceTarget("src/lib.rs#L10-L12"), {
    path: "src/lib.rs",
    fragment: "L10-L12",
    line: 9,
    endLine: 11,
  });
});

test("normalizes safe vault-relative source paths", () => {
  assert.equal(safeVaultPath("src/lib.rs"), "src/lib.rs");
  assert.equal(safeVaultPath("source:src/lib.rs"), "source:src/lib.rs");
  assert.equal(safeVaultPath("src\\windows\\path.rs"), "src/windows/path.rs");
  assert.equal(safeVaultPath("./src//lib.rs"), "src/lib.rs");
});

test("rejects source targets that escape the workspace", () => {
  for (const target of [
    "../secret.rs",
    "src/../secret.rs",
    "/etc/passwd",
    "C:\\Users\\name\\secret.rs",
    "\\\\server\\share\\secret.rs",
    "src\0secret.rs",
    "",
    ".",
  ]) {
    assert.equal(parseSourceTarget(target), undefined, target);
  }
});

test("normalizes windows separators after source prefixes", () => {
  assert.deepEqual(parseSourceTarget("source:src\\lib.rs#L10-L12"), {
    path: "src/lib.rs",
    fragment: "L10-L12",
    line: 9,
    endLine: 11,
  });
});

test("keeps symbolic fragments without inventing line ranges", () => {
  assert.deepEqual(parseSourceTarget("symbol:src/lib.rs#fn:run"), {
    path: "src/lib.rs",
    fragment: "fn:run",
    line: undefined,
    endLine: undefined,
  });
});

test("parses supported source line fragments", () => {
  assert.deepEqual(parseLineFragment("L1"), { line: 0, endLine: undefined });
  assert.deepEqual(parseLineFragment("L4-L8"), { line: 3, endLine: 7 });
  assert.equal(parseLineFragment("fn:run"), undefined);
});
