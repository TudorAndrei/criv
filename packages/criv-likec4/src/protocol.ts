export const CRIV_LIKEC4_PROTOCOL_VERSION = 1 as const;
export const CRIV_LIKEC4_NODE_VERSION = "26.5.1" as const;
export const CRIV_LIKEC4_VERSION = "1.59.2" as const;

export interface CrivLikeC4Position {
  line: number;
  character: number;
}

export interface CrivLikeC4Range {
  start: CrivLikeC4Position;
  end: CrivLikeC4Position;
}

export interface CrivLikeC4Diagnostic {
  message: string;
  file: string;
  line: number | null;
  range: CrivLikeC4Range | null;
}

export interface CrivLikeC4BridgeResponse {
  protocolVersion: typeof CRIV_LIKEC4_PROTOCOL_VERSION;
  nodeVersion: string;
  likec4Version: typeof CRIV_LIKEC4_VERSION;
  revision: number;
  valid: boolean;
  diagnostics: CrivLikeC4Diagnostic[];
  model: unknown | null;
  bridgeError?: string;
}

export interface CrivLikeC4Model {
  protocolVersion: typeof CRIV_LIKEC4_PROTOCOL_VERSION;
  likec4Version: typeof CRIV_LIKEC4_VERSION;
  revision: number;
  workspace: string;
  model: unknown;
  views: { id: string; title: string; sourcePath?: string }[];
  sourceLinks: { element: string; target: string }[];
}

export interface CrivLikeC4View {
  id: string;
  title: string;
  sourcePath?: string;
}

export function preferredLikeC4ViewId(
  documentPath: string,
  views: readonly CrivLikeC4View[],
): string | undefined {
  const normalizedDocumentPath = normalizePath(documentPath);
  return views.find((view) => {
    if (!view.sourcePath) {
      return false;
    }
    const sourcePath = normalizePath(view.sourcePath);
    return normalizedDocumentPath === sourcePath || normalizedDocumentPath.endsWith(`/${sourcePath}`);
  })?.id;
}

export function defaultLikeC4ViewId(views: readonly CrivLikeC4View[]): string | undefined {
  return views.find((view) => view.id === "index")?.id ?? views[0]?.id;
}

/**
 * The workspace-relative document that owns a named view, so an editor can
 * follow renderer navigation with the file it opens.
 */
export function likeC4ViewDocumentPath(
  workspace: string,
  viewId: string,
  views: readonly CrivLikeC4View[],
): string | undefined {
  const sourcePath = views.find((view) => view.id === viewId)?.sourcePath;
  if (!sourcePath) {
    return undefined;
  }
  const root = normalizePath(workspace).replace(/\/+$/, "");
  if (!root) {
    return undefined;
  }
  return `${root}/${normalizePath(sourcePath)}`;
}

function normalizePath(path: string): string {
  return path.replace(/\\/g, "/").replace(/^\.\//, "");
}
