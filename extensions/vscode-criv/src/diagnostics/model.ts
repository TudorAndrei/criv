export interface CrivCheckDiagnosticJson {
  severity?: unknown;
  code?: unknown;
  path?: unknown;
  line?: unknown;
  message?: unknown;
  range?: unknown;
}

export interface DiagnosticPosition {
  line: number;
  character: number;
}

export interface DiagnosticRange {
  start: DiagnosticPosition;
  end: DiagnosticPosition;
}

export interface NormalizedCheckDiagnostic {
  severity: "error" | "warning";
  code: string;
  path: string;
  line?: number;
  message: string;
  range?: DiagnosticRange;
}

export function parseCheckDiagnostics(raw: string): NormalizedCheckDiagnostic[] {
  const parsed = JSON.parse(raw) as unknown;
  if (!Array.isArray(parsed)) {
    throw new Error("Expected criv check JSON output to be an array.");
  }

  return parsed.filter(isDiagnosticRecord).map((diagnostic) => {
    const range = rangeValue(diagnostic.range);
    return {
      severity: diagnostic.severity === "error" ? "error" : "warning",
      code: stringValue(diagnostic.code),
      path: stringValue(diagnostic.path),
      line: numberValue(diagnostic.line),
      message: stringValue(diagnostic.message) || "criv check diagnostic",
      ...(range ? { range } : {}),
    };
  });
}

export function diagnosticRange(diagnostic: NormalizedCheckDiagnostic): DiagnosticRange {
  if (diagnostic.range) {
    return diagnostic.range;
  }
  const line = diagnostic.line === undefined ? 0 : Math.max(diagnostic.line - 1, 0);
  return {
    start: { line, character: 0 },
    end: { line, character: Number.MAX_SAFE_INTEGER },
  };
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

function rangeValue(value: unknown): DiagnosticRange | undefined {
  if (!isRecord(value) || !isRecord(value.start) || !isRecord(value.end)) {
    return undefined;
  }
  const start = positionValue(value.start);
  const end = positionValue(value.end);
  if (!start || !end || comparePositions(end, start) < 0) {
    return undefined;
  }
  return { start, end };
}

function positionValue(value: Record<string, unknown>): DiagnosticPosition | undefined {
  const line = nonNegativeInteger(value.line);
  const character = nonNegativeInteger(value.character);
  return line === undefined || character === undefined ? undefined : { line, character };
}

function nonNegativeInteger(value: unknown): number | undefined {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0
    ? value
    : undefined;
}

function comparePositions(left: DiagnosticPosition, right: DiagnosticPosition): number {
  return left.line - right.line || left.character - right.character;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
