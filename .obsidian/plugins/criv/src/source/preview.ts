import type { App } from "obsidian";
import type { LinkedSource } from "./model";

const PREVIEW_LINE_LIMIT = 80;

export interface SourcePreview {
  path: string;
  language: string;
  text: string;
  startLine: number;
  truncated: boolean;
}

/** Reads a linked source, whose path `resolveSourceResult` confined to the vault. */
export async function readSourcePreview(app: App, linked: LinkedSource): Promise<SourcePreview> {
  const sourcePath = linked.entry.path;
  const raw = await app.vault.adapter.read(sourcePath);
  const lines = raw.split(/\r?\n/);
  const lineRange = linked.lines;
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

interface LanguageSyntax {
  extensions: readonly string[];
  tokens: RegExp;
  comment: string;
  stringPrefixes: readonly string[];
  atoms: boolean;
  keywords: ReadonlySet<string>;
  literals: ReadonlySet<string>;
}

const C_LIKE_TOKENS =
  /\/\/.*|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|`(?:\\.|[^`\\])*`|\b\d+(?:\.\d+)?\b|\b[A-Za-z_][A-Za-z0-9_]*\b/g;
const QUOTES = ['"', "'", "`"];
const C_LIKE_LITERALS = new Set(["false", "null", "true", "undefined"]);
const TYPESCRIPT_KEYWORDS = new Set([
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

function cLike(extensions: readonly string[], keywords: ReadonlySet<string>): LanguageSyntax {
  return {
    extensions,
    tokens: C_LIKE_TOKENS,
    comment: "//",
    stringPrefixes: QUOTES,
    atoms: false,
    keywords,
    literals: C_LIKE_LITERALS,
  };
}

const PLAIN = cLike([], new Set());

const LANGUAGES: Readonly<Record<string, LanguageSyntax>> = {
  rust: cLike(
    ["rs"],
    new Set([
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
    ]),
  ),
  typescript: cLike(["ts", "tsx"], TYPESCRIPT_KEYWORDS),
  javascript: cLike(["js", "jsx"], TYPESCRIPT_KEYWORDS),
  go: cLike(
    ["go"],
    new Set([
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
    ]),
  ),
  python: {
    extensions: ["py"],
    tokens:
      /#.*|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|`(?:\\.|[^`\\])*`|\b\d+(?:\.\d+)?\b|\b[A-Za-z_][A-Za-z0-9_]*\b/g,
    comment: "#",
    stringPrefixes: QUOTES,
    atoms: false,
    keywords: new Set([
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
    ]),
    literals: new Set(["False", "None", "True"]),
  },
  elixir: {
    extensions: ["ex", "exs"],
    tokens:
      /#.*|~[A-Za-z](?:\/(?:\\.|[^/\\])*\/[A-Za-z]*|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*')|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|:[A-Za-z_][A-Za-z0-9_!?@]*|\b\d+(?:\.\d+)?\b|\b[A-Za-z_][A-Za-z0-9_!?]*\b/g,
    comment: "#",
    stringPrefixes: [...QUOTES, "~"],
    atoms: true,
    keywords: new Set([
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
    ]),
    literals: new Set(["false", "nil", "true"]),
  },
};

export function languageForPath(path: string): string {
  const extension = path.split(".").pop()?.toLowerCase();
  const language = Object.entries(LANGUAGES).find(([, syntax]) =>
    syntax.extensions.includes(extension ?? ""),
  );
  return language?.[0] ?? extension ?? "text";
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
  const syntax = LANGUAGES[language] ?? PLAIN;
  const tokens: HighlightToken[] = [];
  let cursor = 0;
  for (const match of line.matchAll(syntax.tokens)) {
    const index = match.index ?? cursor;
    if (index > cursor) {
      tokens.push({ text: line.slice(cursor, index) });
    }
    const text = match[0];
    tokens.push({ text, className: highlightClass(text, syntax) });
    cursor = index + text.length;
  }
  if (cursor < line.length) {
    tokens.push({ text: line.slice(cursor) });
  }
  return tokens;
}

function highlightClass(token: string, syntax: LanguageSyntax): string | undefined {
  if (token.startsWith(syntax.comment)) {
    return "criv-token-comment";
  }
  if (syntax.stringPrefixes.some((prefix) => token.startsWith(prefix))) {
    return "criv-token-string";
  }
  if (syntax.atoms && token.startsWith(":")) {
    return "criv-token-literal";
  }
  if (/^\d/.test(token)) {
    return "criv-token-number";
  }
  if (syntax.keywords.has(token)) {
    return "criv-token-keyword";
  }
  if (syntax.literals.has(token)) {
    return "criv-token-literal";
  }
  if (/^[A-Z][A-Za-z0-9_]*$/.test(token)) {
    return "criv-token-type";
  }
  return undefined;
}

function safeCssSegment(value: string): string {
  return /^[a-z0-9_-]+$/i.test(value) ? value : "text";
}
