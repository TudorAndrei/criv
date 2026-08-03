import { randomBytes } from "node:crypto";
import * as vscode from "vscode";

import { buildC4PreviewHtml } from "./c4PreviewHtml";
import { COMMAND_OPEN_SOURCE_TARGET } from "./commands";
import type { WorkspaceStateStore } from "./stateStore";

export class C4PreviewManager implements vscode.Disposable {
  private panel: vscode.WebviewPanel | undefined;
  private revision = 0;

  constructor(
    private readonly context: vscode.ExtensionContext,
    private readonly store: WorkspaceStateStore,
  ) {}

  async open(
    document = vscode.window.activeTextEditor?.document,
    options: { preserveFocus?: boolean } = {},
  ): Promise<void> {
    if (!document || document.languageId !== "criv-c4") {
      await vscode.window.showWarningMessage("Open a .c4 file before you open its preview.");
      return;
    }
    if (this.store.status.kind !== "ready" || !this.store.status.snapshot.architecture) {
      await vscode.window.showWarningMessage(
        "Run criv watch --once to validate LikeC4 and publish the preview model.",
      );
      return;
    }

    let panel = this.panel;
    if (!panel) {
      panel = vscode.window.createWebviewPanel(
        "criv.c4Preview",
        "criv LikeC4 Preview",
        { viewColumn: vscode.ViewColumn.Beside, preserveFocus: options.preserveFocus ?? false },
        {
          enableScripts: true,
          localResourceRoots: [vscode.Uri.joinPath(this.context.extensionUri, "media")],
        },
      );
      panel.onDidDispose(() => {
        this.panel = undefined;
      });
      panel.webview.onDidReceiveMessage(async (message: unknown) => {
        if (isOpenSourceMessage(message)) {
          await vscode.commands.executeCommand(COMMAND_OPEN_SOURCE_TARGET, message.target);
        }
      });
      this.panel = panel;
    }

    const relativePath = vscode.workspace.asRelativePath(document.uri, false);
    const model = {
      ...this.store.status.snapshot.architecture,
      revision: ++this.revision,
    };
    const nonce = nonceValue();
    panel.title = `Preview ${relativePath}`;
    panel.webview.html = buildC4PreviewHtml({
      cspSource: panel.webview.cspSource,
      nonce,
      rendererUri: panel.webview
        .asWebviewUri(vscode.Uri.joinPath(this.context.extensionUri, "media", "likec4-preview.js"))
        .toString(),
      payload: {
        model,
        colorScheme:
          vscode.window.activeColorTheme.kind === vscode.ColorThemeKind.Light ? "light" : "dark",
      },
    });
    panel.reveal(vscode.ViewColumn.Beside, options.preserveFocus ?? false);
  }

  dispose(): void {
    this.panel?.dispose();
  }
}

export class C4ArtifactDiagnostics implements vscode.Disposable {
  dispose(): void {}
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
  return randomBytes(16).toString("base64");
}
