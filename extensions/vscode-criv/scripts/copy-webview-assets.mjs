import { copyFile, mkdir } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const extensionRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const mediaDir = join(extensionRoot, "media");

await mkdir(mediaDir, { recursive: true });

await Promise.all([
  copyFile(
    join(extensionRoot, "node_modules", "mermaid", "dist", "mermaid.min.js"),
    join(mediaDir, "mermaid.min.js"),
  ),
  copyFile(
    join(extensionRoot, "node_modules", "@viz-js", "viz", "dist", "viz-global.js"),
    join(mediaDir, "viz-global.js"),
  ),
]);
