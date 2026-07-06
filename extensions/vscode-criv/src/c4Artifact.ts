export type C4ArtifactFormat = "mermaid" | "dot" | "unknown";
export type C4ArtifactLevel = "context" | "container" | "component" | "code" | "unknown";

export interface C4ArtifactDiagnostic {
  code: string;
  line: number | null;
  message: string;
}

export interface C4ArtifactSummary {
  format: C4ArtifactFormat;
  level: C4ArtifactLevel;
  generated: boolean;
  diagnostics: C4ArtifactDiagnostic[];
}

export function parseC4Artifact(path: string, text: string): C4ArtifactSummary {
  const diagnostics: C4ArtifactDiagnostic[] = [];
  const level = c4LevelFromPath(path);
  if (level === "unknown") {
    diagnostics.push({
      code: "missing-c4-level",
      line: null,
      message: "Filename should include context, container, component, components, or code.",
    });
  }

  const directives = c4Directives(text);
  let directiveFormat: C4ArtifactFormat = "unknown";
  let assertedFormat: { key: string; value: string | null; line: number } | null = null;
  const generatedDirective = directives.find((directive) => directive.key === "generated");
  for (const directive of directives) {
    if (directive.key === "format") {
      const format = c4FormatFromDirective(directive.value);
      if (format === "unknown") {
        diagnostics.push({
          code: "invalid-c4-format",
          line: directive.line,
          message: "criv:format should be mermaid or dot.",
        });
      } else {
        if (directiveFormat !== "unknown" && directiveFormat !== format) {
          diagnostics.push({
            code: "duplicate-c4-format",
            line: directive.line,
            message: "Conflicting criv:format directives.",
          });
        }
        directiveFormat = format;
        assertedFormat = directive;
      }
    }
    if (!["format", "generated", "source"].includes(directive.key)) {
      diagnostics.push({
        code: "unknown-c4-directive",
        line: directive.line,
        message: `Unknown directive criv:${directive.key}.`,
      });
    }
  }

  const inferredFormat = c4FormatFromText(text);
  if (
    assertedFormat &&
    directiveFormat !== "unknown" &&
    inferredFormat !== "unknown" &&
    directiveFormat !== inferredFormat
  ) {
    diagnostics.push({
      code: "mismatched-c4-format",
      line: firstMeaningfulLine(text)?.line ?? assertedFormat.line,
      message: `criv:format ${directiveFormat} does not match ${inferredFormat} content.`,
    });
  }
  if (inferredFormat === "unknown" && directiveFormat === "unknown") {
    diagnostics.push({
      code: "unknown-c4-format",
      line: firstNonEmptyLine(text),
      message: "Content should start with Mermaid C4 or DOT syntax.",
    });
  }

  const format = inferredFormat !== "unknown" ? inferredFormat : directiveFormat;
  const header = firstMeaningfulLine(text)?.text ?? "";
  if (format === "mermaid") {
    const headerLevel = c4LevelFromMermaidHeader(header);
    if (headerLevel === "unknown") {
      diagnostics.push({
        code: "invalid-c4-mermaid",
        line: firstMeaningfulLine(text)?.line ?? null,
        message: "Mermaid .c4 content should start with C4Context, C4Container, or C4Component.",
      });
    } else if (level === "code" || (level !== "unknown" && level !== headerLevel)) {
      diagnostics.push({
        code: "mismatched-c4-level",
        line: firstMeaningfulLine(text)?.line ?? null,
        message: `Filename level ${level} does not match Mermaid header ${headerLevel}.`,
      });
    }
  }
  if (format === "dot" && level !== "unknown" && level !== "code") {
    diagnostics.push({
      code: "invalid-c4-level",
      line: null,
      message: "DOT .c4 artifacts are expected to be code-level files.",
    });
  }
  if (
    generatedDirective?.value &&
    generatedDirective.value !== "true" &&
    generatedDirective.value !== "false"
  ) {
    diagnostics.push({
      code: "invalid-c4-generated",
      line: generatedDirective.line,
      message: "criv:generated should be true or false.",
    });
  }

  return {
    format,
    level,
    generated: generatedDirective?.value === "true",
    diagnostics,
  };
}

export function sanitizeDotSvg(svg: string): string {
  return svg
    .replace(/<\?xml[\s\S]*?\?>/gi, "")
    .replace(/<!DOCTYPE[\s\S]*?>/gi, "")
    .replace(
      /<\s*(script|foreignObject|iframe|object|embed|image|use)\b[\s\S]*?<\s*\/\s*\1\s*>/gi,
      "",
    )
    .replace(/<\s*(script|foreignObject|iframe|object|embed|image|use)\b[^>]*\/?>/gi, "")
    .replace(/\s+on[a-z0-9_-]+\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+)/gi, "")
    .replace(/\s+(?:href|xlink:href|target)\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+)/gi, "");
}

export function c4SourceTargets(text: string): string[] {
  return c4Directives(text)
    .filter((directive) => directive.key === "source" && directive.value)
    .map((directive) => directive.value ?? "");
}

function c4Directives(text: string): { key: string; value: string | null; line: number }[] {
  return text
    .split(/\r?\n/)
    .map((line, index) => ({ line: index + 1, text: c4CommentText(line.trim()) }))
    .filter((line): line is { line: number; text: string } => line.text !== null)
    .map((line) => ({ line: line.line, text: line.text.trim() }))
    .filter((line) => line.text.startsWith("criv:"))
    .map((line) => {
      const body = line.text.slice("criv:".length).trim();
      const match = body.match(/^(\S+)(?:\s+(.+))?$/);
      return {
        key: match?.[1] ?? "",
        value: match?.[2]?.trim() ?? null,
        line: line.line,
      };
    })
    .filter((directive) => directive.key.length > 0);
}

function c4CommentText(line: string): string | null {
  if (line.startsWith("%%")) {
    return line.slice(2).trim();
  }
  if (line.startsWith("//")) {
    return line.slice(2).trim();
  }
  if (line.startsWith("#")) {
    return line.slice(1).trim();
  }
  return null;
}

function c4FormatFromText(text: string): C4ArtifactFormat {
  const first = firstMeaningfulLine(text)?.text ?? "";
  if (["C4Context", "C4Container", "C4Component"].includes(first)) {
    return "mermaid";
  }
  if (/^(strict\s+)?(di)?graph(?:\s|\{|$)/.test(first)) {
    return "dot";
  }
  return "unknown";
}

function c4FormatFromDirective(value: string | null): C4ArtifactFormat {
  switch ((value ?? "").trim().toLowerCase()) {
    case "mermaid":
    case "mermaid-c4":
      return "mermaid";
    case "dot":
    case "graphviz":
      return "dot";
    default:
      return "unknown";
  }
}

function c4LevelFromPath(path: string): C4ArtifactLevel {
  const stem = (path.split("/").pop() ?? path).replace(/\.[^.]+$/, "").toLowerCase();
  const tokens = stem.split(/[^a-z0-9]+/).filter(Boolean);
  if (tokens.includes("context")) {
    return "context";
  }
  if (tokens.includes("container") || tokens.includes("containers")) {
    return "container";
  }
  if (tokens.includes("component") || tokens.includes("components")) {
    return "component";
  }
  if (tokens.includes("code")) {
    return "code";
  }
  return "unknown";
}

function c4LevelFromMermaidHeader(header: string): C4ArtifactLevel {
  switch (header) {
    case "C4Context":
      return "context";
    case "C4Container":
      return "container";
    case "C4Component":
      return "component";
    default:
      return "unknown";
  }
}

function firstMeaningfulLine(text: string): { line: number; text: string } | null {
  const lines = text.split(/\r?\n/);
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index]?.trim() ?? "";
    if (line && c4CommentText(line) === null) {
      return { line: index + 1, text: line };
    }
  }
  return null;
}

function firstNonEmptyLine(text: string): number | null {
  const lines = text.split(/\r?\n/);
  const index = lines.findIndex((line) => line.trim().length > 0);
  return index === -1 ? null : index + 1;
}
