import { randomBytes } from "node:crypto";
import * as vscode from "vscode";

import { LoadedRevisionOwner } from "@criv/editor-state";

import { buildC4PreviewHtml, buildC4PreviewStatusHtml } from "./c4PreviewHtml";
import { c4NavigationTarget, preferredC4ViewId } from "./c4PreviewModel";
import { COMMAND_OPEN_SOURCE_TARGET } from "./commands";
import type { WorkspaceStateStatus, WorkspaceStateStore } from "./stateStore";

export const C4_PREVIEW_VIEW_TYPE = "criv.c4Preview";

class C4PreviewSurface {
  private revision = 0;
  private readonly selectedViews = new Map<string, string>();

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

  bindMessages(
    webview: vscode.Webview,
    document?: vscode.TextDocument,
    viewColumn?: vscode.ViewColumn,
  ): vscode.Disposable {
    return webview.onDidReceiveMessage(async (message: unknown) => {
      if (isOpenSourceMessage(message)) {
        await vscode.commands.executeCommand(COMMAND_OPEN_SOURCE_TARGET, message.target);
      } else if (document && isSelectViewMessage(message)) {
        this.selectedViews.set(document.uri.toString(), message.viewId);
        await this.followNavigation(document, message.viewId, viewColumn);
      }
    });
  }

  /** Open the file that owns the navigated view, so the tab tracks the diagram. */
  private async followNavigation(
    document: vscode.TextDocument,
    viewId: string,
    viewColumn: vscode.ViewColumn | undefined,
  ): Promise<void> {
    const status = this.store.status;
    if (status.kind !== "ready" || !status.snapshot.architecture) {
      return;
    }
    const architecture = status.snapshot.architecture;
    const target = c4NavigationTarget(
      vscode.workspace.asRelativePath(document.uri, false),
      architecture.workspace,
      viewId,
      architecture.views,
    );
    if (!target) {
      return;
    }
    const folder = vscode.workspace.getWorkspaceFolder(document.uri);
    if (!folder) {
      return;
    }
    await vscode.commands.executeCommand(
      "vscode.openWith",
      vscode.Uri.joinPath(folder.uri, target),
      C4_PREVIEW_VIEW_TYPE,
      { viewColumn: viewColumn ?? vscode.ViewColumn.Active, preview: false },
    );
  }

  async render(
    owner: LoadedRevisionOwner<WebviewPreviewRevision>,
    webview: vscode.Webview,
    document: vscode.TextDocument,
  ): Promise<void> {
    await owner.replace(
      async () => new WebviewPreviewRevision(this.previewHtml(webview, document)),
      (candidate) => {
        webview.html = candidate.html;
      },
    );
  }

  private previewHtml(webview: vscode.Webview, document: vscode.TextDocument): string {
    const status = this.store.status;
    if (status.kind !== "ready" || !status.snapshot.architecture) {
      return buildC4PreviewStatusHtml(webview.cspSource, previewStatusMessage(status));
    }

    const relativePath = vscode.workspace.asRelativePath(document.uri, false);
    const model = {
      ...status.snapshot.architecture,
      revision: ++this.revision,
    };
    const rememberedViewId = this.selectedViews.get(document.uri.toString());
    const viewId = model.views.some((view) => view.id === rememberedViewId)
      ? rememberedViewId
      : preferredC4ViewId(relativePath, model.views);
    if (!viewId) {
      return buildC4PreviewStatusHtml(
        webview.cspSource,
        `${relativePath} declares no named view. Open an architecture file that declares a named view to see a diagram.`,
      );
    }
    const nonce = nonceValue();
    return buildC4PreviewHtml({
      cspSource: webview.cspSource,
      nonce,
      rendererUri: webview
        .asWebviewUri(vscode.Uri.joinPath(this.context.extensionUri, "media", "likec4-preview.js"))
        .toString(),
      payload: {
        model,
        viewId,
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
    const revisions = new LoadedRevisionOwner<WebviewPreviewRevision>();
    this.surface.configure(panel.webview);
    const render = () => void this.surface.render(revisions, panel.webview, document);
    const subscriptions = [
      this.surface.bindMessages(panel.webview, document, panel.viewColumn),
      this.store.onDidChangeStatus(render),
      vscode.workspace.onDidChangeTextDocument((event) => {
        if (event.document.uri.toString() === document.uri.toString()) {
          render();
        }
      }),
      vscode.window.onDidChangeActiveColorTheme(render),
    ];
    panel.onDidDispose(() => {
      revisions.dispose();
      for (const subscription of subscriptions) {
        subscription.dispose();
      }
    });
    render();
  }
}

export class C4PreviewManager implements vscode.Disposable {
  private panel: vscode.WebviewPanel | undefined;
  private document: vscode.TextDocument | undefined;
  private panelSubscriptions: vscode.Disposable[] = [];
  private revisions: LoadedRevisionOwner<WebviewPreviewRevision> | undefined;
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
    if (!document || !isC4Document(document)) {
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
      this.revisions = new LoadedRevisionOwner<WebviewPreviewRevision>();
      this.panelSubscriptions.push(this.surface.bindMessages(panel.webview));
      this.panelSubscriptions.push(
        this.store.onDidChangeStatus(() => {
          if (this.panel && this.document) {
            void this.surface.render(this.revisions!, this.panel.webview, this.document);
          }
        }),
      );
      panel.onDidDispose(() => {
        this.revisions?.dispose();
        this.revisions = undefined;
        this.panel = undefined;
        for (const subscription of this.panelSubscriptions.splice(0)) {
          subscription.dispose();
        }
      });
      this.panel = panel;
    }

    this.document = document;
    const relativePath = vscode.workspace.asRelativePath(document.uri, false);
    panel.title = `Preview ${relativePath}`;
    void this.surface.render(this.revisions!, panel.webview, document);
    panel.reveal(vscode.ViewColumn.Beside, options.preserveFocus ?? false);
  }

  dispose(): void {
    this.panel?.dispose();
    for (const subscription of this.panelSubscriptions.splice(0)) {
      subscription.dispose();
    }
  }
}

class WebviewPreviewRevision {
  constructor(readonly html: string) {}

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

function isSelectViewMessage(value: unknown): value is { type: "selectView"; viewId: string } {
  return (
    typeof value === "object" &&
    value !== null &&
    (value as { type?: unknown }).type === "selectView" &&
    typeof (value as { viewId?: unknown }).viewId === "string"
  );
}

function isC4Document(document: vscode.TextDocument): boolean {
  return (
    document.languageId === "criv-c4" ||
    document.languageId === "likec4" ||
    document.uri.path.endsWith(".c4")
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
