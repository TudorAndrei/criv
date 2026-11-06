import * as wasm from "../pkg/criv_wasm.js";
import { createCrivStateModule, type CrivStateModule } from "./stateModule.ts";
import wasmBytes from "./wasmBytes.ts";

/**
 * The State module backed by the Wasm runtime embedded in the editor bundle.
 * Bundle with `crivWasmBytesPlugin` from `@criv/editor-state/esbuild`.
 */
export function createPackagedCrivStateModule(unavailableMessage: string): CrivStateModule {
  return createCrivStateModule(async () => {
    if (!wasmBytes) {
      throw new Error("The criv Wasm runtime was not embedded in this bundle.");
    }
    wasm.initSync({ module: wasmBytes });
    return wasm;
  }, unavailableMessage);
}
