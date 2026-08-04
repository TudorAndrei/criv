export interface C4ArtifactSummary {
  format: "likec4" | "unknown";
  generated: boolean;
  diagnostics: { code: string; line: number | null; message: string }[];
}

export function parseC4Artifact(_path: string, text: string): C4ArtifactSummary {
  const generated = /^\s*\/\/\s*criv:generated\s+true\s*$/m.test(text);
  const format = /\b(specification|model|views|deployment|global|extend)\s*\{/.test(text)
    ? "likec4"
    : "unknown";
  return {
    format,
    generated,
    diagnostics:
      format === "likec4"
        ? []
        : [
            {
              code: "unknown-c4-format",
              line: firstContentLine(text),
              message: "The .c4 file must contain LikeC4 DSL.",
            },
          ],
  };
}

function firstContentLine(text: string): number | null {
  const index = text.split(/\r?\n/).findIndex((line) => line.trim().length > 0);
  return index < 0 ? null : index + 1;
}
