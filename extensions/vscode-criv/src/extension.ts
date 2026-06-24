import * as vscode from "vscode";

const COMMAND_REFRESH_STATE_VIEW = "criv.refreshStateView";
const COMMAND_OPEN_STATE_JSON = "criv.openStateJson";
const COMMAND_OPEN_SOURCE_TARGET = "criv.openSourceTarget";

export function activate(context: vscode.ExtensionContext): void {
  context.subscriptions.push(
    vscode.commands.registerCommand(COMMAND_REFRESH_STATE_VIEW, async () => {
      await vscode.window.showInformationMessage("criv state view refresh is not implemented yet.");
    }),
    vscode.commands.registerCommand(COMMAND_OPEN_STATE_JSON, openStateJson),
    vscode.commands.registerCommand(COMMAND_OPEN_SOURCE_TARGET, async (target?: unknown) => {
      await vscode.window.showInformationMessage(
        `criv source navigation is not implemented yet${typeof target === "string" ? `: ${target}` : ""}.`,
      );
    }),
  );
}

export function deactivate(): void {
  // VS Code disposes subscriptions registered on the extension context.
}

async function openStateJson(): Promise<void> {
  const root = vscode.workspace.workspaceFolders?.[0]?.uri;
  if (!root) {
    await vscode.window.showWarningMessage("Open a workspace before opening criv state.");
    return;
  }

  const stateUri = vscode.Uri.joinPath(root, ".criv", "state.json");
  try {
    const document = await vscode.workspace.openTextDocument(stateUri);
    await vscode.window.showTextDocument(document, { preview: false });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    await vscode.window.showWarningMessage(`Could not open .criv/state.json: ${message}`);
  }
}
