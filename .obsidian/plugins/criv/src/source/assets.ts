import type { CrivAssetEntry } from "@criv/editor-state";

export function isPassiveImage(asset: CrivAssetEntry): boolean {
  return asset.mime.startsWith("image/");
}

export function assetResourceUrl(resource: string, hash: string): string {
  const separator = resource.includes("?") ? "&" : "?";
  return `${resource}${separator}criv-asset=${encodeURIComponent(hash)}`;
}

export function replaceFailedAssetPreview(container: ParentNode, showError: () => void): void {
  container.querySelector(".criv-asset-preview")?.remove();
  showError();
}
