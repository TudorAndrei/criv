import assert from "node:assert/strict";
import test from "node:test";

import { diagnosticRange, parseCheckDiagnostics } from "../../../src/diagnostics/model";

test("normalizes criv check JSON diagnostics", () => {
  assert.deepEqual(
    parseCheckDiagnostics(
      JSON.stringify([
        {
          severity: "error",
          code: "broken-link",
          path: "docs/index.md",
          line: 12,
          message: "broken link",
        },
      ]),
    ),
    [
      {
        severity: "error",
        code: "broken-link",
        path: "docs/index.md",
        line: 12,
        message: "broken link",
      },
    ],
  );
});

test("carries the repair criv reports with a diagnostic", () => {
  assert.deepEqual(
    parseCheckDiagnostics(
      JSON.stringify([
        {
          severity: "error",
          code: "markdown-format",
          path: "docs/index.md",
          message: "bad heading",
          fix: "Run `criv check --fix`.",
        },
      ]),
    ),
    [
      {
        severity: "error",
        code: "markdown-format",
        path: "docs/index.md",
        line: undefined,
        message: "bad heading",
        fix: "Run `criv check --fix`.",
      },
    ],
  );
});

test("rejects non-array criv check JSON", () => {
  assert.throws(() => parseCheckDiagnostics("{}"), /Expected criv check JSON output/);
});

test("accepts additive exact ranges and ignores unknown fields", () => {
  const [diagnostic] = parseCheckDiagnostics(
    JSON.stringify([
      {
        severity: "warning",
        code: "invalid-likec4",
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
