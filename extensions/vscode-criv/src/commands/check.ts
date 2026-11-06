import * as vscode from "vscode";
import { errorMessage } from "@criv/editor-state";

import type { CrivCheckDiagnostics } from "../diagnostics/publisher";
import type { CheckRunOwner } from "../diagnostics/runs";
import type { WorkspaceState } from "../state/store";
import { runCriv, trustedReadyRoot, type CrivRun } from "./criv";

const CHECK_MAX_OUTPUT_BYTES = 16 * 1024 * 1024;

type CheckAttempt =
  | { kind: "aborted" }
  | { kind: "blocked"; message: string }
  | { kind: "ran"; root: vscode.Uri; run: CrivRun };

/** Run `criv check` and publish its diagnostics unless a newer check started. */
export async function runCheck(
  state: WorkspaceState,
  diagnostics: CrivCheckDiagnostics,
  runs: CheckRunOwner,
): Promise<void> {
  await runs.run(
    (signal) => collectCheck(state, signal),
    (attempt, signal) => publishCheck(diagnostics, attempt, signal),
    async (error) => {
      await vscode.window.showWarningMessage(`criv check failed: ${errorMessage(error)}`);
    },
  );
}

async function collectCheck(state: WorkspaceState, signal: AbortSignal): Promise<CheckAttempt> {
  const ready = await trustedReadyRoot(state);
  if (signal.aborted) {
    return { kind: "aborted" };
  }
  if (ready.kind === "blocked") {
    return ready;
  }
  const run = await runCriv(ready.root, ["check", "--format", "json"], "criv check", {
    signal,
    maxOutputBytes: CHECK_MAX_OUTPUT_BYTES,
  });
  return { kind: "ran", root: ready.root, run };
}

async function publishCheck(
  diagnostics: CrivCheckDiagnostics,
  attempt: CheckAttempt,
  signal: AbortSignal,
): Promise<void> {
  if (signal.aborted || attempt.kind === "aborted") {
    return;
  }
  if (attempt.kind === "blocked") {
    await vscode.window.showWarningMessage(attempt.message);
    return;
  }
  const { run } = attempt;
  if (run.kind === "cancelled") {
    return;
  }
  if (run.kind === "failed") {
    await vscode.window.showWarningMessage(run.message);
    return;
  }
  if (run.result.stdoutTruncated) {
    diagnostics.clear();
    await vscode.window.showWarningMessage(
      `criv check diagnostics exceeded the ${CHECK_MAX_OUTPUT_BYTES / (1024 * 1024)} MiB capture limit; diagnostics were cleared.`,
    );
    return;
  }

  try {
    diagnostics.setFromJson(attempt.root, run.result.stdout);
  } catch (error) {
    await vscode.window.showWarningMessage(
      `Could not parse criv check diagnostics: ${errorMessage(error)}`,
    );
    return;
  }

  if (run.result.code === 0) {
    await vscode.window.showInformationMessage("criv check completed.");
  } else {
    await vscode.window.showWarningMessage("criv check completed with diagnostics.");
  }
}
