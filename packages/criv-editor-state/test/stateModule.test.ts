import assert from "node:assert/strict";
import test from "node:test";

import {
  CRIV_LOADED_STATE_DISPOSED,
  CRIV_WASM_LOAD_ERROR,
  CrivWasmLoadError,
} from "../src/contract.ts";
import { createCrivStateModule } from "../src/stateModule.ts";

test("keeps the Wasm State error and frees a revision whose projection fails", async () => {
  const stateError = new Error("invalid State");
  const invalid = createCrivStateModule(async () => ({
    LoadedState: class {
      constructor() {
        throw stateError;
      }
    },
  }));
  assert.equal(await rejectedError(invalid.loadState("raw")), stateError);

  let frees = 0;
  const projectionError = new Error("projection failed");
  const failing = createCrivStateModule(async () => ({
    LoadedState: class {
      initialProjections() {
        throw projectionError;
      }
      free() {
        frees += 1;
      }
    },
  }));
  assert.equal(await rejectedError(failing.loadState("raw")), projectionError);
  assert.equal(frees, 1);
});

test("captures projections once, delegates queries, and frees on first dispose", async () => {
  let projectionCalls = 0;
  let frees = 0;
  const projections = { summary: { schema: "fixture" } };
  const stateModule = createCrivStateModule(async () => ({
    LoadedState: class {
      initialProjections() {
        projectionCalls += 1;
        return projections;
      }
      lookupSourceTarget(target: string) {
        return { kind: "resolved", canonical_target: target };
      }
      suggestSelectors(query: string, limit: number) {
        return [{ target: `${query}:${limit}` }];
      }
      free() {
        frees += 1;
      }
    },
  }));

  const revision = await stateModule.loadState("raw");
  assert.equal(revision.initialProjections(), projections);
  assert.equal(revision.initialProjections(), projections);
  assert.equal(projectionCalls, 1);
  assert.deepEqual(revision.lookupSourceTarget("src/lib.rs"), {
    kind: "resolved",
    canonical_target: "src/lib.rs",
  });
  assert.deepEqual(revision.suggestSelectors("run", 3), [{ target: "run:3" }]);

  revision.dispose();
  revision.dispose();
  assert.equal(frees, 1);
  assert.throws(
    () => revision.lookupSourceTarget("src/lib.rs"),
    (error: unknown) => isCoded(error, CRIV_LOADED_STATE_DISPOSED),
  );
});

test("caches one module-load failure as a runtime error with its cause", async () => {
  let attempts = 0;
  const cause = new Error("missing module");
  const stateModule = createCrivStateModule(async () => {
    attempts += 1;
    throw cause;
  });

  const first = await rejectedError(stateModule.loadState("first"));
  const second = await rejectedError(stateModule.loadState("second"));

  assert.equal(attempts, 1);
  assert.equal(first, second);
  assert.ok(first instanceof CrivWasmLoadError);
  assert.equal(first.code, CRIV_WASM_LOAD_ERROR);
  assert.equal(first.cause, cause);
});

test("rejects a module without a LoadedState constructor as unavailable", async () => {
  for (const moduleValue of [undefined, {}, { LoadedState: "not a constructor" }]) {
    const stateModule = createCrivStateModule(async () => moduleValue);
    const error = await rejectedError(stateModule.loadState("raw"));
    assert.ok(error instanceof CrivWasmLoadError);
  }
});

function isCoded(error: unknown, code: string): boolean {
  return error instanceof Error && (error as Error & { code?: unknown }).code === code;
}

async function rejectedError(promise: Promise<unknown>): Promise<Error> {
  try {
    await promise;
  } catch (error) {
    assert.ok(error instanceof Error);
    return error;
  }
  assert.fail("Expected the promise to reject");
}
