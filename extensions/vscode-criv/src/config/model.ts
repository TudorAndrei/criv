import path from "node:path";

export interface InspectedConfiguration<T> {
  defaultValue?: T;
  globalValue?: T;
  workspaceValue?: T;
  workspaceFolderValue?: T;
  globalLanguageValue?: T;
  workspaceLanguageValue?: T;
  workspaceFolderLanguageValue?: T;
}

export function machineScopedValue<T>(
  inspection: InspectedConfiguration<T> | undefined,
  fallback: T,
): T {
  return inspection?.globalValue ?? inspection?.defaultValue ?? fallback;
}

export function hasWorkspaceConfigurationValue<T>(
  inspection: InspectedConfiguration<T> | undefined,
): boolean {
  return (
    inspection?.workspaceValue !== undefined ||
    inspection?.workspaceFolderValue !== undefined ||
    inspection?.workspaceLanguageValue !== undefined ||
    inspection?.workspaceFolderLanguageValue !== undefined
  );
}

export function executablePathError(command: string): string | undefined {
  const trimmed = command.trim();
  if (!trimmed) {
    return "criv.binaryPath must not be empty.";
  }
  if (trimmed !== command) {
    return "criv.binaryPath must not include leading or trailing whitespace.";
  }
  if (path.isAbsolute(command)) {
    return undefined;
  }
  if (command.includes("/") || command.includes("\\") || command.startsWith(".")) {
    return "criv.binaryPath must be a command name on PATH or an absolute path from user or machine settings.";
  }
  return undefined;
}
