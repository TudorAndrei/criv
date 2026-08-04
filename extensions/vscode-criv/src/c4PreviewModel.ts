import { likeC4ViewDocumentPath, preferredLikeC4ViewId } from "@criv/likec4/protocol";

export interface C4PreviewView {
  id: string;
  title: string;
  sourcePath?: string;
}

export function preferredC4ViewId(
  documentPath: string,
  views: readonly C4PreviewView[],
): string | undefined {
  return preferredLikeC4ViewId(documentPath, views);
}

/**
 * The workspace-relative file the editor should open when the renderer
 * navigates to `viewId`, or `undefined` when the open document already owns it.
 */
export function c4NavigationTarget(
  documentPath: string,
  workspace: string,
  viewId: string,
  views: readonly C4PreviewView[],
): string | undefined {
  const target = likeC4ViewDocumentPath(workspace, viewId, views);
  if (!target || normalizePath(documentPath) === target) {
    return undefined;
  }
  return target;
}

function normalizePath(path: string): string {
  return path.replace(/\\/g, "/").replace(/^\.\//, "");
}
