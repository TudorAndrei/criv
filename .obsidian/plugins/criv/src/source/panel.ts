import { ItemView, Notice, TFile } from "obsidian";
import type { App, WorkspaceLeaf } from "obsidian";
import {
  errorMessage,
  formatBytes,
  type CrivAssetEntry,
  type CrivInitialProjections,
} from "@criv/editor-state";
import type { FrontmatterPatternTarget, LinkedSource } from "./model";
import { frontmatterPatternTargets, linkedSourcesFromMarkdown } from "./model";
import type { StatePort } from "../ports";
import { assetResourceUrl } from "./assets";
import { languageForPath, readSourcePreview, renderPreview, renderPreviewError } from "./preview";

export const SOURCE_PANEL_VIEW_TYPE = "criv-source-panel";

export class ObsidianSourcePanelOwner {
  constructor(
    private readonly app: App,
    private readonly state: StatePort,
    private readonly externalEditorUrl: () => string,
  ) {}

  createView(leaf: WorkspaceLeaf): ItemView {
    return new CrivSourceView(leaf, this, this.state);
  }

  async open(): Promise<void> {
    await this.ensure(true);
  }

  async ensure(reveal: boolean): Promise<void> {
    const existing = this.leaf();
    if (existing) {
      if (reveal) {
        this.app.workspace.revealLeaf(existing);
      }
      if (existing.view instanceof CrivSourceView) {
        await existing.view.render();
      }
      return;
    }

    const leaf = this.app.workspace.getRightLeaf(false);
    if (!leaf) {
      return;
    }
    await leaf.setViewState({ type: SOURCE_PANEL_VIEW_TYPE, active: reveal });
    if (reveal) {
      this.app.workspace.revealLeaf(leaf);
    }
  }

  async refresh(): Promise<void> {
    const leaf = this.leaf();
    if (leaf?.view instanceof CrivSourceView) {
      await leaf.view.render();
    }
  }

  async linkedSourcesForActiveFile(): Promise<LinkedSource[]> {
    const file = this.app.workspace.getActiveFile();
    if (!file) {
      return [];
    }
    const markdown = await this.app.vault.cachedRead(file);
    return linkedSourcesFromMarkdown(markdown, this.state.sourceResolver());
  }

  frontmatterPatternTargets(state: CrivInitialProjections): FrontmatterPatternTarget[] {
    const file = this.app.workspace.getActiveFile();
    if (!file) {
      return [];
    }
    const frontmatter = this.app.metadataCache.getFileCache(file)?.frontmatter;
    return frontmatterPatternTargets(frontmatter, state);
  }

  assetResourcePath(asset: CrivAssetEntry): string | null {
    const file = this.app.vault.getAbstractFileByPath(asset.path);
    return file instanceof TFile
      ? assetResourceUrl(this.app.vault.getResourcePath(file), asset.hash)
      : null;
  }

  async openAsset(asset: CrivAssetEntry): Promise<void> {
    const file = this.app.vault.getAbstractFileByPath(asset.path);
    if (!(file instanceof TFile)) {
      throw new Error(`${asset.path} is not a file in this vault`);
    }
    await this.app.workspace.getLeaf(true).openFile(file);
  }

  openExternal(path: string): void {
    const url = this.externalEditorUrl().replace("{path}", encodeURI(path));
    window.open(url);
  }

  private leaf(): WorkspaceLeaf | null {
    const [first, ...duplicates] = this.app.workspace.getLeavesOfType(SOURCE_PANEL_VIEW_TYPE);
    for (const duplicate of duplicates) {
      duplicate.detach();
    }
    return first ?? null;
  }
}

class CrivSourceView extends ItemView {
  constructor(
    leaf: WorkspaceLeaf,
    private readonly owner: ObsidianSourcePanelOwner,
    private readonly state: StatePort,
  ) {
    super(leaf);
  }

  getViewType(): string {
    return SOURCE_PANEL_VIEW_TYPE;
  }

  getDisplayText(): string {
    return "criv";
  }

  async onOpen(): Promise<void> {
    await this.render();
  }

  async render(): Promise<void> {
    const container = this.containerEl.children[1] as HTMLElement;
    container.empty();
    container.addClass("criv-panel");
    const state = await this.state.load();
    if (!state) {
      container.createEl("p", {
        cls: "criv-empty",
        text: this.state.statusMessage(),
      });
      return;
    }

    const linkedSources = await this.owner.linkedSourcesForActiveFile();
    const header = container.createDiv({ cls: "criv-panel-header" });
    header.createEl("h3", { text: "Linked source files" });
    header.createSpan({ text: `${linkedSources.length}` });

    if (linkedSources.length === 0) {
      container.createEl("p", {
        cls: "criv-empty",
        text: "No source links in the active note.",
      });
    }

    for (const source of linkedSources) {
      await this.renderLinkedSource(container, source);
    }

    const assets = state.assets;
    const assetHeader = container.createDiv({ cls: "criv-panel-header" });
    assetHeader.createEl("h3", { text: "Documentation assets" });
    assetHeader.createSpan({ text: `${assets.length}` });
    if (assets.length === 0) {
      container.createEl("p", { cls: "criv-empty", text: "No previewable assets." });
    }
    for (const asset of assets) {
      await this.renderAsset(container, asset);
    }

    const frontmatterPatterns = this.owner.frontmatterPatternTargets(state);
    container.createEl("h3", { cls: "criv-section-title", text: "Pattern targets" });
    if (frontmatterPatterns.length === 0) {
      container.createEl("p", { cls: "criv-empty", text: "No frontmatter pattern targets." });
    }
    for (const target of frontmatterPatterns) {
      const row = container.createDiv({ cls: "criv-pattern-row" });
      row.addClass(target.status === "unresolved" ? "criv-warning" : "criv-pattern-ref");
      row.createSpan({ text: `${target.source}: ${target.id}` });
      row.createSpan({
        text:
          target.status === "local"
            ? "local target"
            : target.status === "resolved"
              ? `${target.matches.length} match${target.matches.length === 1 ? "" : "es"}`
              : "unresolved",
      });
      this.renderPatternMatches(container, target);
    }
  }

  private renderPatternMatches(container: HTMLElement, target: FrontmatterPatternTarget): void {
    if (target.status === "local" || target.status === "unresolved") {
      return;
    }
    const list = container.createDiv({ cls: "criv-pattern-match-list" });
    if (target.matches.length === 0) {
      list.createDiv({ cls: "criv-pattern-match-empty", text: "No matches" });
      return;
    }
    for (const match of target.matches) {
      const item = list.createDiv({ cls: "criv-pattern-match" });
      const head = item.createDiv({ cls: "criv-pattern-match-head" });
      head.createSpan({ text: match.file });
      head.createSpan({ text: match.range ?? "range unavailable" });
      const captures = Object.entries(match.captures);
      if (captures.length > 0) {
        const captureList = item.createDiv({ cls: "criv-pattern-captures" });
        for (const [name, value] of captures) {
          const capture = captureList.createDiv({ cls: "criv-pattern-capture" });
          capture.createSpan({ text: `$${name}` });
          capture.createEl("code", { text: value });
        }
      }
    }
  }

  private async renderLinkedSource(container: HTMLElement, source: LinkedSource): Promise<void> {
    const card = container.createDiv({ cls: "criv-source-card" });
    const head = card.createDiv({ cls: "criv-source-card-head" });
    const openButton = head.createEl("button", { text: source.entry.path });
    openButton.onclick = () => this.owner.openExternal(source.entry.path);
    head.createSpan({ text: source.entry.mime ?? languageForPath(source.entry.path) });

    try {
      const preview = await readSourcePreview(this.app, source);
      renderPreview(card, preview, true);
    } catch {
      renderPreviewError(card, source.entry.path);
    }
  }

  private async renderAsset(container: HTMLElement, asset: CrivAssetEntry): Promise<void> {
    const card = container.createDiv({ cls: "criv-asset-card" });
    const head = card.createDiv({ cls: "criv-source-card-head" });
    const openButton = head.createEl("button", { text: asset.path });
    const showError = () => {
      card.querySelector(".criv-asset-preview")?.remove();
      renderPreviewError(card, asset.path);
    };
    openButton.onclick = () => {
      void this.owner.openAsset(asset).catch((error: unknown) => {
        new Notice(`Could not open ${asset.path}: ${errorMessage(error)}`);
        showError();
      });
    };
    head.createSpan({ text: `${asset.mime} · ${formatBytes(asset.bytes)}` });

    if (!asset.mime.startsWith("image/")) {
      const name = asset.mime === "application/pdf" ? "PDF" : "asset";
      card.createDiv({ cls: "criv-asset-document", text: `Open the ${name} in Obsidian` });
      return;
    }
    const resource = this.owner.assetResourcePath(asset);
    if (!resource) {
      renderPreviewError(card, asset.path);
      return;
    }
    const image = card.createEl("img", {
      cls: "criv-asset-preview",
      attr: { alt: asset.path, loading: "lazy", src: resource },
    });
    image.onerror = showError;
  }
}
