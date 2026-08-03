export interface C4PreviewView {
  id: string;
  title: string;
  sourcePath?: string;
}

export function preferredC4ViewId(
  documentPath: string,
  views: readonly C4PreviewView[],
): string | undefined {
  const normalizedDocumentPath = normalizePath(documentPath);
  return views.find((view) => {
    if (!view.sourcePath) {
      return false;
    }
    const sourcePath = normalizePath(view.sourcePath);
    return (
      normalizedDocumentPath === sourcePath || normalizedDocumentPath.endsWith(`/${sourcePath}`)
    );
  })?.id;
}

function normalizePath(path: string): string {
  return path.replaceAll("\\", "/").replace(/^\.\//, "");
}
