import * as vscode from "vscode";

import type { CrivArtifactEntry, CrivStateSnapshot } from "./stateModel";
import type { WorkspaceStateStatus } from "./stateStore";
import type { CrivSourceEntry } from "./wasm";

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

    if (element.kind === "section" && element.section && this.status.kind === "ready") {
      return sectionChildren(element.section, this.status.snapshot);
    }

    return [];
  }

  dispose(): void {
    this.didChangeTreeData.dispose();
  }

  private rootItems(): CrivTreeItem[] {
    switch (this.status.kind) {
      case "ready":
        return readyRootItems(this.status.snapshot);
      case "loading":
        return [new CrivTreeItem("Loading criv state", "message")];
      case "missing":
      case "unavailable":
      case "invalid":
        return [new CrivTreeItem(this.status.message, "message")];
    }
  }
}

type TreeSection = "summary" | "sources" | "patterns" | "c4";

class CrivTreeItem extends vscode.TreeItem {
  constructor(
    label: string,
    readonly kind: "section" | "message" | "source" | "pattern" | "artifact",
    readonly section?: TreeSection,
  ) {
    super(
      label,
      kind === "section"
        ? vscode.TreeItemCollapsibleState.Collapsed
        : vscode.TreeItemCollapsibleState.None,
    );
  }
}

function readyRootItems(snapshot: CrivStateSnapshot): CrivTreeItem[] {
  return [
    sectionItem("Summary", "summary", `${snapshot.summary.node_count} nodes`),
    sectionItem("Source Files", "sources", `${snapshot.sources.length}`),
    sectionItem("Registered Patterns", "patterns", `${snapshot.registeredPatterns.length}`),
    sectionItem(".c4 Artifacts", "c4", `${snapshot.c4Artifacts.length}`),
  ];
}

function sectionChildren(section: TreeSection, snapshot: CrivStateSnapshot): CrivTreeItem[] {
  switch (section) {
    case "summary":
      return summaryItems(snapshot);
    case "sources":
      return snapshot.sources.map(sourceItem);
    case "patterns":
      return snapshot.registeredPatterns.length > 0
        ? snapshot.registeredPatterns.map(patternItem)
        : [new CrivTreeItem("No registered patterns", "message")];
    case "c4":
      return snapshot.c4Artifacts.length > 0
        ? snapshot.c4Artifacts.map(artifactItem)
        : [new CrivTreeItem("No .c4 artifacts", "message")];
  }
}

function summaryItems(snapshot: CrivStateSnapshot): CrivTreeItem[] {
  return [
    new CrivTreeItem(`Schema: ${snapshot.summary.schema}`, "message"),
    new CrivTreeItem(`Nodes: ${snapshot.summary.node_count}`, "message"),
    new CrivTreeItem(`Edges: ${snapshot.summary.edge_count}`, "message"),
    new CrivTreeItem(`Sources: ${snapshot.summary.source_count}`, "message"),
    new CrivTreeItem(`Patterns: ${snapshot.summary.pattern_count}`, "message"),
  ];
}

function sectionItem(label: string, section: TreeSection, description: string): CrivTreeItem {
  const item = new CrivTreeItem(label, "section", section);
  item.description = description;
  return item;
}

function sourceItem(source: CrivSourceEntry): CrivTreeItem {
  const item = new CrivTreeItem(source.path, "source");
  item.description = source.mime;
  item.command = {
    command: "criv.openSourceTarget",
    title: "Open Source Target",
    arguments: [source.path],
  };
  return item;
}

function patternItem(pattern: string): CrivTreeItem {
  return new CrivTreeItem(pattern, "pattern");
}

function artifactItem(artifact: CrivArtifactEntry): CrivTreeItem {
  const item = new CrivTreeItem(artifact.label, "artifact");
  item.description = artifact.path === artifact.label ? undefined : artifact.path;
  item.command = {
    command: "criv.openSourceTarget",
    title: "Open Source Target",
    arguments: [artifact.target],
  };
  return item;
}
