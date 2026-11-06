export function assetResourceUrl(resource: string, hash: string): string {
  const separator = resource.includes("?") ? "&" : "?";
  return `${resource}${separator}criv-asset=${encodeURIComponent(hash)}`;
}
