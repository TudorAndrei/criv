import assert from "node:assert/strict";

import * as vscode from "vscode";

const EXPECTED_COMMANDS = [
  "criv.refreshStateView",
  "criv.openStateJson",
  "criv.openSourceTarget",
  "criv.runWatchOnce",
  "criv.runCheck",
  "criv.queryUndocumentedCode",
  "criv.previewC4",
];

export async function run(): Promise<void> {
  const extension = vscode.extensions.getExtension("criv.vscode-criv");
  assert.ok(extension, "Expected the criv extension to be installed in the test host.");
  await extension.activate();

  const commands = await vscode.commands.getCommands(true);
  for (const command of EXPECTED_COMMANDS) {
    assert.ok(commands.includes(command), `Expected command ${command} to be registered.`);
  }

  await assertC4UsesCustomPreview();
  await assertNoJsonDiagnostics([
    "extensions/vscode-criv/package.json",
    "extensions/vscode-criv/language-configuration.json",
  ]);
}

async function assertC4UsesCustomPreview(): Promise<void> {
  const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
  assert.ok(workspaceFolder, "Expected the repository root to be opened as the test workspace.");
  const uri = vscode.Uri.joinPath(
    workspaceFolder.uri,
    "docs",
    "architecture",
    "01-system-context.c4",
  );

  await vscode.commands.executeCommand("vscode.open", uri);
  await delay(250);

  const input = vscode.window.tabGroups.activeTabGroup.activeTab?.input;
  assert.ok(input instanceof vscode.TabInputCustom, "Expected .c4 to open in a custom editor.");
  assert.equal(input.viewType, "criv.c4Preview");
  await vscode.commands.executeCommand("workbench.action.closeActiveEditor");
}

async function assertNoJsonDiagnostics(relativePaths: readonly string[]): Promise<void> {
  const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
  assert.ok(workspaceFolder, "Expected the repository root to be opened as the test workspace.");

  const diagnosticsByFile: string[] = [];
  for (const relativePath of relativePaths) {
    const uri = vscode.Uri.joinPath(workspaceFolder.uri, ...relativePath.split("/"));
    const document = await vscode.workspace.openTextDocument(uri);
    await vscode.window.showTextDocument(document, { preview: false });
    const diagnostics = await waitForDiagnostics(uri);
    const relevantDiagnostics = diagnostics.filter(
      (diagnostic) =>
        diagnostic.severity === vscode.DiagnosticSeverity.Error ||
        diagnostic.severity === vscode.DiagnosticSeverity.Warning,
    );

    for (const diagnostic of relevantDiagnostics) {
      diagnosticsByFile.push(
        `${relativePath}:${diagnostic.range.start.line + 1}:${
          diagnostic.range.start.character + 1
        } ${diagnostic.message}`,
      );
    }
  }

  assert.deepEqual(diagnosticsByFile, [], "Expected VS Code JSON diagnostics to be clean.");
}

async function waitForDiagnostics(uri: vscode.Uri): Promise<readonly vscode.Diagnostic[]> {
  for (let attempt = 0; attempt < 20; attempt += 1) {
    await delay(250);
    const diagnostics = vscode.languages.getDiagnostics(uri);
    if (diagnostics.length > 0) {
      return diagnostics;
    }
  }
  return vscode.languages.getDiagnostics(uri);
}

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}
