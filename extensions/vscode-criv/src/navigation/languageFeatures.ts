import * as vscode from "vscode";
import type { CrivSelectorSuggestion } from "@criv/editor-state";

import { CRIV_COMMANDS } from "../commands/identifiers";
import {
  analyzeSourceReferences,
  appendSourceHoverContents,
  completionToken,
  referenceAtOffset,
  sourceReferenceDiagnostic,
  sourceReferenceDiagnosticCode,
  type SourceReference,
} from "./references";
import type { WorkspaceState } from "../state/store";

const SOURCE_SELECTOR = [{ language: "markdown", scheme: "file" }];

export function registerSourceLanguageFeatures(
  context: vscode.ExtensionContext,
  store: WorkspaceState,
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
  constructor(private readonly store: WorkspaceState) {}

  provideDocumentLinks(document: vscode.TextDocument): vscode.DocumentLink[] {
    return documentReferences(this.store, document).flatMap(({ resolution, start, end }) =>
      resolution.kind === "resolved"
        ? [
            new vscode.DocumentLink(
              rangeFromOffsets(document, start, end),
              vscode.Uri.parse(
                `command:${CRIV_COMMANDS.openSourceTarget}?${encodeURIComponent(JSON.stringify([resolution.canonical_target]))}`,
              ),
            ),
          ]
        : [],
    );
  }
}

class SourceHoverProvider implements vscode.HoverProvider {
  constructor(private readonly store: WorkspaceState) {}

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
  constructor(private readonly store: WorkspaceState) {}

  provideCompletionItems(
    document: vscode.TextDocument,
    position: vscode.Position,
  ): vscode.CompletionItem[] {
    const offset = document.offsetAt(position);
    const token = completionToken(document.getText(), offset);
    const range = rangeFromOffsets(document, token.start, offset);
    const suggestions = this.store.suggestSelectors(token.query, 50);
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

  constructor(private readonly store: WorkspaceState) {
    this.subscriptions.push(
      store.onDidChange(() => this.updateVisibleDocuments()),
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
      diagnostic.code = sourceReferenceDiagnosticCode(reference);
      return [diagnostic];
    });
    this.collection.set(document.uri, diagnostics);
  }
}

function documentReferences(
  store: WorkspaceState,
  document: vscode.TextDocument,
): SourceReference[] {
  if (store.status.kind !== "ready" || !isSourceSelectorDocument(document)) {
    return [];
  }
  return analyzeSourceReferences(document.getText(), (target) => store.lookupSourceTarget(target));
}

function rangeFromOffsets(document: vscode.TextDocument, start: number, end: number): vscode.Range {
  return new vscode.Range(document.positionAt(start), document.positionAt(end));
}

function completionKind(kind: CrivSelectorSuggestion["kind"]): vscode.CompletionItemKind {
  switch (kind) {
    case "file":
    case "code":
      return vscode.CompletionItemKind.File;
    case "function":
    case "callback":
    case "macro":
    case "macro-callback":
      return vscode.CompletionItemKind.Function;
    case "method":
      return vscode.CompletionItemKind.Method;
    case "class":
      return vscode.CompletionItemKind.Class;
    case "struct":
      return vscode.CompletionItemKind.Struct;
    case "module":
    case "external-module":
      return vscode.CompletionItemKind.Module;
    case "behaviour":
    case "protocol":
      return vscode.CompletionItemKind.Interface;
    default:
      return vscode.CompletionItemKind.Reference;
  }
}

function isSourceSelectorDocument(document: vscode.TextDocument): boolean {
  return document.languageId === "markdown";
}
