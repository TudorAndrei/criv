import * as vscode from "vscode";

import { diagnosticRange, parseCheckDiagnostics } from "./model";
import { safeVaultPath } from "../navigation/target";

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
      const range = diagnosticRange(item);
      const diagnostic = new vscode.Diagnostic(
        new vscode.Range(
          range.start.line,
          range.start.character,
          range.end.line,
          range.end.character,
        ),
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

function severityValue(value: string): vscode.DiagnosticSeverity {
  return value === "error" ? vscode.DiagnosticSeverity.Error : vscode.DiagnosticSeverity.Warning;
}
