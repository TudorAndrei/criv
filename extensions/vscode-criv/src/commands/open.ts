import * as vscode from "vscode";
import {
  CRIV_STATE_PATH,
  crivStatusMessage,
  errorMessage,
  lookupAsset,
  sourceTargetProblem,
} from "@criv/editor-state";

import { planSourceTargetOpen } from "../navigation/references";
import type { WorkspaceState } from "../state/store";

export async function openStateJson(state: WorkspaceState): Promise<void> {
  const status = await state.loaded();
  if (status.kind === "loading" || !status.source) {
    await vscode.window.showWarningMessage(crivStatusMessage(status));
    return;
  }
  try {
    const document = await vscode.workspace.openTextDocument(status.source.stateUri);
    await vscode.window.showTextDocument(document, { preview: false });
  } catch (error) {
    await vscode.window.showWarningMessage(
      `Could not open ${CRIV_STATE_PATH}: ${errorMessage(error)}`,
    );
  }
}

export async function openSourceTarget(state: WorkspaceState, target: unknown): Promise<void> {
  if (typeof target !== "string" || !target.trim()) {
    await vscode.window.showWarningMessage("Choose a criv source target to open.");
    return;
  }
  const status = await state.loaded();
  if (status.kind !== "ready") {
    await vscode.window.showWarningMessage(crivStatusMessage(status));
    return;
  }

  const plan = planSourceTargetOpen((lookup) => state.lookupSourceTarget(lookup), target);
  if (plan.kind !== "resolved") {
    await vscode.window.showWarningMessage(sourceTargetProblem(target, plan));
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
  const status = await state.loaded();
  if (status.kind !== "ready") {
    await vscode.window.showWarningMessage(crivStatusMessage(status));
    return;
  }
  const lookup = lookupAsset(status.projections.assets, requestedPath);
  switch (lookup.kind) {
    case "invalid":
      await vscode.window.showWarningMessage("Choose a valid documentation asset to open.");
      return;
    case "unauthorized":
      await vscode.window.showWarningMessage("The active criv State does not contain this asset.");
      return;
    case "resolved":
      break;
  }
  const { asset } = lookup;
  try {
    const uri = vscode.Uri.joinPath(status.source.root, ...asset.path.split("/"));
    await vscode.commands.executeCommand("vscode.open", uri);
  } catch (error) {
    await vscode.window.showWarningMessage(`Could not open ${asset.path}: ${errorMessage(error)}`);
  }
}
