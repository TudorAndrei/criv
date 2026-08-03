import { randomBytes } from "node:crypto";
import * as vscode from "vscode";

import { buildC4PreviewHtml, buildC4PreviewStatusHtml } from "./c4PreviewHtml";
import { preferredC4ViewId } from "./c4PreviewModel";
import { COMMAND_OPEN_SOURCE_TARGET } from "./commands";
import type { WorkspaceStateStatus, WorkspaceStateStore } from "./stateStore";

export const C4_PREVIEW_VIEW_TYPE = "criv.c4Preview";

class C4PreviewSurface {
  private revision = 0;

  constructor(
    private readonly context: vscode.ExtensionContext,
    private readonly store: WorkspaceStateStore,
  ) {}

  configure(webview: vscode.Webview): void {
    webview.options = {
      enableScripts: true,
      localResourceRoots: [vscode.Uri.joinPath(this.context.extensionUri, "media")],
    };
  }

  bindMessages(webview: vscode.Webview): vscode.Disposable {
    return webview.onDidReceiveMessage(async (message: unknown) => {
      if (isOpenSourceMessage(message)) {
        await vscode.commands.executeCommand(COMMAND_OPEN_SOURCE_TARGET, message.target);
      }
    });
  }

  render(webview: vscode.Webview, document: vscode.TextDocument): void {
    const status = this.store.status;
    if (status.kind !== "ready" || !status.snapshot.architecture) {
      webview.html = buildC4PreviewStatusHtml(webview.cspSource, previewStatusMessage(status));
      return;
    }

    const relativePath = vscode.workspace.asRelativePath(document.uri, false);
    const model = {
      ...status.snapshot.architecture,
      revision: ++this.revision,
    };
    const nonce = nonceValue();
    webview.html = buildC4PreviewHtml({
      cspSource: webview.cspSource,
      nonce,
      rendererUri: webview
        .asWebviewUri(vscode.Uri.joinPath(this.context.extensionUri, "media", "likec4-preview.js"))
        .toString(),
      payload: {
        model,
        viewId: preferredC4ViewId(relativePath, model.views),
        colorScheme:
          vscode.window.activeColorTheme.kind === vscode.ColorThemeKind.Light ? "light" : "dark",
      },
    });
  }
}

export class C4PreviewEditorProvider implements vscode.CustomTextEditorProvider {
  private readonly surface: C4PreviewSurface;

  constructor(
    context: vscode.ExtensionContext,
    private readonly store: WorkspaceStateStore,
  ) {
    this.surface = new C4PreviewSurface(context, store);
  }

  async resolveCustomTextEditor(
    document: vscode.TextDocument,
    panel: vscode.WebviewPanel,
  ): Promise<void> {
    this.surface.configure(panel.webview);
    const render = () => this.surface.render(panel.webview, document);
    const subscriptions = [
      this.surface.bindMessages(panel.webview),
      this.store.onDidChangeStatus(render),
      vscode.workspace.onDidChangeTextDocument((event) => {
        if (event.document.uri.toString() === document.uri.toString()) {
          render();
        }
      }),
      vscode.window.onDidChangeActiveColorTheme(render),
    ];
    panel.onDidDispose(() => {
      for (const subscription of subscriptions) {
        subscription.dispose();
      }
    });
    render();
  }
}

export class C4PreviewManager implements vscode.Disposable {
  private panel: vscode.WebviewPanel | undefined;
  private panelSubscriptions: vscode.Disposable[] = [];
  private readonly surface: C4PreviewSurface;

  constructor(
    context: vscode.ExtensionContext,
    private readonly store: WorkspaceStateStore,
  ) {
    this.surface = new C4PreviewSurface(context, store);
  }

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
        "criv.c4PreviewPanel",
        "criv LikeC4 Preview",
        { viewColumn: vscode.ViewColumn.Beside, preserveFocus: options.preserveFocus ?? false },
        {},
      );
      this.surface.configure(panel.webview);
      this.panelSubscriptions.push(this.surface.bindMessages(panel.webview));
      panel.onDidDispose(() => {
        this.panel = undefined;
        for (const subscription of this.panelSubscriptions.splice(0)) {
          subscription.dispose();
        }
      });
      this.panel = panel;
    }

    const relativePath = vscode.workspace.asRelativePath(document.uri, false);
    panel.title = `Preview ${relativePath}`;
    this.surface.render(panel.webview, document);
    panel.reveal(vscode.ViewColumn.Beside, options.preserveFocus ?? false);
  }

  dispose(): void {
    this.panel?.dispose();
    for (const subscription of this.panelSubscriptions.splice(0)) {
      subscription.dispose();
    }
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

function previewStatusMessage(status: WorkspaceStateStatus): string {
  if (status.kind === "loading") {
    return "Loading the criv architecture state…";
  }
  if (status.kind === "ready") {
    return "Run criv watch --once to validate LikeC4 and publish the preview model.";
  }
  return status.message;
}
