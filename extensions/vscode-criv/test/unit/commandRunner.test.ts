import assert from "node:assert/strict";
import { mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { runProcess } from "../../src/commandRunner";

test("runs a fake criv executable and captures stdout", async () => {
  const root = await mkdtemp(join(tmpdir(), "criv-vscode-runner-"));
  const fakeCriv = join(root, "fake-criv.mjs");
  await writeFile(
    fakeCriv,
    [
      "#!/usr/bin/env node",
      "const args = process.argv.slice(2);",
      "if (args.join(' ') === 'check --format json') {",
      "  console.log(JSON.stringify([{severity:'warning', code:'demo', path:'README.md', line:2, message:'demo warning'}]));",
      "  process.exit(0);",
      "}",
      "console.error(`unexpected args: ${args.join(' ')}`);",
      "process.exit(2);",
    ].join("\n"),
    { mode: 0o755 },
  );

  const result = await runProcess(fakeCriv, ["check", "--format", "json"], { cwd: root });
  assert.equal(result.code, 0);
  assert.equal(result.cancelled, false);
  assert.match(result.stdout, /demo warning/);
});
