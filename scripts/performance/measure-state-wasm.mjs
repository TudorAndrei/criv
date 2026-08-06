#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { basename, resolve } from "node:path";
import { performance } from "node:perf_hooks";
import { parseArgs } from "node:util";
import { fileURLToPath } from "node:url";

const OUTPUT_SCHEMA = "criv.state-wasm-loaded-revision.v1";
const OPERATIONS = [
  "cold_load_and_initial_projections",
  "initial_projections_after_load",
  "lookup_present",
  "lookup_missing",
  "selector_empty",
  "selector_exact",
  "selector_suffix",
  "selector_missing",
  "replacement_lifetime",
];

const definitions = {
  cold_load_and_initial_projections:
    "load the packaged Wasm module, read and decode one State revision, and return the initial projection batch",
  initial_projections_after_load:
    "return the initial projection batch after module, file, and State revision load",
  lookup_present: "look up one existing graph-node ID after module and file load",
  lookup_missing: "look up one absent graph-node ID after module and file load",
  selector_empty: "request source selectors with an empty query after module and file load",
  selector_exact: "request source selectors with one exact source path after module and file load",
  selector_suffix: "request source selectors with one source basename after module and file load",
  selector_missing: "request source selectors with one absent query after module and file load",
  replacement_lifetime:
    "load, project, and free the same State twenty times after five warm-up replacements",
};

function options() {
  return parseArgs({
    options: {
      state: { type: "string" },
      package: { type: "string" },
      samples: { type: "string", default: "5" },
      output: { type: "string" },
      "allow-low-samples": { type: "boolean", default: false },
      worker: { type: "boolean", default: false },
      operation: { type: "string" },
    },
    strict: true,
  }).values;
}

function required(value, name) {
  if (!value) throw new Error(`--${name} is required`);
  return resolve(value);
}

function loadPackage(packageRoot) {
  return createRequire(import.meta.url)(packageRoot);
}

function loadRevision(wasm, raw) {
  return new wasm.LoadedState(raw);
}

function operationInputs(projections) {
  const presentNode = projections.nodes[0]?.id ?? "__criv_missing_node__";
  const exactSource = projections.sources[0]?.path ?? "__criv_missing_source__";
  return {
    presentNode,
    exactSource,
    suffixSource: basename(exactSource),
  };
}

function runOperation(revision, operation, inputs) {
  switch (operation) {
    case "initial_projections_after_load":
    case "cold_load_and_initial_projections":
      return revision.initialProjections();
    case "lookup_present":
      return revision.lookupNode(inputs.presentNode);
    case "lookup_missing":
      return revision.lookupNode("__criv_missing_node__");
    case "selector_empty":
      return revision.suggestSelectors("", 20);
    case "selector_exact":
      return revision.suggestSelectors(inputs.exactSource, 20);
    case "selector_suffix":
      return revision.suggestSelectors(inputs.suffixSource, 20);
    case "selector_missing":
      return revision.suggestSelectors("__criv_missing_selector__", 20);
    default:
      throw new Error(`unsupported operation: ${operation}`);
  }
}

function runReplacementLifetime(wasm, raw) {
  const replace = () => {
    const revision = loadRevision(wasm, raw);
    revision.initialProjections();
    revision.free();
    globalThis.gc?.();
  };
  for (let warmup = 0; warmup < 5; warmup += 1) replace();
  const rss = [];
  for (let replacement = 0; replacement < 20; replacement += 1) {
    replace();
    rss.push(process.memoryUsage().rss);
  }
  return rss;
}

function worker(values) {
  const state = required(values.state, "state");
  const packageRoot = required(values.package, "package");
  const operation = values.operation;
  if (!OPERATIONS.includes(operation)) throw new Error(`invalid --operation: ${operation}`);

  let wasm;
  let raw;
  let revision;
  let inputs;
  let replacementRssBytes;
  let start;
  if (operation === "cold_load_and_initial_projections") {
    start = performance.now();
    wasm = loadPackage(packageRoot);
    raw = readFileSync(state, "utf8");
    revision = loadRevision(wasm, raw);
    revision.initialProjections();
  } else if (operation === "replacement_lifetime") {
    wasm = loadPackage(packageRoot);
    raw = readFileSync(state, "utf8");
    start = performance.now();
    replacementRssBytes = runReplacementLifetime(wasm, raw);
  } else {
    wasm = loadPackage(packageRoot);
    raw = readFileSync(state, "utf8");
    revision = loadRevision(wasm, raw);
    if (operation !== "initial_projections_after_load") {
      inputs = operationInputs(revision.initialProjections());
    }
    start = performance.now();
    runOperation(revision, operation, inputs);
  }
  const seconds = (performance.now() - start) / 1000;
  revision?.free();
  process.stdout.write(
    `${JSON.stringify({
      seconds,
      max_rss_bytes: process.resourceUsage().maxRSS * 1024,
      replacement_rss_bytes: replacementRssBytes,
    })}\n`,
  );
}

function median(values) {
  const ordered = [...values].sort((left, right) => left - right);
  const middle = Math.floor(ordered.length / 2);
  return ordered.length % 2 === 1 ? ordered[middle] : (ordered[middle - 1] + ordered[middle]) / 2;
}

function timing(raw) {
  const values = raw.map((sample) => sample.seconds);
  const center = median(values);
  return {
    samples: values.length,
    minimum_seconds: Math.min(...values),
    median_seconds: center,
    maximum_seconds: Math.max(...values),
    median_absolute_deviation_seconds: median(values.map((value) => Math.abs(value - center))),
  };
}

function memory(raw) {
  const values = raw.map((sample) => sample.max_rss_bytes);
  return {
    unit: "bytes",
    minimum: Math.min(...values),
    median: median(values),
    maximum: Math.max(...values),
    note: "Node process maximum RSS includes the runtime, packaged Wasm module, State bytes, and operation result.",
  };
}

function findWasmModule(packageRoot) {
  const file = readdirSync(packageRoot)
    .filter((name) => name.endsWith(".wasm"))
    .sort()[0];
  if (!file) throw new Error(`no .wasm module found in ${packageRoot}`);
  return resolve(packageRoot, file);
}

function runWorker(state, packageRoot, operation) {
  const nodeArgs = operation === "replacement_lifetime" ? ["--expose-gc"] : [];
  const result = spawnSync(
    process.execPath,
    [
      ...nodeArgs,
      fileURLToPath(import.meta.url),
      "--worker",
      "--state",
      state,
      "--package",
      packageRoot,
      "--operation",
      operation,
    ],
    { encoding: "utf8" },
  );
  if (result.status !== 0) {
    throw new Error(`${operation} worker failed: ${result.stderr.trim()}`);
  }
  return JSON.parse(result.stdout);
}

function parent(values) {
  const state = required(values.state, "state");
  const packageRoot = required(values.package, "package");
  const samples = Number.parseInt(values.samples, 10);
  if (!Number.isInteger(samples) || samples < 1 || (samples < 3 && !values["allow-low-samples"])) {
    throw new Error("samples must be at least 3 (use --allow-low-samples only for smoke tests)");
  }
  const stateBytes = statSync(state).size;
  const wasmModule = findWasmModule(packageRoot);
  const operations = {};
  for (const operation of OPERATIONS) {
    runWorker(state, packageRoot, operation);
    const raw = [];
    for (let sample = 0; sample < samples; sample += 1) {
      raw.push(runWorker(state, packageRoot, operation));
    }
    operations[operation] = {
      definition: definitions[operation],
      cache_state:
        operation === "cold_load_and_initial_projections"
          ? "fresh process and module after one untimed process warm-up; operating-system file cache is not reset"
          : operation === "replacement_lifetime"
            ? "fresh process with five untimed load, project, free, and garbage-collection cycles"
            : "fresh process with module and State revision loaded before the timed operation, after one untimed process warm-up",
      raw,
      timing: timing(raw),
      peak_rss: memory(raw),
      ...(operation === "replacement_lifetime"
        ? { replacement_lifetime: replacementLifetime(raw) }
        : {}),
    };
  }
  const output = {
    schema: OUTPUT_SCHEMA,
    samples,
    state,
    state_bytes: stateBytes,
    package: packageRoot,
    wasm_module: wasmModule,
    wasm_module_bytes: statSync(wasmModule).size,
    operations,
  };
  const report = `${JSON.stringify(output, null, 2)}\n`;
  if (values.output) writeFileSync(resolve(values.output), report);
  else process.stdout.write(report);
}

function replacementLifetime(raw) {
  const samples = raw.map((sample) => sample.replacement_rss_bytes);
  const ratios = samples.map((sample) => Math.max(...sample) / sample[0]);
  return {
    cycles: 20,
    warmup_cycles: 5,
    sample_rss_bytes: samples,
    maximum_to_first_ratio: Math.max(...ratios),
    passes_110_percent_limit: ratios.every((ratio) => ratio <= 1.1),
  };
}

try {
  const values = options();
  if (values.worker) worker(values);
  else parent(values);
} catch (error) {
  process.stderr.write(`measure-state-wasm: ${error instanceof Error ? error.message : error}\n`);
  process.exitCode = 1;
}
