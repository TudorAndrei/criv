import * as vscode from "vscode";

import { C4PreviewEditorProvider, C4PreviewManager, C4_PREVIEW_VIEW_TYPE } from "./c4Preview";
import { CrivCheckDiagnostics } from "./checkDiagnostics";
import { CHECK_MAX_OUTPUT_BYTES, completeCheckStdout } from "./checkOutput";
import { CheckRunOwner } from "./checkRunOwner";
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
import { crivConfiguration, executablePathError } from "./config";
import { registerSourceLanguageFeatures } from "./languageFeatures";
import { ambiguousSourceTargetMessage, planSourceTargetOpen } from "./sourceReferences";
import { WorkspaceStateStore, type WorkspaceStateStatus } from "./stateStore";
import { CrivStateTreeProvider } from "./tree";
import { createVscodeStateHost } from "./vscodeStateHost";
import { loadState } from "./wasm";

export { CRIV_COMMANDS };

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const store = new WorkspaceStateStore(createVscodeStateHost(), { loadState });
  const treeProvider = new CrivStateTreeProvider();
  const checkDiagnostics = new CrivCheckDiagnostics();
  const checkRuns = new CheckRunOwner();
  const c4Preview = new C4PreviewManager(context, store);
  const c4PreviewEditor = new C4PreviewEditorProvider(context, store);
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
    checkRuns,
    c4Preview,
    statusBar,
  );
  context.subscriptions.push(store.onDidChangeStatus(updateSurfaces));
  context.subscriptions.push(
    vscode.window.registerTreeDataProvider("criv.stateView", treeProvider),
    vscode.window.registerCustomEditorProvider(C4_PREVIEW_VIEW_TYPE, c4PreviewEditor, {
      webviewOptions: { retainContextWhenHidden: true },
      supportsMultipleEditorsPerDocument: true,
    }),
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
      await runCheck(store, checkDiagnostics, checkRuns);
    }),
    vscode.commands.registerCommand(COMMAND_QUERY_UNDOCUMENTED_CODE, async () => {
      await runUndocumentedCodeQuery(store);
    }),
    vscode.commands.registerCommand(COMMAND_PREVIEW_C4, async () => {
      await c4Preview.open();
    }),
    vscode.workspace.onDidSaveTextDocument(async (document) => {
      await maybeRunCheckOnSave(store, checkDiagnostics, checkRuns, document);
    }),
    vscode.workspace.onDidChangeWorkspaceFolders(async () => {
      await store.refresh();
    }),
  );

  updateSurfaces(store.status);
  statusBar.show();
  await store.refresh();
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
  checkRuns: CheckRunOwner,
): Promise<void> {
  await checkRuns.run(
    (signal) => collectCheckAttempt(store, signal),
    (attempt, signal) => publishCheckAttempt(checkDiagnostics, attempt, signal),
    async (error) => {
      const message = error instanceof Error ? error.message : String(error);
      await vscode.window.showWarningMessage(`criv check failed: ${message}`);
    },
  );
}

type CheckAttempt =
  | { kind: "warning"; message: string }
  | {
      kind: "completed";
      root: vscode.Uri;
      result: CommandResult;
      configurationWarning?: string;
    };

async function collectCheckAttempt(
  store: WorkspaceStateStore,
  signal: AbortSignal,
): Promise<CheckAttempt> {
  const rootResult = await resolveCheckRoot(store);
  if (signal.aborted) {
    return { kind: "warning", message: "" };
  }
  if (rootResult.kind === "warning") {
    return rootResult;
  }

  const config = crivConfiguration();
  const executableError = executablePathError(config.binaryPath);
  if (executableError) {
    return { kind: "warning", message: executableError };
  }

  const result = await runCheckWithProgress(rootResult.root, config.binaryPath, signal);
  return {
    kind: "completed",
    root: rootResult.root,
    result,
    configurationWarning: config.workspaceExecutionOverrideIgnored
      ? "Workspace criv command-execution settings were ignored. Configure criv.binaryPath and criv.checkOnSave in user or machine settings."
      : undefined,
  };
}

async function resolveCheckRoot(
  store: WorkspaceStateStore,
): Promise<{ kind: "ready"; root: vscode.Uri } | { kind: "warning"; message: string }> {
  if (!vscode.workspace.isTrusted) {
    return {
      kind: "warning",
      message: "Trust this workspace before running local criv commands.",
    };
  }

  const status = await ensureLoadedState(store);
  return status.kind === "ready"
    ? { kind: "ready", root: status.root }
    : { kind: "warning", message: messageForStatus(status) };
}

async function runCheckWithProgress(
  root: vscode.Uri,
  binaryPath: string,
  ownerSignal: AbortSignal,
): Promise<CommandResult> {
  return vscode.window.withProgress(
    {
      location: vscode.ProgressLocation.Notification,
      cancellable: true,
      title: "criv check",
    },
    async (_progress, token) => {
      const controller = new AbortController();
      const cancel = () => controller.abort();
      const cancellation = token.onCancellationRequested(cancel);
      ownerSignal.addEventListener("abort", cancel, { once: true });
      if (ownerSignal.aborted) {
        cancel();
      }
      try {
        return await runProcess(binaryPath, ["check", "--format", "json"], {
          cwd: root.fsPath,
          signal: controller.signal,
          maxOutputBytes: CHECK_MAX_OUTPUT_BYTES,
        });
      } finally {
        cancellation.dispose();
        ownerSignal.removeEventListener("abort", cancel);
      }
    },
  );
}

async function publishCheckAttempt(
  checkDiagnostics: CrivCheckDiagnostics,
  attempt: CheckAttempt,
  signal: AbortSignal,
): Promise<void> {
  if (signal.aborted) {
    return;
  }
  if (attempt.kind === "warning") {
    if (attempt.message) {
      await vscode.window.showWarningMessage(attempt.message);
    }
    return;
  }
  if (attempt.result.cancelled) {
    return;
  }

  if (attempt.configurationWarning) {
    await vscode.window.showWarningMessage(attempt.configurationWarning);
    if (signal.aborted) {
      return;
    }
  }

  const stdout = completeCheckStdout(attempt.result);
  if (stdout === undefined) {
    checkDiagnostics.clear();
    await vscode.window.showWarningMessage(
      `criv check diagnostics exceeded the ${CHECK_MAX_OUTPUT_BYTES / (1024 * 1024)} MiB capture limit; diagnostics were cleared.`,
    );
    return;
  }

  try {
    checkDiagnostics.setFromJson(attempt.root, stdout);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    await vscode.window.showWarningMessage(`Could not parse criv check diagnostics: ${message}`);
    return;
  }

  if (attempt.result.code === 0) {
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
  checkRuns: CheckRunOwner,
  document: vscode.TextDocument,
): Promise<void> {
  if (!crivConfiguration().checkOnSave || !isCheckOnSaveDocument(document)) {
    return;
  }
  await runCheck(store, checkDiagnostics, checkRuns);
}

async function runCrivWithProgress(
  root: vscode.Uri,
  args: readonly string[],
  title: string,
  options: { maxOutputBytes?: number } = {},
): Promise<CommandResult | undefined> {
  const config = crivConfiguration();
  const executableError = executablePathError(config.binaryPath);
  if (executableError) {
    await vscode.window.showWarningMessage(executableError);
    return undefined;
  }
  if (config.workspaceExecutionOverrideIgnored) {
    await vscode.window.showWarningMessage(
      "Workspace criv command-execution settings were ignored. Configure criv.binaryPath and criv.checkOnSave in user or machine settings.",
    );
  }
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
          maxOutputBytes: options.maxOutputBytes,
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
  const truncated = [
    result.stderrTruncated ? "stderr" : undefined,
    result.stdoutTruncated ? "stdout" : undefined,
  ].filter((stream): stream is string => stream !== undefined);
  const suffix = truncated.length > 0 ? ` (${truncated.join(" and ")} truncated)` : "";
  return `${command} failed: ${detail}${suffix}`;
}

function isCheckOnSaveDocument(document: vscode.TextDocument): boolean {
  return (
    document.languageId === "markdown" ||
    document.languageId === "criv-c4" ||
    document.languageId === "likec4"
  );
}

export function deactivate(): void {
  // VS Code disposes subscriptions registered on the extension context.
}

async function openStateJson(store: WorkspaceStateStore): Promise<void> {
  const status = await ensureLoadedState(store);
  if (
    status.kind !== "ready" &&
    !(status.kind === "missing" && status.reason === "state") &&
    status.kind !== "unavailable" &&
    status.kind !== "invalid"
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

  const result = planSourceTargetOpen(
    (lookupTarget) => store.lookupSourceTarget(lookupTarget),
    target,
  );
  if (result.kind === "ambiguous") {
    await vscode.window.showWarningMessage(
      ambiguousSourceTargetMessage(target, result.candidates, result.total_candidate_count),
    );
    return;
  }
  if (result.kind === "malformed") {
    await vscode.window.showWarningMessage(`Malformed criv source target: ${target}`);
    return;
  }
  if (result.kind === "unresolved") {
    await vscode.window.showWarningMessage(`Could not resolve criv source target: ${target}`);
    return;
  }
  if (result.kind === "resolved") {
    // Continue with the canonical node below.
  } else {
    return;
  }

  const parsed = result.target;

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
    case "missing":
    case "unavailable":
    case "invalid":
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
    case "missing":
    case "unavailable":
    case "invalid":
      statusBar.text = "$(warning) criv";
      statusBar.tooltip = status.message;
      statusBar.show();
      return;
  }
}
