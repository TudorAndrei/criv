import * as vscode from "vscode";

import { runCheck } from "./commands/check";
import { runCrivInReadyRoot } from "./commands/criv";
import { CRIV_COMMANDS } from "./commands/identifiers";
import { openAsset, openSourceTarget, openStateJson } from "./commands/open";
import { crivConfiguration } from "./config/reader";
import { CrivCheckDiagnostics } from "./diagnostics/publisher";
import { CheckRunOwner } from "./diagnostics/runs";
import { registerSourceLanguageFeatures } from "./navigation/languageFeatures";
import {
  createWorkspaceState,
  type WorkspaceState,
  type WorkspaceStateStatus,
} from "./state/store";
import { CrivStateTreeProvider } from "./tree";

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const state = createWorkspaceState();
  const treeProvider = new CrivStateTreeProvider();
  const checkDiagnostics = new CrivCheckDiagnostics();
  const checkRuns = new CheckRunOwner();
  const statusBar = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
  statusBar.command = CRIV_COMMANDS.refreshStateView;

  const updateSurfaces = (status: WorkspaceStateStatus) => {
    updateStatusBar(statusBar, status);
    treeProvider.update(status);
  };

  context.subscriptions.push(
    state,
    treeProvider,
    checkDiagnostics,
    checkRuns,
    statusBar,
    state.onDidChange(updateSurfaces),
    vscode.window.registerTreeDataProvider("criv.stateView", treeProvider),
    vscode.commands.registerCommand(CRIV_COMMANDS.refreshStateView, () => state.refresh()),
    vscode.commands.registerCommand(CRIV_COMMANDS.openStateJson, () => openStateJson(state)),
    vscode.commands.registerCommand(CRIV_COMMANDS.openSourceTarget, (target?: unknown) =>
      openSourceTarget(state, target),
    ),
    vscode.commands.registerCommand(CRIV_COMMANDS.openAsset, (path?: unknown) =>
      openAsset(state, path),
    ),
    vscode.commands.registerCommand(CRIV_COMMANDS.runWatchOnce, () => runWatchOnce(state)),
    vscode.commands.registerCommand(CRIV_COMMANDS.runCheck, () =>
      runCheck(state, checkDiagnostics, checkRuns),
    ),
    vscode.commands.registerCommand(CRIV_COMMANDS.queryUndocumentedCode, () =>
      queryUndocumentedCode(state),
    ),
    vscode.workspace.onDidSaveTextDocument(async (document) => {
      if (crivConfiguration().checkOnSave && document.languageId === "markdown") {
        await runCheck(state, checkDiagnostics, checkRuns);
      }
    }),
    vscode.workspace.onDidChangeWorkspaceFolders(() => state.refresh()),
  );
  registerSourceLanguageFeatures(context, state);

  updateSurfaces(state.status);
  statusBar.show();
  await state.refresh();
}

async function runWatchOnce(state: WorkspaceState): Promise<void> {
  const title = "criv watch --once";
  if (!(await runCrivInReadyRoot(state, ["watch", "--once"], title))) {
    return;
  }
  await state.refresh();
  await vscode.window.showInformationMessage(`${title} completed.`);
}

async function queryUndocumentedCode(state: WorkspaceState): Promise<void> {
  const result = await runCrivInReadyRoot(
    state,
    ["query", "nodes", "--kind", "code", "--without-docs", "--format", "json"],
    "criv query undocumented code",
  );
  if (!result) {
    return;
  }
  const document = await vscode.workspace.openTextDocument({
    content: result.stdout,
    language: "json",
  });
  await vscode.window.showTextDocument(document, { preview: false });
}

function updateStatusBar(statusBar: vscode.StatusBarItem, status: WorkspaceStateStatus): void {
  switch (status.kind) {
    case "ready": {
      const { summary } = status.projections;
      statusBar.text = `$(graph) criv ${summary.source_count} sources`;
      statusBar.tooltip = new vscode.MarkdownString(
        [
          `Schema: ${summary.schema}`,
          `Nodes: ${summary.node_count}`,
          `Edges: ${summary.edge_count}`,
        ].join("\n\n"),
      );
      return;
    }
    case "loading":
      statusBar.text = "$(sync~spin) criv";
      statusBar.tooltip = "Loading criv state";
      return;
    default:
      statusBar.text = "$(warning) criv";
      statusBar.tooltip = status.message;
  }
}
