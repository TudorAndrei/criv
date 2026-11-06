import * as vscode from "vscode";
import { errorMessage } from "@criv/editor-state";

import { crivConfiguration } from "../config/reader";
import { executablePathError } from "../config/model";
import { loadedStatus, statusMessage, type WorkspaceState } from "../state/store";
import { runProcess, type CommandResult } from "./runner";

export type ReadyRoot = { kind: "ready"; root: vscode.Uri } | { kind: "blocked"; message: string };

export type CrivRun =
  | { kind: "completed"; result: CommandResult }
  | { kind: "cancelled" }
  | { kind: "failed"; message: string };

/** The root of loaded State in a trusted workspace, or why local commands cannot run. */
export async function trustedReadyRoot(state: WorkspaceState): Promise<ReadyRoot> {
  if (!vscode.workspace.isTrusted) {
    return { kind: "blocked", message: "Trust this workspace before running local criv commands." };
  }
  const status = await loadedStatus(state);
  return status.kind === "ready"
    ? { kind: "ready", root: status.source.root }
    : { kind: "blocked", message: statusMessage(status) };
}

/** Run the configured criv executable in `root` under a cancellable progress notification. */
export async function runCriv(
  root: vscode.Uri,
  args: readonly string[],
  title: string,
  options: { signal?: AbortSignal; maxOutputBytes?: number } = {},
): Promise<CrivRun> {
  const config = crivConfiguration();
  const executableError = executablePathError(config.binaryPath);
  if (executableError) {
    return { kind: "failed", message: executableError };
  }
  if (config.workspaceExecutionOverrideIgnored) {
    void vscode.window.showWarningMessage(
      "Workspace criv command-execution settings were ignored. Configure criv.binaryPath and criv.checkOnSave in user or machine settings.",
    );
  }
  return vscode.window.withProgress(
    { location: vscode.ProgressLocation.Notification, cancellable: true, title },
    async (_progress, token) => {
      const controller = new AbortController();
      const cancel = () => controller.abort();
      const cancellation = token.onCancellationRequested(cancel);
      options.signal?.addEventListener("abort", cancel, { once: true });
      if (options.signal?.aborted) {
        cancel();
      }
      try {
        const result = await runProcess(config.binaryPath, args, {
          cwd: root.fsPath,
          signal: controller.signal,
          maxOutputBytes: options.maxOutputBytes,
        });
        return result.cancelled ? { kind: "cancelled" } : { kind: "completed", result };
      } catch (error) {
        return { kind: "failed", message: `${title} failed: ${errorMessage(error)}` };
      } finally {
        cancellation.dispose();
        options.signal?.removeEventListener("abort", cancel);
      }
    },
  );
}

/** Run criv in the trusted State root. Warn and return nothing unless it exits with 0. */
export async function runCrivInReadyRoot(
  state: WorkspaceState,
  args: readonly string[],
  title: string,
): Promise<CommandResult | undefined> {
  const ready = await trustedReadyRoot(state);
  if (ready.kind === "blocked") {
    await vscode.window.showWarningMessage(ready.message);
    return undefined;
  }
  const run = await runCriv(ready.root, args, title);
  if (run.kind === "failed") {
    await vscode.window.showWarningMessage(run.message);
  } else if (run.kind === "completed" && run.result.code !== 0) {
    await vscode.window.showWarningMessage(commandFailureMessage(title, run.result));
  } else if (run.kind === "completed") {
    return run.result;
  }
  return undefined;
}

function commandFailureMessage(title: string, result: CommandResult): string {
  const detail = result.stderr.trim() || result.stdout.trim() || `exit code ${result.code}`;
  const truncated = [
    result.stderrTruncated ? "stderr" : undefined,
    result.stdoutTruncated ? "stdout" : undefined,
  ].filter((stream): stream is string => stream !== undefined);
  const suffix = truncated.length > 0 ? ` (${truncated.join(" and ")} truncated)` : "";
  return `${title} failed: ${detail}${suffix}`;
}
