export interface ParsedSourceTarget {
  path: string;
  fragment?: string;
  line?: number;
  endLine?: number;
}

const SOURCE_ID_PREFIXES = ["code:", "symbol:", "note:", "source:"];

export function parseSourceTarget(target: string): ParsedSourceTarget | undefined {
  const normalized = normalizeSourceTarget(target.trim());
  if (!normalized) {
    return undefined;
  }

  const [path, fragment] = splitFragment(normalized);
  const safePath = safeVaultPath(path);
  if (!safePath) {
    return undefined;
  }

  const parsedLines = parseLineFragment(fragment);
  return {
    path: safePath,
    fragment,
    line: parsedLines?.line,
    endLine: parsedLines?.endLine,
  };
}

export function normalizeSourceTarget(target: string): string {
  for (const prefix of SOURCE_ID_PREFIXES) {
    if (target.startsWith(prefix)) {
      return target.slice(prefix.length);
    }
  }
  return target;
}

export function parseLineFragment(fragment: string | undefined):
  | {
      line: number;
      endLine?: number;
    }
  | undefined {
  if (!fragment?.startsWith("L")) {
    return undefined;
  }

  const lineMatch = /^L(?<start>\d+)(?:-L?(?<end>\d+))?(?::.*)?$/.exec(fragment);
  const start = lineMatch?.groups?.start;
  if (!start) {
    return undefined;
  }

  const line = Math.max(Number.parseInt(start, 10) - 1, 0);
  const endLine = lineMatch.groups?.end
    ? Math.max(Number.parseInt(lineMatch.groups.end, 10) - 1, line)
    : undefined;
  return { line, endLine };
}

export function safeVaultPath(value: unknown): string | undefined {
  if (typeof value !== "string") {
    return undefined;
  }
  const path = value.trim().replace(/\\/g, "/");
  if (
    !path ||
    path.startsWith("/") ||
    path.startsWith("//") ||
    /^[A-Za-z]:/.test(path) ||
    path.includes("\0")
  ) {
    return undefined;
  }

  const segments: string[] = [];
  for (const segment of path.split("/")) {
    if (!segment || segment === ".") {
      continue;
    }
    if (segment === "..") {
      return undefined;
    }
    segments.push(segment);
  }

  return segments.length > 0 ? segments.join("/") : undefined;
}

function splitFragment(target: string): [string, string | undefined] {
  const index = target.indexOf("#");
  if (index === -1) {
    return [target, undefined];
  }
  return [target.slice(0, index), target.slice(index + 1)];
}
