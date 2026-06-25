import * as vscode from "vscode";

export interface CrivExtensionConfiguration {
  binaryPath: string;
  automaticRefresh: boolean;
  checkOnSave: boolean;
  previewC4OnOpen: boolean;
}

export function crivConfiguration(): CrivExtensionConfiguration {
  const config = vscode.workspace.getConfiguration("criv");
  return {
    binaryPath: config.get("binaryPath", "criv"),
    automaticRefresh: config.get("automaticRefresh", true),
    checkOnSave: config.get("checkOnSave", false),
    previewC4OnOpen: config.get("previewC4OnOpen", true),
  };
}
