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

test("truncates excessive command output", async () => {
  const root = await mkdtemp(join(tmpdir(), "criv-vscode-runner-"));
  const fakeCriv = join(root, "fake-criv-output.mjs");
  await writeFile(
    fakeCriv,
    [
      "#!/usr/bin/env node",
      "process.stdout.write('x'.repeat(256));",
      "process.stderr.write('y'.repeat(256));",
    ].join("\n"),
    { mode: 0o755 },
  );

  const result = await runProcess(fakeCriv, [], {
    cwd: root,
    maxOutputBytes: 32,
  });

  assert.equal(result.code, 0);
  assert.equal(result.stdout.startsWith("x".repeat(32)), true);
  assert.match(result.stdout, /criv output truncated after 32 bytes/);
  assert.equal(result.stderr.startsWith("y".repeat(32)), true);
  assert.match(result.stderr, /criv output truncated after 32 bytes/);
});

test("forces cancellation when a command ignores termination", async () => {
  const root = await mkdtemp(join(tmpdir(), "criv-vscode-runner-"));
  const fakeCriv = join(root, "fake-criv-cancel.mjs");
  await writeFile(
    fakeCriv,
    [
      "#!/usr/bin/env node",
      "process.on('SIGTERM', () => {});",
      "setInterval(() => {}, 1000);",
    ].join("\n"),
    { mode: 0o755 },
  );
  const controller = new AbortController();
  const running = runProcess(fakeCriv, [], {
    cwd: root,
    signal: controller.signal,
    forceKillAfterMs: 25,
  });

  setTimeout(() => controller.abort(), 25);
  const result = await running;

  assert.equal(result.cancelled, true);
});
