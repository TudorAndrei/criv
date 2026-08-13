import { FileView, Notice } from "obsidian";
import type { App, Command, TFile, WorkspaceLeaf } from "obsidian";
import { GenerationRevisionOwner } from "@criv/editor-state";
import { preferredLikeC4ViewId } from "@criv/likec4/protocol";
import { CrivLikeC4Renderer } from "@criv/likec4/renderer";
import type { C4ViewPort, DisposableSubscription, ObsidianStateStatus } from "./ports";

export const C4_VIEW_TYPE = "criv-c4-view";

interface ObsidianCommandRegistry {
  commands?: Record<string, Command>;
}

export class CrivC4View extends FileView {
  private source = "";
  private draftSource = "";
  private sourcePath: string | null = null;
  private mode: "preview" | "source" = "preview";
  private dirtyBadgeEl: HTMLElement | null = null;
  private sourceEditorEl: HTMLTextAreaElement | null = null;
  private sourceSaveHandlerRegistered = false;
  private likec4Renderer: CrivLikeC4Renderer | null = null;
  private likec4ViewSelect: HTMLSelectElement | null = null;
  private readonly previewRevisions = new GenerationRevisionOwner<CrivC4PreviewRevision>();
  private stateStatusSubscription: DisposableSubscription | null = null;
  private previewClosed = false;

  constructor(
    leaf: WorkspaceLeaf,
    private readonly port: C4ViewPort,
    private readonly createLikeC4Renderer: (
      surface: HTMLElement,
      options: ConstructorParameters<typeof CrivLikeC4Renderer>[1],
    ) => CrivLikeC4Renderer = (surface, options) => new CrivLikeC4Renderer(surface, options),
  ) {
    super(leaf);
    this.registerSourceSaveHandler();
  }

  getViewType(): string {
    return C4_VIEW_TYPE;
  }

  getDisplayText(): string {
    return this.file?.basename ?? "C4";
  }

  async onOpen(): Promise<void> {
    this.previewClosed = false;
    this.registerSourceSaveHandler();
    this.stateStatusSubscription ??= this.port.onStateStatusChange((status) => {
      void this.acceptStateStatus(status);
    });
    await this.acceptStateStatus(this.port.currentStateStatus());
  }

  async onClose(): Promise<void> {
    this.previewRevisions.dispose();
    this.shutdown();
  }

  shutdown(): void {
    if (this.previewClosed) {
      return;
    }
    this.previewClosed = true;
    this.previewRevisions.dispose();
    this.stateStatusSubscription?.dispose();
    this.stateStatusSubscription = null;
    this.likec4Renderer = null;
    this.likec4ViewSelect = null;
  }

  async onLoadFile(file: TFile): Promise<void> {
    this.source = await this.app.vault.cachedRead(file);
    this.draftSource = this.source;
    this.sourcePath = file.path;
    this.mode = "preview";
    this.sourceEditorEl = null;
    await this.render();
  }

  async onUnloadFile(file: TFile): Promise<void> {
    if (this.sourcePath === file.path) {
      this.source = "";
      this.draftSource = "";
      this.sourcePath = null;
      this.sourceEditorEl = null;
    }
    await this.render();
  }

  async render(): Promise<void> {
    this.registerSourceSaveHandler();
    const container = this.containerEl.children[1] as HTMLElement;

    if (!this.file) {
      this.clearPreviewRevision();
      container.empty();
      container.addClass("criv-c4-view");
      container.createEl("p", { cls: "criv-empty", text: "No C4 file selected." });
      return;
    }

    if (this.mode === "preview") {
      await this.acceptStateStatus(this.port.currentStateStatus());
      return;
    }

    this.clearPreviewRevision();
    await this.sourceForCurrentFile();
    const source = this.currentSource();
    const chrome = this.buildChrome(container, source, this.file.basename);
    this.dirtyBadgeEl = chrome.dirtyBadge;
    this.updateDirtyBadge();
    this.renderSourceEditor(chrome.body);
  }

  async acceptStateStatus(status: ObsidianStateStatus): Promise<void> {
    if (this.previewClosed) {
      return;
    }
    const container = this.containerEl.children[1] as HTMLElement;
    if (!this.file) {
      this.clearPreviewRevision();
      container.empty();
      container.addClass("criv-c4-view");
      container.createEl("p", { cls: "criv-empty", text: "No C4 file selected." });
      return;
    }
    if (this.mode !== "preview") {
      if (status.kind !== "ready" && status.kind !== "loading") {
        this.clearPreviewForStatus(status.generation);
      }
      return;
    }
    if (status.kind === "loading") {
      if (!this.previewRevisions.current) {
        this.renderPreviewStatus(container, this.file, "Loading preview…", false);
      }
      return;
    }
    if (status.kind !== "ready") {
      if (this.previewRevisions.clear(status.generation)) {
        this.resetPreviewReferences();
        this.renderPreviewStatus(container, this.file, status.message, true);
      }
      return;
    }
    await this.renderPreviewRevision(container, this.file, status);
  }

  canSaveSourceFromShortcut(): boolean {
    return this.mode === "source" && this.sourceEditorEl !== null;
  }

  async saveSourceFromShortcut(): Promise<void> {
    if (this.sourceEditorEl) {
      await this.saveSource(this.sourceEditorEl.value);
    }
  }

  private buildChrome(
    container: HTMLElement,
    source: string,
    basename: string,
  ): { body: HTMLElement; dirtyBadge: HTMLElement; toolbar: HTMLElement } {
    container.empty();
    container.addClass("criv-c4-view");
    const header = container.createDiv({ cls: "criv-c4-header" });
    header.createEl("h3", { text: basename });
    const meta = header.createDiv({ cls: "criv-c4-meta" });
    meta.createSpan({ text: "likec4" });
    meta.createSpan({ text: "model" });
    if (/^\s*\/\/\s*criv:generated\s+true\s*$/m.test(source)) {
      meta.createSpan({ text: "generated" });
    }
    const dirtyBadge = meta.createSpan({
      cls: "criv-warning criv-c4-dirty",
      text: "unsaved",
    });
    const toolbar = header.createDiv({ cls: "criv-c4-toolbar" });
    this.renderToolbar(toolbar);
    const body = container.createDiv({ cls: "criv-c4-body" });
    return { body, dirtyBadge, toolbar };
  }

  private async sourceForCurrentFile(): Promise<string> {
    if (!this.file) {
      return "";
    }
    if (this.sourcePath !== this.file.path) {
      this.source = await this.app.vault.cachedRead(this.file);
      this.draftSource = this.source;
      this.sourcePath = this.file.path;
      this.mode = "preview";
      this.sourceEditorEl = null;
    }
    return this.source;
  }

  private currentSource(): string {
    return this.sourceDirty() ? this.draftSource : this.source;
  }

  private sourceDirty(): boolean {
    return this.draftSource !== this.source;
  }

  private renderToolbar(toolbar: HTMLElement): void {
    this.toolbarButton(toolbar, "Preview", "Preview diagram", this.mode === "preview", () => {
      this.mode = "preview";
      void this.render();
    });
    this.toolbarButton(toolbar, "Source", "Edit source", this.mode === "source", () => {
      this.mode = "source";
      void this.render();
    });
  }

  private toolbarButton(
    toolbar: HTMLElement,
    text: string,
    tooltip: string,
    active: boolean,
    onClick: () => void,
  ): void {
    const button = toolbar.createEl("button", { text, attr: { "aria-label": tooltip } });
    button.setAttribute("title", tooltip);
    if (active) {
      button.addClass("is-active");
    }
    button.onclick = onClick;
  }

  private async renderPreviewRevision(
    container: HTMLElement,
    file: TFile,
    status: Extract<ObsidianStateStatus, { kind: "ready" }>,
  ): Promise<void> {
    const selectedViewId = this.likec4Renderer?.currentViewId();
    if (!this.previewRevisions.current) {
      const loading = this.buildChrome(container, this.currentSource(), file.basename);
      loading.body.createEl("p", { cls: "criv-c4-render-status", text: "Loading preview…" });
      this.dirtyBadgeEl = loading.dirtyBadge;
      this.updateDirtyBadge();
    }
    const result = await this.previewRevisions.replace(
      status.generation,
      async (attempt) => {
        const source =
          this.sourcePath === file.path
            ? this.currentSource()
            : await this.app.vault.cachedRead(file);
        attempt.assertCurrent();
        const architecture = status.state.architecture;
        if (!architecture) {
          throw new Error(
            "Run criv watch --once to validate LikeC4 and publish the preview model.",
          );
        }
        const root = document.createElement("div");
        const chrome = this.buildChrome(root, source, file.basename);
        const viewport = chrome.body.createDiv({ cls: "criv-c4-preview" });
        const surface = viewport.createDiv({ cls: "criv-c4-preview-surface" });
        const nextViewId =
          selectedViewId && architecture.views.some((view) => view.id === selectedViewId)
            ? selectedViewId
            : preferredLikeC4ViewId(file.path, architecture.views);
        if (!nextViewId) {
          surface.createEl("p", {
            cls: "criv-c4-render-status",
            text: `${file.path} declares no named view. Open an architecture file that declares a named view to see a diagram.`,
          });
          return new CrivC4PreviewRevision(root, null, null, chrome.dirtyBadge, source, file.path);
        }
        const renderer = this.createLikeC4Renderer(surface, {
          colorScheme: document.body.classList.contains("theme-dark") ? "dark" : "light",
          onOpenSource: (target) => this.port.openValidatedSource(target),
          onSelectView: (viewId) => {
            if (this.likec4ViewSelect) {
              this.likec4ViewSelect.value = viewId;
            }
          },
        });
        renderer.replace(architecture, nextViewId);
        const viewSelect = this.renderLikeC4Controls(chrome.toolbar, renderer);
        return new CrivC4PreviewRevision(
          root,
          renderer,
          viewSelect,
          chrome.dirtyBadge,
          source,
          file.path,
        );
      },
      (candidate) => {
        container.empty();
        while (candidate.root.firstChild) {
          container.appendChild(candidate.root.firstChild);
        }
        container.addClass("criv-c4-view");
        this.likec4Renderer = candidate.renderer;
        this.likec4ViewSelect = candidate.viewSelect;
        this.dirtyBadgeEl = candidate.dirtyBadge;
        this.sourceEditorEl = null;
        if (this.sourcePath !== candidate.sourcePath) {
          this.source = candidate.source;
          this.draftSource = candidate.source;
          this.sourcePath = candidate.sourcePath;
        }
        this.updateDirtyBadge();
      },
    );
    if (result.kind === "failed") {
      this.likec4Renderer = null;
      this.likec4ViewSelect = null;
      const failed = this.buildChrome(container, this.currentSource(), file.basename);
      failed.body.createEl("p", {
        cls: "criv-c4-render-error",
        text: result.error instanceof Error ? result.error.message : String(result.error),
      });
      this.dirtyBadgeEl = failed.dirtyBadge;
      this.updateDirtyBadge();
    }
  }

  private renderLikeC4Controls(
    toolbar: HTMLElement,
    renderer: CrivLikeC4Renderer,
  ): HTMLSelectElement | null {
    let viewSelect: HTMLSelectElement | null = null;
    const views = renderer.views();
    if (views.length > 1) {
      const select = toolbar.createEl("select", { attr: { "aria-label": "Architecture view" } });
      for (const view of views) {
        select.createEl("option", { text: view.title, value: view.id });
      }
      viewSelect = select;
      select.value = renderer.currentViewId() ?? "";
      select.onchange = () => renderer.selectView(select.value);
    }
    this.toolbarButton(toolbar, "Export SVG", "Export the current view as SVG", false, () => {
      const svg = renderer.exportSvg();
      if (!svg) {
        new Notice("The LikeC4 view is not ready for export.");
        return;
      }
      const url = URL.createObjectURL(new Blob([svg], { type: "image/svg+xml" }));
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = `${this.file?.basename ?? "architecture"}.svg`;
      anchor.click();
      URL.revokeObjectURL(url);
    });
    return viewSelect;
  }

  private clearPreviewRevision(): void {
    this.previewRevisions.invalidate();
    this.resetPreviewReferences();
  }

  private clearPreviewForStatus(generation: number): void {
    if (this.previewRevisions.clear(generation)) {
      this.resetPreviewReferences();
    }
  }

  private resetPreviewReferences(): void {
    this.likec4Renderer = null;
    this.likec4ViewSelect = null;
    this.dirtyBadgeEl = null;
    this.sourceEditorEl = null;
  }

  private renderPreviewStatus(
    container: HTMLElement,
    file: TFile,
    message: string,
    error: boolean,
  ): void {
    const chrome = this.buildChrome(container, this.currentSource(), file.basename);
    chrome.body.createEl("p", {
      cls: error ? "criv-c4-render-error" : "criv-c4-render-status",
      text: message,
    });
    this.dirtyBadgeEl = chrome.dirtyBadge;
    this.sourceEditorEl = null;
    this.updateDirtyBadge();
  }

  private renderSourceEditor(body: HTMLElement): void {
    const sourcePanel = body.createDiv({ cls: "criv-c4-source" });
    const editor = sourcePanel.createEl("textarea", { cls: "criv-c4-editor" });
    this.sourceEditorEl = editor;
    editor.value = this.draftSource;
    editor.spellcheck = false;
    editor.oninput = () => {
      this.draftSource = editor.value;
      this.updateDirtyBadge();
    };
    const saveOnModS = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "s") {
        event.preventDefault();
        event.stopPropagation();
        void this.saveSource(editor.value);
      }
    };
    editor.addEventListener("keydown", saveOnModS, { capture: true });
    editor.focus();
  }

  private async saveSource(value: string): Promise<void> {
    if (!this.file) {
      return;
    }
    this.draftSource = value;
    await this.app.vault.modify(this.file, value);
    this.source = value;
    this.sourcePath = this.file.path;
    await this.render();
  }

  private updateDirtyBadge(): void {
    this.dirtyBadgeEl?.toggleClass("is-hidden", !this.sourceDirty());
  }

  private registerSourceSaveHandler(): void {
    if (this.sourceSaveHandlerRegistered || !this.scope) {
      return;
    }
    this.scope.register(["Mod"], "s", () => {
      if (this.mode !== "source" || !this.sourceEditorEl) {
        return;
      }
      void this.saveSource(this.sourceEditorEl.value);
      return false;
    });
    this.sourceSaveHandlerRegistered = true;
  }
}

export function refreshC4Views(app: App, status: ObsidianStateStatus): Promise<void> {
  const views = app.workspace
    .getLeavesOfType(C4_VIEW_TYPE)
    .map((leaf) => leaf.view)
    .filter((view): view is CrivC4View => view instanceof CrivC4View);
  return Promise.all(views.map((view) => view.acceptStateStatus(status))).then(() => undefined);
}

export function shutdownC4Views(app: App): void {
  for (const leaf of app.workspace.getLeavesOfType(C4_VIEW_TYPE)) {
    if (leaf.view instanceof CrivC4View) {
      leaf.view.shutdown();
    }
  }
}

export function patchNativeSaveCommand(
  app: App,
  registerCleanup: (cleanup: () => void) => void,
): void {
  const saveCommand = obsidianCommands(app)?.commands?.["editor:save-file"];
  if (!saveCommand) {
    return;
  }
  const originalCheckCallback = saveCommand.checkCallback;
  saveCommand.checkCallback = (checking: boolean) => {
    const c4View = app.workspace.getActiveViewOfType(CrivC4View);
    if (c4View?.canSaveSourceFromShortcut()) {
      if (!checking) {
        void c4View.saveSourceFromShortcut();
      }
      return true;
    }
    return originalCheckCallback?.(checking);
  };
  registerCleanup(() => {
    saveCommand.checkCallback = originalCheckCallback;
  });
}

class CrivC4PreviewRevision {
  constructor(
    readonly root: HTMLElement,
    readonly renderer: CrivLikeC4Renderer | null,
    readonly viewSelect: HTMLSelectElement | null,
    readonly dirtyBadge: HTMLElement,
    readonly source: string,
    readonly sourcePath: string,
  ) {}

  dispose(): void {
    this.renderer?.dispose();
  }
}

function obsidianCommands(app: App): ObsidianCommandRegistry | null {
  return (app as unknown as { commands?: ObsidianCommandRegistry }).commands ?? null;
}
