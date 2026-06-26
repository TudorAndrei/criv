import * as vscode from "vscode";

import {
  executablePathError,
  hasWorkspaceConfigurationValue,
  machineScopedValue,
} from "./configModel";

export interface CrivExtensionConfiguration {
  binaryPath: string;
  automaticRefresh: boolean;
  checkOnSave: boolean;
  previewC4OnOpen: boolean;
  workspaceExecutionOverrideIgnored: boolean;
}

export function crivConfiguration(): CrivExtensionConfiguration {
  const config = vscode.workspace.getConfiguration("criv");
  const binaryPath = config.inspect<string>("binaryPath");
  const checkOnSave = config.inspect<boolean>("checkOnSave");
  return {
    binaryPath: machineScopedValue(binaryPath, "criv"),
    automaticRefresh: config.get("automaticRefresh", true),
    checkOnSave: machineScopedValue(checkOnSave, false),
    previewC4OnOpen: config.get("previewC4OnOpen", true),
    workspaceExecutionOverrideIgnored:
      hasWorkspaceConfigurationValue(binaryPath) || hasWorkspaceConfigurationValue(checkOnSave),
  };
}

export { executablePathError };
