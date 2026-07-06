import * as vscode from "vscode";

import { parseCheckDiagnostics } from "./checkDiagnosticModel";
import { safeVaultPath } from "./sourceTarget";

export class CrivCheckDiagnostics implements vscode.Disposable {
  private readonly collection = vscode.languages.createDiagnosticCollection("criv-check");

  setFromJson(root: vscode.Uri, raw: string): void {
    const diagnosticsByUri = new Map<string, vscode.Diagnostic[]>();
    for (const item of parseCheckDiagnostics(raw)) {
      const safePath = safeVaultPath(item.path);
      if (!safePath) {
        continue;
      }

      const uri = vscode.Uri.joinPath(root, ...safePath.split("/"));
      const diagnostic = new vscode.Diagnostic(
        diagnosticRange(item.line),
        item.message,
        severityValue(item.severity),
      );
      diagnostic.source = "criv";
      diagnostic.code = item.code || undefined;
      const key = uri.toString();
      const diagnostics = diagnosticsByUri.get(key) ?? [];
      diagnostics.push(diagnostic);
      diagnosticsByUri.set(key, diagnostics);
    }

    this.collection.clear();
    for (const [uri, diagnostics] of diagnosticsByUri) {
      this.collection.set(vscode.Uri.parse(uri), diagnostics);
    }
  }

  clear(): void {
    this.collection.clear();
  }

  dispose(): void {
    this.collection.dispose();
  }
}

function diagnosticRange(line: number | undefined): vscode.Range {
  const zeroBasedLine = line === undefined ? 0 : Math.max(line - 1, 0);
  return new vscode.Range(zeroBasedLine, 0, zeroBasedLine, Number.MAX_SAFE_INTEGER);
}

function severityValue(value: string): vscode.DiagnosticSeverity {
  return value === "error" ? vscode.DiagnosticSeverity.Error : vscode.DiagnosticSeverity.Warning;
}
