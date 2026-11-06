import * as vscode from "vscode";
import {
  formatBytes,
  type CrivAssetEntry,
  type CrivInitialProjections,
  type CrivSourceEntry,
} from "@criv/editor-state";

import { CRIV_COMMANDS } from "./commands/identifiers";
import type { WorkspaceStateStatus } from "./state/store";

export class CrivStateTreeProvider implements vscode.TreeDataProvider<CrivTreeItem> {
  private status: WorkspaceStateStatus = { generation: 0, kind: "loading" };
  private readonly didChangeTreeData = new vscode.EventEmitter<CrivTreeItem | undefined>();

  readonly onDidChangeTreeData = this.didChangeTreeData.event;

  update(status: WorkspaceStateStatus): void {
    this.status = status;
    this.didChangeTreeData.fire(undefined);
  }

  getTreeItem(element: CrivTreeItem): vscode.TreeItem {
    return element;
  }

  getChildren(element?: CrivTreeItem): CrivTreeItem[] {
    if (!element) {
      return this.rootItems();
    }

    if (element.section && this.status.kind === "ready") {
      return sectionChildren(element.section, this.status.projections);
    }

    return [];
  }

  dispose(): void {
    this.didChangeTreeData.dispose();
  }

  private rootItems(): CrivTreeItem[] {
    switch (this.status.kind) {
      case "ready":
        return readyRootItems(this.status.projections);
      case "loading":
        return [new CrivTreeItem("Loading criv state")];
      case "missing":
      case "unavailable":
      case "invalid":
        return [new CrivTreeItem(this.status.message)];
    }
  }
}

type TreeSection = "summary" | "sources" | "assets" | "patterns";

/** A section item expands to its section's rows; every other item is a leaf. */
class CrivTreeItem extends vscode.TreeItem {
  constructor(
    label: string,
    readonly section?: TreeSection,
  ) {
    super(
      label,
      section ? vscode.TreeItemCollapsibleState.Collapsed : vscode.TreeItemCollapsibleState.None,
    );
  }
}

function readyRootItems(projections: CrivInitialProjections): CrivTreeItem[] {
  return [
    sectionItem("Summary", "summary", `${projections.summary.node_count} nodes`),
    sectionItem("Source Files", "sources", `${projections.sources.length}`),
    sectionItem("Documentation Assets", "assets", `${projections.assets.length}`),
    sectionItem("Registered Patterns", "patterns", `${projections.registeredPatterns.length}`),
  ];
}

function sectionChildren(
  section: TreeSection,
  projections: CrivInitialProjections,
): CrivTreeItem[] {
  switch (section) {
    case "summary":
      return summaryItems(projections);
    case "sources":
      return projections.sources.map(sourceItem);
    case "assets":
      return projections.assets.length > 0
        ? projections.assets.map(assetItem)
        : [new CrivTreeItem("No previewable assets")];
    case "patterns":
      return projections.registeredPatterns.length > 0
        ? projections.registeredPatterns.map(patternItem)
        : [new CrivTreeItem("No registered patterns")];
  }
}

function summaryItems(projections: CrivInitialProjections): CrivTreeItem[] {
  return [
    new CrivTreeItem(`Schema: ${projections.summary.schema}`),
    new CrivTreeItem(`Nodes: ${projections.summary.node_count}`),
    new CrivTreeItem(`Edges: ${projections.summary.edge_count}`),
    new CrivTreeItem(`Sources: ${projections.summary.source_count}`),
    new CrivTreeItem(`Assets: ${projections.summary.asset_count}`),
    new CrivTreeItem(`Patterns: ${projections.summary.pattern_count}`),
  ];
}

function sectionItem(label: string, section: TreeSection, description: string): CrivTreeItem {
  const item = new CrivTreeItem(label, section);
  item.description = description;
  return item;
}

function sourceItem(source: CrivSourceEntry): CrivTreeItem {
  const item = new CrivTreeItem(source.path);
  item.description = source.mime;
  item.command = {
    command: CRIV_COMMANDS.openSourceTarget,
    title: "Open Source Target",
    arguments: [source.path],
  };
  return item;
}

function assetItem(asset: CrivAssetEntry): CrivTreeItem {
  const item = new CrivTreeItem(asset.path);
  item.description = `${asset.mime} · ${formatBytes(asset.bytes)}`;
  item.command = {
    command: CRIV_COMMANDS.openAsset,
    title: "Open Documentation Asset",
    arguments: [asset.path],
  };
  return item;
}

function patternItem(pattern: string): CrivTreeItem {
  return new CrivTreeItem(pattern);
}
