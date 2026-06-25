import * as vscode from "vscode";

import { C4ArtifactDiagnostics, C4PreviewManager } from "./c4Preview";
import { CrivCheckDiagnostics } from "./checkDiagnostics";
import {
  COMMAND_OPEN_SOURCE_TARGET,
  COMMAND_OPEN_STATE_JSON,
  COMMAND_PREVIEW_C4,
  COMMAND_REFRESH_STATE_VIEW,
  COMMAND_QUERY_UNDOCUMENTED_CODE,
  COMMAND_RUN_CHECK,
  COMMAND_RUN_WATCH_ONCE,
  CRIV_COMMANDS,
} from "./commands";
import { runProcess, type CommandResult } from "./commandRunner";
import { crivConfiguration } from "./config";
import { registerSourceLanguageFeatures } from "./languageFeatures";
import { parseSourceTarget } from "./sourceTarget";
import { WorkspaceStateStore, type WorkspaceStateStatus } from "./stateStore";
import { CrivStateTreeProvider } from "./tree";

export { CRIV_COMMANDS };

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const store = new WorkspaceStateStore();
  const treeProvider = new CrivStateTreeProvider();
  const checkDiagnostics = new CrivCheckDiagnostics();
  const c4Diagnostics = new C4ArtifactDiagnostics();
  const c4Preview = new C4PreviewManager(context);
  const statusBar = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
  statusBar.command = COMMAND_REFRESH_STATE_VIEW;

  const updateSurfaces = (status: WorkspaceStateStatus) => {
    updateStatusBar(statusBar, status);
    treeProvider.update(status);
  };

  context.subscriptions.push(
    store,
    treeProvider,
    checkDiagnostics,
    c4Diagnostics,
    c4Preview,
    statusBar,
  );
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
    vscode.commands.registerCommand(COMMAND_RUN_WATCH_ONCE, async () => {
      await runWatchOnce(store);
    }),
    vscode.commands.registerCommand(COMMAND_RUN_CHECK, async () => {
      await runCheck(store, checkDiagnostics);
    }),
    vscode.commands.registerCommand(COMMAND_QUERY_UNDOCUMENTED_CODE, async () => {
      await runUndocumentedCodeQuery(store);
    }),
    vscode.commands.registerCommand(COMMAND_PREVIEW_C4, async () => {
      await c4Preview.open();
    }),
    vscode.window.onDidChangeActiveTextEditor(async (editor) => {
      await maybePreviewC4OnOpen(c4Preview, editor?.document);
    }),
    vscode.workspace.onDidSaveTextDocument(async (document) => {
      await maybeRunCheckOnSave(store, checkDiagnostics, document);
    }),
  );

  updateSurfaces(store.status);
  statusBar.show();
  await store.refresh();
  await maybePreviewC4OnOpen(c4Preview, vscode.window.activeTextEditor?.document);
}

async function runWatchOnce(store: WorkspaceStateStore): Promise<void> {
  const root = await trustedCrivRoot(store);
  if (!root) {
    return;
  }

  const result = await runCrivWithProgress(root, ["watch", "--once"], "criv watch --once");
  if (!result || result.cancelled) {
    return;
  }

  if (result.code === 0) {
    if (crivConfiguration().automaticRefresh) {
      await store.refresh();
    }
    await vscode.window.showInformationMessage("criv watch --once completed.");
  } else {
    await vscode.window.showWarningMessage(commandFailureMessage("criv watch --once", result));
  }
}

async function runCheck(
  store: WorkspaceStateStore,
  checkDiagnostics: CrivCheckDiagnostics,
): Promise<void> {
  const root = await trustedCrivRoot(store);
  if (!root) {
    return;
  }

  const result = await runCrivWithProgress(root, ["check", "--format", "json"], "criv check");
  if (!result || result.cancelled) {
    return;
  }

  try {
    checkDiagnostics.setFromJson(root, result.stdout);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    await vscode.window.showWarningMessage(`Could not parse criv check diagnostics: ${message}`);
    return;
  }

  if (result.code === 0) {
    await vscode.window.showInformationMessage("criv check completed.");
  } else {
    await vscode.window.showWarningMessage("criv check completed with diagnostics.");
  }
}

async function runUndocumentedCodeQuery(store: WorkspaceStateStore): Promise<void> {
  const root = await trustedCrivRoot(store);
  if (!root) {
    return;
  }

  const result = await runCrivWithProgress(
    root,
    ["query", "nodes", "--kind", "code", "--without-docs", "--format", "json"],
    "criv query undocumented code",
  );
  if (!result || result.cancelled) {
    return;
  }

  if (result.code !== 0) {
    await vscode.window.showWarningMessage(
      commandFailureMessage("criv query undocumented code", result),
    );
    return;
  }

  const document = await vscode.workspace.openTextDocument({
    content: result.stdout.trim() ? result.stdout : "[]\n",
    language: "json",
  });
  await vscode.window.showTextDocument(document, { preview: false });
}

async function maybeRunCheckOnSave(
  store: WorkspaceStateStore,
  checkDiagnostics: CrivCheckDiagnostics,
  document: vscode.TextDocument,
): Promise<void> {
  if (!crivConfiguration().checkOnSave || !isCheckOnSaveDocument(document)) {
    return;
  }
  await runCheck(store, checkDiagnostics);
}

async function maybePreviewC4OnOpen(
  c4Preview: C4PreviewManager,
  document: vscode.TextDocument | undefined,
): Promise<void> {
  if (
    !crivConfiguration().previewC4OnOpen ||
    !document ||
    document.languageId !== "criv-c4" ||
    document.uri.scheme !== "file"
  ) {
    return;
  }
  await c4Preview.open(document, { preserveFocus: true });
}

async function runCrivWithProgress(
  root: vscode.Uri,
  args: readonly string[],
  title: string,
): Promise<CommandResult | undefined> {
  const config = crivConfiguration();
  return vscode.window.withProgress(
    { location: vscode.ProgressLocation.Notification, cancellable: true, title },
    async (_progress, token) => {
      const controller = new AbortController();
      const cancel = () => controller.abort();
      token.onCancellationRequested(cancel);
      try {
        return await runProcess(config.binaryPath, args, {
          cwd: root.fsPath,
          signal: controller.signal,
        });
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        await vscode.window.showWarningMessage(`${title} failed: ${message}`);
        return undefined;
      }
    },
  );
}

async function trustedCrivRoot(store: WorkspaceStateStore): Promise<vscode.Uri | undefined> {
  if (!vscode.workspace.isTrusted) {
    await vscode.window.showWarningMessage(
      "Trust this workspace before running local criv commands.",
    );
    return undefined;
  }

  const status = await ensureLoadedState(store);
  if (status.kind !== "ready") {
    await vscode.window.showWarningMessage(messageForStatus(status));
    return undefined;
  }
  return status.root;
}

function commandFailureMessage(command: string, result: CommandResult): string {
  const detail = result.stderr.trim() || result.stdout.trim() || `exit code ${result.code}`;
  return `${command} failed: ${detail}`;
}

function isCheckOnSaveDocument(document: vscode.TextDocument): boolean {
  return document.languageId === "markdown" || document.languageId === "criv-c4";
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
