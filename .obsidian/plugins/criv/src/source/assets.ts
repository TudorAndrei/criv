import type { AssetIndexEntry } from "./model";
import { safeVaultPath } from "./model";

export function resolveActiveAsset(
  assets: readonly AssetIndexEntry[],
  requestedPath: unknown,
): AssetIndexEntry | null {
  const path = safeVaultPath(requestedPath);
  return path ? (assets.find((asset) => asset.path === path) ?? null) : null;
}

export function isPassiveImage(asset: AssetIndexEntry): boolean {
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
