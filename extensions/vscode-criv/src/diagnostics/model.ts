export interface CrivCheckDiagnosticJson {
  severity?: unknown;
  code?: unknown;
  path?: unknown;
  line?: unknown;
  message?: unknown;
}

export interface NormalizedCheckDiagnostic {
  severity: "error" | "warning";
  code: string;
  path: string;
  line?: number;
  message: string;
}

export function parseCheckDiagnostics(raw: string): NormalizedCheckDiagnostic[] {
  const parsed = JSON.parse(raw) as unknown;
  if (!Array.isArray(parsed)) {
    throw new Error("Expected criv check JSON output to be an array.");
  }

  return parsed.filter(isDiagnosticRecord).map((diagnostic) => ({
    severity: diagnostic.severity === "error" ? "error" : "warning",
    code: stringValue(diagnostic.code),
    path: stringValue(diagnostic.path),
    line: numberValue(diagnostic.line),
    message: stringValue(diagnostic.message) || "criv check diagnostic",
  }));
}

function isDiagnosticRecord(value: unknown): value is CrivCheckDiagnosticJson {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function stringValue(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function numberValue(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}
