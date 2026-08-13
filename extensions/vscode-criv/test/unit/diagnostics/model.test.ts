import assert from "node:assert/strict";
import test from "node:test";

import { parseCheckDiagnostics } from "../../../src/diagnostics/model";

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

test("rejects non-array criv check JSON", () => {
  assert.throws(() => parseCheckDiagnostics("{}"), /Expected criv check JSON output/);
});
