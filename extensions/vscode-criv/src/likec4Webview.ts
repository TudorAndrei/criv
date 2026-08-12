import { CrivLikeC4Renderer } from "@criv/likec4/renderer";
import type { CrivLikeC4Model } from "@criv/likec4/protocol";

declare function acquireVsCodeApi(): { postMessage(message: unknown): void };

const payload = JSON.parse(document.getElementById("payload")?.textContent ?? "null") as {
  model: CrivLikeC4Model;
  viewId: string;
  colorScheme?: "light" | "dark";
} | null;
const container = document.getElementById("diagram");
if (!payload || !container) {
  throw new Error("The LikeC4 preview payload is missing.");
}
const vscode = acquireVsCodeApi();
const select = document.getElementById("view") as HTMLSelectElement | null;
const renderer = new CrivLikeC4Renderer(container, {
  colorScheme: payload.colorScheme,
  onOpenSource: (target) => vscode.postMessage({ type: "openSource", target }),
  onSelectView: (viewId) => {
    if (select) {
      select.value = viewId;
    }
    vscode.postMessage({ type: "selectView", viewId });
  },
});
renderer.replace(payload.model, payload.viewId);
if (select) {
  for (const view of renderer.views()) {
    select.add(new Option(view.title, view.id));
  }
  const currentViewId = renderer.currentViewId();
  if (currentViewId) {
    select.value = currentViewId;
  }
  select.disabled = select.options.length < 2;
  select.addEventListener("change", () => renderer.selectView(select.value));
}
document.getElementById("export")?.addEventListener("click", () => {
  const svg = renderer.exportSvg();
  if (!svg) {
    return;
  }
  const url = URL.createObjectURL(new Blob([svg], { type: "image/svg+xml" }));
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = "architecture.svg";
  anchor.click();
  URL.revokeObjectURL(url);
});
window.addEventListener("unload", () => renderer.dispose());
