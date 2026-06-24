import * as vscode from "vscode";

import {
  COMMAND_OPEN_SOURCE_TARGET,
  COMMAND_OPEN_STATE_JSON,
  COMMAND_REFRESH_STATE_VIEW,
  CRIV_COMMANDS,
} from "./commands";
import { registerSourceLanguageFeatures } from "./languageFeatures";
import { parseSourceTarget } from "./sourceTarget";
import { WorkspaceStateStore, type WorkspaceStateStatus } from "./stateStore";
import { CrivStateTreeProvider } from "./tree";

export { CRIV_COMMANDS };

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const store = new WorkspaceStateStore();
  const treeProvider = new CrivStateTreeProvider();
  const statusBar = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
  statusBar.command = COMMAND_REFRESH_STATE_VIEW;

  const updateSurfaces = (status: WorkspaceStateStatus) => {
    updateStatusBar(statusBar, status);
    treeProvider.update(status);
  };

  context.subscriptions.push(store, treeProvider, statusBar);
  context.subscriptions.push(store.onDidChangeStatus(updateSurfaces));
  context.subscriptions.push(
    vscode.window.registerTreeDataProvider("criv.stateView", treeProvider),
  );
  registerSourceLanguageFeatures(context, store);

  context.subscriptions.push(
    vscode.commands.registerCommand(COMMAND_REFRESH_STATE_VIEW, async () => {
      await store.refresh();
    }),
    vscode.commands.registerCommand(COMMAND_OPEN_STATE_JSON, async () => {
      await openStateJson(store);
    }),
    vscode.commands.registerCommand(COMMAND_OPEN_SOURCE_TARGET, async (target?: unknown) => {
      await openSourceTarget(store, target);
    }),
  );

  updateSurfaces(store.status);
  statusBar.show();
  await store.refresh();
}

export function deactivate(): void {
  // VS Code disposes subscriptions registered on the extension context.
}

async function openStateJson(store: WorkspaceStateStore): Promise<void> {
  const status = await ensureLoadedState(store);
  if (
    status.kind !== "ready" &&
    status.kind !== "missing-state" &&
    status.kind !== "invalid-state"
  ) {
    await vscode.window.showWarningMessage(messageForStatus(status));
    return;
  }

  try {
    const document = await vscode.workspace.openTextDocument(status.stateUri);
    await vscode.window.showTextDocument(document, { preview: false });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    await vscode.window.showWarningMessage(`Could not open .criv/state.json: ${message}`);
  }
}

async function openSourceTarget(store: WorkspaceStateStore, target: unknown): Promise<void> {
  if (typeof target !== "string" || !target.trim()) {
    await vscode.window.showWarningMessage("Choose a criv source target to open.");
    return;
  }

  const status = await ensureLoadedState(store);
  if (status.kind !== "ready") {
    await vscode.window.showWarningMessage(messageForStatus(status));
    return;
  }

  const node = await store.lookupNode(target);
  const parsed = parseSourceTarget(node?.source_target ?? node?.path ?? target);
  if (!parsed) {
    await vscode.window.showWarningMessage(`Could not resolve criv source target: ${target}`);
    return;
  }

  const uri = vscode.Uri.joinPath(status.root, ...parsed.path.split("/"));
  try {
    const document = await vscode.workspace.openTextDocument(uri);
    const options: vscode.TextDocumentShowOptions = { preview: false };
    if (parsed.line !== undefined) {
      const endLine = parsed.endLine ?? parsed.line;
      options.selection = new vscode.Range(parsed.line, 0, endLine, Number.MAX_SAFE_INTEGER);
    }
    await vscode.window.showTextDocument(document, options);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    await vscode.window.showWarningMessage(`Could not open ${parsed.path}: ${message}`);
  }
}

async function ensureLoadedState(store: WorkspaceStateStore): Promise<WorkspaceStateStatus> {
  if (store.status.kind === "loading") {
    return store.refresh();
  }
  return store.status;
}

function messageForStatus(status: WorkspaceStateStatus): string {
  switch (status.kind) {
    case "ready":
      return "criv state is loaded.";
    case "loading":
      return "criv state is still loading.";
    case "missing-workspace":
    case "missing-state":
    case "invalid-state":
      return status.message;
  }
}

function updateStatusBar(statusBar: vscode.StatusBarItem, status: WorkspaceStateStatus): void {
  switch (status.kind) {
    case "ready":
      statusBar.text = `$(graph) criv ${status.snapshot.summary.source_count} sources`;
      statusBar.tooltip = new vscode.MarkdownString(
        [
          `Schema: ${status.snapshot.summary.schema}`,
          `Nodes: ${status.snapshot.summary.node_count}`,
          `Edges: ${status.snapshot.summary.edge_count}`,
          `.c4 artifacts: ${status.snapshot.c4Artifacts.length}`,
        ].join("\n\n"),
      );
      statusBar.show();
      return;
    case "loading":
      statusBar.text = "$(sync~spin) criv";
      statusBar.tooltip = "Loading criv state";
      statusBar.show();
      return;
    case "missing-workspace":
    case "missing-state":
    case "invalid-state":
      statusBar.text = "$(warning) criv";
      statusBar.tooltip = status.message;
      statusBar.show();
      return;
  }
}
