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
  assert.equal(result.stdoutTruncated, false);
  assert.equal(result.stderrTruncated, false);
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
  assert.equal(result.stdout, "x".repeat(32));
  assert.equal(result.stderr, "y".repeat(32));
  assert.equal(result.stdoutTruncated, true);
  assert.equal(result.stderrTruncated, true);
});

test("tracks stdout and stderr truncation independently", async () => {
  const root = await mkdtemp(join(tmpdir(), "criv-vscode-runner-"));
  const fakeCriv = join(root, "fake-criv-output.mjs");
  await writeFile(
    fakeCriv,
    [
      "#!/usr/bin/env node",
      "process.stdout.write('x'.repeat(64));",
      "process.stderr.write('ok');",
    ].join("\n"),
    { mode: 0o755 },
  );

  const result = await runProcess(fakeCriv, [], {
    cwd: root,
    maxOutputBytes: 8,
  });

  assert.equal(result.stdout, "x".repeat(8));
  assert.equal(result.stderr, "ok");
  assert.equal(result.stdoutTruncated, true);
  assert.equal(result.stderrTruncated, false);
});

test("drops an incomplete UTF-8 sequence at the capture boundary", async () => {
  const root = await mkdtemp(join(tmpdir(), "criv-vscode-runner-"));
  const fakeCriv = join(root, "fake-criv-utf8.mjs");
  await writeFile(
    fakeCriv,
    [
      "#!/usr/bin/env node",
      "process.stdout.write(Buffer.from([0x61, 0xf0, 0x9f, 0x98, 0x80]));",
    ].join("\n"),
    { mode: 0o755 },
  );

  const result = await runProcess(fakeCriv, [], {
    cwd: root,
    maxOutputBytes: 4,
  });

  assert.equal(result.stdout, "a");
  assert.equal(result.stdout.includes("�"), false);
  assert.equal(result.stdoutTruncated, true);
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

test("cancels a command when its signal was already aborted", async () => {
  const root = await mkdtemp(join(tmpdir(), "criv-vscode-runner-"));
  const fakeCriv = join(root, "fake-criv-pre-cancel.mjs");
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
  controller.abort();

  const result = await runProcess(fakeCriv, [], {
    cwd: root,
    signal: controller.signal,
    forceKillAfterMs: 25,
  });

  assert.equal(result.cancelled, true);
});
