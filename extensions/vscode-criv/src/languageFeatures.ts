import * as vscode from "vscode";

import { COMMAND_OPEN_SOURCE_TARGET } from "./commands";
import {
  analyzeSourceReferences,
  appendSourceHoverContents,
  buildSourceTargetIndex,
  completionToken,
  referenceAtOffset,
  sourceReferenceDiagnostic,
  type SourceReference,
} from "./sourceReferences";
import type { WorkspaceStateStore } from "./stateStore";

const SOURCE_SELECTOR = [
  { language: "markdown", scheme: "file" },
  { language: "criv-c4", scheme: "file" },
  { language: "likec4", scheme: "file" },
];

export function registerSourceLanguageFeatures(
  context: vscode.ExtensionContext,
  store: WorkspaceStateStore,
): void {
  const diagnostics = new SourceDiagnostics(store);
  context.subscriptions.push(
    diagnostics,
    vscode.languages.registerDocumentLinkProvider(
      SOURCE_SELECTOR,
      new SourceDocumentLinkProvider(store),
    ),
    vscode.languages.registerHoverProvider(SOURCE_SELECTOR, new SourceHoverProvider(store)),
    vscode.languages.registerCompletionItemProvider(
      SOURCE_SELECTOR,
      new SourceCompletionProvider(store),
      "#",
      ":",
      "/",
    ),
  );
}

class SourceDocumentLinkProvider implements vscode.DocumentLinkProvider {
  constructor(private readonly store: WorkspaceStateStore) {}

  provideDocumentLinks(document: vscode.TextDocument): vscode.DocumentLink[] {
    const references = documentReferences(this.store, document);
    return references
      .filter((reference) => reference.canonicalTarget)
      .map((reference) => {
        const target = reference.canonicalTarget ?? reference.target;
        return new vscode.DocumentLink(
          rangeFromOffsets(document, reference.start, reference.end),
          vscode.Uri.parse(
            `command:${COMMAND_OPEN_SOURCE_TARGET}?${encodeURIComponent(JSON.stringify([target]))}`,
          ),
        );
      });
  }
}

class SourceHoverProvider implements vscode.HoverProvider {
  constructor(private readonly store: WorkspaceStateStore) {}

  provideHover(document: vscode.TextDocument, position: vscode.Position): vscode.Hover | undefined {
    const reference = referenceAtOffset(
      documentReferences(this.store, document),
      document.offsetAt(position),
    );
    if (!reference) {
      return undefined;
    }

    const contents = new vscode.MarkdownString(undefined, true);
    appendSourceHoverContents(contents, reference);
    return new vscode.Hover(contents, rangeFromOffsets(document, reference.start, reference.end));
  }
}

class SourceCompletionProvider implements vscode.CompletionItemProvider {
  constructor(private readonly store: WorkspaceStateStore) {}

  async provideCompletionItems(
    document: vscode.TextDocument,
    position: vscode.Position,
  ): Promise<vscode.CompletionItem[]> {
    const offset = document.offsetAt(position);
    const token = completionToken(document.getText(), offset);
    const range = rangeFromOffsets(document, token.start, offset);
    const suggestions = await this.store.suggestSelectors(token.query, 50);
    return suggestions.map((suggestion) => {
      const item = new vscode.CompletionItem(suggestion.target, completionKind(suggestion.kind));
      item.detail = suggestion.detail;
      item.documentation = suggestion.path;
      item.insertText = suggestion.target;
      item.range = range;
      return item;
    });
  }
}

class SourceDiagnostics implements vscode.Disposable {
  private readonly collection = vscode.languages.createDiagnosticCollection("criv-source");
  private readonly subscriptions: vscode.Disposable[] = [];

  constructor(private readonly store: WorkspaceStateStore) {
    this.subscriptions.push(
      store.onDidChangeStatus(() => this.updateVisibleDocuments()),
      vscode.workspace.onDidOpenTextDocument((document) => this.update(document)),
      vscode.workspace.onDidChangeTextDocument((event) => this.update(event.document)),
      vscode.workspace.onDidCloseTextDocument((document) => this.collection.delete(document.uri)),
    );
    this.updateVisibleDocuments();
  }

  dispose(): void {
    this.collection.dispose();
    for (const subscription of this.subscriptions) {
      subscription.dispose();
    }
  }

  private updateVisibleDocuments(): void {
    for (const document of vscode.workspace.textDocuments) {
      this.update(document);
    }
  }

  private update(document: vscode.TextDocument): void {
    if (!isSourceSelectorDocument(document)) {
      this.collection.delete(document.uri);
      return;
    }

    const references = documentReferences(this.store, document);
    const diagnostics = references.flatMap((reference) => {
      const message = sourceReferenceDiagnostic(reference);
      if (!message) {
        return [];
      }
      const severity = reference.legacy
        ? vscode.DiagnosticSeverity.Warning
        : vscode.DiagnosticSeverity.Information;
      const diagnostic = new vscode.Diagnostic(
        rangeFromOffsets(document, reference.start, reference.end),
        message,
        severity,
      );
      diagnostic.source = "criv";
      diagnostic.code = reference.legacy ? "legacy-source-target" : "unresolved-source-target";
      return [diagnostic];
    });
    this.collection.set(document.uri, diagnostics);
  }
}

function documentReferences(
  store: WorkspaceStateStore,
  document: vscode.TextDocument,
): SourceReference[] {
  if (store.status.kind !== "ready" || !isSourceSelectorDocument(document)) {
    return [];
  }
  return analyzeSourceReferences(document.getText(), buildSourceTargetIndex(store.status.snapshot));
}

function rangeFromOffsets(document: vscode.TextDocument, start: number, end: number): vscode.Range {
  return new vscode.Range(document.positionAt(start), document.positionAt(end));
}

function completionKind(kind: string): vscode.CompletionItemKind {
  switch (kind) {
    case "file":
    case "code":
      return vscode.CompletionItemKind.File;
    case "function":
    case "fn":
      return vscode.CompletionItemKind.Function;
    case "class":
    case "type":
      return vscode.CompletionItemKind.Class;
    default:
      return vscode.CompletionItemKind.Reference;
  }
}

function isSourceSelectorDocument(document: vscode.TextDocument): boolean {
  return (
    document.languageId === "markdown" ||
    document.languageId === "criv-c4" ||
    document.languageId === "likec4"
  );
}
