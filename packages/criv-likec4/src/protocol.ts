import contract from "../../../assets/likec4-contract.json" with { type: "json" };

export const CRIV_LIKEC4_PROTOCOL_VERSION = contract.protocolVersion;
export const CRIV_LIKEC4_VERSION = contract.likec4Version;

export interface CrivLikeC4Model {
  protocolVersion: typeof CRIV_LIKEC4_PROTOCOL_VERSION;
  likec4Version: typeof CRIV_LIKEC4_VERSION;
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
