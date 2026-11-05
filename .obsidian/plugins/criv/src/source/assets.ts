import { safeVaultPath, type CrivAssetEntry } from "@criv/editor-state";

export function resolveActiveAsset(
  assets: readonly CrivAssetEntry[],
  requestedPath: unknown,
): CrivAssetEntry | null {
  const path = safeVaultPath(requestedPath);
  return path ? (assets.find((asset) => asset.path === path) ?? null) : null;
}

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
