import assert from "node:assert/strict";
import test from "node:test";

import { CHECK_MAX_OUTPUT_BYTES, completeCheckStdout } from "../../src/checkOutput";
import type { CommandResult } from "../../src/commandRunner";

function result(overrides: Partial<CommandResult> = {}): CommandResult {
  return {
    code: 0,
    signal: null,
    stdout: "[]\n",
    stderr: "",
    stdoutTruncated: false,
    stderrTruncated: false,
    cancelled: false,
    ...overrides,
  };
}

test("returns complete JSON check output", () => {
  assert.equal(completeCheckStdout(result()), "[]\n");
  assert.equal(CHECK_MAX_OUTPUT_BYTES, 16 * 1024 * 1024);
});

test("withholds truncated JSON check output from parsers", () => {
  assert.equal(
    completeCheckStdout(result({ stdout: '[{"severity":"error"', stdoutTruncated: true })),
    undefined,
  );
});
