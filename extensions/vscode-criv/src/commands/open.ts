import * as vscode from "vscode";
import { ambiguousSourceTargetMessage, errorMessage } from "@criv/editor-state";

import { openActiveAsset } from "../assets";
import { planSourceTargetOpen } from "../navigation/references";
import { loadedStatus, statusMessage, type WorkspaceState } from "../state/store";

export async function openStateJson(state: WorkspaceState): Promise<void> {
  const status = await loadedStatus(state);
  if (status.kind === "loading" || !status.source) {
    await vscode.window.showWarningMessage(statusMessage(status));
    return;
  }
  try {
    const document = await vscode.workspace.openTextDocument(status.source.stateUri);
    await vscode.window.showTextDocument(document, { preview: false });
  } catch (error) {
    await vscode.window.showWarningMessage(
      `Could not open .criv/state.json: ${errorMessage(error)}`,
    );
  }
}

export async function openSourceTarget(state: WorkspaceState, target: unknown): Promise<void> {
  if (typeof target !== "string" || !target.trim()) {
    await vscode.window.showWarningMessage("Choose a criv source target to open.");
    return;
  }
  const status = await loadedStatus(state);
  if (status.kind !== "ready") {
    await vscode.window.showWarningMessage(statusMessage(status));
    return;
  }

  const plan = planSourceTargetOpen((lookup) => state.lookupSourceTarget(lookup), target);
  if (plan.kind === "ambiguous") {
    await vscode.window.showWarningMessage(
      ambiguousSourceTargetMessage(target, plan.candidates, plan.total_candidate_count),
    );
    return;
  }
  if (plan.kind === "unresolved") {
    await vscode.window.showWarningMessage(`Could not resolve criv source target: ${target}`);
    return;
  }

  const uri = vscode.Uri.joinPath(status.source.root, ...plan.path.split("/"));
  try {
    const document = await vscode.workspace.openTextDocument(uri);
    await vscode.window.showTextDocument(document, {
      preview: false,
      selection: plan.lines
        ? new vscode.Range(plan.lines.start - 1, 0, plan.lines.end - 1, Number.MAX_SAFE_INTEGER)
        : undefined,
    });
  } catch (error) {
    await vscode.window.showWarningMessage(`Could not open ${plan.path}: ${errorMessage(error)}`);
  }
}

export async function openAsset(state: WorkspaceState, requestedPath: unknown): Promise<void> {
  const status = await loadedStatus(state);
  if (status.kind !== "ready") {
    await vscode.window.showWarningMessage(statusMessage(status));
    return;
  }
  const result = await openActiveAsset(status.projections.assets, requestedPath, async (asset) => {
    const uri = vscode.Uri.joinPath(status.source.root, ...asset.path.split("/"));
    await vscode.commands.executeCommand("vscode.open", uri);
  });
  switch (result.kind) {
    case "invalid":
      await vscode.window.showWarningMessage("Choose a valid documentation asset to open.");
      return;
    case "unauthorized":
      await vscode.window.showWarningMessage("The active criv State does not contain this asset.");
      return;
    case "failed":
      await vscode.window.showWarningMessage(
        `Could not open ${result.asset.path}: ${errorMessage(result.error)}`,
      );
      return;
    case "opened":
      return;
  }
}
