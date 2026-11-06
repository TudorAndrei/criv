import * as vscode from "vscode";

import { hasWorkspaceConfigurationValue, machineScopedValue } from "./model";

export interface CrivExtensionConfiguration {
  binaryPath: string;
  checkOnSave: boolean;
  workspaceExecutionOverrideIgnored: boolean;
}

export function crivConfiguration(): CrivExtensionConfiguration {
  const config = vscode.workspace.getConfiguration("criv");
  const binaryPath = config.inspect<string>("binaryPath");
  const checkOnSave = config.inspect<boolean>("checkOnSave");
  return {
    binaryPath: machineScopedValue(binaryPath, "criv"),
    checkOnSave: machineScopedValue(checkOnSave, false),
    workspaceExecutionOverrideIgnored:
      hasWorkspaceConfigurationValue(binaryPath) || hasWorkspaceConfigurationValue(checkOnSave),
  };
}
