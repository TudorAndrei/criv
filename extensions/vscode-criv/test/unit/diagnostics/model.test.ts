import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import { diagnosticRange, parseCheckDiagnostics } from "../../../src/diagnostics/model";

test("reads the criv check JSON that the CLI writes", () => {
  const output = readFileSync(
    resolve(__dirname, "../../../../../fixtures/editor/check-diagnostics.v1.json"),
    "utf8",
  );

  assert.deepEqual(parseCheckDiagnostics(output), [
    {
      severity: "error",
      code: "broken-link",
      path: "docs/a.md",
      line: 3,
      message: "wiki-link `[[missing]]` does not resolve",
      range: {
        start: { line: 2, character: 4 },
        end: { line: 2, character: 15 },
      },
      fix: "Correct the link target, or add the note it names.",
    },
    {
      severity: "warning",
      code: "source-wikilink",
      path: "docs/b.md",
      line: undefined,
      message:
        "wiki-link `[[src/lib.rs]]` targets source `src/lib.rs`; Wikilinks are reserved for document references",
      fix: "Rewrite the target as an AST-aware selector, as in `src/main.rs#fn:run`.",
    },
  ]);
});

test("rejects non-array criv check JSON", () => {
  assert.throws(() => parseCheckDiagnostics("{}"), /Expected criv check JSON output/);
});

test("accepts additive exact ranges and ignores unknown fields", () => {
  const [diagnostic] = parseCheckDiagnostics(
    JSON.stringify([
      {
        severity: "warning",
        code: "unresolved-target",
        path: "docs/architecture/model.c4",
        line: 3,
        message: "invalid model",
        range: {
          start: { line: 2, character: 4 },
          end: { line: 3, character: 1 },
        },
        futureField: { ignored: true },
      },
    ]),
  );

  assert.deepEqual(diagnostic?.range, {
    start: { line: 2, character: 4 },
    end: { line: 3, character: 1 },
  });
  assert.deepEqual(diagnosticRange(diagnostic!), diagnostic?.range);
});

test("uses the complete-line fallback for old or invalid ranges", () => {
  const diagnostics = parseCheckDiagnostics(
    JSON.stringify([
      {
        path: "docs/old.md",
        line: 4,
        message: "old producer",
      },
      {
        path: "docs/invalid.md",
        line: 7,
        message: "invalid range",
        range: {
          start: { line: 3, character: 2 },
          end: { line: 2, character: 1 },
        },
      },
    ]),
  );

  assert.deepEqual(diagnosticRange(diagnostics[0]!), {
    start: { line: 3, character: 0 },
    end: { line: 3, character: Number.MAX_SAFE_INTEGER },
  });
  assert.deepEqual(diagnosticRange(diagnostics[1]!), {
    start: { line: 6, character: 0 },
    end: { line: 6, character: Number.MAX_SAFE_INTEGER },
  });
});

test("widens an empty exact range for a visible diagnostic", () => {
  const [diagnostic] = parseCheckDiagnostics(
    JSON.stringify([
      {
        path: "docs/empty.md",
        line: 3,
        message: "empty exact range",
        range: {
          start: { line: 2, character: 4 },
          end: { line: 2, character: 4 },
        },
      },
    ]),
  );

  assert.deepEqual(diagnosticRange(diagnostic!), {
    start: { line: 2, character: 4 },
    end: { line: 2, character: 5 },
  });
});
