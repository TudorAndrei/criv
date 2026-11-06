import type { BuildOptions, Plugin } from "esbuild";

export declare const crivWasmBytesPlugin: Plugin;

export declare function bundleTests(
  options: BuildOptions & { testRoot: string; outdir: string; entryName: RegExp },
): Promise<void>;
