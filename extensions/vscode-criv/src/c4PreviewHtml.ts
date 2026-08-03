import type { CrivLikeC4Model } from "@criv/likec4/protocol";

export interface C4PreviewPayload {
  model: CrivLikeC4Model;
  viewId?: string;
  colorScheme: "light" | "dark";
}

export interface C4PreviewHtmlOptions {
  cspSource: string;
  nonce: string;
  rendererUri: string;
  payload: C4PreviewPayload;
}

export function buildC4PreviewHtml(options: C4PreviewHtmlOptions): string {
  const payload = escapeScriptJson(JSON.stringify(options.payload));
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src ${options.cspSource} data:; font-src ${options.cspSource} data:; style-src ${options.cspSource} 'unsafe-inline'; script-src 'nonce-${options.nonce}' 'wasm-unsafe-eval';">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>criv LikeC4 Preview</title>
<style>html, body { width: 100%; height: 100%; margin: 0; overflow: hidden; } body { background: var(--vscode-editor-background); color: var(--vscode-foreground); display: grid; grid-template-rows: auto 1fr; } #controls { display: flex; gap: 8px; padding: 8px; } #diagram { min-height: 0; overflow: hidden; } button, select { background: var(--vscode-button-secondaryBackground); color: var(--vscode-button-secondaryForeground); border: 0; padding: 4px 8px; }</style>
</head>
<body>
<div id="controls"><label for="view">View</label><select id="view" aria-label="Architecture view"></select><button id="export" type="button">Export SVG</button></div>
<div id="diagram" role="region" aria-label="LikeC4 architecture view"></div>
<script nonce="${options.nonce}" type="application/json" id="payload">${payload}</script>
<script nonce="${options.nonce}" src="${options.rendererUri}"></script>
</body>
</html>`;
}

export function buildC4PreviewStatusHtml(cspSource: string, message: string): string {
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${cspSource} 'unsafe-inline';">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>criv LikeC4 Preview</title>
<style>html, body { height: 100%; margin: 0; } body { background: var(--vscode-editor-background); color: var(--vscode-foreground); display: grid; place-items: center; font-family: var(--vscode-font-family); } p { max-width: 42rem; padding: 24px; }</style>
</head>
<body><p>${escapeHtml(message)}</p></body>
</html>`;
}

function escapeScriptJson(json: string): string {
  return json.replace(/</g, "\\u003c").replace(/>/g, "\\u003e").replace(/&/g, "\\u0026");
}

function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}
