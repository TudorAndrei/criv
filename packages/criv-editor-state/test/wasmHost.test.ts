import assert from "node:assert/strict";
import test from "node:test";

import {
  CRIV_STATE_SCHEMA_UNSUPPORTED,
  CrivStateContractError,
  CRIV_LOADED_STATE_DISPOSED,
  CRIV_WASM_LOAD_ERROR,
  CrivLoadedStateDisposedError,
  CrivWasmLoadError,
  createCrivWasmHost,
} from "../src/wasmHost.ts";

test("normalizes Wasm State failures to a stable code and base message", async () => {
  const host = createCrivWasmHost(async () => ({
    LoadedState: class {
      constructor() {
        throw new Error(
          "criv-state-schema-unsupported: unsupported criv state schema: criv.state.v2",
        );
      }
    },
  }), "runtime unavailable");

  await assert.rejects(host.loadState("{}"), (error: unknown) => {
    assert.ok(error instanceof CrivStateContractError);
    assert.equal(error.code, CRIV_STATE_SCHEMA_UNSUPPORTED);
    assert.equal(error.message, "unsupported criv state schema: criv.state.v2");
    return true;
  });
});

test("captures initial projections and delegates prepared queries", async () => {
  let projectionCalls = 0;
  let frees = 0;
  let selectorLimit: number | undefined;
  const projections = { state: "ready" };
  const host = createCrivWasmHost<{ state: string }, string, string>(
    async () => ({
      LoadedState: class {
        initialProjections() {
          projectionCalls += 1;
          return projections;
        }
        lookupSourceTarget(target: string) {
          return `node:${target}`;
        }
        suggestSelectors(query: string, limit: number) {
          selectorLimit = limit;
          return [`selector:${query}`];
        }
        free() {
          frees += 1;
        }
      },
    }),
    "editor runtime unavailable",
  );

  const revision = await host.loadState("raw State");

  assert.equal(revision.initialProjections(), projections);
  assert.equal(revision.initialProjections(), projections);
  assert.equal(projectionCalls, 1);
  assert.equal(revision.lookupSourceTarget("target"), "node:target");
  assert.deepEqual(revision.suggestSelectors("query"), ["selector:query"]);
  assert.equal(selectorLimit, 20);

  revision.dispose();
  revision.dispose();
  assert.equal(frees, 1);
  assert.throws(
    () => revision.lookupSourceTarget("target"),
    (error: unknown) => {
      assert.ok(error instanceof CrivLoadedStateDisposedError);
      assert.equal(error.code, CRIV_LOADED_STATE_DISPOSED);
      return true;
    },
  );
});

test("caches one module-load failure with stable error identity", async () => {
  let attempts = 0;
  const cause = new Error("missing module");
  const host = createCrivWasmHost(async () => {
    attempts += 1;
    throw cause;
  }, "reload this editor");

  const first = await rejectedError(host.loadState("first"));
  const second = await rejectedError(host.loadState("second"));

  assert.equal(attempts, 1);
  assert.equal(first, second);
  assert.ok(first instanceof CrivWasmLoadError);
  assert.equal(first.code, CRIV_WASM_LOAD_ERROR);
  assert.equal(first.message, "reload this editor");
  assert.equal(first.cause, cause);
});

test("wraps a missing LoadedState export as a runtime-load error", async () => {
  for (const moduleValue of [undefined, {}, { LoadedState: "not a constructor" }]) {
    const host = createCrivWasmHost(async () => moduleValue, "bad runtime");
    const error = await rejectedError(host.loadState("raw"));

    assert.ok(error instanceof CrivWasmLoadError);
    assert.equal(error.code, CRIV_WASM_LOAD_ERROR);
    assert.equal(error.message, "bad runtime");
  }
});

test("keeps State errors distinct and frees failed projection candidates", async () => {
  const stateError = new Error("invalid State");
  const invalidStateHost = createCrivWasmHost(async () => ({
    LoadedState: class {
      constructor() {
        throw stateError;
      }
    },
  }), "runtime unavailable");

  assert.equal(await rejectedError(invalidStateHost.loadState("invalid")), stateError);

  let frees = 0;
  const projectionError = new Error("projection failed");
  const projectionHost = createCrivWasmHost(async () => ({
    LoadedState: class {
      initialProjections() {
        throw projectionError;
      }
      lookupSourceTarget() {}
      suggestSelectors() {
        return [];
      }
      free() {
        frees += 1;
      }
    },
  }), "runtime unavailable");

  assert.equal(await rejectedError(projectionHost.loadState("valid")), projectionError);
  assert.equal(frees, 1);
});

async function rejectedError(promise: Promise<unknown>): Promise<Error> {
  try {
    await promise;
  } catch (error) {
    assert.ok(error instanceof Error);
    return error;
  }
  assert.fail("Expected the promise to reject");
}
