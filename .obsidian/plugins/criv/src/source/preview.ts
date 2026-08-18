import type { App } from "obsidian";
import type { LinkedSource } from "./model";
import { parseLineRange, safeVaultPath } from "./model";

const PREVIEW_LINE_LIMIT = 80;

export interface SourcePreview {
  path: string;
  language: string;
  text: string;
  startLine: number;
  truncated: boolean;
}

export async function readSourcePreview(app: App, linked: LinkedSource): Promise<SourcePreview> {
  const sourcePath = safeVaultPath(linked.entry.path);
  if (!sourcePath) {
    throw new Error(`Invalid source path ${linked.entry.path}`);
  }
  const raw = await app.vault.adapter.read(sourcePath);
  const lines = raw.split(/\r?\n/);
  const lineRange = parseLineRange(linked.fragment);
  const start = lineRange?.start ?? 1;
  const end = lineRange?.end ?? Math.min(lines.length, start + PREVIEW_LINE_LIMIT - 1);
  const selected = lines.slice(Math.max(0, start - 1), Math.min(lines.length, end));
  const truncated = !lineRange && start + selected.length - 1 < lines.length;
  return {
    path: sourcePath,
    language: languageForPath(sourcePath),
    text: selected.join("\n"),
    startLine: start,
    truncated,
  };
}

export function renderPreview(
  container: HTMLElement,
  preview: SourcePreview,
  compact: boolean,
): void {
  container.querySelector(".criv-preview-loading")?.remove();
  container.querySelector(".criv-preview-error")?.remove();
  container.querySelector(".criv-preview-body")?.remove();

  const body = container.createDiv({ cls: "criv-preview-body" });
  if (!compact) {
    body.createDiv({ cls: "criv-preview-path", text: preview.path });
  }
  const meta = body.createDiv({ cls: "criv-preview-meta" });
  meta.createSpan({ text: preview.language || "text" });
  meta.createSpan({ text: `L${preview.startLine}` });
  if (preview.truncated) {
    meta.createSpan({ text: "truncated" });
  }
  const source = body.createDiv({ cls: "criv-source-preview" });
  source.createEl("pre", {
    cls: "criv-source-lines",
    text: lineNumbers(preview.text, preview.startLine),
  });
  renderHighlightedCode(source, preview);
}

export function renderPreviewError(container: HTMLElement, path: string): void {
  container.querySelector(".criv-preview-loading")?.remove();
  container.querySelector(".criv-preview-body")?.remove();
  container.createDiv({
    cls: "criv-preview-error",
    text: `Could not read ${path}`,
  });
}

export function positionHoverPreview(preview: HTMLElement, event: MouseEvent): void {
  const margin = 16;
  const width = Math.min(560, window.innerWidth - margin * 2);
  preview.style.width = `${width}px`;
  preview.style.left = `${Math.min(event.clientX + margin, window.innerWidth - width - margin)}px`;
  preview.style.top = `${Math.min(event.clientY + margin, window.innerHeight - 260)}px`;
}

export function languageForPath(path: string): string {
  const extension = path.split(".").pop()?.toLowerCase();
  switch (extension) {
    case "rs":
      return "rust";
    case "ts":
    case "tsx":
      return "typescript";
    case "js":
    case "jsx":
      return "javascript";
    case "py":
      return "python";
    case "go":
      return "go";
    case "ex":
    case "exs":
      return "elixir";
    default:
      return extension ?? "text";
  }
}

interface HighlightToken {
  text: string;
  className?: string;
}

function lineNumbers(text: string, startLine: number): string {
  return text
    .split("\n")
    .map((_line, index) => String(startLine + index).padStart(4, " "))
    .join("\n");
}

function renderHighlightedCode(container: HTMLElement, preview: SourcePreview): void {
  const pre = container.createEl("pre", {
    cls: "criv-source-code criv-source-code-highlighted",
  });
  const code = pre.createEl("code", {
    cls: `language-${safeCssSegment(preview.language)}`,
  });
  const lines = preview.text.split("\n");
  lines.forEach((line, lineIndex) => {
    for (const token of highlightSourceLine(line, preview.language)) {
      if (token.className) {
        code.createSpan({ cls: token.className, text: token.text });
      } else {
        code.appendText(token.text);
      }
    }
    if (lineIndex + 1 < lines.length) {
      code.appendText("\n");
    }
  });
}

export function highlightSourceLine(line: string, language: string): HighlightToken[] {
  const tokens: HighlightToken[] = [];
  const tokenPattern = tokenPatternFor(language);
  let cursor = 0;
  for (const match of line.matchAll(tokenPattern)) {
    const index = match.index ?? cursor;
    if (index > cursor) {
      tokens.push({ text: line.slice(cursor, index) });
    }
    const text = match[0];
    tokens.push({ text, className: highlightClass(text, language) });
    cursor = index + text.length;
  }
  if (cursor < line.length) {
    tokens.push({ text: line.slice(cursor) });
  }
  return tokens;
}

function tokenPatternFor(language: string): RegExp {
  if (language === "elixir") {
    return /#.*|~[A-Za-z](?:\/(?:\\.|[^/\\])*\/[A-Za-z]*|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*')|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|:[A-Za-z_][A-Za-z0-9_!?@]*|\b\d+(?:\.\d+)?\b|\b[A-Za-z_][A-Za-z0-9_!?]*\b/g;
  }
  if (language === "python") {
    return /#.*|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|`(?:\\.|[^`\\])*`|\b\d+(?:\.\d+)?\b|\b[A-Za-z_][A-Za-z0-9_]*\b/g;
  }
  return /\/\/.*|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|`(?:\\.|[^`\\])*`|\b\d+(?:\.\d+)?\b|\b[A-Za-z_][A-Za-z0-9_]*\b/g;
}

function highlightClass(token: string, language: string): string | undefined {
  if (
    token.startsWith("//") ||
    ((language === "python" || language === "elixir") && token.startsWith("#"))
  ) {
    return "criv-token-comment";
  }
  if (
    token.startsWith('"') ||
    token.startsWith("'") ||
    token.startsWith("`") ||
    (language === "elixir" && token.startsWith("~"))
  ) {
    return "criv-token-string";
  }
  if (language === "elixir" && token.startsWith(":")) {
    return "criv-token-literal";
  }
  if (/^\d/.test(token)) {
    return "criv-token-number";
  }
  if (keywordSet(language).has(token)) {
    return "criv-token-keyword";
  }
  if (literalSet(language).has(token)) {
    return "criv-token-literal";
  }
  if (/^[A-Z][A-Za-z0-9_]*$/.test(token)) {
    return "criv-token-type";
  }
  return undefined;
}

function keywordSet(language: string): Set<string> {
  switch (language) {
    case "rust":
      return new Set([
        "as",
        "async",
        "await",
        "const",
        "crate",
        "enum",
        "fn",
        "for",
        "if",
        "impl",
        "let",
        "match",
        "mod",
        "mut",
        "pub",
        "return",
        "self",
        "static",
        "struct",
        "trait",
        "type",
        "use",
        "where",
        "while",
      ]);
    case "typescript":
    case "javascript":
      return new Set([
        "async",
        "await",
        "class",
        "const",
        "else",
        "export",
        "for",
        "from",
        "function",
        "if",
        "import",
        "interface",
        "let",
        "new",
        "private",
        "return",
        "type",
      ]);
    case "python":
      return new Set([
        "as",
        "async",
        "await",
        "class",
        "def",
        "elif",
        "else",
        "for",
        "from",
        "if",
        "import",
        "in",
        "lambda",
        "return",
        "self",
        "while",
      ]);
    case "go":
      return new Set([
        "const",
        "defer",
        "else",
        "for",
        "func",
        "go",
        "if",
        "import",
        "interface",
        "package",
        "range",
        "return",
        "struct",
        "type",
        "var",
      ]);
    case "elixir":
      return new Set([
        "after",
        "alias",
        "and",
        "case",
        "catch",
        "cond",
        "def",
        "defcallback",
        "defdelegate",
        "defexception",
        "defguard",
        "defguardp",
        "defimpl",
        "defmacro",
        "defmacrop",
        "defmodule",
        "defoverridable",
        "defp",
        "defprotocol",
        "defstruct",
        "do",
        "else",
        "end",
        "fn",
        "for",
        "if",
        "import",
        "in",
        "not",
        "or",
        "quote",
        "receive",
        "require",
        "rescue",
        "try",
        "unless",
        "unquote",
        "use",
        "when",
        "with",
      ]);
    default:
      return new Set();
  }
}

function literalSet(language: string): Set<string> {
  if (language === "elixir") {
    return new Set(["false", "nil", "true"]);
  }
  if (language === "python") {
    return new Set(["False", "None", "True"]);
  }
  return new Set(["false", "null", "true", "undefined"]);
}

function safeCssSegment(value: string): string {
  return /^[a-z0-9_-]+$/i.test(value) ? value : "text";
}
