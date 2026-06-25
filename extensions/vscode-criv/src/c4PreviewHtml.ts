import type { C4ArtifactFormat } from "./c4Artifact";

export interface C4PreviewPayload {
  format: C4ArtifactFormat;
  source: string;
  sources: string[];
}

export interface C4PreviewHtmlOptions {
  cspSource: string;
  nonce: string;
  mermaidUri: string;
  vizUri: string;
  payload: C4PreviewPayload;
}

export function buildC4PreviewHtml(options: C4PreviewHtmlOptions): string {
  const payload = escapeScriptJson(JSON.stringify(options.payload));
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src ${options.cspSource} data:; style-src ${options.cspSource} 'unsafe-inline'; script-src 'nonce-${options.nonce}' 'wasm-unsafe-eval';">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>criv C4 Preview</title>
<style>
body { margin: 0; font-family: var(--vscode-font-family); color: var(--vscode-foreground); background: var(--vscode-editor-background); }
main { padding: 16px; }
#diagram svg { max-width: 100%; height: auto; }
.error { color: var(--vscode-errorForeground); white-space: pre-wrap; }
pre { overflow: auto; padding: 12px; background: var(--vscode-textCodeBlock-background); }
.sources { display: flex; flex-wrap: wrap; gap: 8px; margin: 0 0 12px; }
button { color: var(--vscode-button-foreground); background: var(--vscode-button-background); border: 0; padding: 4px 8px; cursor: pointer; }
button:hover { background: var(--vscode-button-hoverBackground); }
</style>
</head>
<body>
<main>
<div class="sources" id="sources"></div>
<div id="diagram"></div>
<pre id="fallback"></pre>
</main>
<script nonce="${options.nonce}" src="${options.mermaidUri}"></script>
<script nonce="${options.nonce}" src="${options.vizUri}"></script>
<script nonce="${options.nonce}" type="application/json" id="payload">${payload}</script>
<script nonce="${options.nonce}">
const vscode = acquireVsCodeApi();
const payload = JSON.parse(document.getElementById("payload").textContent);
const diagram = document.getElementById("diagram");
const fallback = document.getElementById("fallback");
const sources = document.getElementById("sources");
fallback.textContent = payload.source;
for (const target of payload.sources) {
  const button = document.createElement("button");
  button.textContent = target;
  button.addEventListener("click", () => vscode.postMessage({ type: "openSource", target }));
  sources.appendChild(button);
}
function sanitizeDotSvg(svg) {
  return svg
    .replace(/<\\?xml[\\s\\S]*?\\?>/gi, "")
    .replace(/<!DOCTYPE[\\s\\S]*?>/gi, "")
    .replace(/<\\s*(script|foreignObject|iframe|object|embed|image|use)\\b[\\s\\S]*?<\\s*\\/\\s*\\1\\s*>/gi, "")
    .replace(/<\\s*(script|foreignObject|iframe|object|embed|image|use)\\b[^>]*\\/?>/gi, "")
    .replace(/\\s+on[a-z0-9_-]+\\s*=\\s*(?:"[^"]*"|'[^']*'|[^\\s>]+)/gi, "")
    .replace(/\\s+(?:href|xlink:href|target)\\s*=\\s*(?:"[^"]*"|'[^']*'|[^\\s>]+)/gi, "");
}
async function render() {
  try {
    if (payload.format === "mermaid") {
      mermaid.initialize({ startOnLoad: false, securityLevel: "strict", theme: "base" });
      const rendered = await mermaid.render("criv-c4-preview", payload.source);
      diagram.innerHTML = rendered.svg;
      fallback.hidden = true;
      return;
    }
    if (payload.format === "dot") {
      const viz = await Viz.instance();
      diagram.innerHTML = sanitizeDotSvg(viz.renderString(payload.source, { format: "svg" }));
      fallback.hidden = true;
      return;
    }
    diagram.innerHTML = '<div class="error">Unknown .c4 format.</div>';
  } catch (error) {
    diagram.innerHTML = '<div class="error">' + String(error?.message ?? error) + '</div>';
  }
}
void render();
</script>
</body>
</html>`;
}

function escapeScriptJson(json: string): string {
  return json.replace(/</g, "\\u003c").replace(/>/g, "\\u003e").replace(/&/g, "\\u0026");
}
