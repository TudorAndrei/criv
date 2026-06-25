import * as vscode from "vscode";

import { c4SourceTargets, parseC4Artifact } from "./c4Artifact";
import { buildC4PreviewHtml } from "./c4PreviewHtml";
import { COMMAND_OPEN_SOURCE_TARGET } from "./commands";

export class C4PreviewManager implements vscode.Disposable {
  private panel: vscode.WebviewPanel | undefined;

  constructor(private readonly context: vscode.ExtensionContext) {}

  async open(document = vscode.window.activeTextEditor?.document): Promise<void> {
    if (!document || document.languageId !== "criv-c4") {
      await vscode.window.showWarningMessage("Open a .c4 file before previewing it.");
      return;
    }

    const panel =
      this.panel ??
      vscode.window.createWebviewPanel(
        "criv.c4Preview",
        "criv C4 Preview",
        vscode.ViewColumn.Beside,
        {
          enableScripts: true,
          localResourceRoots: [vscode.Uri.joinPath(this.context.extensionUri, "media")],
        },
      );
    this.panel = panel;
    panel.onDidDispose(() => {
      this.panel = undefined;
    });
    panel.webview.onDidReceiveMessage(async (message: unknown) => {
      if (isOpenSourceMessage(message)) {
        await vscode.commands.executeCommand(COMMAND_OPEN_SOURCE_TARGET, message.target);
      }
    });

    const relativePath = vscode.workspace.asRelativePath(document.uri, false);
    const summary = parseC4Artifact(relativePath, document.getText());
    const nonce = nonceValue();
    panel.title = `Preview ${relativePath}`;
    panel.webview.html = buildC4PreviewHtml({
      cspSource: panel.webview.cspSource,
      nonce,
      mermaidUri: panel.webview
        .asWebviewUri(vscode.Uri.joinPath(this.context.extensionUri, "media", "mermaid.min.js"))
        .toString(),
      vizUri: panel.webview
        .asWebviewUri(vscode.Uri.joinPath(this.context.extensionUri, "media", "viz-global.js"))
        .toString(),
      payload: {
        format: summary.format,
        source: document.getText(),
        sources: c4SourceTargets(document.getText()),
      },
    });
    panel.reveal(vscode.ViewColumn.Beside);
  }

  dispose(): void {
    this.panel?.dispose();
  }
}

export class C4ArtifactDiagnostics implements vscode.Disposable {
  private readonly collection = vscode.languages.createDiagnosticCollection("criv-c4");
  private readonly subscriptions: vscode.Disposable[] = [];

  constructor() {
    this.subscriptions.push(
      vscode.workspace.onDidOpenTextDocument((document) => this.update(document)),
      vscode.workspace.onDidChangeTextDocument((event) => this.update(event.document)),
      vscode.workspace.onDidCloseTextDocument((document) => this.collection.delete(document.uri)),
    );
    for (const document of vscode.workspace.textDocuments) {
      this.update(document);
    }
  }

  dispose(): void {
    this.collection.dispose();
    for (const subscription of this.subscriptions) {
      subscription.dispose();
    }
  }

  private update(document: vscode.TextDocument): void {
    if (document.languageId !== "criv-c4") {
      this.collection.delete(document.uri);
      return;
    }

    const relativePath = vscode.workspace.asRelativePath(document.uri, false);
    const summary = parseC4Artifact(relativePath, document.getText());
    const diagnostics = summary.diagnostics.map((item) => {
      const line = item.line === null ? 0 : Math.max(item.line - 1, 0);
      const diagnostic = new vscode.Diagnostic(
        new vscode.Range(line, 0, line, Number.MAX_SAFE_INTEGER),
        item.message,
        vscode.DiagnosticSeverity.Warning,
      );
      diagnostic.source = "criv";
      diagnostic.code = item.code;
      return diagnostic;
    });
    this.collection.set(document.uri, diagnostics);
  }
}

function isOpenSourceMessage(value: unknown): value is { type: "openSource"; target: string } {
  return (
    typeof value === "object" &&
    value !== null &&
    (value as { type?: unknown }).type === "openSource" &&
    typeof (value as { target?: unknown }).target === "string"
  );
}

function nonceValue(): string {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
  let value = "";
  for (let index = 0; index < 32; index += 1) {
    value += alphabet[Math.floor(Math.random() * alphabet.length)] ?? "0";
  }
  return value;
}
