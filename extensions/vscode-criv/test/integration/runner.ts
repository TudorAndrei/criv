import assert from "node:assert/strict";
import { mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import * as vscode from "vscode";

import { CRIV_COMMANDS } from "../../src/commands/identifiers";

export async function run(): Promise<void> {
  const extension = vscode.extensions.getExtension("criv.vscode-criv");
  assert.ok(extension, "Expected the criv extension to be installed in the test host.");
  await extension.activate();

  const commands = await vscode.commands.getCommands(true);
  for (const command of Object.values(CRIV_COMMANDS)) {
    assert.ok(commands.includes(command), `Expected command ${command} to be registered.`);
  }

  await assertNoManifestDiagnostics("extensions/vscode-criv/package.json");
}

/**
 * Report VS Code's own manifest diagnostics. A deliberately broken manifest
 * opened after the real one proves the JSON service has validated both.
 */
async function assertNoManifestDiagnostics(relativePath: string): Promise<void> {
  const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
  assert.ok(workspaceFolder, "Expected the repository root to be opened as the test workspace.");
  const manifest = vscode.Uri.joinPath(workspaceFolder.uri, ...relativePath.split("/"));
  const control = vscode.Uri.file(
    join(await mkdtemp(join(tmpdir(), "criv-manifest-")), "package.json"),
  );
  await writeFile(control.fsPath, JSON.stringify({ contributes: { commands: 1 } }));

  for (const uri of [manifest, control]) {
    await vscode.window.showTextDocument(await vscode.workspace.openTextDocument(uri), {
      preview: false,
    });
  }
  await diagnosticsReported(control, "Expected VS Code to report the broken control manifest.");

  const problems = vscode.languages
    .getDiagnostics(manifest)
    .filter((diagnostic) => diagnostic.severity <= vscode.DiagnosticSeverity.Warning)
    .map(
      (diagnostic) =>
        `${relativePath}:${diagnostic.range.start.line + 1}:${diagnostic.range.start.character + 1} ${diagnostic.message}`,
    );
  assert.deepEqual(problems, [], "Expected VS Code manifest diagnostics to be clean.");
}

async function diagnosticsReported(uri: vscode.Uri, message: string): Promise<void> {
  const reported = () => vscode.languages.getDiagnostics(uri).length > 0;
  if (reported()) {
    return;
  }
  await new Promise<void>((resolve, reject) => {
    const timeout = setTimeout(() => {
      subscription.dispose();
      reject(new assert.AssertionError({ message }));
    }, 30_000);
    const subscription = vscode.languages.onDidChangeDiagnostics(() => {
      if (reported()) {
        clearTimeout(timeout);
        subscription.dispose();
        resolve();
      }
    });
  });
}
